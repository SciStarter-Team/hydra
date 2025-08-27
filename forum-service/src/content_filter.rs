use anyhow::Result;
use async_nats::service::Request;
use common::{
    message::{
        forum::ContentFilterResult,
        user::{BasicInfo, UserInfo},
    },
    service::request,
    shared::db,
    timestring::TimeString,
};
use regex::Regex;
use std::sync::{Arc, LazyLock};
use tokio::sync::RwLock;

// Global cached regex - loaded once at startup and on explicit reload
static FILTER_REGEX: LazyLock<Arc<RwLock<Option<Regex>>>> = LazyLock::new(|| Arc::new(RwLock::new(None)));

pub async fn check_content(req: &Request) -> Result<ContentFilterResult> {
    let content: String = flexbuffers::from_slice(&req.message.payload)?;
    check_content_internal(&content).await
}

pub async fn check_content_internal(content: &str) -> Result<ContentFilterResult> {
    let regex_guard = FILTER_REGEX.read().await;
    
    if let Some(ref compiled_regex) = *regex_guard {
        // Find all matches and extract the matched words
        let flagged_words: Vec<String> = compiled_regex
            .find_iter(content)
            .map(|m| m.as_str().to_uppercase())
            .collect();

        let is_allowed = flagged_words.is_empty();

        Ok(ContentFilterResult {
            is_allowed,
            flagged_words,
        })
    } else {
        // No bad words loaded, allow all content
        Ok(ContentFilterResult {
            is_allowed: true,
            flagged_words: vec![],
        })
    }
}

// Initialize the filter at startup
pub async fn initialize_filter() -> Result<()> {
    let compiled_regex = build_filter_regex().await?;
    let mut regex_guard = FILTER_REGEX.write().await;
    *regex_guard = compiled_regex;
    Ok(())
}

// Reload the filter (for NATS endpoint)
pub async fn reload_filter(req: &Request) -> Result<()> {
    let user_id: ulid::Ulid = flexbuffers::from_slice(&req.message.payload)?;
    
    // Check permissions - only staff can reload filter
    let user_info: BasicInfo = request("user.basic_info.by_user_id", user_id.to_string()).await?;
    if !user_info.is_staff {
        return Err(anyhow::anyhow!("Permission denied"));
    }

    let compiled_regex = build_filter_regex().await?;
    let mut regex_guard = FILTER_REGEX.write().await;
    *regex_guard = compiled_regex;
    
    tracing::info!("Bad words filter reloaded by user {}", user_id);
    Ok(())
}

// Build the compiled regex from database
async fn build_filter_regex() -> Result<Option<Regex>> {
    let db = db();
    let bad_words: Vec<String> = sqlx::query_scalar!(r#"SELECT "word" FROM "bad_words""#)
        .fetch_all(db)
        .await?
        .into_iter()
        .flatten()
        .collect();

    if bad_words.is_empty() {
        tracing::warn!("No bad words found in database");
        return Ok(None);
    }

    // Build single regex with all bad words as alternatives
    let escaped_words: Vec<String> = bad_words
        .iter()
        .map(|word| format!(r"\b{}\b", regex::escape(word)))
        .collect();
    
    let pattern = format!(r"(?i)({})", escaped_words.join("|"));
    
    match Regex::new(&pattern) {
        Ok(compiled_regex) => {
            tracing::info!("Compiled bad words filter with {} words", bad_words.len());
            Ok(Some(compiled_regex))
        }
        Err(e) => {
            tracing::error!("Failed to compile bad words regex: {}", e);
            Ok(None)
        }
    }
}

pub async fn add_bad_word(req: &Request) -> Result<()> {
    let (user_id, word): (ulid::Ulid, String) = flexbuffers::from_slice(&req.message.payload)?;

    // Get user info to check permissions
    let user_info: BasicInfo = request("user.basic_info.by_user_id", user_id.to_string()).await?;
    if !user_info.is_staff() {
        return Err(anyhow::anyhow!("Permission denied"));
    }

    let db = db();
    let now = TimeString::now();
    let now_str = now.to_string();

    sqlx::query!(
        r#"INSERT INTO "bad_words" ("word", "created_at") VALUES (upper(?), ?) ON CONFLICT ("word") DO NOTHING"#,
        word,
        now_str
    )
    .execute(db)
    .await?;

    // Reload the filter to include the new word
    let compiled_regex = build_filter_regex().await?;
    let mut regex_guard = FILTER_REGEX.write().await;
    *regex_guard = compiled_regex;

    Ok(())
}

pub async fn remove_bad_word(req: &Request) -> Result<()> {
    let (user_id, word): (ulid::Ulid, String) = flexbuffers::from_slice(&req.message.payload)?;

    // Get user info to check permissions
    let user_info: BasicInfo = request("user.basic_info.by_user_id", user_id.to_string()).await?;
    if !user_info.is_staff() {
        return Err(anyhow::anyhow!("Permission denied"));
    }

    let db = db();

    sqlx::query!(r#"DELETE FROM "bad_words" WHERE "word" = upper(?)"#, word)
        .execute(db)
        .await?;

    // Reload the filter to remove the word
    let compiled_regex = build_filter_regex().await?;
    let mut regex_guard = FILTER_REGEX.write().await;
    *regex_guard = compiled_regex;

    Ok(())
}
