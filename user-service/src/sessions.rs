use std::time::Duration;

use anyhow::Error;
use async_nats::service::Request;
use common::{message::user::Session, misc, shared::db};
use jiff::Timestamp;
use tokio::time::sleep;

/// How many rows a single cleanup statement touches. Each batch is its own
/// statement, so the write lock is held for one batch at a time rather than
/// for the whole sweep, and sessions stored meanwhile are not kept waiting.
const CLEANUP_BATCH: i64 = 5000;

/// The pause between batches, which gives waiting writers their turn.
const CLEANUP_PAUSE: Duration = Duration::from_millis(10);

/// Set in `misc` once every session expiry is spelled [canonical_expiry]'s
/// way. See [canonicalize_expiries].
const CANONICAL_EXPIRIES_KEY: &str = "sessions-expiry-canonical";

/// The one spelling of a session expiry the sessions table holds: RFC 3339 in
/// UTC, as jiff writes a [Timestamp]. The frontend already writes this, but
/// orrery's sync spells an expiry `2026-03-11 11:22:28.559868+00`.
///
/// One spelling is what lets expiry be compared as text, and so through the
/// `sessions_by_expires` index. Two spellings do not even compare correctly:
/// orrery's space sorts before the `T` of the same day.
pub(crate) fn canonical_expiry(expires: &str) -> Result<String, Error> {
    Ok(expires.parse::<Timestamp>()?.to_string())
}

/// The bound below which a canonical expiry has certainly passed, compared as
/// text.
///
/// Canonical expiries have fractional seconds of varying length, or none, so
/// they do not all sort as the instants they name: `…:05.5Z` sorts before
/// `…:05Z`, because `.` sorts before `Z`. A bound with no fraction, a whole
/// second in the past, is only ever greater than an expiry that is earlier
/// than now: every expiry in its own second is at most a second later than
/// it, and every expiry in a later second sorts after it.
fn expired_bound(now: Timestamp) -> Result<String, Error> {
    Ok(Timestamp::from_second(now.as_second() - 1)?.to_string())
}

pub(crate) async fn read_session(session_id: impl AsRef<str>) -> Result<Option<Session>, Error> {
    let session_id = session_id.as_ref();

    Ok(sqlx::query_as!(
        Session,
        r#"select "session_id", "expires", "data" from "sessions" where "session_id" = ?"#,
        session_id
    )
    .fetch_optional(db())
    .await?)
}

pub(crate) async fn save_session(session: &Session) -> Result<(), Error> {
    let expires = session
        .expires
        .as_deref()
        .map(canonical_expiry)
        .transpose()?;

    sqlx::query!(
        r#"insert into "sessions" ("session_id", "expires", "data")
           values (?, ?, ?)
           on conflict ("session_id") do
           update set "expires" = excluded."expires", "data" = excluded."data""#,
        session.session_id,
        expires,
        session.data
    )
    .execute(db())
    .await?;

    Ok(())
}

pub(crate) async fn delete_session(session_id: impl AsRef<str>) -> Result<(), Error> {
    let session_id = session_id.as_ref();

    sqlx::query!(
        r#"delete from "sessions" where "session_id" = ?"#,
        session_id
    )
    .execute(db())
    .await?;

    Ok(())
}

/// Rewrites every expiry not already spelled [canonical_expiry]'s way, and
/// deletes the sessions among them that have already expired. Does nothing
/// once it has finished once, because [save_session] keeps every expiry it
/// writes canonical.
///
/// Walks the table in rowid order, a batch at a time, so the sweep is linear
/// however many rows there are and never holds the write lock for long.
pub(crate) async fn canonicalize_expiries() -> Result<(), Error> {
    if misc::get::<String>(CANONICAL_EXPIRIES_KEY).await?.is_some() {
        return Ok(());
    }

    let mut after = 0_i64;
    let mut rewritten = 0_u64;
    let mut deleted = 0_u64;

    loop {
        let batch = sqlx::query!(
            r#"select "rowid" as "rowid!: i64", "expires" as "expires!"
               from "sessions"
               where "rowid" > ? and "expires" is not null and substr("expires", 11, 1) != 'T'
               order by "rowid"
               limit ?"#,
            after,
            CLEANUP_BATCH
        )
        .fetch_all(db())
        .await?;

        let Some(last) = batch.last() else {
            break;
        };
        after = last.rowid;

        let now = Timestamp::now();
        let mut tx = db().begin().await?;

        for row in &batch {
            // An expiry that does not parse can never be recognized as passed,
            // and the frontend refuses to load a session carrying one, so the
            // session is no use to anyone.
            match row.expires.parse::<Timestamp>() {
                Ok(expires) if expires > now => {
                    let canonical = expires.to_string();
                    sqlx::query!(
                        r#"update "sessions" set "expires" = ? where "rowid" = ?"#,
                        canonical,
                        row.rowid
                    )
                    .execute(&mut *tx)
                    .await?;
                    rewritten += 1;
                }
                _ => {
                    sqlx::query!(r#"delete from "sessions" where "rowid" = ?"#, row.rowid)
                        .execute(&mut *tx)
                        .await?;
                    deleted += 1;
                }
            }
        }

        tx.commit().await?;
        sleep(CLEANUP_PAUSE).await;
    }

    misc::set(CANONICAL_EXPIRIES_KEY, Timestamp::now().to_string()).await?;

    println!(
        "Session expiries canonicalized: {rewritten} rewritten, {deleted} expired and deleted"
    );

    Ok(())
}

/// Deletes every session whose expiry has passed, a batch at a time, finding
/// them through the `sessions_by_expires` index. Relies on every expiry being
/// canonical, so [canonicalize_expiries] has to have run first.
pub(crate) async fn delete_expired() -> Result<u64, Error> {
    let bound = expired_bound(Timestamp::now())?;
    let mut total = 0;

    loop {
        let deleted = sqlx::query!(
            r#"delete from "sessions"
               where "rowid" in (
                 select "rowid" from "sessions" where "expires" < ? limit ?
               )"#,
            bound,
            CLEANUP_BATCH
        )
        .execute(db())
        .await?
        .rows_affected();

        total += deleted;

        if deleted < CLEANUP_BATCH as u64 {
            return Ok(total);
        }

        sleep(CLEANUP_PAUSE).await;
    }
}

/// Store session data
///
/// Does not reply, so the requestor should not wait for a
/// response. In other words, use publish rather than request when
/// communicating with this endpoint.
pub async fn store(req: &Request) -> Result<(), Error> {
    let session: Session = flexbuffers::from_slice(&req.message.payload)?;

    save_session(&session).await
}

/// Fetch session data
pub async fn retrieve(req: &Request) -> Result<Option<Session>, Error> {
    let session_id: String = flexbuffers::from_slice(&req.message.payload)?;

    Ok(read_session(session_id).await?)
}

/// Delete session data
///
/// Replies once the session is gone, so that a logout can wait until no later
/// request is able to load the session it ended.
pub async fn delete(req: &Request) -> Result<(), Error> {
    let session_id: String = flexbuffers::from_slice(&req.message.payload)?;

    delete_session(session_id).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orrery_and_frontend_expiries_share_one_spelling() {
        assert_eq!(
            canonical_expiry("2026-03-11 11:22:28.559868+00").unwrap(),
            "2026-03-11T11:22:28.559868Z"
        );
        assert_eq!(
            canonical_expiry("2027-09-15T15:35:32.262184152Z").unwrap(),
            "2027-09-15T15:35:32.262184152Z"
        );
    }

    /// The bound must never sort above an expiry that has not passed, however
    /// many fractional digits that expiry has.
    #[test]
    fn the_bound_never_sorts_above_a_live_expiry() {
        let now: Timestamp = "2026-09-15T15:35:32.25Z".parse().unwrap();
        let bound = expired_bound(now).unwrap();

        for live in [
            "2026-09-15T15:35:32.25Z",
            "2026-09-15T15:35:32.5Z",
            "2026-09-15T15:35:33Z",
            "2026-09-15T15:35:33.000000001Z",
        ] {
            let live = canonical_expiry(live).unwrap();
            assert!(
                live.as_str() >= bound.as_str(),
                "{live} sorts below {bound}"
            );
        }

        for passed in ["2026-09-15T15:35:30.999Z", "2026-09-15T15:35:31.5Z"] {
            let passed = canonical_expiry(passed).unwrap();
            assert!(
                passed.as_str() < bound.as_str(),
                "{passed} does not sort below {bound}"
            );
        }
    }
}
