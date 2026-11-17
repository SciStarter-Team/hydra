use anyhow::Error;
use async_nats::service::Request;
use common::{
    message::user::{
        BookmarkCheckRequest, BookmarkCheckResponse, BookmarkCreateRequest, BookmarkCreateResponse,
        BookmarkDeleteRequest, EntityCountsRequest, EntityCountsResponse,
    },
    service::{decode_structured_message, log_activity},
    shared::db,
    timestring::TimeString,
    transaction::Immediate,
};
use std::collections::HashSet;

pub async fn create_bookmark(req: &Request) -> Result<BookmarkCreateResponse, Error> {
    let message: BookmarkCreateRequest = decode_structured_message(req.message.clone())?;

    let user_id_str = message.user_id.to_string();
    let now = TimeString::now();
    let now_str = now.to_string();

    // Saving what is already saved only marks it active again; a new save
    // is starred, as orrery starred them.
    sqlx::query!(
        r#"
        INSERT INTO bookmarks (user_id, project_id, created, activity, starred)
        VALUES (?, ?, ?, ?, 1)
        ON CONFLICT(user_id, project_id)
        DO UPDATE SET activity = ?
        "#,
        user_id_str,
        message.project_id,
        now_str,
        now_str,
        now_str
    )
    .execute(db())
    .await?;

    log_activity(
        Some(now),
        "bookmark_create",
        Some(message.user_id),
        None::<ulid::Ulid>,
        Some(message.project_id),
    )
    .await?;

    Ok(BookmarkCreateResponse {
        success: true,
        message: "Bookmark created successfully".to_string(),
    })
}

/// `user.bookmarks.delete`: withdraw a save.
pub async fn delete_bookmark(req: &Request) -> Result<bool, Error> {
    let message: BookmarkDeleteRequest = decode_structured_message(req.message.clone())?;

    let user_id = message.user_id.to_string();
    let removed = sqlx::query!(
        r#"DELETE FROM "bookmarks" WHERE "user_id" = ? AND "project_id" = ?"#,
        user_id,
        message.project_id,
    )
    .execute(db())
    .await?
    .rows_affected()
        > 0;

    if removed {
        log_activity(
            Some(TimeString::now()),
            "bookmark_delete",
            Some(message.user_id),
            None::<ulid::Ulid>,
            Some(message.project_id),
        )
        .await?;
    }

    Ok(removed)
}

pub async fn list_by_user(req: &Request) -> Result<Vec<String>, Error> {
    let user_id: String = flexbuffers::from_slice(&req.message.payload)?;

    let project_ids = sqlx::query_scalar!(
        r#"SELECT "project_id"
        FROM "bookmarks"
        WHERE "user_id" = ?
        ORDER BY "activity" DESC"#,
        user_id
    )
    .fetch_all(db())
    .await?;

    Ok(project_ids)
}

/// How many users have saved each of the requested projects. Serves the dashboard's owned-entity "saves" figures.
pub async fn count_by_projects(req: &Request) -> Result<EntityCountsResponse, Error> {
    let message: EntityCountsRequest = decode_structured_message(req.message.clone())?;

    let mut counts = Vec::new();
    for entity_id in &message.entity_ids {
        let count = sqlx::query_scalar!(
            r#"SELECT COUNT(*) as "count!: i64"
            FROM "bookmarks"
            WHERE "project_id" = ?"#,
            entity_id
        )
        .fetch_one(db())
        .await?;

        if count > 0 {
            counts.push((entity_id.clone(), count));
        }
    }

    Ok(EntityCountsResponse { counts })
}

pub async fn check_bookmark(req: &Request) -> Result<BookmarkCheckResponse, Error> {
    let message: BookmarkCheckRequest = decode_structured_message(req.message.clone())?;

    let user_id_str = message.user_id.to_string();

    let result = sqlx::query_scalar!(
        r#"
        SELECT 1 as "result: i32"
        FROM bookmarks
        WHERE user_id = ? AND project_id = ?
        LIMIT 1
        "#,
        user_id_str,
        message.project_id
    )
    .fetch_optional(db())
    .await?;

    Ok(BookmarkCheckResponse {
        has_bookmark: result.is_some(),
    })
}

/// One save orrery holds, as `/staff/sync/bookmark_keys` lists it.
#[derive(Debug, serde::Deserialize)]
struct OrreryBookmarkKey {
    id: i64,
    user_ulid: String,
    entity_ulid: String,
}

/// How many keys orrery sends a page.
const ORRERY_KEYS_PAGE: usize = 5000;

/// The most saves one pass may withdraw, as a share of all of them. Orrery's
/// members withdraw a handful a day; a pass that would take more than this
/// is taken to have been sent a broken list, and withdraws nothing.
const MOST_WITHDRAWN: f64 = 0.01;

/// The fewest a pass may always withdraw, however few saves there are.
const ALWAYS_WITHDRAWABLE: usize = 100;

/// Withdraw the saves orrery has withdrawn, once a day until hydra becomes
/// the primary site.
///
/// Orrery deletes a withdrawn save, which its bookmark feed cannot show, so
/// each pass reads every save orrery holds and withdraws those here that
/// orrery no longer has. A save made or changed since the pass began is left
/// alone, since orrery's list may predate it. From cutover on, saves are
/// hydra's own, and orrery's list is no longer the record of them.
pub async fn reconcile_with_orrery() {
    loop {
        match crate::primary_since::read(db()).await {
            Ok(Some(_)) => {
                println!("Hydra is primary; no longer reconciling saves with orrery");
                return;
            }
            Ok(None) => match reconcile_once().await {
                Ok(withdrawn) => println!("Reconciled saves with orrery: {withdrawn} withdrawn"),
                Err(e) => eprintln!("Reconciling saves with orrery failed: {e:#}"),
            },
            Err(e) => eprintln!("Reading whether hydra is primary failed: {e:#}"),
        }

        tokio::time::sleep(std::time::Duration::from_secs(24 * 60 * 60)).await;
    }
}

async fn reconcile_once() -> Result<usize, Error> {
    let began = TimeString::now();
    let held = orrery_saves().await?;

    let ours = sqlx::query!(
        r#"SELECT "user_id", "project_id" FROM "bookmarks"
           WHERE unixepoch("activity", 'subsec') < unixepoch(?, 'subsec')"#,
        began,
    )
    .fetch_all(db())
    .await?;

    let total: i64 = sqlx::query_scalar!(r#"SELECT COUNT(*) FROM "bookmarks""#)
        .fetch_one(db())
        .await?;

    let withdrawn: Vec<(String, String)> = ours
        .into_iter()
        .map(|row| (row.user_id, row.project_id))
        .filter(|key| !held.contains(key))
        .collect();

    let most = ALWAYS_WITHDRAWABLE.max((total as f64 * MOST_WITHDRAWN) as usize);
    if withdrawn.len() > most {
        anyhow::bail!(
            "orrery's list would withdraw {} of {total} saves, more than the {most} one pass may",
            withdrawn.len()
        );
    }

    let mut tx = Immediate::begin_with_retry(db(), None, None).await?;
    for (user_id, project_id) in &withdrawn {
        sqlx::query!(
            r#"DELETE FROM "bookmarks"
               WHERE "user_id" = ? AND "project_id" = ?
                 AND unixepoch("activity", 'subsec') < unixepoch(?, 'subsec')"#,
            user_id,
            project_id,
            began,
        )
        .execute(&mut tx)
        .await?;
    }
    tx.commit().await?;

    Ok(withdrawn.len())
}

/// Every save orrery holds, as (account, project). Fails rather than answer
/// with part of the list.
async fn orrery_saves() -> Result<HashSet<(String, String)>, Error> {
    let client = reqwest::Client::new();
    let mut held = HashSet::new();
    let mut after_id = 0;

    loop {
        let page: Vec<OrreryBookmarkKey> = client
            .get("https://scistarter.org/staff/sync/bookmark_keys")
            .bearer_auth(env!("SYNC_TOKEN"))
            .query(&[("after_id", after_id.to_string())])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        let full = page.len() >= ORRERY_KEYS_PAGE;
        for key in page {
            after_id = after_id.max(key.id);
            held.insert((key.user_ulid, key.entity_ulid));
        }

        if !full {
            return Ok(held);
        }
    }
}
