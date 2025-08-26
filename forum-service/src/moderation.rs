use anyhow::Result;
use async_nats::service::Request;
use common::timestring::TimeString;
use common::{
    message::{
        forum::*,
        user::{BasicInfo, UserInfo},
    },
    service::{log_activity, request},
    shared::db,
    ulid::AsUlid,
};
use ulid::Ulid;

use crate::forums::check_forum_management_permission;

pub async fn moderate_content(req: &Request) -> Result<()> {
    let action: ModerationAction = flexbuffers::from_slice(&req.message.payload)?;
    let user_id = action.user_id;

    let db = db();
    let target_id_str = action.target_id.to_string();

    // Get forum_id based on target type and ID
    let forum_id = match action.target_type {
        ModerationTarget::Topic => {
            let row = sqlx::query!(
                r#"SELECT "forum_id" FROM "topics" WHERE "topic_id" = ?"#,
                target_id_str
            )
            .fetch_one(db)
            .await?;
            row.forum_id
                .ok_or_else(|| anyhow::anyhow!("Topic has no associated forum"))?
        }
        ModerationTarget::Post => {
            let row = sqlx::query!(
                r#"SELECT t."forum_id" FROM "posts" p 
                   JOIN "topics" t ON p."topic_id" = t."topic_id" 
                   WHERE p."post_id" = ?"#,
                target_id_str
            )
            .fetch_one(db)
            .await?;
            row.forum_id
                .ok_or_else(|| anyhow::anyhow!("Post's topic has no associated forum"))?
        }
    };

    // Check permissions for this specific forum
    let forum_id_ulid = forum_id.as_ulid()?;
    if !check_forum_management_permission(forum_id_ulid, Some(user_id)).await? {
        return Err(anyhow::anyhow!("Permission denied"));
    }

    let now = TimeString::now();
    let log_id = Ulid::new();

    match action.target_type {
        ModerationTarget::Topic => {
            match action.action {
                ModerationActionType::Delete => {
                    sqlx::query!(
                        r#"UPDATE "topics" SET "is_deleted" = 1 WHERE "topic_id" = ?"#,
                        target_id_str
                    )
                    .execute(db)
                    .await?;
                }
                ModerationActionType::Pin => {
                    sqlx::query!(
                        r#"UPDATE "topics" SET "is_pinned" = 1 WHERE "topic_id" = ?"#,
                        target_id_str
                    )
                    .execute(db)
                    .await?;
                }
                ModerationActionType::Unpin => {
                    sqlx::query!(
                        r#"UPDATE "topics" SET "is_pinned" = 0 WHERE "topic_id" = ?"#,
                        target_id_str
                    )
                    .execute(db)
                    .await?;
                }
                ModerationActionType::Lock => {
                    sqlx::query!(
                        r#"UPDATE "topics" SET "is_locked" = 1 WHERE "topic_id" = ?"#,
                        target_id_str
                    )
                    .execute(db)
                    .await?;
                }
                ModerationActionType::Unlock => {
                    sqlx::query!(
                        r#"UPDATE "topics" SET "is_locked" = 0 WHERE "topic_id" = ?"#,
                        target_id_str
                    )
                    .execute(db)
                    .await?;
                }
                ModerationActionType::Flag => {
                    sqlx::query!(
                        r#"UPDATE "topics" SET "is_flagged" = 1 WHERE "topic_id" = ?"#,
                        target_id_str
                    )
                    .execute(db)
                    .await?;
                }
                ModerationActionType::Unflag => {
                    sqlx::query!(
                        r#"UPDATE "topics" SET "is_flagged" = 0 WHERE "topic_id" = ?"#,
                        target_id_str
                    )
                    .execute(db)
                    .await?;
                }
            }
        }
        ModerationTarget::Post => match action.action {
            ModerationActionType::Delete => {
                sqlx::query!(
                    r#"UPDATE "posts" SET "is_deleted" = 1 WHERE "post_id" = ?"#,
                    target_id_str
                )
                .execute(db)
                .await?;
            }
            ModerationActionType::Flag => {
                sqlx::query!(
                    r#"UPDATE "posts" SET "is_flagged" = 1 WHERE "post_id" = ?"#,
                    target_id_str
                )
                .execute(db)
                .await?;
            }
            ModerationActionType::Unflag => {
                sqlx::query!(
                    r#"UPDATE "posts" SET "is_flagged" = 0 WHERE "post_id" = ?"#,
                    target_id_str
                )
                .execute(db)
                .await?;
            }
            _ => {
                return Err(anyhow::anyhow!("Action not supported for posts"));
            }
        },
    }

    // Log the moderation action
    let target_type_str = match action.target_type {
        ModerationTarget::Topic => "topic",
        ModerationTarget::Post => "post",
    };

    let action_str = match action.action {
        ModerationActionType::Delete => "delete",
        ModerationActionType::Pin => "pin",
        ModerationActionType::Unpin => "unpin",
        ModerationActionType::Lock => "lock",
        ModerationActionType::Unlock => "unlock",
        ModerationActionType::Flag => "flag",
        ModerationActionType::Unflag => "unflag",
    };

    let log_id_str = log_id.to_string();
    let user_id_str = user_id.to_string();
    let now_str = now.to_string();

    sqlx::query!(
        r#"INSERT INTO "moderation_log" ("log_id", "moderator_id", "target_type", "target_id", "action", "reason", "created_at")
           VALUES (?, ?, ?, ?, ?, ?, ?)"#,
        log_id_str,
        user_id_str,
        target_type_str,
        target_id_str,
        action_str,
        action.reason,
        now_str
    )
    .execute(db)
    .await?;

    // Log the activity
    log_activity(
        Some(now),
        "moderation_action",
        Some(user_id),
        Some(action.target_id),
        Some(
            serde_json::json!({
                "target_type": target_type_str,
                "action": action_str,
                "reason": action.reason
            })
            .to_string(),
        ),
    )
    .await?;

    Ok(())
}

#[derive(Debug)]
struct ModerationLogRow {
    log_id: Option<String>,
    moderator_id: Option<String>,
    target_type: Option<String>,
    target_id: Option<String>,
    action: Option<String>,
    reason: Option<String>,
    created_at: Option<String>,
}

#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub struct ModerationLogEntry {
    pub log_id: Ulid,
    pub moderator_id: Ulid,
    pub moderator_handle: String,
    pub target_type: String,
    pub target_id: Ulid,
    pub action: String,
    pub reason: Option<String>,
    pub created_at: TimeString,
}

pub async fn get_moderation_log(req: &Request) -> Result<Vec<ModerationLogEntry>> {
    let (user_id, query): (Ulid, (Option<String>, Option<i64>, Option<i64>)) =
        flexbuffers::from_slice(&req.message.payload)?;

    // Get user info to check permissions
    let user_info: BasicInfo = request("user.basic_info.by_user_id", user_id.to_string()).await?;
    if !user_info.is_staff() {
        return Err(anyhow::anyhow!("Permission denied"));
    }

    let (target_id, limit, offset) = query;
    let limit = limit.unwrap_or(50).min(100);
    let offset = offset.unwrap_or(0);

    let db = db();

    let rows = if let Some(target_id) = target_id {
        sqlx::query_as!(
            ModerationLogRow,
            r#"SELECT log_id, moderator_id, target_type, target_id, action, reason, created_at
               FROM "moderation_log"
               WHERE target_id = ?
               ORDER BY created_at DESC
               LIMIT ? OFFSET ?"#,
            target_id,
            limit,
            offset
        )
        .fetch_all(db)
        .await?
    } else {
        sqlx::query_as!(
            ModerationLogRow,
            r#"SELECT log_id, moderator_id, target_type, target_id, action, reason, created_at
               FROM "moderation_log"
               ORDER BY created_at DESC
               LIMIT ? OFFSET ?"#,
            limit,
            offset
        )
        .fetch_all(db)
        .await?
    };

    let mut log_entries = Vec::new();
    for row in rows {
        // Get moderator info from user-service
        let moderator_id_str = row
            .moderator_id
            .ok_or_else(|| anyhow::anyhow!("Missing moderator_id"))?;
        let moderator_info: BasicInfo =
            request("user.basic_info.by_user_id", moderator_id_str).await?;

        log_entries.push(ModerationLogEntry {
            log_id: row
                .log_id
                .ok_or_else(|| anyhow::anyhow!("Missing log_id"))?
                .as_ulid()?,
            moderator_id: moderator_info.user_id,
            moderator_handle: moderator_info.handle,
            target_type: row
                .target_type
                .ok_or_else(|| anyhow::anyhow!("Missing target_type"))?,
            target_id: row
                .target_id
                .ok_or_else(|| anyhow::anyhow!("Missing target_id"))?
                .as_ulid()?,
            action: row
                .action
                .ok_or_else(|| anyhow::anyhow!("Missing action"))?,
            reason: row.reason,
            created_at: TimeString::try_from(
                row.created_at
                    .ok_or_else(|| anyhow::anyhow!("Missing created_at"))?,
            )?,
        });
    }

    Ok(log_entries)
}

pub async fn add_forum_moderator(req: &Request) -> Result<()> {
    let action: AddForumModerator = flexbuffers::from_slice(&req.message.payload)?;

    let requester_info: Option<BasicInfo> = request(
        "user.basic_info.by_user_id",
        action.requester_id.to_string(),
    )
    .await?;
    let requester_info = requester_info.ok_or_else(|| anyhow::anyhow!("Requester user not found"))?;
    if !requester_info.is_staff() {
        return Err(anyhow::anyhow!(
            "Permission denied: only staff can manage forum moderators"
        ));
    }

    let user_info: Option<BasicInfo> =
        request("user.basic_info.by_user_id", action.user_id.to_string()).await?;
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("Target user not found"))?;
    if user_info.is_staff() {
        return Err(anyhow::anyhow!(
            "Staff members cannot be added as forum-specific moderators"
        ));
    }

    let db = db();
    let forum_id_str = action.forum_id.to_string();
    let user_id_str = action.user_id.to_string();
    let now = TimeString::now().to_string();

    sqlx::query!(
        r#"INSERT INTO "forum_moderators" ("forum_id", "user_id", "created_at") VALUES (?, ?, ?)"#,
        forum_id_str,
        user_id_str,
        now
    )
    .execute(db)
    .await?;

    log_activity(
        Some(TimeString::now()),
        "forum_moderator_added",
        Some(action.requester_id),
        Some(action.forum_id),
        Some(
            serde_json::json!({
                "moderator_user_id": action.user_id.to_string(),
                "moderator_handle": user_info.handle
            })
            .to_string(),
        ),
    )
    .await?;

    Ok(())
}

pub async fn remove_forum_moderator(req: &Request) -> Result<()> {
    let action: RemoveForumModerator = flexbuffers::from_slice(&req.message.payload)?;

    let requester_info: Option<BasicInfo> = request(
        "user.basic_info.by_user_id",
        action.requester_id.to_string(),
    )
    .await?;
    let requester_info = requester_info.ok_or_else(|| anyhow::anyhow!("Requester user not found"))?;
    if !requester_info.is_staff() {
        return Err(anyhow::anyhow!(
            "Permission denied: only staff can manage forum moderators"
        ));
    }

    let user_info: Option<BasicInfo> =
        request("user.basic_info.by_user_id", action.user_id.to_string()).await?;
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("Target user not found"))?;

    let db = db();
    let forum_id_str = action.forum_id.to_string();
    let user_id_str = action.user_id.to_string();

    let result = sqlx::query!(
        r#"DELETE FROM "forum_moderators" WHERE "forum_id" = ? AND "user_id" = ?"#,
        forum_id_str,
        user_id_str
    )
    .execute(db)
    .await?;

    if result.rows_affected() == 0 {
        return Err(anyhow::anyhow!("User is not a moderator of this forum"));
    }

    log_activity(
        Some(TimeString::now()),
        "forum_moderator_removed",
        Some(action.requester_id),
        Some(action.forum_id),
        Some(
            serde_json::json!({
                "moderator_user_id": action.user_id.to_string(),
                "moderator_handle": user_info.handle
            })
            .to_string(),
        ),
    )
    .await?;

    Ok(())
}

#[derive(Debug)]
struct ForumModeratorRow {
    user_id: Option<String>,
    created_at: Option<String>,
}

pub async fn list_forum_moderators(req: &Request) -> Result<Vec<ForumModeratorInfo>> {
    let (requester_id, forum_id): (Ulid, Ulid) = flexbuffers::from_slice(&req.message.payload)?;

    let requester_info: BasicInfo =
        request("user.basic_info.by_user_id", requester_id.to_string()).await?;
    if !requester_info.is_staff() {
        return Err(anyhow::anyhow!(
            "Permission denied: only staff can view forum moderators"
        ));
    }

    let db = db();
    let forum_id_str = forum_id.to_string();

    let rows = sqlx::query_as!(
        ForumModeratorRow,
        r#"SELECT "user_id", "created_at" FROM "forum_moderators" WHERE "forum_id" = ? ORDER BY "created_at" ASC"#,
        forum_id_str
    )
    .fetch_all(db)
    .await?;

    let mut moderators = Vec::new();
    for row in rows {
        let user_id_str = row
            .user_id
            .ok_or_else(|| anyhow::anyhow!("Missing user_id"))?;
        let user_info: BasicInfo = request("user.basic_info.by_user_id", user_id_str).await?;

        moderators.push(ForumModeratorInfo {
            forum_id,
            user_id: user_info.user_id,
            user_handle: user_info.handle,
            created_at: TimeString::try_from(
                row.created_at
                    .ok_or_else(|| anyhow::anyhow!("Missing created_at"))?,
            )?,
        });
    }

    Ok(moderators)
}
