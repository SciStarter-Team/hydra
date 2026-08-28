use anyhow::Error;
use async_nats::service::Request;
use common::{
    message::user::{
        EntityCountsRequest, EntityCountsResponse, JoinCountsByUsersRequest,
        JoinCountsByUsersResponse, ProjectJoinCheckRequest, ProjectJoinCheckResponse,
        ProjectJoinRequest, ProjectJoinResponse, UserJoinEntry, UserJoinsListRequest,
        UserJoinsListResponse,
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

/// How many users have joined each of the requested projects. Serves
/// the dashboard's owned-entity "joins" figures.
pub async fn count_by_projects(req: &Request) -> Result<EntityCountsResponse, Error> {
    let message: EntityCountsRequest = decode_structured_message(req.message.clone())?;

    let mut counts = Vec::new();
    for entity_id in &message.entity_ids {
        let count = sqlx::query_scalar!(
            r#"SELECT COUNT(*) as "count!: i64"
            FROM "project_joins"
            WHERE "project_id" = ?"#,
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

/// How many projects each of these people has joined
/// (`user.joins.count_by_users`).
///
/// One grouped query rather than a count per person: the curated-page staff
/// report asks about everyone on a page at once, which can be thousands of
/// people.
pub async fn count_by_users(req: &Request) -> Result<JoinCountsByUsersResponse, Error> {
    use sqlx::{QueryBuilder, Row, Sqlite};

    let message: JoinCountsByUsersRequest = decode_structured_message(req.message.clone())?;

    if message.user_ids.is_empty() {
        return Ok(JoinCountsByUsersResponse::default());
    }

    let mut counts = Vec::new();

    // SQLite caps a statement at 999 bound parameters by default.
    for chunk in message.user_ids.chunks(900) {
        let mut qb: QueryBuilder<Sqlite> = QueryBuilder::new(
            r#"SELECT "user_id", COUNT(*) AS "count" FROM "project_joins" WHERE "user_id" IN ("#,
        );
        let mut separated = qb.separated(", ");
        for user_id in chunk {
            separated.push_bind(user_id.clone());
        }
        qb.push(r#") GROUP BY "user_id""#);

        for row in qb.build().fetch_all(db()).await? {
            counts.push((row.get::<String, _>("user_id"), row.get::<i64, _>("count")));
        }
    }

    Ok(JoinCountsByUsersResponse { counts })
}

pub async fn list_by_user_id(req: &Request) -> Result<Vec<String>, Error> {
    let user_id: String = flexbuffers::from_slice(&req.message.payload)?;

    let rows = sqlx::query_scalar!(
        "SELECT project_id FROM project_joins WHERE user_id = ?",
        user_id
    )
    .fetch_all(db())
    .await?;

    Ok(rows)
}

/// The user_ids of everyone who has joined `project_id` — the project's
/// participants. Serves the entity broadcast composer, which messages all of
/// a project's participants at once. The request payload is the project_id
/// string (mirrors `list_by_user_id`).
pub async fn list_by_project_id(req: &Request) -> Result<Vec<String>, Error> {
    let project_id: String = flexbuffers::from_slice(&req.message.payload)?;

    let rows = sqlx::query_scalar!(
        "SELECT user_id FROM project_joins WHERE project_id = ?",
        project_id
    )
    .fetch_all(db())
    .await?;

    Ok(rows)
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
