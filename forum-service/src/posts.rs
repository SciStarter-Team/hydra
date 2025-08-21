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

pub async fn create_post(req: &Request) -> Result<PostInfo> {
    let message: CreatePost = flexbuffers::from_slice(&req.message.payload)?;
    let user_id = message.user_id;
    let post_id = Ulid::new();
    let post_id_str = post_id.to_string();
    let user_id_str = user_id.to_string();
    let now = TimeString::now();
    let now_str = now.to_string();
    let topic_id_str = message.topic_id.to_string();

    // Check if topic exists and is not locked
    let topic_row = sqlx::query!(
        r#"SELECT is_locked, is_deleted FROM "topics" WHERE "topic_id" = ?"#,
        topic_id_str
    )
    .fetch_optional(db())
    .await?
    .ok_or_else(|| anyhow::anyhow!("Topic not found"))?;

    if topic_row.is_locked == 1 {
        return Err(anyhow::anyhow!("Topic is locked"));
    }

    if topic_row.is_deleted == 1 {
        return Err(anyhow::anyhow!("Topic is deleted"));
    }

    // Check content for bad words
    let filter_result: ContentFilterResult = 
        crate::content_filter::check_content_internal(&message.content).await?;
    
    if !filter_result.is_allowed {
        return Err(anyhow::anyhow!("Content contains prohibited words: {:?}", filter_result.flagged_words));
    }

    // If replying to a post, check that parent post exists
    if let Some(parent_id) = message.parent_post_id {
        let parent_id_str = parent_id.to_string();
        let parent_exists = sqlx::query!(
            r#"SELECT "post_id" FROM "posts" WHERE "post_id" = ? AND "topic_id" = ? AND "is_deleted" = 0"#,
            parent_id_str,
            topic_id_str
        )
        .fetch_optional(db())
        .await?
        .is_some();

        if !parent_exists {
            return Err(anyhow::anyhow!("Parent post not found"));
        }
    }

    let db = db();

    // Insert the post
    let parent_post_id_str = message.parent_post_id.map(|id| id.to_string());
    sqlx::query!(
        r#"INSERT INTO "posts" ("post_id", "topic_id", "content", "author_id", "parent_post_id", "created_at")
           VALUES (?, ?, ?, ?, ?, ?)"#,
        post_id_str,
        topic_id_str,
        message.content,
        user_id_str,
        parent_post_id_str,
        now_str
    )
    .execute(db)
    .await?;

    // Update topic's updated_at timestamp
    sqlx::query!(
        r#"UPDATE "topics" SET "updated_at" = ? WHERE "topic_id" = ?"#,
        now_str,
        topic_id_str
    )
    .execute(db)
    .await?;

    // Log the activity
    log_activity(
        Some(now),
        "post_created",
        Some(user_id),
        Some(post_id),
        Some(serde_json::json!({
            "topic_id": message.topic_id.to_string(),
            "parent_post_id": message.parent_post_id.map(|id| id.to_string())
        }).to_string()),
    )
    .await?;

    get_post_internal(post_id).await
}

pub async fn get_post(req: &Request) -> Result<PostInfo> {
    let post_id_str: String = flexbuffers::from_slice(&req.message.payload)?;
    let post_id = post_id_str.as_ulid()?;
    get_post_internal(post_id).await
}

async fn get_post_internal(post_id: Ulid) -> Result<PostInfo> {
    let db = db();
    let post_id_str = post_id.to_string();

    let row = sqlx::query!(
        r#"SELECT post_id, topic_id, content, author_id, parent_post_id,
                  created_at, updated_at, is_deleted
           FROM "posts"
           WHERE post_id = ?"#,
        post_id_str
    )
    .fetch_optional(db)
    .await?
    .ok_or_else(|| anyhow::anyhow!("Post not found"))?;

    // Get user info from user-service
    let author_info: BasicInfo = request("user.basic_info.by_user_id", row.author_id.clone()).await?;

    Ok(PostInfo {
        post_id,
        topic_id: row.topic_id.as_ulid()?,
        content: row.content,
        author_id: author_info.user_id,
        author_handle: author_info.handle,
        author_is_public: true, // We'll need to extend BasicInfo or create a new message type for public flag
        created_at: TimeString::try_from(row.created_at)?,
        updated_at: row.updated_at.as_ref().map(|s| TimeString::try_from(s.clone())).transpose()?,
        parent_post_id: row.parent_post_id.as_ref().map(|s| s.as_ulid()).transpose()?,
        is_deleted: row.is_deleted == 1,
    })
}

pub async fn list_posts(req: &Request) -> Result<Vec<PostInfo>> {
    let query: PostListQuery = flexbuffers::from_slice(&req.message.payload)?;
    let db = db();
    let limit = query.limit.unwrap_or(50).min(100);
    let offset = query.offset.unwrap_or(0);

    let topic_id_str = query.topic_id.to_string();
    let rows = sqlx::query!(
        r#"SELECT post_id, topic_id, content, author_id, parent_post_id,
                  created_at, updated_at, is_deleted
           FROM "posts"
           WHERE topic_id = ? AND is_deleted = 0
           ORDER BY created_at ASC
           LIMIT ? OFFSET ?"#,
        topic_id_str,
        limit,
        offset
    )
    .fetch_all(db)
    .await?;

    let mut posts = Vec::new();
    for row in rows {
        let post_id = row.post_id.ok_or_else(|| anyhow::anyhow!("Post ID is null"))?.as_ulid()?;
        let topic_id = row.topic_id.as_ulid()?;

        // Get user info from user-service
        let author_info: BasicInfo = request("user.basic_info.by_user_id", row.author_id.clone()).await?;

        posts.push(PostInfo {
            post_id,
            topic_id,
            content: row.content,
            author_id: author_info.user_id,
            author_handle: author_info.handle,
            author_is_public: true, // We'll need to extend BasicInfo or create a new message type for public flag
            created_at: TimeString::try_from(row.created_at)?,
            updated_at: row.updated_at.as_ref().map(|s| TimeString::try_from(s.clone())).transpose()?,
            parent_post_id: row.parent_post_id.as_ref().map(|s| s.as_ulid()).transpose()?,
            is_deleted: row.is_deleted == 1,
        });
    }

    Ok(posts)
}

pub async fn update_post(req: &Request) -> Result<PostInfo> {
    let (post_id_str, new_content, user_id): (String, String, Ulid) = flexbuffers::from_slice(&req.message.payload)?;
    let post_id = post_id_str.as_ulid()?;

    // Check if user owns the post or has moderation permissions
    let post = get_post_internal(post_id).await?;
    
    // Get user info to check permissions
    let user_info: BasicInfo = request("user.basic_info.by_user_id", user_id.to_string()).await?;
    if post.author_id != user_id && !user_info.is_staff() {
        return Err(anyhow::anyhow!("Permission denied"));
    }

    // Check content for bad words
    let filter_result: ContentFilterResult = 
        crate::content_filter::check_content_internal(&new_content).await?;
    
    if !filter_result.is_allowed {
        return Err(anyhow::anyhow!("Content contains prohibited words: {:?}", filter_result.flagged_words));
    }

    let now = TimeString::now();
    let now_str = now.to_string();
    let post_id_str = post_id.to_string();
    let db = db();

    // Update the post
    sqlx::query!(
        r#"UPDATE "posts" SET "content" = ?, "updated_at" = ?
           WHERE "post_id" = ?"#,
        new_content,
        now_str,
        post_id_str
    )
    .execute(db)
    .await?;

    // Log the activity
    log_activity(
        Some(now),
        "post_updated",
        Some(user_id),
        Some(post_id),
        Some(serde_json::json!({
            "topic_id": post.topic_id.to_string()
        }).to_string()),
    )
    .await?;

    get_post_internal(post_id).await
}