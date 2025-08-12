use anyhow::Result;
use duckdb::params;

use crate::database::create_connection;

// User tags cache management functions
pub async fn update_user_tags_cache(user_id: String, tags: Vec<String>) -> Result<()> {
    let conn = create_connection()?;
    
    // Use DuckDB's conflict resolution instead of explicit transactions
    // First, remove existing tags for this user
    conn.execute("DELETE FROM user_tags WHERE user_id = ?", params![user_id])?;
    
    // Insert new tags using INSERT OR IGNORE for conflict resolution
    for tag in tags {
        conn.execute(
            "INSERT OR IGNORE INTO user_tags (user_id, tag) VALUES (?, ?)",
            params![user_id, tag]
        )?;
    }
    
    Ok(())
}

#[allow(dead_code)]
pub async fn get_user_tags_from_cache(user_id: &str) -> Result<Option<Vec<String>>> {
    let conn = create_connection()?;
    
    let mut stmt = conn.prepare("SELECT tag FROM user_tags WHERE user_id = ? ORDER BY tag")?;
    let tag_iter = stmt.query_map([user_id], |row| {
        Ok(row.get::<_, String>(0)?)
    })?;
    
    let tags: Result<Vec<String>, _> = tag_iter.collect();
    let tags = tags?;
    
    if tags.is_empty() {
        Ok(None) // No cache entry found
    } else {
        Ok(Some(tags))
    }
}

#[allow(dead_code)]
pub async fn fetch_user_tags_from_service(user_id: &str) -> Result<Vec<String>> {
    use common::service::request;
    
    // Call user service to get user tags
    let tags: Vec<String> = request("user.tags.get", user_id).await.unwrap_or_default();
    Ok(tags)
}

#[allow(dead_code)]
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
            if update_user_tags_cache(user_id.to_string(), tags.clone()).await.is_ok() {
                break;
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        }
    }
    
    Ok(tags)
}

#[allow(dead_code)]
pub async fn add_user_tag(user_id: &str, tag: &str) -> Result<()> {
    let conn = create_connection()?;
    conn.execute(
        "INSERT OR IGNORE INTO user_tags (user_id, tag) VALUES (?, ?)",
        params![user_id, tag]
    )?;
    Ok(())
}

#[allow(dead_code)]
pub async fn remove_user_tag(user_id: &str, tag: &str) -> Result<()> {
    let conn = create_connection()?;
    conn.execute(
        "DELETE FROM user_tags WHERE user_id = ? AND tag = ?",
        params![user_id, tag]
    )?;
    Ok(())
}

#[allow(dead_code)]
pub async fn remove_user_from_cache(user_id: &str) -> Result<()> {
    let conn = create_connection()?;
    conn.execute("DELETE FROM user_tags WHERE user_id = ?", params![user_id])?;
    Ok(())
}

#[allow(dead_code)]
pub async fn get_users_with_tag(tag: &str) -> Result<Vec<String>> {
    let conn = create_connection()?;
    
    let mut stmt = conn.prepare("SELECT DISTINCT user_id FROM user_tags WHERE tag = ?")?;
    let user_iter = stmt.query_map([tag], |row| {
        Ok(row.get::<_, String>(0)?)
    })?;
    
    let users: Result<Vec<String>, _> = user_iter.collect();
    users.map_err(Into::into)
}

#[allow(dead_code)]
pub async fn get_popular_tags(limit: usize) -> Result<Vec<(String, i64)>> {
    let conn = create_connection()?;
    
    let mut stmt = conn.prepare("SELECT tag, COUNT(*) as count FROM user_tags GROUP BY tag ORDER BY count DESC LIMIT ?")?;
    let tag_iter = stmt.query_map([limit], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?;
    
    let tags: Result<Vec<(String, i64)>, _> = tag_iter.collect();
    tags.map_err(Into::into)
}