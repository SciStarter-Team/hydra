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

pub async fn check_content(req: &Request) -> Result<ContentFilterResult> {
    let content: String = flexbuffers::from_slice(&req.message.payload)?;
    check_content_internal(&content).await
}

pub async fn check_content_internal(content: &str) -> Result<ContentFilterResult> {
    let db = db();

    // Get all bad words from database
    let bad_words: Vec<String> = sqlx::query_scalar!(r#"SELECT "word" FROM "bad_words""#)
        .fetch_all(db)
        .await?
        .into_iter()
        .flatten()
        .collect();

    let mut flagged_words = Vec::new();

    // Check for bad words (whole word matches only)
    for bad_word in &bad_words {
        let bad_word = bad_word.to_uppercase();
        let regex_pattern = format!(r"\b{}\b", regex::escape(&bad_word));
        if let Ok(re) = regex::RegexBuilder::new(&regex_pattern)
            .case_insensitive(true)
            .build()
        {
            if re.is_match(content) {
                flagged_words.push(bad_word.clone());
            }
        }
    }

    let is_allowed = flagged_words.is_empty();

    Ok(ContentFilterResult {
        is_allowed,
        flagged_words,
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
    let now = TimeString::now();
    let now_str = now.to_string();

    sqlx::query!(
        r#"INSERT INTO "bad_words" ("word", "created_at") VALUES (upper(?), ?) ON CONFLICT ("word") DO NOTHING"#,
        word,
        now_str
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

    sqlx::query!(r#"DELETE FROM "bad_words" WHERE "word" = upper(?)"#, word)
        .execute(db)
        .await?;

    Ok(())
}
