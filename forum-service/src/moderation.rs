use anyhow::Result;
use async_nats::service::Request;
use common::{
    message::{forum::*, user::{BasicInfo, UserInfo}},
    service::{log_activity, request},
    shared::db,
    ulid::AsUlid,
};
use common::timestring::TimeString;
use ulid::Ulid;

pub async fn moderate_content(req: &Request) -> Result<()> {
    let action: ModerationAction = flexbuffers::from_slice(&req.message.payload)?;
    let user_id = action.user_id;
    
    // Get user info to check permissions
    let user_info: BasicInfo = request("user.basic_info.by_user_id", user_id.to_string()).await?;
    if !user_info.is_staff() {
        return Err(anyhow::anyhow!("Permission denied"));
    }

    let now = TimeString::now();
    let log_id = Ulid::new();
    let target_id_str = action.target_id.to_string();
    let db = db();

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
                },
                ModerationActionType::Pin => {
                    sqlx::query!(
                        r#"UPDATE "topics" SET "is_pinned" = 1 WHERE "topic_id" = ?"#,
                        target_id_str
                    )
                    .execute(db)
                    .await?;
                },
                ModerationActionType::Unpin => {
                    sqlx::query!(
                        r#"UPDATE "topics" SET "is_pinned" = 0 WHERE "topic_id" = ?"#,
                        target_id_str
                    )
                    .execute(db)
                    .await?;
                },
                ModerationActionType::Lock => {
                    sqlx::query!(
                        r#"UPDATE "topics" SET "is_locked" = 1 WHERE "topic_id" = ?"#,
                        target_id_str
                    )
                    .execute(db)
                    .await?;
                },
                ModerationActionType::Unlock => {
                    sqlx::query!(
                        r#"UPDATE "topics" SET "is_locked" = 0 WHERE "topic_id" = ?"#,
                        target_id_str
                    )
                    .execute(db)
                    .await?;
                },
                ModerationActionType::Flag => {
                    // Flag doesn't change the topic state, just logs the action
                },
            }
        },
        ModerationTarget::Post => {
            match action.action {
                ModerationActionType::Delete => {
                    sqlx::query!(
                        r#"UPDATE "posts" SET "is_deleted" = 1 WHERE "post_id" = ?"#,
                        target_id_str
                    )
                    .execute(db)
                    .await?;
                },
                _ => {
                    return Err(anyhow::anyhow!("Action not supported for posts"));
                }
            }
        }
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
        Some(serde_json::json!({
            "target_type": target_type_str,
            "action": action_str,
            "reason": action.reason
        }).to_string()),
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
    let (user_id, query): (Ulid, (Option<String>, Option<i64>, Option<i64>)) = flexbuffers::from_slice(&req.message.payload)?;
    
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
        let moderator_id_str = row.moderator_id.ok_or_else(|| anyhow::anyhow!("Missing moderator_id"))?;
        let moderator_info: BasicInfo = request("user.basic_info.by_user_id", moderator_id_str).await?;

        log_entries.push(ModerationLogEntry {
            log_id: row.log_id.ok_or_else(|| anyhow::anyhow!("Missing log_id"))?.as_ulid()?,
            moderator_id: moderator_info.user_id,
            moderator_handle: moderator_info.handle,
            target_type: row.target_type.ok_or_else(|| anyhow::anyhow!("Missing target_type"))?,
            target_id: row.target_id.ok_or_else(|| anyhow::anyhow!("Missing target_id"))?.as_ulid()?,
            action: row.action.ok_or_else(|| anyhow::anyhow!("Missing action"))?,
            reason: row.reason,
            created_at: TimeString::try_from(row.created_at.ok_or_else(|| anyhow::anyhow!("Missing created_at"))?)?,
        });
    }

    Ok(log_entries)
}