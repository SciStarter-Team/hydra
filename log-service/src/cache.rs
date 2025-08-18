use crate::error::Result;
use common::{shared::db, transaction::Immediate};

/// SQLite-based cache system for user data following user-service patterns
/// Uses the same database connection pool and transaction patterns as the user-service

// ================================
// USER TAGS CACHE
// ================================

/// Update user tags cache (replaces all existing tags for the user)
/// Follows the same pattern as user-service set_user_tags
pub async fn update_user_tags_cache(user_id: String, tags: Vec<String>) -> Result<()> {
    let updated_at = jiff::Timestamp::now().to_string();

    let mut tx = Immediate::begin_with_retry(db(), None, None).await?;

    // Remove all existing tags for this user
    sqlx::query!(
        r#"DELETE FROM "user_tags_cache" WHERE "user_id" = ?"#,
        user_id
    )
    .execute(&mut tx)
    .await?;

    // Insert new tags
    for tag in &tags {
        sqlx::query!(
            r#"INSERT INTO "user_tags_cache" ("user_id", "tag", "updated_at") VALUES (?, ?, ?)"#,
            user_id,
            tag,
            updated_at
        )
        .execute(&mut tx)
        .await?;
    }

    tx.commit().await?;
    Ok(())
}

/// Get user tags from cache
/// Follows the same pattern as user-service get_user_tags
pub async fn get_user_tags_from_cache(user_id: &str) -> Result<Option<Vec<String>>> {
    let tags = sqlx::query_scalar!(
        r#"SELECT "tag" FROM "user_tags_cache" WHERE "user_id" = ? ORDER BY "tag""#,
        user_id
    )
    .fetch_all(db())
    .await?;

    if tags.is_empty() {
        Ok(None) // No cache entry found
    } else {
        Ok(Some(tags))
    }
}

/// Fetch user tags from user-service via NATS
pub async fn fetch_user_tags_from_service(user_id: &str) -> Result<Vec<String>> {
    use common::service::request;

    // Call user service to get user tags
    let tags: Vec<String> = request("user.tags.get", user_id).await.unwrap_or_default();
    Ok(tags)
}

/// Get user tags with cache-through pattern
pub async fn get_user_tags(user_id: &str) -> Result<Vec<String>> {
    // Try cache first
    if let Some(tags) = get_user_tags_from_cache(user_id).await? {
        return Ok(tags);
    }

    // Cache miss - fetch from user service and cache
    let tags = fetch_user_tags_from_service(user_id).await?;
    if !tags.is_empty() {
        // Use a retry mechanism for cache updates in case of conflicts
        for _attempt in 0..3 {
            if update_user_tags_cache(user_id.to_string(), tags.clone())
                .await
                .is_ok()
            {
                break;
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        }
    }

    Ok(tags)
}

/// Add a single tag to user's existing tags
/// Follows the same pattern as user-service add_user_tag
pub async fn add_user_tag(user_id: &str, tag: &str) -> Result<()> {
    let updated_at = jiff::Timestamp::now().to_string();

    // Insert the tag (ignore if already exists)
    sqlx::query!(
        r#"INSERT OR IGNORE INTO "user_tags_cache" ("user_id", "tag", "updated_at") VALUES (?, ?, ?)"#,
        user_id,
        tag,
        updated_at
    )
    .execute(db())
    .await?;

    Ok(())
}

/// Remove a single tag from user's tags
/// Follows the same pattern as user-service remove_user_tag
pub async fn remove_user_tag(user_id: &str, tag: &str) -> Result<()> {
    // Remove the tag
    sqlx::query!(
        r#"DELETE FROM "user_tags_cache" WHERE "user_id" = ? AND "tag" = ?"#,
        user_id,
        tag
    )
    .execute(db())
    .await?;

    Ok(())
}

/// Remove all tags for a user from cache
pub async fn remove_user_from_cache(user_id: &str) -> Result<()> {
    sqlx::query!(
        r#"DELETE FROM "user_tags_cache" WHERE "user_id" = ?"#,
        user_id
    )
    .execute(db())
    .await?;

    Ok(())
}

/// Get all users with a specific tag
pub async fn get_users_with_tag(tag: &str) -> Result<Vec<String>> {
    let users = sqlx::query_scalar!(
        r#"SELECT DISTINCT "user_id" FROM "user_tags_cache" WHERE "tag" = ?"#,
        tag
    )
    .fetch_all(db())
    .await?;

    Ok(users)
}

/// Get popular tags with counts
pub async fn get_popular_tags(limit: i64) -> Result<Vec<(String, i64)>> {
    #[derive(sqlx::FromRow)]
    struct TagCount {
        tag: String,
        count: i64,
    }

    let tag_counts = sqlx::query_as!(
        TagCount,
        r#"SELECT "tag", COUNT(*) as "count" 
           FROM "user_tags_cache" 
           GROUP BY "tag" 
           ORDER BY "count" DESC 
           LIMIT ?"#,
        limit
    )
    .fetch_all(db())
    .await?;

    Ok(tag_counts
        .into_iter()
        .map(|tc| (tc.tag, tc.count))
        .collect())
}

// ================================
// EMAIL ADDRESS CACHE
// ================================

/// Update user emails cache (replaces all existing emails for the user)
/// Follows the same pattern as user-service set_user_emails
pub async fn update_user_emails_cache(user_id: String, emails: Vec<String>) -> Result<()> {
    let updated_at = jiff::Timestamp::now().to_string();

    let mut tx = Immediate::begin_with_retry(db(), None, None).await?;

    // Remove all existing emails for this user
    sqlx::query!(
        r#"DELETE FROM "user_emails_cache" WHERE "user_id" = ?"#,
        user_id
    )
    .execute(&mut tx)
    .await?;

    // Insert new emails
    for email in &emails {
        sqlx::query!(
            r#"INSERT INTO "user_emails_cache" ("user_id", "email", "updated_at") VALUES (?, ?, ?)"#,
            user_id,
            email,
            updated_at
        )
        .execute(&mut tx)
        .await?;
    }

    tx.commit().await?;
    Ok(())
}

/// Get user emails from cache
/// Follows the same pattern as user-service get_user_emails
pub async fn get_user_emails_from_cache(user_id: &str) -> Result<Option<Vec<String>>> {
    let emails = sqlx::query_scalar!(
        r#"SELECT "email" FROM "user_emails_cache" WHERE "user_id" = ? ORDER BY "email""#,
        user_id
    )
    .fetch_all(db())
    .await?;

    if emails.is_empty() {
        Ok(None) // No cache entry found
    } else {
        Ok(Some(emails))
    }
}

/// Fetch user emails from user-service via NATS
pub async fn fetch_user_emails_from_service(user_id: &str) -> Result<Vec<String>> {
    use common::service::request;

    // Call user service to get user emails
    let emails: Vec<String> = request("user.emails.get", user_id)
        .await
        .unwrap_or_default();
    Ok(emails)
}

/// Get user emails with cache-through pattern
pub async fn get_user_emails(user_id: &str) -> Result<Vec<String>> {
    // Try cache first
    if let Some(emails) = get_user_emails_from_cache(user_id).await? {
        return Ok(emails);
    }

    // Cache miss - fetch from user service and cache
    let emails = fetch_user_emails_from_service(user_id).await?;
    if !emails.is_empty() {
        // Use a retry mechanism for cache updates in case of conflicts
        for _attempt in 0..3 {
            if update_user_emails_cache(user_id.to_string(), emails.clone())
                .await
                .is_ok()
            {
                break;
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        }
    }

    Ok(emails)
}

/// Add a single email to user's existing emails
/// Follows the same pattern as user-service add_user_email
pub async fn add_user_email(user_id: &str, email: &str) -> Result<()> {
    let updated_at = jiff::Timestamp::now().to_string();

    // Insert the email (ignore if already exists)
    sqlx::query!(
        r#"INSERT OR IGNORE INTO "user_emails_cache" ("user_id", "email", "updated_at") VALUES (?, ?, ?)"#,
        user_id,
        email,
        updated_at
    )
    .execute(db())
    .await?;

    Ok(())
}

/// Remove a single email from user's emails
/// Follows the same pattern as user-service remove_user_email
pub async fn remove_user_email(user_id: &str, email: &str) -> Result<()> {
    // Remove the email
    sqlx::query!(
        r#"DELETE FROM "user_emails_cache" WHERE "user_id" = ? AND "email" = ?"#,
        user_id,
        email
    )
    .execute(db())
    .await?;

    Ok(())
}

/// Remove all emails for a user from cache
pub async fn remove_user_emails_from_cache(user_id: &str) -> Result<()> {
    sqlx::query!(
        r#"DELETE FROM "user_emails_cache" WHERE "user_id" = ?"#,
        user_id
    )
    .execute(db())
    .await?;

    Ok(())
}

// ================================
// BULK OPERATIONS
// ================================

/// Bulk update multiple users' tags - useful for batch operations
pub async fn bulk_update_user_tags(
    updates: std::collections::HashMap<String, Vec<String>>,
) -> Result<()> {
    let updated_at = jiff::Timestamp::now().to_string();

    let mut tx = Immediate::begin_with_retry(db(), None, None).await?;

    for (user_id, tags) in updates {
        // Remove all existing tags for this user
        sqlx::query!(
            r#"DELETE FROM "user_tags_cache" WHERE "user_id" = ?"#,
            user_id
        )
        .execute(&mut tx)
        .await?;

        // Insert new tags
        for tag in &tags {
            sqlx::query!(
                r#"INSERT INTO "user_tags_cache" ("user_id", "tag", "updated_at") VALUES (?, ?, ?)"#,
                user_id,
                tag,
                updated_at
            )
            .execute(&mut tx)
            .await?;
        }
    }

    tx.commit().await?;
    Ok(())
}

/// Bulk update multiple users' emails
pub async fn bulk_update_user_emails(
    updates: std::collections::HashMap<String, Vec<String>>,
) -> Result<()> {
    let updated_at = jiff::Timestamp::now().to_string();

    let mut tx = Immediate::begin_with_retry(db(), None, None).await?;

    for (user_id, emails) in updates {
        // Remove all existing emails for this user
        sqlx::query!(
            r#"DELETE FROM "user_emails_cache" WHERE "user_id" = ?"#,
            user_id
        )
        .execute(&mut tx)
        .await?;

        // Insert new emails
        for email in &emails {
            sqlx::query!(
                r#"INSERT INTO "user_emails_cache" ("user_id", "email", "updated_at") VALUES (?, ?, ?)"#,
                user_id,
                email,
                updated_at
            )
            .execute(&mut tx)
            .await?;
        }
    }

    tx.commit().await?;
    Ok(())
}

// ================================
// CACHE STATISTICS AND MAINTENANCE
// ================================

/// Cache statistics and health checks
pub async fn get_cache_stats() -> Result<CacheStats> {
    // Get user_tags_cache stats
    let total_users_with_tags =
        sqlx::query_scalar!(r#"SELECT COUNT(DISTINCT "user_id") FROM "user_tags_cache""#)
            .fetch_one(db())
            .await?;

    let total_tag_entries = sqlx::query_scalar!(r#"SELECT COUNT(*) FROM "user_tags_cache""#)
        .fetch_one(db())
        .await?;

    // Get user_emails_cache stats
    let total_users_with_emails =
        sqlx::query_scalar!(r#"SELECT COUNT(*) FROM "user_emails_cache""#)
            .fetch_one(db())
            .await?;

    Ok(CacheStats {
        total_users_with_tags: total_users_with_tags as usize,
        total_tag_entries: total_tag_entries as usize,
        total_users_with_emails: total_users_with_emails as usize,
        cache_hit_rate: None, // TODO: implement cache hit tracking if needed
    })
}

#[derive(Debug)]
pub struct CacheStats {
    pub total_users_with_tags: usize,
    pub total_tag_entries: usize,
    pub total_users_with_emails: usize,
    pub cache_hit_rate: Option<f64>,
}

/// Clean up old cache entries (optional maintenance function)
pub async fn cleanup_old_cache_entries(days_old: i64) -> Result<()> {
    let cutoff_date = (jiff::Timestamp::now() - jiff::Span::new().days(days_old)).to_string();

    sqlx::query!(
        r#"DELETE FROM "user_tags_cache" WHERE "updated_at" < ?"#,
        cutoff_date
    )
    .execute(db())
    .await?;

    sqlx::query!(
        r#"DELETE FROM "user_emails_cache" WHERE "updated_at" < ?"#,
        cutoff_date
    )
    .execute(db())
    .await?;

    // Cleanup completed successfully - no output needed for library function

    Ok(())
}
