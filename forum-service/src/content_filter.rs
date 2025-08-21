use anyhow::Result;
use async_nats::service::Request;
use common::{
    message::{forum::ContentFilterResult, user::{BasicInfo, UserInfo}},
    service::request,
    shared::db,
};

pub async fn check_content(req: &Request) -> Result<ContentFilterResult> {
    let content: String = flexbuffers::from_slice(&req.message.payload)?;
    check_content_internal(&content).await
}

pub async fn check_content_internal(content: &str) -> Result<ContentFilterResult> {
    let db = db();

    // Get all bad words from database
    let bad_words: Vec<String> = sqlx::query_scalar!(
        r#"SELECT "word" FROM "bad_words""#
    )
    .fetch_all(db)
    .await?
    .into_iter()
    .flatten()
    .collect();

    let content_lower = content.to_lowercase();
    let mut flagged_words = Vec::new();

    // Check for bad words
    for bad_word in &bad_words {
        if content_lower.contains(&bad_word.to_lowercase()) {
            flagged_words.push(bad_word.clone());
        }
    }

    let is_allowed = flagged_words.is_empty();

    // TODO: In the future, we could implement suggested content replacement
    let suggested_content = if !is_allowed {
        Some(replace_bad_words(content, &flagged_words))
    } else {
        None
    };

    Ok(ContentFilterResult {
        is_allowed,
        flagged_words,
        suggested_content,
    })
}

pub async fn add_bad_word(req: &Request) -> Result<()> {
    let (user_id, word): (ulid::Ulid, String) = flexbuffers::from_slice(&req.message.payload)?;
    
    // Get user info to check permissions
    let user_info: BasicInfo = request("user.basic_info.by_user_id", user_id.to_string()).await?;
    if !user_info.is_staff() {
        return Err(anyhow::anyhow!("Permission denied"));
    }

    let db = db();
    let word_lower = word.to_lowercase();

    sqlx::query!(
        r#"INSERT INTO "bad_words" ("word") VALUES (?) ON CONFLICT ("word") DO NOTHING"#,
        word_lower
    )
    .execute(db)
    .await?;

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
    let word_lower = word.to_lowercase();

    sqlx::query!(
        r#"DELETE FROM "bad_words" WHERE "word" = ?"#,
        word_lower
    )
    .execute(db)
    .await?;

    Ok(())
}

fn replace_bad_words(content: &str, bad_words: &[String]) -> String {
    let mut result = content.to_string();
    
    for bad_word in bad_words {
        let replacement = "*".repeat(bad_word.len());
        result = result.replace(&bad_word.to_lowercase(), &replacement);
        result = result.replace(&capitalize_first(&bad_word.to_lowercase()), &replacement);
        result = result.replace(&bad_word.to_uppercase(), &replacement);
    }
    
    result
}

fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_replace_bad_words() {
        let content = "This is a bad word and another Bad Word";
        let bad_words = vec!["bad".to_string(), "word".to_string()];
        
        let result = replace_bad_words(content, &bad_words);
        assert!(result.contains("***"));
        assert!(result.contains("****"));
    }

    #[test]
    fn test_capitalize_first() {
        assert_eq!(capitalize_first("hello"), "Hello");
        assert_eq!(capitalize_first(""), "");
        assert_eq!(capitalize_first("a"), "A");
    }
}