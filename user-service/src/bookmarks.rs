use anyhow::Error;
use async_nats::service::Request;
use common::{
    message::user::{
        BookmarkCheckRequest, BookmarkCheckResponse, BookmarkCreateRequest, BookmarkCreateResponse,
        EntityCountsRequest, EntityCountsResponse,
    },
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

pub async fn list_by_user(req: &Request) -> Result<Vec<String>, Error> {
    let user_id: String = flexbuffers::from_slice(&req.message.payload)?;

    let project_ids = sqlx::query_scalar!(
        r#"SELECT "project_id"
        FROM "bookmarks"
        WHERE "user_id" = ? AND "active" = 1
        ORDER BY "activity" DESC"#,
        user_id
    )
    .fetch_all(db())
    .await?;

    Ok(project_ids)
}

/// How many users have an active bookmark on each of the requested
/// projects. Serves the dashboard's owned-entity "saves" figures.
pub async fn count_by_projects(req: &Request) -> Result<EntityCountsResponse, Error> {
    let message: EntityCountsRequest = decode_structured_message(req.message.clone())?;

    let mut counts = Vec::new();
    for entity_id in &message.entity_ids {
        let count = sqlx::query_scalar!(
            r#"SELECT COUNT(*) as "count!: i64"
            FROM "bookmarks"
            WHERE "project_id" = ? AND "active" = 1"#,
            entity_id
        )
        .fetch_one(db())
        .await?;

        if count > 0 {
            counts.push((entity_id.clone(), count));
        }
    }

    Ok(EntityCountsResponse { counts })
}

pub async fn check_bookmark(req: &Request) -> Result<BookmarkCheckResponse, Error> {
    let message: BookmarkCheckRequest = decode_structured_message(req.message.clone())?;

    let user_id_str = message.user_id.to_string();

    let result = sqlx::query_scalar!(
        r#"
        SELECT 1 as "result: i32"
        FROM bookmarks
        WHERE user_id = ? AND project_id = ? AND active = 1
        LIMIT 1
        "#,
        user_id_str,
        message.project_id
    )
    .fetch_optional(db())
    .await?;

    Ok(BookmarkCheckResponse {
        has_bookmark: result.is_some(),
    })
}
