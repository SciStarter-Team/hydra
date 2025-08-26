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

pub async fn check_forum_management_permission(
    forum_id: Ulid,
    user_id: Option<Ulid>,
) -> Result<bool> {
    let Some(user_id) = user_id else {
        return Ok(false);
    };

    // Check if user is staff
    let user_info: BasicInfo = request("user.basic_info.by_user_id", user_id.to_string()).await?;
    if user_info.is_staff() {
        return Ok(true);
    }

    // Check if forum_id equals user_id (personal forum)
    if forum_id == user_id {
        return Ok(true);
    }

    // Check if user is in the manager table for this forum
    let db = db();
    let user_id_str = user_id.to_string();
    let forum_id_str = forum_id.to_string();

    let manager_row = sqlx::query!(
        r#"SELECT "user_id" FROM "forum_moderators" WHERE "forum_id" = ? AND "user_id" = ?"#,
        forum_id_str,
        user_id_str
    )
    .fetch_optional(db)
    .await?;

    Ok(manager_row.is_some())
}

pub async fn create_forum(req: &Request) -> Result<ForumInfo> {
    let message: CreateForum = flexbuffers::from_slice(&req.message.payload)?;
    let user_id = message.user_id;

    // Only staff can create forums (forum_id doesn't exist yet, so we check staff directly)
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

    // Copy categories from general forum to new forum
    copy_general_forum_categories_to_new_forum(forum_id, user_id).await?;

    // Log the activity
    log_activity(
        Some(now),
        "forum_created",
        Some(user_id),
        Some(forum_id),
        Some(
            serde_json::json!({
                "slug": message.slug,
                "name": message.name
            })
            .to_string(),
        ),
    )
    .await?;

    get_forum_internal(forum_id, Some(user_id)).await
}

pub async fn get_forum_by_slug(req: &Request) -> Result<ForumInfo> {
    let message: GetForumBySlug = flexbuffers::from_slice(&req.message.payload)?;

    let db = db();
    let row = sqlx::query!(
        r#"SELECT "forum_id" FROM "forums" WHERE "slug" = ? AND "is_active" = 1"#,
        message.slug
    )
    .fetch_optional(db)
    .await?
    .ok_or_else(|| anyhow::anyhow!("Forum not found"))?;

    let forum_id = row
        .forum_id
        .ok_or_else(|| anyhow::anyhow!("Forum ID is null"))?
        .as_ulid()?;
    get_forum_internal(forum_id, message.user_id).await
}

pub async fn get_forum_by_id(req: &Request) -> Result<ForumInfo> {
    let message: GetForumById = flexbuffers::from_slice(&req.message.payload)?;
    get_forum_internal(message.forum_id, message.user_id).await
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
               WHERE is_active = 1 AND slug NOT LIKE 'user-%'
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
               WHERE slug NOT LIKE 'user-%'
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
        let forum_id_str = row
            .0
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Forum ID is null"))?;
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
            updated_at: row
                .5
                .as_ref()
                .map(|s| TimeString::try_from(s.clone()))
                .transpose()?,
            is_active: row.6 == 1,
            display_order: row.7 as i32,
            topic_count,
            can_manage: false, // list_forums doesn't support user_id currently
        });
    }

    Ok(forums)
}

pub async fn update_forum(req: &Request) -> Result<ForumInfo> {
    let message: UpdateForum = flexbuffers::from_slice(&req.message.payload)?;
    let user_id = message.user_id;

    // Check if user has management permission for this forum
    if !check_forum_management_permission(message.forum_id, Some(user_id)).await? {
        return Err(anyhow::anyhow!("Permission denied"));
    }

    let forum_id_str = message.forum_id.to_string();
    let now = TimeString::now();
    let now_str = now.to_string();
    let db = db();

    // Get current forum to merge updates
    let current = get_forum_internal(message.forum_id, Some(user_id)).await?;

    let new_name = message.name.unwrap_or(current.name);
    let new_description = message.description.or(current.description);
    let new_display_order = message.display_order.unwrap_or(current.display_order);
    let new_is_active = if message.is_active.unwrap_or(current.is_active) {
        1
    } else {
        0
    };

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
        Some(
            serde_json::json!({
                "forum_name": new_name
            })
            .to_string(),
        ),
    )
    .await?;

    get_forum_internal(message.forum_id, Some(user_id)).await
}

async fn get_forum_internal(forum_id: Ulid, user_id: Option<Ulid>) -> Result<ForumInfo> {
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

    // Check management permissions
    let can_manage = check_forum_management_permission(forum_id, user_id).await?;

    Ok(ForumInfo {
        forum_id,
        slug: row.slug,
        name: row.name,
        description: row.description,
        created_at: TimeString::try_from(row.created_at)?,
        updated_at: row
            .updated_at
            .as_ref()
            .map(|s| TimeString::try_from(s.clone()))
            .transpose()?,
        is_active: row.is_active == 1,
        display_order: row.display_order as i32,
        topic_count,
        can_manage,
    })
}

pub async fn get_personal_forum(req: &Request) -> Result<ForumInfo> {
    let user_id: Ulid = flexbuffers::from_slice(&req.message.payload)?;

    // Get user info to get their handle for the forum name
    let user_info: BasicInfo = request("user.basic_info.by_user_id", user_id.to_string()).await?;

    let db = db();
    let user_id_str = user_id.to_string();

    // Check if personal forum already exists (forum_id == user_id)
    let existing = sqlx::query!(
        r#"SELECT "forum_id" FROM "forums" WHERE "forum_id" = ?"#,
        user_id_str
    )
    .fetch_optional(db)
    .await?;

    if existing.is_some() {
        return get_forum_internal(user_id, Some(user_id)).await;
    }

    let now = TimeString::now();
    let now_str = now.to_string();

    // Create personal forum with user_id as forum_id
    // Use user_id for slug since it's guaranteed unique
    let slug = format!("user-{}", user_id_str);
    let name = format!("{}'s Personal Forum", user_info.handle); // Use actual handle, not display_handle
    let description = Some(format!(
        "Personal discussion space for {}",
        user_info.handle
    ));

    sqlx::query!(
        r#"INSERT INTO "forums" ("forum_id", "slug", "name", "description", "created_at", "display_order", "is_active")
           VALUES (?, ?, ?, ?, ?, ?, ?)"#,
        user_id_str,
        slug,
        name,
        description,
        now_str,
        1000000, // High display order to put personal forums at the end
        1
    )
    .execute(db)
    .await?;

    // Log the activity
    log_activity(
        Some(now),
        "personal_forum_created",
        Some(user_id),
        Some(user_id), // forum_id is same as user_id for personal forums
        Some(
            serde_json::json!({
                "slug": slug,
                "name": name
            })
            .to_string(),
        ),
    )
    .await?;

    get_forum_internal(user_id, Some(user_id)).await
}

async fn copy_general_forum_categories_to_new_forum(new_forum_id: Ulid, _user_id: Ulid) -> Result<()> {
    let db = db();
    let now = TimeString::now();
    let now_str = now.to_string();
    let new_forum_id_str = new_forum_id.to_string();

    // Get general forum categories
    let general_categories = sqlx::query!(
        r#"SELECT fc.category_name 
           FROM "forum_categories" fc
           JOIN "forums" f ON fc.forum_id = f.forum_id
           WHERE f.slug = 'general' AND fc.is_active = 1
           ORDER BY fc.category_name ASC"#
    )
    .fetch_all(db)
    .await?;

    // Copy each category to the new forum
    for category_row in general_categories {
        let category_id = Ulid::new();
        let category_id_str = category_id.to_string();

        sqlx::query!(
            r#"INSERT INTO "forum_categories" ("category_id", "forum_id", "category_name", "created_at")
               VALUES (?, ?, ?, ?)"#,
            category_id_str,
            new_forum_id_str,
            category_row.category_name,
            now_str
        )
        .execute(db)
        .await?;
    }

    Ok(())
}
