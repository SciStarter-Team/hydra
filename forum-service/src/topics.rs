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
use sqlx::Row;
use std::collections::HashSet;
use ulid::Ulid;


fn normalize_title(title: &str) -> HashSet<String> {
    let stop_words = [
        "the", "a", "an", "and", "or", "but", "in", "on", "at", "to", "for", "of", "with", "by",
        "is", "are", "was", "were", "be", "been", "have", "has", "had", "do", "does", "did",
        "will", "would", "could", "should", "may", "might", "can", "what", "why", "how", "when",
        "where", "who", "which",
    ];

    title
        .to_lowercase()
        .split_whitespace()
        .map(|word| word.trim_matches(|c: char| !c.is_alphanumeric()))
        .filter(|word| word.len() > 2 && !stop_words.contains(word))
        .map(|word| word.to_string())
        .collect()
}

fn titles_similar(title1: &str, title2: &str) -> bool {
    let words1 = normalize_title(title1);
    let words2 = normalize_title(title2);

    if words1.is_empty() || words2.is_empty() {
        return false;
    }

    let overlap = words1.intersection(&words2).count();
    let min_words = words1.len().min(words2.len());

    overlap as f32 / min_words as f32 >= 0.7
}

pub async fn create_topic(req: &Request) -> Result<TopicInfo> {
    let message: CreateTopic = flexbuffers::from_slice(&req.message.payload)?;
    let user_id = message.user_id;
    let topic_id = Ulid::new();
    let now = TimeString::now();

    // Note: Similar topic checking removed - users can create topics with similar titles
    // The suggest_similar_topics function is still available for UI to show suggestions
    let db = db();

    // Check content for bad words
    let filter_result: ContentFilterResult = crate::content_filter::check_content_internal(
        &format!("{}\n{}", message.title, message.content),
    )
    .await?;

    if !filter_result.is_allowed {
        return Err(anyhow::anyhow!(
            "Content contains prohibited words: {:?}",
            filter_result.flagged_words
        ));
    }
    let mut tx = db.begin().await?;

    // Insert the topic
    let topic_id_str = topic_id.to_string();
    let user_id_str = user_id.to_string();
    let forum_id_str = message.forum_id.to_string();
    let now_str = now.to_string();
    sqlx::query!(
        r#"INSERT INTO "topics" ("topic_id", "forum_id", "title", "content", "author_id", "created_at", "category")
           VALUES (?, ?, ?, ?, ?, ?, ?)"#,
        topic_id_str,
        forum_id_str,
        message.title,
        message.content,
        user_id_str,
        now_str,
        message.category
    )
    .execute(&mut *tx)
    .await?;

    // Insert tags
    for tag in &message.tags {
        sqlx::query!(
            r#"INSERT INTO "topic_tags" ("topic_id", "tag") VALUES (?, ?)"#,
            topic_id_str,
            tag
        )
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;

    // Log the activity
    log_activity(
        Some(now),
        "topic_created",
        Some(user_id),
        Some(topic_id),
        Some(
            serde_json::json!({
                "title": message.title,
                "category": message.category
            })
            .to_string(),
        ),
    )
    .await?;

    get_topic_internal(topic_id).await
}

pub async fn get_topic(req: &Request) -> Result<TopicInfo> {
    let topic_id_str: String = flexbuffers::from_slice(&req.message.payload)?;
    let topic_id = topic_id_str.as_ulid()?;
    get_topic_internal(topic_id).await
}

async fn get_topic_internal(topic_id: Ulid) -> Result<TopicInfo> {
    let db = db();
    let topic_id_str = topic_id.to_string();

    let row = sqlx::query!(
        r#"SELECT topic_id, forum_id, title, content, author_id, created_at, updated_at, category,
                  is_pinned, is_locked, is_deleted, is_flagged
           FROM "topics"
           WHERE topic_id = ?"#,
        topic_id_str
    )
    .fetch_optional(db)
    .await?
    .ok_or_else(|| anyhow::anyhow!("Topic not found"))?;

    // Get post count
    let post_count = sqlx::query_scalar!(
        r#"SELECT COUNT(*) FROM "posts" WHERE "topic_id" = ? AND "is_deleted" = 0"#,
        topic_id_str
    )
    .fetch_one(db)
    .await?;

    // Get tags
    let tags: Vec<String> = sqlx::query_scalar!(
        r#"SELECT "tag" FROM "topic_tags" WHERE "topic_id" = ?"#,
        topic_id_str
    )
    .fetch_all(db)
    .await?;

    // Get user info from user-service
    let author_info: BasicInfo =
        request("user.basic_info.by_user_id", row.author_id.clone()).await?;

    Ok(TopicInfo {
        topic_id,
        forum_id: row
            .forum_id
            .ok_or_else(|| anyhow::anyhow!("Forum ID is null"))?
            .as_ulid()?,
        title: row.title,
        content: row.content,
        author_id: author_info.user_id,
        author_handle: author_info.display_handle().to_string(),
        author_is_public: !author_info.is_private(),
        created_at: TimeString::try_from(row.created_at)?,
        updated_at: row
            .updated_at
            .as_ref()
            .map(|s| TimeString::try_from(s.clone()))
            .transpose()?,
        category: row.category,
        tags,
        post_count,
        is_pinned: row.is_pinned == 1,
        is_locked: row.is_locked == 1,
        is_deleted: row.is_deleted == 1,
        is_flagged: row.is_flagged == 1,
    })
}

pub async fn list_topics(req: &Request) -> Result<Vec<TopicInfo>> {
    let query: TopicListQuery = flexbuffers::from_slice(&req.message.payload)?;
    let db = db();
    let limit = query.limit.unwrap_or(50).min(100);
    let offset = query.offset.unwrap_or(0);

    // Use QueryBuilder for dynamic query construction
    let mut query_builder = sqlx::QueryBuilder::new(
        r#"SELECT topic_id, forum_id, title, content, author_id, created_at, updated_at, category,
                  is_pinned, is_locked, is_deleted, is_flagged
           FROM topics WHERE is_deleted = 0"#
    );

    // Add category filter if specified
    if let Some(category) = &query.category {
        query_builder.push(" AND category = ");
        query_builder.push_bind(category);
    }

    // Add search filter if specified
    let search_pattern = query.search.as_ref().map(|search| format!("%{}%", search));
    if let Some(pattern) = &search_pattern {
        query_builder.push(" AND (title LIKE ");
        query_builder.push_bind(pattern);
        query_builder.push(" OR content LIKE ");
        query_builder.push_bind(pattern);
        query_builder.push(")");
    }

    // Add ordering
    let order_clause = match query.sort_by {
        TopicSortBy::CreatedDesc => " ORDER BY is_pinned DESC, created_at DESC",
        TopicSortBy::CreatedAsc => " ORDER BY is_pinned DESC, created_at ASC",
        TopicSortBy::UpdatedDesc => " ORDER BY is_pinned DESC, COALESCE(updated_at, created_at) DESC",
        TopicSortBy::UpdatedAsc => " ORDER BY is_pinned DESC, COALESCE(updated_at, created_at) ASC",
        TopicSortBy::PostCount => " ORDER BY is_pinned DESC, created_at DESC", // We'll sort by post count after fetching
    };
    query_builder.push(order_clause);

    // Add pagination
    query_builder.push(" LIMIT ");
    query_builder.push_bind(limit);
    query_builder.push(" OFFSET ");
    query_builder.push_bind(offset);

    let rows = query_builder.build().fetch_all(db).await?;

    let mut topics = Vec::new();
    for row in rows {
        let topic_id: String = row.get("topic_id");
        let topic_id = topic_id.as_ulid()?;
        let topic_id_str = topic_id.to_string();

        // Get tags for this topic
        let tags: Vec<String> = sqlx::query_scalar!(
            r#"SELECT "tag" FROM "topic_tags" WHERE "topic_id" = ?"#,
            topic_id_str
        )
        .fetch_all(db)
        .await?;

        // Filter by tags if specified
        if !query.tags.is_empty() {
            let has_matching_tag = query.tags.iter().any(|t| tags.contains(t));
            if !has_matching_tag {
                continue;
            }
        }

        // Get post count
        let post_count = sqlx::query_scalar!(
            r#"SELECT COUNT(*) FROM "posts" WHERE "topic_id" = ? AND "is_deleted" = 0"#,
            topic_id_str
        )
        .fetch_one(db)
        .await?;

        let author_id: String = row.get("author_id");
        let author_id = author_id.as_ulid()?;

        // Get user info from user-service
        let author_info: BasicInfo =
            request("user.basic_info.by_user_id", author_id.to_string()).await?;

        let forum_id: String = row.get("forum_id");
        let forum_id = forum_id.as_ulid()?;

        topics.push(TopicInfo {
            topic_id,
            forum_id,
            title: row.get("title"),
            content: row.get("content"),
            author_id: author_info.user_id,
            author_handle: author_info.display_handle().to_string(),
            author_is_public: !author_info.is_private(),
            created_at: TimeString::try_from(row.get::<String, _>("created_at"))?,
            updated_at: row
                .get::<Option<String>, _>("updated_at")
                .as_ref()
                .map(|s| TimeString::try_from(s.clone()))
                .transpose()?,
            category: row.get("category"),
            tags,
            post_count,
            is_pinned: row.get::<i32, _>("is_pinned") == 1,
            is_locked: row.get::<i32, _>("is_locked") == 1,
            is_deleted: row.get::<i32, _>("is_deleted") == 1,
            is_flagged: row.get::<i32, _>("is_flagged") == 1,
        });
    }

    // Sort by post count if requested
    if matches!(query.sort_by, TopicSortBy::PostCount) {
        topics.sort_by(|a, b| b.post_count.cmp(&a.post_count));
    }

    Ok(topics)
}

pub async fn update_topic(req: &Request) -> Result<TopicInfo> {
    let (topic_id_str, update_req): (String, CreateTopic) =
        flexbuffers::from_slice(&req.message.payload)?;
    let topic_id = topic_id_str.as_ulid()?;
    let user_id = update_req.user_id;

    // Check if user owns the topic or has moderation permissions
    let topic = get_topic_internal(topic_id).await?;

    // Get user info to check permissions
    let user_info: BasicInfo = request("user.basic_info.by_user_id", user_id.to_string()).await?;
    if topic.author_id != user_id && !user_info.is_staff() {
        return Err(anyhow::anyhow!("Permission denied"));
    }

    // Check content for bad words
    let filter_result: ContentFilterResult = crate::content_filter::check_content_internal(
        &format!("{}\n{}", update_req.title, update_req.content),
    )
    .await?;

    if !filter_result.is_allowed {
        return Err(anyhow::anyhow!(
            "Content contains prohibited words: {:?}",
            filter_result.flagged_words
        ));
    }

    let now = TimeString::now();
    let now_str = now.to_string();
    let topic_id_str = topic_id.to_string();
    let db = db();
    let mut tx = db.begin().await?;

    // Update the topic
    sqlx::query!(
        r#"UPDATE "topics" SET "title" = ?, "content" = ?, "updated_at" = ?, "category" = ?
           WHERE "topic_id" = ?"#,
        update_req.title,
        update_req.content,
        now_str,
        update_req.category,
        topic_id_str
    )
    .execute(&mut *tx)
    .await?;

    // Update tags - delete old ones and insert new ones
    sqlx::query!(
        r#"DELETE FROM "topic_tags" WHERE "topic_id" = ?"#,
        topic_id_str
    )
    .execute(&mut *tx)
    .await?;

    for tag in &update_req.tags {
        sqlx::query!(
            r#"INSERT INTO "topic_tags" ("topic_id", "tag") VALUES (?, ?)"#,
            topic_id_str,
            tag
        )
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;

    // Log the activity
    log_activity(
        Some(now),
        "topic_updated",
        Some(user_id),
        Some(topic_id),
        Some(
            serde_json::json!({
                "title": update_req.title,
                "category": update_req.category
            })
            .to_string(),
        ),
    )
    .await?;

    get_topic_internal(topic_id).await
}

pub async fn suggest_similar_topics(req: &Request) -> Result<Vec<TopicInfo>> {
    let (forum_slug, title): (String, String) = flexbuffers::from_slice(&req.message.payload)?;
    let db = db();

    let recent_topics = sqlx::query!(
        r#"SELECT t."topic_id", t."title" FROM "topics" t
           JOIN "forums" f ON t."forum_id" = f."forum_id"
           WHERE f."slug" = ? AND f."is_active" = 1 AND t."is_deleted" = 0 
           ORDER BY t."created_at" DESC 
           LIMIT 100"#,
        forum_slug
    )
    .fetch_all(db)
    .await?;

    let mut similar_topics = Vec::new();

    for topic_row in recent_topics {
        if titles_similar(&title, &topic_row.title) {
            if let Some(topic_id_str) = &topic_row.topic_id {
                if let Ok(topic_id) = topic_id_str.as_ulid() {
                    if let Ok(topic_info) = get_topic_internal(topic_id).await {
                        similar_topics.push(topic_info);
                        if similar_topics.len() >= 5 {
                            break;
                        }
                    }
                }
            }
        }
    }

    Ok(similar_topics)
}

pub async fn get_user_topics(req: &Request) -> Result<Vec<TopicInfo>> {
    let (user_id_str, limit, offset): (String, Option<i32>, Option<i32>) =
        flexbuffers::from_slice(&req.message.payload)?;
    let db = db();
    let limit = limit.unwrap_or(20).min(50) as i64;
    let offset = offset.unwrap_or(0) as i64;

    let rows = sqlx::query!(
        r#"SELECT topic_id FROM "topics" 
           WHERE "author_id" = ? AND "is_deleted" = 0 
           ORDER BY "created_at" DESC 
           LIMIT ? OFFSET ?"#,
        user_id_str,
        limit,
        offset
    )
    .fetch_all(db)
    .await?;

    let mut topics = Vec::new();
    for row in rows {
        if let Some(topic_id_str) = &row.topic_id {
            if let Ok(topic_id) = topic_id_str.as_ulid() {
                if let Ok(topic_info) = get_topic_internal(topic_id).await {
                    topics.push(topic_info);
                }
            }
        }
    }

    Ok(topics)
}
