use anyhow::Error;
use async_nats::service::Request;
use common::{
    linked_platform::ZOONIVERSE_LOOKUP_INTERVAL_HOURS,
    message::user::{
        ZooniverseIdentityAnswer, ZooniverseIdentityQuestion, ZooniverseLink,
        ZooniverseListLinksRequest,
    },
    shared::db,
    timestring::TimeString,
    ulid::AsUlid,
};
use jiff::{Span, Timestamp};

/// Every account with a Zooniverse username, when it connected, and the
/// Panoptes account it names if that is known, with when that arrived from
/// orrery if it did.
///
/// The classification importer works from a project's whole history rather
/// than from a per-account watermark, so it cannot ask about one account at a
/// time: it takes the whole list and floors each credit against the matching
/// entry. A null connection time means the account was connected before
/// August 2026, which `common::linked_platform` reads as the cutover.
pub async fn list_links(req: &Request) -> Result<Vec<ZooniverseLink>, Error> {
    let input: ZooniverseListLinksRequest = flexbuffers::from_slice(&req.message.payload)?;

    struct Row {
        user_id: String,
        zooniverse_linked_at: Option<String>,
        zooniverse_id: Option<i64>,
        zooniverse_id_received_at: Option<String>,
    }

    let rows = sqlx::query_as!(
        Row,
        r#"SELECT user_id, zooniverse_linked_at, zooniverse_id, zooniverse_id_received_at
           FROM users
           WHERE zooniverse_username != '' AND zooniverse_username IS NOT NULL
           ORDER BY user_id
           LIMIT ? OFFSET ?"#,
        input.limit,
        input.offset,
    )
    .fetch_all(db())
    .await?;

    let links = rows
        .into_iter()
        .filter_map(|row| {
            Some(ZooniverseLink {
                user_id: row.user_id.as_ulid().ok()?,
                zooniverse_linked_at: row.zooniverse_linked_at,
                zooniverse_id: row.zooniverse_id,
                zooniverse_id_received_at: row.zooniverse_id_received_at,
            })
        })
        .collect();

    Ok(links)
}

/// The accounts naming a Zooniverse login whose Panoptes account isn't known
/// and is due to be asked about: never asked, or asked longer ago than
/// `ZOONIVERSE_LOOKUP_INTERVAL_HOURS` without Panoptes knowing it.
pub async fn to_identify(_req: &Request) -> Result<Vec<ZooniverseIdentityQuestion>, Error> {
    let due = TimeString::from(
        Timestamp::now().checked_sub(Span::new().hours(ZOONIVERSE_LOOKUP_INTERVAL_HOURS))?,
    );

    struct Row {
        user_id: String,
        zooniverse_username: String,
    }

    let rows = sqlx::query_as!(
        Row,
        r#"SELECT user_id, zooniverse_username
           FROM users
           WHERE zooniverse_username != ''
             AND zooniverse_id IS NULL
             AND (zooniverse_checked_at IS NULL OR zooniverse_checked_at < ?)
           ORDER BY user_id"#,
        due,
    )
    .fetch_all(db())
    .await?;

    Ok(rows
        .into_iter()
        .filter_map(|row| {
            Some(ZooniverseIdentityQuestion {
                user_id: row.user_id.as_ulid().ok()?,
                zooniverse_username: row.zooniverse_username,
            })
        })
        .collect())
}

/// Keep what Panoptes said about an account's Zooniverse login.
///
/// A login it knows is stored as the account spells it, which differs from
/// what was typed by capitalization at most: the same account either way, so
/// this is not a new connection and leaves the connection time alone. The id
/// was found here rather than received from orrery, so it carries no
/// `zooniverse_id_received_at`.
/// Nothing is written unless the account still names the login that was
/// looked up.
pub async fn record_identity(req: &Request) -> Result<(), Error> {
    let answer: ZooniverseIdentityAnswer = flexbuffers::from_slice(&req.message.payload)?;
    let user_id = answer.user_id.to_string();
    let checked = TimeString::now();

    match answer.account {
        Some(account) => {
            sqlx::query!(
                r#"UPDATE "users" SET
                       "zooniverse_id" = ?,
                       "zooniverse_username" = ?,
                       "zooniverse_checked_at" = ?,
                       "zooniverse_id_received_at" = NULL
                   WHERE "user_id" = ? AND "zooniverse_username" = ?"#,
                account.id,
                account.login,
                checked,
                user_id,
                answer.looked_up,
            )
            .execute(db())
            .await?;
        }
        None => {
            sqlx::query!(
                r#"UPDATE "users" SET "zooniverse_checked_at" = ?
                   WHERE "user_id" = ? AND "zooniverse_username" = ?"#,
                checked,
                user_id,
                answer.looked_up,
            )
            .execute(db())
            .await?;
        }
    }

    Ok(())
}
