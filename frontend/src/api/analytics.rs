// Analytics and recommendations API endpoints
// Ported from orrery/orrery/api/views/recommendations.py

use anyhow::Error;
use common::{
    message::{
        log::{UserFirstParticipationRequest, UserFirstParticipationResponse},
        user::{
            AiOptedOutRequest, AiOptedOutResponse, IdentifierType, UserJoinsListRequest,
            UserJoinsListResponse, UserMatchRequest, UserMatchResponse, ValidateApiKeyRequest,
            ValidateApiKeyResponse,
        },
    },
    service::request,
    shared::db,
};
use poem::{
    IntoResponse, Response, handler,
    web::{Json, Query},
};
use serde::Deserialize;

// ============================================================================
// GET /api/affiliated
// ============================================================================

#[handler]
pub async fn affiliated() -> Result<Response, Error> {
    let rows = sqlx::query!(
        r#"SELECT e.slug, p.legacy_id
           FROM projects p
           JOIN entities e ON e.target = p.project_id
           WHERE p.is_affiliate = 1"#
    )
    .fetch_all(db())
    .await?;

    let projects: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|row| {
            serde_json::json!({
                "project_id": row.legacy_id.unwrap_or(0),
                "slug": row.slug
            })
        })
        .collect();

    Ok(Json(serde_json::json!({ "projects": projects })).into_response())
}

// ============================================================================
// GET /api/ai-opted-out
// ============================================================================

#[handler]
pub async fn ai_opted_out() -> Result<Response, Error> {
    let response: AiOptedOutResponse =
        request("user.profile.ai_opted_out", AiOptedOutRequest {}).await?;

    let mut opted_out = serde_json::Map::new();
    for entry in response.opted_out {
        opted_out.insert(
            entry.opaque_id,
            entry
                .opted_out_at
                .map(serde_json::Value::String)
                .unwrap_or(serde_json::Value::Null),
        );
    }

    Ok(Json(serde_json::json!({ "opted_out": opted_out })).into_response())
}

// ============================================================================
// GET /api/check-join
// ============================================================================

#[derive(Debug, Deserialize)]
struct CheckJoinQuery {
    key: Option<String>,
    profile: Option<String>,
}

#[handler]
pub async fn check_join(Query(query): Query<CheckJoinQuery>) -> Result<Response, Error> {
    // Validate API key
    let Some(key) = query.key else {
        return Ok(poem::http::StatusCode::NOT_FOUND.into());
    };

    let validation: ValidateApiKeyResponse = request(
        "user.api_key.validate",
        ValidateApiKeyRequest { api_key: key },
    )
    .await?;

    if !validation.valid {
        return Ok(poem::http::StatusCode::NOT_FOUND.into());
    }

    let Some(opaque_id) = query.profile else {
        return Ok(Json(serde_json::json!({"error": "No such profile"})).into_response());
    };

    let response: UserJoinsListResponse = request(
        "user.joins.list_by_user",
        UserJoinsListRequest {
            opaque_id: opaque_id.clone(),
        },
    )
    .await?;

    if !response.found {
        return Ok(Json(serde_json::json!({"error": "No such profile"})).into_response());
    }

    // The join timestamp is the user's first participation in each project,
    // which lives in the log service. Resolve the user's ULID, then build a
    // project_id -> first-participation-time map.
    let user_match: UserMatchResponse = request(
        "user.match",
        UserMatchRequest {
            identifier: opaque_id,
            identifier_type: IdentifierType::Opaque,
        },
    )
    .await?;

    let mut first_when: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    if let Some(user_id) = user_match.user_id {
        let firsts: UserFirstParticipationResponse = request(
            "log.user_first_participation",
            UserFirstParticipationRequest { user_id },
        )
        .await?;
        for entry in firsts.entries {
            first_when.insert(entry.project_id, entry.when);
        }
    }

    // Look up project names and legacy IDs for each joined project
    let mut projects = Vec::new();
    for join in response.projects {
        let row = sqlx::query!(
            r#"SELECT name, legacy_id FROM projects WHERE project_id = ?"#,
            join.project_id
        )
        .fetch_optional(db())
        .await?;

        if let Some(row) = row {
            let when = first_when
                .get(&join.project_id)
                .map(|w| serde_json::Value::String(w.clone()))
                .unwrap_or(serde_json::Value::Null);
            projects.push(serde_json::json!([
                row.name,
                row.legacy_id.unwrap_or(0),
                when
            ]));
        }
    }

    Ok(Json(serde_json::json!({"projects": projects})).into_response())
}

// ============================================================================
// GET /api/project-region-name
// ============================================================================

#[derive(Debug, Deserialize)]
struct ProjectRegionNameQuery {
    id: Option<i64>,
}

#[handler]
pub async fn project_region_name(
    Query(query): Query<ProjectRegionNameQuery>,
) -> Result<Response, Error> {
    let Some(project_id) = query.id.filter(|id| *id > 0) else {
        return Ok(poem::Response::builder()
            .status(poem::http::StatusCode::BAD_REQUEST)
            .body("id must be provided, and greater than zero"));
    };

    let row = sqlx::query!(
        r#"SELECT location_name, regions_description, city, state, country
           FROM projects WHERE legacy_id = ?"#,
        project_id
    )
    .fetch_optional(db())
    .await?;

    let Some(row) = row else {
        return Ok(poem::Response::builder()
            .status(poem::http::StatusCode::BAD_REQUEST)
            .body("id does not identify a project"));
    };

    let name = if !row.location_name.is_empty() {
        row.location_name
    } else if !row.regions_description.is_empty() {
        row.regions_description
    } else if !row.city.is_empty() && !row.state.is_empty() {
        format!("{}, {}", row.city, row.state)
    } else if !row.state.is_empty() {
        row.state
    } else if !row.country.is_empty() {
        row.country
    } else {
        String::new()
    };

    Ok(poem::Response::builder()
        .content_type("text/plain")
        .body(name))
}
