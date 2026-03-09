use anyhow::Error;
use async_nats::service::Request;
use common::{
    message::user::{
        ProjectJoinCheckRequest, ProjectJoinCheckResponse, ProjectJoinRequest,
        ProjectJoinResponse, UserJoinEntry, UserJoinsListRequest, UserJoinsListResponse,
    },
    service::{decode_structured_message, log_activity},
    shared::db,
    timestring::TimeString,
};

pub async fn create_join(req: &Request) -> Result<ProjectJoinResponse, Error> {
    let message: ProjectJoinRequest = decode_structured_message(req.message.clone())?;

    let user_id_str = message.user_id.to_string();

    // Insert project join
    sqlx::query!(
        r#"
        INSERT INTO project_joins (user_id, project_id)
        VALUES (?, ?)
        ON CONFLICT(user_id, project_id) DO NOTHING
        "#,
        user_id_str,
        message.project_id
    )
    .execute(db())
    .await?;

    log_activity(
        Some(TimeString::now()),
        "project_join",
        Some(message.user_id),
        None::<ulid::Ulid>,
        Some(message.project_id),
    )
    .await?;

    Ok(ProjectJoinResponse {
        success: true,
        message: "Project joined successfully".to_string(),
    })
}

pub async fn check_join(req: &Request) -> Result<ProjectJoinCheckResponse, Error> {
    let message: ProjectJoinCheckRequest = decode_structured_message(req.message.clone())?;

    let user_id_str = message.user_id.to_string();

    let result = sqlx::query_scalar!(
        r#"
        SELECT 1 as "result: i32"
        FROM project_joins
        WHERE user_id = ? AND project_id = ?
        LIMIT 1
        "#,
        user_id_str,
        message.project_id
    )
    .fetch_optional(db())
    .await?;

    Ok(ProjectJoinCheckResponse {
        has_joined: result.is_some(),
    })
}

pub async fn list_user_joins(req: &Request) -> Result<UserJoinsListResponse, Error> {
    let message: UserJoinsListRequest = decode_structured_message(req.message.clone())?;

    // Look up user_id from opaque_id
    let user_id = sqlx::query_scalar!(
        "SELECT user_id FROM users WHERE opaque_id = ? LIMIT 1",
        message.opaque_id
    )
    .fetch_optional(db())
    .await?;

    let Some(user_id) = user_id else {
        return Ok(UserJoinsListResponse {
            found: false,
            projects: vec![],
        });
    };

    let rows = sqlx::query_scalar!(
        "SELECT project_id FROM project_joins WHERE user_id = ?",
        user_id
    )
    .fetch_all(db())
    .await?;

    Ok(UserJoinsListResponse {
        found: true,
        projects: rows
            .into_iter()
            .map(|project_id| UserJoinEntry { project_id })
            .collect(),
    })
}
