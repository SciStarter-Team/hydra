use anyhow::Error;
use async_nats::service::Request;
use common::{
    message::user::{BookmarkCreateRequest, BookmarkCreateResponse},
    service::{decode_structured_message, log_activity},
    shared::db,
    timestring::TimeString,
};

pub async fn create_bookmark(req: &Request) -> Result<BookmarkCreateResponse, Error> {
    let message: BookmarkCreateRequest = decode_structured_message(req.message.clone())?;

    let user_id_str = message.user_id.to_string();
    let now = TimeString::now();
    let now_str = now.to_string();

    // Insert or update bookmark (upsert pattern)
    sqlx::query!(
        r#"
        INSERT INTO bookmarks (user_id, project_id, created, activity, active)
        VALUES (?, ?, ?, ?, 1)
        ON CONFLICT(user_id, project_id)
        DO UPDATE SET activity = ?, active = 1
        "#,
        user_id_str,
        message.project_id,
        now_str,
        now_str,
        now_str
    )
    .execute(db())
    .await?;

    log_activity(
        Some(now),
        "bookmark_create",
        Some(message.user_id),
        None::<ulid::Ulid>,
        Some(message.project_id),
    )
    .await?;

    Ok(BookmarkCreateResponse {
        success: true,
        message: "Bookmark created successfully".to_string(),
    })
}
