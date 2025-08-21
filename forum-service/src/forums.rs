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

pub async fn create_forum(req: &Request) -> Result<ForumInfo> {
    let message: CreateForum = flexbuffers::from_slice(&req.message.payload)?;
    let user_id = message.user_id;
    
    // Get user info to check permissions - only staff can create forums
    let user_info: BasicInfo = request("user.basic_info.by_user_id", user_id.to_string()).await?;
    if !user_info.is_staff() {
        return Err(anyhow::anyhow!("Permission denied"));
    }

    let forum_id = Ulid::new();
    let forum_id_str = forum_id.to_string();
    let now = TimeString::now();
    let now_str = now.to_string();
    let display_order = message.display_order.unwrap_or(0);

    let db = db();

    // Check if slug already exists
    let existing = sqlx::query!(
        r#"SELECT "forum_id" FROM "forums" WHERE "slug" = ?"#,
        message.slug
    )
    .fetch_optional(db)
    .await?;

    if existing.is_some() {
        return Err(anyhow::anyhow!("Forum slug already exists"));
    }

    // Insert the forum
    sqlx::query!(
        r#"INSERT INTO "forums" ("forum_id", "slug", "name", "description", "created_at", "display_order")
           VALUES (?, ?, ?, ?, ?, ?)"#,
        forum_id_str,
        message.slug,
        message.name,
        message.description,
        now_str,
        display_order
    )
    .execute(db)
    .await?;

    // Log the activity
    log_activity(
        Some(now),
        "forum_created",
        Some(user_id),
        Some(forum_id),
        Some(serde_json::json!({
            "slug": message.slug,
            "name": message.name
        }).to_string()),
    )
    .await?;

    get_forum_internal(forum_id).await
}

pub async fn get_forum_by_slug(req: &Request) -> Result<ForumInfo> {
    let slug: String = flexbuffers::from_slice(&req.message.payload)?;
    
    let db = db();
    let row = sqlx::query!(
        r#"SELECT "forum_id" FROM "forums" WHERE "slug" = ? AND "is_active" = 1"#,
        slug
    )
    .fetch_optional(db)
    .await?
    .ok_or_else(|| anyhow::anyhow!("Forum not found"))?;

    let forum_id = row.forum_id.ok_or_else(|| anyhow::anyhow!("Forum ID is null"))?.as_ulid()?;
    get_forum_internal(forum_id).await
}

pub async fn get_forum_by_id(req: &Request) -> Result<ForumInfo> {
    let forum_id_str: String = flexbuffers::from_slice(&req.message.payload)?;
    let forum_id = forum_id_str.as_ulid()?;
    get_forum_internal(forum_id).await
}

pub async fn list_forums(req: &Request) -> Result<Vec<ForumInfo>> {
    let query: ForumListQuery = flexbuffers::from_slice(&req.message.payload)?;
    let db = db();
    let limit = query.limit.unwrap_or(50).min(100);
    let offset = query.offset.unwrap_or(0);
    let active_only = query.active_only.unwrap_or(true);

    let rows: Vec<_> = if active_only {
        sqlx::query!(
            r#"SELECT forum_id, slug, name, description, created_at, updated_at, is_active, display_order
               FROM "forums"
               WHERE is_active = 1
               ORDER BY display_order ASC, created_at ASC
               LIMIT ? OFFSET ?"#,
            limit,
            offset
        )
        .fetch_all(db)
        .await?
        .into_iter()
        .map(|row| (row.forum_id, row.slug, row.name, row.description, row.created_at, row.updated_at, row.is_active, row.display_order))
        .collect()
    } else {
        sqlx::query!(
            r#"SELECT forum_id, slug, name, description, created_at, updated_at, is_active, display_order
               FROM "forums"
               ORDER BY display_order ASC, created_at ASC
               LIMIT ? OFFSET ?"#,
            limit,
            offset
        )
        .fetch_all(db)
        .await?
        .into_iter()
        .map(|row| (row.forum_id, row.slug, row.name, row.description, row.created_at, row.updated_at, row.is_active, row.display_order))
        .collect()
    };

    let mut forums = Vec::new();
    for row in rows {
        let forum_id_str = row.0.clone().ok_or_else(|| anyhow::anyhow!("Forum ID is null"))?;
        let forum_id = forum_id_str.as_ulid()?;
        
        // Get topic count for this forum
        let topic_count = sqlx::query_scalar!(
            r#"SELECT COUNT(*) FROM "topics" WHERE "forum_id" = ? AND "is_deleted" = 0"#,
            forum_id_str
        )
        .fetch_one(db)
        .await?;

        forums.push(ForumInfo {
            forum_id,
            slug: row.1,
            name: row.2,
            description: row.3,
            created_at: TimeString::try_from(row.4)?,
            updated_at: row.5.as_ref().map(|s| TimeString::try_from(s.clone())).transpose()?,
            is_active: row.6 == 1,
            display_order: row.7 as i32,
            topic_count,
        });
    }

    Ok(forums)
}

pub async fn update_forum(req: &Request) -> Result<ForumInfo> {
    let message: UpdateForum = flexbuffers::from_slice(&req.message.payload)?;
    let user_id = message.user_id;
    
    // Get user info to check permissions - only staff can update forums
    let user_info: BasicInfo = request("user.basic_info.by_user_id", user_id.to_string()).await?;
    if !user_info.is_staff() {
        return Err(anyhow::anyhow!("Permission denied"));
    }

    let forum_id_str = message.forum_id.to_string();
    let now = TimeString::now();
    let now_str = now.to_string();
    let db = db();

    // Get current forum to merge updates
    let current = get_forum_internal(message.forum_id).await?;
    
    let new_name = message.name.unwrap_or(current.name);
    let new_description = message.description.or(current.description);
    let new_display_order = message.display_order.unwrap_or(current.display_order);
    let new_is_active = if message.is_active.unwrap_or(current.is_active) { 1 } else { 0 };

    // Update the forum with all fields
    sqlx::query!(
        r#"UPDATE "forums" SET "name" = ?, "description" = ?, "display_order" = ?, "is_active" = ?, "updated_at" = ?
           WHERE "forum_id" = ?"#,
        new_name,
        new_description,
        new_display_order,
        new_is_active,
        now_str,
        forum_id_str
    )
    .execute(db)
    .await?;

    // Log the activity
    log_activity(
        Some(now),
        "forum_updated",
        Some(user_id),
        Some(message.forum_id),
        Some(serde_json::json!({
            "forum_name": new_name
        }).to_string()),
    )
    .await?;

    get_forum_internal(message.forum_id).await
}

async fn get_forum_internal(forum_id: Ulid) -> Result<ForumInfo> {
    let db = db();
    let forum_id_str = forum_id.to_string();

    let row = sqlx::query!(
        r#"SELECT forum_id, slug, name, description, created_at, updated_at, is_active, display_order
           FROM "forums"
           WHERE forum_id = ?"#,
        forum_id_str
    )
    .fetch_optional(db)
    .await?
    .ok_or_else(|| anyhow::anyhow!("Forum not found"))?;

    // Get topic count for this forum
    let topic_count = sqlx::query_scalar!(
        r#"SELECT COUNT(*) FROM "topics" WHERE "forum_id" = ? AND "is_deleted" = 0"#,
        forum_id_str
    )
    .fetch_one(db)
    .await?;

    Ok(ForumInfo {
        forum_id,
        slug: row.slug,
        name: row.name,
        description: row.description,
        created_at: TimeString::try_from(row.created_at)?,
        updated_at: row.updated_at.as_ref().map(|s| TimeString::try_from(s.clone())).transpose()?,
        is_active: row.is_active == 1,
        display_order: row.display_order as i32,
        topic_count,
    })
}