//! Deleting accounts: the one way an account is deleted, whether staff
//! delete it here or orrery deleted it ([`sweep_orrery_deletions`]).
//!
//! An account is deleted as orrery deletes one: what hangs off it goes with
//! it -- its emails, joins, saves, places, progress, badges, certificates,
//! form submissions, messages and sessions -- while staff records of what
//! was done to it (masquerades, permissions it granted) keep its id, and a
//! claim code it claimed stays spent. Each deletion is recorded in
//! `deleted_users`, which `user.deleted.since` hands out, so that every other
//! service forgets what it keeps about the account
//! (`common::user_deletions`).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::Duration;

use anyhow::{Error, anyhow};
use async_nats::service::Request;
use common::{
    message::user::{UserDeletion, UserDeletionsRequest, UserDeletionsResponse},
    misc,
    service::decode_structured_message,
    shared::db,
    timestring::TimeString,
    transaction::Immediate,
};
use ulid::Ulid;

/// How many accounts one transaction deletes, so that the write lock is
/// held for one batch at a time.
const BATCH: usize = 200;

/// How many deletions one answer to `user.deleted.since` hands out.
const PAGE: i64 = 1000;

/// How often the accounts orrery has deleted are deleted here.
const SWEEP_EVERY: Duration = Duration::from_secs(24 * 60 * 60);

/// The pause after a sweep that failed.
const AFTER_FAILURE: Duration = Duration::from_secs(60 * 60);

/// The most of the accounts one sweep may delete, as a fraction (1 in this
/// many), and the fewest it may always delete. Orrery deletes an account
/// now and then; a list of ids that would take more is a fault on its side,
/// and is refused, unless staff allowed it ([`ALLOWANCE`]).
const SWEEP_LIMIT: usize = 100;
const ALWAYS_DELETABLE: usize = 100;

/// How many accounts the next sweep may delete, however many that is of
/// them, when staff expect orrery to have deleted that many at once. A sweep
/// that needs it uses it up.
const ALLOWANCE: &str = "user-deletion-allowance";

/// Orrery's account ids come a page at a time: at most this many.
const ORRERY_IDS_PAGE: usize = 10_000;

/// An account to delete, with orrery's id for it if it has one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Doomed {
    pub user_id: Ulid,
    pub legacy_v3_id: Option<i64>,
}

/// Delete the accounts `doomed`, with everything that hangs off them, and
/// record each deletion; the number deleted. An account already deleted is
/// passed over.
pub async fn delete_users(doomed: &[Doomed]) -> Result<usize, Error> {
    let mut gone = Gone::default();

    for batch in doomed.chunks(BATCH) {
        let mut tx = Immediate::begin_with_retry(db(), None, None).await?;
        for account in batch {
            if let Some(addresses) = delete_user(&mut tx, account).await? {
                gone.accounts.insert(account.user_id);
                gone.usernames.extend(addresses);
            }
        }
        tx.commit().await?;
    }

    gone.delete_sessions().await?;
    Ok(gone.accounts.len())
}

/// Delete the account `doomed` within `tx`, unless it is already gone; the
/// addresses it had, if it was deleted.
async fn delete_user(tx: &mut Immediate, doomed: &Doomed) -> Result<Option<Vec<String>>, Error> {
    let id = doomed.user_id.to_string();

    let addresses = sqlx::query_scalar!(r#"SELECT "email" FROM "emails" WHERE "user_id" = ?"#, id)
        .fetch_all(&mut *tx)
        .await?;

    // What has no foreign key to the account. The rest goes with it, by
    // `ON DELETE CASCADE`.
    sqlx::query!(
        r#"DELETE FROM "ambassador_messages" WHERE "sender" = ?1 OR "recipient" = ?1"#,
        id
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query!(
        r#"DELETE FROM "ambassador_progress" WHERE "user_id" = ?"#,
        id
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query!(r#"DELETE FROM "certificates" WHERE "user_id" = ?"#, id)
        .execute(&mut *tx)
        .await?;
    sqlx::query!(
        r#"DELETE FROM "community_memberships" WHERE "user_id" = ?"#,
        id
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query!(
        r#"DELETE FROM "data_ethics_progress" WHERE "user_id" = ?"#,
        id
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query!(
        r#"DELETE FROM "data_literacy_progress" WHERE "user_id" = ?"#,
        id
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query!(
        r#"DELETE FROM "foundations_progress" WHERE "user_id" = ?"#,
        id
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query!(
        r#"DELETE FROM "higher_ed_progress" WHERE "user_id" = ?"#,
        id
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query!(r#"DELETE FROM "ideal_progress" WHERE "user_id" = ?"#, id)
        .execute(&mut *tx)
        .await?;
    sqlx::query!(r#"DELETE FROM "k12_progress" WHERE "user_id" = ?"#, id)
        .execute(&mut *tx)
        .await?;
    sqlx::query!(
        r#"DELETE FROM "libraries_hubs_progress" WHERE "user_id" = ?"#,
        id
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query!(
        r#"DELETE FROM "library_network_signups" WHERE "user_id" = ?"#,
        id
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query!(
        r#"DELETE FROM "one_million_2024_progress" WHERE "user_id" = ?"#,
        id
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query!(
        r#"DELETE FROM "one_million_2025_progress" WHERE "user_id" = ?"#,
        id
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query!(r#"DELETE FROM "permissions" WHERE "user_id" = ?"#, id)
        .execute(&mut *tx)
        .await?;
    sqlx::query!(r#"DELETE FROM "user_badges" WHERE "user_id" = ?"#, id)
        .execute(&mut *tx)
        .await?;
    sqlx::query!(
        r#"DELETE FROM "user_set_evaluations" WHERE "user_id" = ?"#,
        id
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query!(r#"DELETE FROM "user_set_results" WHERE "user_id" = ?"#, id)
        .execute(&mut *tx)
        .await?;
    sqlx::query!(r#"DELETE FROM "webauthn_sessions" WHERE "user_id" = ?"#, id)
        .execute(&mut *tx)
        .await?;

    let deleted = sqlx::query!(r#"DELETE FROM "users" WHERE "user_id" = ?"#, id)
        .execute(&mut *tx)
        .await?
        .rows_affected()
        > 0;
    if !deleted {
        return Ok(None);
    }

    let now = TimeString::now().to_string();
    sqlx::query!(
        r#"INSERT INTO "deleted_users" ("user_id", "legacy_v3_id", "deleted_at")
           VALUES (?, ?, ?)
           ON CONFLICT ("user_id") DO NOTHING"#,
        id,
        doomed.legacy_v3_id,
        now
    )
    .execute(&mut *tx)
    .await?;
    Ok(Some(addresses))
}

/// The accounts deleted, as a session may name them: by account, or, in a
/// session copied from orrery that hasn't been used here yet, by the address
/// it signed in with (`auth_username`). Such a session would otherwise sign
/// in as whichever account next has that address.
#[derive(Debug, Default)]
struct Gone {
    accounts: HashSet<Ulid>,
    usernames: HashSet<String>,
}

impl Gone {
    /// Delete the sessions signed in as an account that's gone.
    async fn delete_sessions(&self) -> Result<(), Error> {
        if self.accounts.is_empty() {
            return Ok(());
        }

        let mut after = 0_i64;
        loop {
            let batch = sqlx::query!(
                r#"SELECT "rowid" AS "rowid!: i64", "data" FROM "sessions"
                   WHERE "rowid" > ? ORDER BY "rowid" LIMIT 5000"#,
                after
            )
            .fetch_all(db())
            .await?;
            let Some(last) = batch.last() else {
                return Ok(());
            };
            after = last.rowid;

            let signed_in: Vec<i64> = batch
                .iter()
                .filter(|row| self.signs_in(&row.data))
                .map(|row| row.rowid)
                .collect();
            for rowid in signed_in {
                sqlx::query!(r#"DELETE FROM "sessions" WHERE "rowid" = ?"#, rowid)
                    .execute(db())
                    .await?;
            }
        }
    }

    /// Whether the session `data` signs in as an account that's gone.
    fn signs_in(&self, data: &[u8]) -> bool {
        match SignedInAs::of(data) {
            Some(SignedInAs::Account(user_id)) => self.accounts.contains(&user_id),
            Some(SignedInAs::Username(username)) => self.usernames.contains(&username),
            None => false,
        }
    }
}

/// Who a session is signed in as, read as the frontend reads it: its
/// `basic_info`, or failing that the `auth_username` it signs in by.
#[derive(Debug, PartialEq, Eq)]
enum SignedInAs {
    Account(Ulid),
    Username(String),
}

impl SignedInAs {
    fn of(data: &[u8]) -> Option<SignedInAs> {
        let data: BTreeMap<String, serde_json::Value> = flexbuffers::from_slice(data).ok()?;
        match data.get("basic_info") {
            Some(info) => Some(SignedInAs::Account(
                info.get("user_id")?.as_str()?.parse().ok()?,
            )),
            None => Some(SignedInAs::Username(
                data.get("auth_username")?.as_str()?.to_string(),
            )),
        }
    }
}

/// `user.deleted.since`: the deletions after the one `after`, in order, a
/// page at a time.
pub async fn deleted_since(req: &Request) -> Result<UserDeletionsResponse, Error> {
    let request: UserDeletionsRequest = decode_structured_message(req.message.clone())?;
    let rows = sqlx::query!(
        r#"SELECT "seq" AS "seq!: i64", "user_id" FROM "deleted_users"
           WHERE "seq" > ? ORDER BY "seq" LIMIT ?"#,
        request.after,
        PAGE
    )
    .fetch_all(db())
    .await?;

    let deletions = rows
        .into_iter()
        .map(|row| {
            Ok(UserDeletion {
                seq: row.seq,
                user_id: row.user_id.parse()?,
            })
        })
        .collect::<Result<_, Error>>()?;
    Ok(UserDeletionsResponse { deletions })
}

/// Delete the accounts orrery has deleted, once a day until hydra becomes
/// the primary site; from then on, accounts are hydra's own.
pub async fn sweep_orrery_deletions() {
    loop {
        match crate::primary_since::read(db()).await {
            Ok(Some(_)) => {
                println!("Hydra is primary; no longer deleting the accounts orrery deletes");
                return;
            }
            Ok(None) => match sweep().await {
                Ok(deleted) => {
                    println!("Deleted {deleted} accounts orrery no longer has");
                    tokio::time::sleep(SWEEP_EVERY).await;
                }
                Err(e) => {
                    eprintln!("Deleting the accounts orrery deleted failed: {e:#}");
                    tokio::time::sleep(AFTER_FAILURE).await;
                }
            },
            Err(e) => {
                eprintln!("Reading whether hydra is primary failed: {e:#}");
                tokio::time::sleep(AFTER_FAILURE).await;
            }
        }
    }
}

/// Delete the accounts copied from orrery that orrery no longer has; the
/// number deleted.
async fn sweep() -> Result<usize, Error> {
    // The copies are listed before orrery's ids are read, so that an account
    // copied meanwhile isn't taken for one orrery lacks.
    let copied = sqlx::query!(
        r#"SELECT "user_id", "legacy_v3_id" AS "legacy_v3_id!: i64" FROM "users"
           WHERE "legacy_v3_id" IS NOT NULL"#
    )
    .fetch_all(db())
    .await?
    .into_iter()
    .map(|row| Ok((row.legacy_v3_id, row.user_id.parse::<Ulid>()?)))
    .collect::<Result<HashMap<i64, Ulid>, Error>>()?;

    let present = orrery_ids().await?;
    let doomed = absent(&copied, &present);

    let allowance = misc::get::<i64>(ALLOWANCE).await?.unwrap_or(0).max(0) as usize;
    let allowed = allowed(copied.len(), doomed.len(), allowance)?;

    let deleted = delete_users(&doomed).await?;
    if allowed == Allowed::ByAllowance {
        misc::del(ALLOWANCE).await?;
    }
    Ok(deleted)
}

/// Why a sweep may delete what it would.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Allowed {
    /// Within what a sweep may always delete.
    WithinLimit,
    /// Beyond that, but within what staff allowed.
    ByAllowance,
}

/// Whether a sweep of `copies` accounts may delete `absent` of them, given
/// staff's `allowance`; why not, if it may not.
fn allowed(copies: usize, absent: usize, allowance: usize) -> Result<Allowed, Error> {
    let limit = ALWAYS_DELETABLE.max(copies / SWEEP_LIMIT);
    if absent <= limit {
        Ok(Allowed::WithinLimit)
    } else if absent <= allowance {
        Ok(Allowed::ByAllowance)
    } else {
        Err(anyhow!(
            "orrery's account ids would delete {absent} of {copies} accounts, more than the \
             {limit} a sweep may (or the {allowance} staff allowed)"
        ))
    }
}

/// The accounts among `copied` (by orrery's id) that orrery's ids
/// (`present`) lack.
fn absent(copied: &HashMap<i64, Ulid>, present: &HashSet<i64>) -> Vec<Doomed> {
    let mut doomed: Vec<Doomed> = copied
        .iter()
        .filter(|(orrery_id, _)| !present.contains(orrery_id))
        .map(|(&orrery_id, &user_id)| Doomed {
            user_id,
            legacy_v3_id: Some(orrery_id),
        })
        .collect();
    doomed.sort_by_key(|doomed| doomed.legacy_v3_id);
    doomed
}

/// Every account id orrery has. Fails rather than answer with part of them.
async fn orrery_ids() -> Result<HashSet<i64>, Error> {
    let client = reqwest::Client::new();
    let mut present = HashSet::new();
    let mut last_id = 0_i64;

    loop {
        let page: Vec<i64> = client
            .get("https://scistarter.org/staff/sync/user_ids")
            .bearer_auth(env!("SYNC_TOKEN"))
            .query(&[("last_id", last_id.to_string())])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        let Some(&last) = page.last() else {
            return Ok(present);
        };
        if page.len() > ORRERY_IDS_PAGE
            || page.first().is_some_and(|&first| first <= last_id)
            || page.windows(2).any(|pair| pair[0] >= pair[1])
        {
            return Err(anyhow!(
                "orrery's account ids after {last_id} are out of order"
            ));
        }

        present.extend(page);
        last_id = last;
    }
}

#[cfg(test)]
mod tests {
    use assert2::check;

    use super::*;

    #[test]
    fn a_sweep_deletes_no_more_than_its_share_unless_staff_allow_it() {
        check!(allowed(300_000, 2_999, 0).unwrap() == Allowed::WithinLimit);
        check!(allowed(300_000, 3_001, 0).is_err());
        check!(allowed(300_000, 14_721, 15_000).unwrap() == Allowed::ByAllowance);
        check!(allowed(300_000, 15_001, 15_000).is_err());
        // However few accounts there are, a few may go.
        check!(allowed(500, 100, 0).unwrap() == Allowed::WithinLimit);
    }

    #[test]
    fn the_accounts_orrery_lacks_are_doomed_in_its_order() {
        let a = Ulid::from_parts(1, 1);
        let b = Ulid::from_parts(2, 2);
        let c = Ulid::from_parts(3, 3);
        let copied = HashMap::from([(30, c), (10, a), (20, b)]);
        let doomed = absent(&copied, &HashSet::from([20]));
        check!(
            doomed
                == vec![
                    Doomed {
                        user_id: a,
                        legacy_v3_id: Some(10)
                    },
                    Doomed {
                        user_id: c,
                        legacy_v3_id: Some(30)
                    },
                ]
        );
    }

    #[test]
    fn a_session_is_signed_in_as_its_account_or_else_its_username() {
        let user = Ulid::from_parts(7, 7);
        let encode = |data: serde_json::Value| {
            let data: BTreeMap<String, serde_json::Value> = serde_json::from_value(data).unwrap();
            flexbuffers::to_vec(&data).unwrap()
        };

        let signed_in = encode(serde_json::json!({
            "basic_info": { "user_id": user.to_string(), "handle": "ada" },
            "auth_username": "someone@example.org",
        }));
        check!(SignedInAs::of(&signed_in) == Some(SignedInAs::Account(user)));

        let copied = encode(serde_json::json!({ "auth_username": "ada@example.org" }));
        check!(
            SignedInAs::of(&copied) == Some(SignedInAs::Username("ada@example.org".to_string()))
        );
        check!(SignedInAs::of(&encode(serde_json::json!({}))).is_none());

        let gone = Gone {
            accounts: HashSet::from([user]),
            usernames: HashSet::from(["ada@example.org".to_string()]),
        };
        check!(gone.signs_in(&signed_in));
        check!(gone.signs_in(&copied));
        check!(!gone.signs_in(&encode(
            serde_json::json!({ "auth_username": "bob@example.org" })
        )));
    }
}
