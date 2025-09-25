use anyhow::Result;
use async_nats::service::Request;
use common::timestring::TimeString;
use common::{message::forum::*, service::log_activity, shared::db, ulid::AsUlid};
use ulid::Ulid;

use crate::forums::check_forum_management_permission;

pub async fn create_forum_category(req: &Request) -> Result<ForumCategoryInfo> {
    let message: CreateForumCategory = flexbuffers::from_slice(&req.message.payload)?;
    let user_id = message.user_id;

    // Check if user has management permission for this forum
    if !check_forum_management_permission(message.forum_id, Some(user_id)).await? {
        return Err(anyhow::anyhow!("Permission denied"));
    }

    let category_id = Ulid::new();
    let category_id_str = category_id.to_string();
    let forum_id_str = message.forum_id.to_string();
    let now = TimeString::now();
    let now_str = now.to_string();

    let db = db();

    // Check if category name already exists for this forum
    let existing = sqlx::query!(
        r#"SELECT "category_id" FROM "forum_categories" WHERE "forum_id" = ? AND "category_name" = ?"#,
        forum_id_str,
        message.category_name
    )
    .fetch_optional(db)
    .await?;

    if existing.is_some() {
        return Err(anyhow::anyhow!("Category already exists for this forum"));
    }

    // Insert the category
    sqlx::query!(
        r#"INSERT INTO "forum_categories" ("category_id", "forum_id", "category_name", "created_at")
           VALUES (?, ?, ?, ?)"#,
        category_id_str,
        forum_id_str,
        message.category_name,
        now_str
    )
    .execute(db)
    .await?;

    // Log the activity
    log_activity(
        Some(now),
        "forum_category_created",
        Some(user_id),
        Some(message.forum_id),
        Some(
            serde_json::json!({
                "category_name": message.category_name,
                "category_id": category_id_str
            })
            .to_string(),
        ),
    )
    .await?;

    get_forum_category_internal(category_id).await
}

pub async fn get_forum_categories(req: &Request) -> Result<Vec<ForumCategoryInfo>> {
    let message: GetForumCategories = flexbuffers::from_slice(&req.message.payload)?;
    let forum_id_str = message.forum_id.to_string();
    let active_only = message.active_only.unwrap_or(true);

    let db = db();

    let rows: Vec<_> = if active_only {
        sqlx::query!(
            r#"SELECT category_id, forum_id, category_name, is_active, created_at, updated_at
               FROM "forum_categories"
               WHERE forum_id = ? AND is_active = 1
               ORDER BY category_name ASC"#,
            forum_id_str
        )
        .fetch_all(db)
        .await?
        .into_iter()
        .map(|row| {
            (
                row.category_id,
                row.forum_id,
                row.category_name,
                row.is_active,
                row.created_at,
                row.updated_at,
            )
        })
        .collect()
    } else {
        sqlx::query!(
            r#"SELECT category_id, forum_id, category_name, is_active, created_at, updated_at
               FROM "forum_categories"
               WHERE forum_id = ?
               ORDER BY category_name ASC"#,
            forum_id_str
        )
        .fetch_all(db)
        .await?
        .into_iter()
        .map(|row| {
            (
                row.category_id,
                row.forum_id,
                row.category_name,
                row.is_active,
                row.created_at,
                row.updated_at,
            )
        })
        .collect()
    };

    let mut categories = Vec::new();
    for row in rows {
        let category_id_str = row
            .0
            .ok_or_else(|| anyhow::anyhow!("Category ID is null"))?;
        let category_id = category_id_str.as_ulid()?;
        let forum_id = row.1.as_ulid()?;

        categories.push(ForumCategoryInfo {
            category_id,
            forum_id,
            category_name: row.2,
            is_active: row.3 == 1,
            created_at: TimeString::try_from(row.4)?,
            updated_at: row
                .5
                .as_ref()
                .map(|s| TimeString::try_from(s.clone()))
                .transpose()?,
        });
    }

    Ok(categories)
}

pub async fn update_forum_category(req: &Request) -> Result<ForumCategoryInfo> {
    let message: UpdateForumCategory = flexbuffers::from_slice(&req.message.payload)?;
    let user_id = message.user_id;
    let category_id_str = message.category_id.to_string();

    let db = db();

    // Get current category to check forum ownership
    let current = get_forum_category_internal(message.category_id).await?;

    // Check if user has management permission for this forum
    if !check_forum_management_permission(current.forum_id, Some(user_id)).await? {
        return Err(anyhow::anyhow!("Permission denied"));
    }

    let now = TimeString::now();
    let now_str = now.to_string();

    let new_name = message
        .category_name
        .clone()
        .unwrap_or(current.category_name.clone());
    let new_is_active = if message.is_active.unwrap_or(current.is_active) {
        1
    } else {
        0
    };

    // If updating name, check for duplicates
    if let Some(ref name) = message.category_name {
        if name != &current.category_name {
            let forum_id_str = current.forum_id.to_string();
            let existing = sqlx::query!(
                r#"SELECT "category_id" FROM "forum_categories" WHERE "forum_id" = ? AND "category_name" = ? AND "category_id" != ?"#,
                forum_id_str,
                name,
                category_id_str
            )
            .fetch_optional(db)
            .await?;

            if existing.is_some() {
                return Err(anyhow::anyhow!(
                    "Category name already exists for this forum"
                ));
            }
        }
    }

    // Update the category
    sqlx::query!(
        r#"UPDATE "forum_categories" SET "category_name" = ?, "is_active" = ?, "updated_at" = ?
           WHERE "category_id" = ?"#,
        new_name,
        new_is_active,
        now_str,
        category_id_str
    )
    .execute(db)
    .await?;

    // Log the activity
    log_activity(
        Some(now),
        "forum_category_updated",
        Some(user_id),
        Some(current.forum_id),
        Some(
            serde_json::json!({
                "category_name": new_name,
                "category_id": category_id_str
            })
            .to_string(),
        ),
    )
    .await?;

    get_forum_category_internal(message.category_id).await
}

pub async fn delete_forum_category(req: &Request) -> Result<()> {
    let message: DeleteForumCategory = flexbuffers::from_slice(&req.message.payload)?;
    let user_id = message.user_id;
    let category_id_str = message.category_id.to_string();

    // Get current category to check forum ownership
    let current = get_forum_category_internal(message.category_id).await?;

    // Check if user has management permission for this forum
    if !check_forum_management_permission(current.forum_id, Some(user_id)).await? {
        return Err(anyhow::anyhow!("Permission denied"));
    }

    let db = db();
    let now = TimeString::now();

    // Delete the category
    sqlx::query!(
        r#"DELETE FROM "forum_categories" WHERE "category_id" = ?"#,
        category_id_str
    )
    .execute(db)
    .await?;

    // Log the activity
    log_activity(
        Some(now),
        "forum_category_deleted",
        Some(user_id),
        Some(current.forum_id),
        Some(
            serde_json::json!({
                "category_name": current.category_name,
                "category_id": category_id_str
            })
            .to_string(),
        ),
    )
    .await?;

    Ok(())
}

async fn get_forum_category_internal(category_id: Ulid) -> Result<ForumCategoryInfo> {
    let db = db();
    let category_id_str = category_id.to_string();

    let row = sqlx::query!(
        r#"SELECT category_id, forum_id, category_name, is_active, created_at, updated_at
           FROM "forum_categories"
           WHERE category_id = ?"#,
        category_id_str
    )
    .fetch_optional(db)
    .await?
    .ok_or_else(|| anyhow::anyhow!("Category not found"))?;

    let forum_id = row.forum_id.as_ulid()?;

    Ok(ForumCategoryInfo {
        category_id,
        forum_id,
        category_name: row.category_name,
        is_active: row.is_active == 1,
        created_at: TimeString::try_from(row.created_at)?,
        updated_at: row
            .updated_at
            .as_ref()
            .map(|s| TimeString::try_from(s.clone()))
            .transpose()?,
    })
}
