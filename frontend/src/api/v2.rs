// V2 API endpoints for participation recording
// Ported from orrery/orrery/api/views/v2.py

use poem::{handler, web::{Path, Form, Query}, Response, IntoResponse};
use serde::Deserialize;
use anyhow::Error;
use ulid::Ulid;
use common::message::user::{IdentifierType, UserMatchRequest, UserMatchResponse, UserOpaqueIdRequest, UserOpaqueIdResponse, ValidateApiKeyRequest, ValidateApiKeyResponse, WorkflowProgressRequest, WorkflowProgressResponse, WorkflowType};
use common::message::log::ProjectParticipationRecord;
use common::service::{request, send};
use common::shared::db;
use crate::middleware::authorization::Authenticated;

/// Form data for participation recording
#[derive(Debug, Deserialize)]
struct ParticipationForm {
    #[serde(rename = "where")]
    where_point: Option<String>,  // GeoJSON point
    when: Option<String>,          // ISO datetime
    count: Option<i32>,
    #[serde(rename = "type")]
    event_type: Option<String>,
    origin: Option<i16>,
    duration: Option<f64>,
    magnitude: Option<i32>,
    fidelity: Option<f64>,
    extra: Option<String>,

    // Identifier fields (depend on endpoint)
    hashed: Option<String>,
    opaque: Option<String>,
    email: Option<String>,
    #[serde(rename = "Email")]  // Handle both cases
    email_cap: Option<String>,
    token: Option<String>,
    #[serde(rename = "Token")]  // Handle both cases
    token_cap: Option<String>,

    // API key for authentication
    key: Option<String>,
}

/// Query params for API key authentication
#[derive(Debug, Deserialize)]
struct ApiKeyQuery {
    key: Option<String>,
}

/// Lightweight helper to get entity_id (target) from slug
async fn get_entity_id_for_slug(slug: &str) -> Result<String, Error> {
    let row = sqlx::query!(
        "SELECT target FROM entities WHERE slug = ? LIMIT 1",
        slug
    )
    .fetch_one(db())
    .await?;

    Ok(row.target)
}

/// Check if an API key is valid and has permission to write to a project
async fn check_api_key_permission(api_key: &str, project_id: &str) -> Result<bool, Error> {
    // Validate the API key
    let validation: ValidateApiKeyResponse = request(
        "user.api_key.validate",
        ValidateApiKeyRequest {
            api_key: api_key.to_string(),
        }
    ).await?;

    if !validation.valid {
        return Ok(false);
    }

    // Staff/superuser can always write
    if validation.is_staff || validation.is_superuser {
        return Ok(true);
    }

    let Some(user_id) = validation.user_id else {
        return Ok(false);
    };

    // Check if user is the project page creator or a page manager
    let has_permission = sqlx::query_scalar!(
        r#"SELECT 1 as "has_permission: bool" FROM projects
         WHERE project_id = ? AND page_creator_id = ?
         UNION
         SELECT 1 FROM project_page_editors
         WHERE project_id = ? AND user_id = ?
         LIMIT 1"#,
        project_id,
        user_id,
        project_id,
        user_id
    )
    .fetch_optional(db())
    .await?;

    Ok(has_permission.is_some())
}

/// Record participation event helper
/// Returns None if user doesn't exist (for privacy)
async fn record_participation(
    slug: &str,
    identifier: &str,
    identifier_type: IdentifierType,
    form: ParticipationForm,
    api_key: Option<String>,
    origin_override: i16,
) -> Result<Response, Error> {
    // Check if event type is "incidental" - don't record these
    if let Some(ref event_type) = form.event_type {
        if event_type == "incidental" {
            return Ok(poem::web::Json(serde_json::json!({
                "result": "Not saving incidental event"
            })).into_response());
        }
    }

    // Verify project exists and get entity_id
    let entity_id = get_entity_id_for_slug(slug).await
        .map_err(|_| anyhow::anyhow!("Project slug does not identify a project"))?;

    // Match user by identifier
    let user_match: UserMatchResponse = request(
        "user.match",
        UserMatchRequest {
            identifier: identifier.to_string(),
            identifier_type,
        }
    ).await?;

    let Some(user_id_str) = user_match.user_id else {
        // User doesn't exist - return success but don't record (privacy)
        return Ok(poem::web::Json(serde_json::json!({
            "result": "success",
            "operation": "discard",
            "identifier": identifier
        })).into_response());
    };

    let user_id = Ulid::from_string(&user_id_str)?;

    // Check API key if provided
    let authenticated = if let Some(ref key) = api_key {
        check_api_key_permission(key, &entity_id).await?
    } else {
        false
    };

    // Determine origin (priority: origin param, form.origin, override)
    let origin = form.origin.unwrap_or(origin_override);

    // Record the participation
    send(
        "log.project_participation_record",
        ProjectParticipationRecord {
            user_id,
            project_id: entity_id,
            origin: Some(origin),
            when: form.when,
            duration: form.duration,
            where_point: form.where_point,
            magnitude: form.magnitude,
            authenticated: Some(authenticated),
            extra: form.extra.or(Some(String::new())),
            fidelity: form.fidelity,
            task_name: form.event_type,
        }
    ).await?;

    Ok(poem::web::Json(serde_json::json!({
        "result": "success",
        "operation": "record"
    })).into_response())
}

/// Record participation event via hashed email
/// POST /api/participation/hashed/:slug
#[handler]
pub async fn record_event_hashed(
    Path(slug): Path<String>,
    Query(query): Query<ApiKeyQuery>,
    Form(mut form): Form<ParticipationForm>,
) -> Result<Response, Error> {
    // Merge query key into form key
    if form.key.is_none() {
        form.key = query.key;
    }

    let hashed = form.hashed.clone()
        .ok_or_else(|| anyhow::anyhow!("Missing 'hashed' field"))?;

    record_participation(
        &slug,
        &hashed,
        IdentifierType::Hashed,
        form,
        None, // API key handled separately for hashed endpoint
        3, // origin 3 = hashed email
    ).await
}

#[derive(Debug, Deserialize)]
struct ConnectQuery {
    #[serde(rename = "allow-anonymous")]
    allow_anonymous: Option<bool>,
}

/// Helper function for connect logic
async fn do_connect_opaque(
    slug: String,
    idx: usize,
    allow_anonymous: bool,
    user_info: Option<std::sync::Arc<common::message::user::BasicInfo>>,
) -> Result<Response, Error> {
    // Get entity_id (target) from slug
    let entity_id = get_entity_id_for_slug(&slug).await
        .map_err(|_| anyhow::anyhow!("Project slug does not identify a project"))?;

    // Query the appropriate connect URL from projects table
    let target = match idx {
        1 => {
            sqlx::query_scalar!(
                "SELECT connect_opaque_url1 FROM projects WHERE project_id = ? LIMIT 1",
                entity_id
            )
            .fetch_one(db())
            .await?
        }
        2 => {
            sqlx::query_scalar!(
                "SELECT connect_opaque_url2 FROM projects WHERE project_id = ? LIMIT 1",
                entity_id
            )
            .fetch_one(db())
            .await?
        }
        _ => {
            sqlx::query_scalar!(
                "SELECT connect_opaque_url FROM projects WHERE project_id = ? LIMIT 1",
                entity_id
            )
            .fetch_one(db())
            .await?
        }
    };

    if target.is_empty() {
        return Err(anyhow::anyhow!("Project is not set up for this feature"));
    }

    // If user is authenticated, append opaque_id
    if let Some(user) = user_info.as_ref() {
        // Get opaque_id from user-service
        let opaque_response: UserOpaqueIdResponse = request(
            "user.opaque_id.get",
            UserOpaqueIdRequest {
                user_id: user.user_id.to_string(),
            }
        ).await?;

        // If opaque_id exists, append it to URL
        if let Some(opaque_id) = opaque_response.opaque_id {
            let redirect_url = if target.contains('?') {
                format!("{}&scistarter_id={}", target, opaque_id)
            } else {
                format!("{}?scistarter_id={}", target, opaque_id)
            };
            Ok(poem::web::Redirect::see_other(redirect_url).into_response())
        } else {
            // No opaque_id, redirect without it
            Ok(poem::web::Redirect::see_other(target).into_response())
        }
    } else if allow_anonymous {
        // Allow anonymous redirect
        Ok(poem::web::Redirect::see_other(target).into_response())
    } else {
        // Not authenticated and not allowing anonymous - redirect to login
        Ok(poem::web::Redirect::see_other(format!("/login?next=/api/participation/connect/{}", slug)).into_response())
    }
}

/// Redirect to project with opaque_id
/// GET /api/participation/connect/:slug
#[handler]
pub async fn connect_opaque(
    Path(slug): Path<String>,
    Query(query): Query<ConnectQuery>,
    Authenticated(user_info): Authenticated,
) -> Result<Response, Error> {
    do_connect_opaque(slug, 0, query.allow_anonymous.unwrap_or(false), user_info).await
}

/// Redirect to project with opaque_id (with index)
/// GET /api/participation/connect/:slug/:idx
#[handler]
pub async fn connect_opaque_with_idx(
    Path((slug, idx)): Path<(String, usize)>,
    Query(query): Query<ConnectQuery>,
    Authenticated(user_info): Authenticated,
) -> Result<Response, Error> {
    do_connect_opaque(slug, idx, query.allow_anonymous.unwrap_or(false), user_info).await
}

/// Record participation event via opaque user ID
/// POST /api/participation/opaque/:slug
#[handler]
pub async fn record_event_opaque(
    Path(slug): Path<String>,
    Query(query): Query<ApiKeyQuery>,
    Form(mut form): Form<ParticipationForm>,
) -> Result<Response, Error> {
    // Merge query key into form key
    if form.key.is_none() {
        form.key = query.key;
    }

    let opaque = form.opaque.clone()
        .ok_or_else(|| anyhow::anyhow!("Missing 'opaque' field"))?;

    let api_key = form.key.clone();

    record_participation(
        &slug,
        &opaque,
        IdentifierType::Opaque,
        form,
        api_key,
        7, // origin 7 = opaque ID
    ).await
}

/// Record participation event via plaintext email
/// POST /api/participation/email/:slug
#[handler]
pub async fn record_event_email(
    Path(slug): Path<String>,
    Query(query): Query<ApiKeyQuery>,
    Form(mut form): Form<ParticipationForm>,
) -> Result<Response, Error> {
    // Merge query key into form key
    if form.key.is_none() {
        form.key = query.key;
    }

    // Handle both 'email' and 'Email' fields
    let email = form.email.clone()
        .or(form.email_cap.clone())
        .ok_or_else(|| anyhow::anyhow!("The email value is required"))?;

    let api_key = form.key.clone();

    record_participation(
        &slug,
        &email,
        IdentifierType::Email,
        form,
        api_key,
        8, // origin 8 = plaintext email
    ).await
}

/// Record participation event via link token
/// POST /api/participation/link-token/:slug
#[handler]
pub async fn record_event_link_token(
    Path(slug): Path<String>,
    Query(query): Query<ApiKeyQuery>,
    Form(mut form): Form<ParticipationForm>,
) -> Result<Response, Error> {
    // Merge query key into form key
    if form.key.is_none() {
        form.key = query.key;
    }

    // Handle both 'token' and 'Token' fields
    let token = form.token.clone()
        .or(form.token_cap.clone())
        .ok_or_else(|| anyhow::anyhow!("The token value is required"))?;

    let api_key = form.key.clone();

    record_participation(
        &slug,
        &token,
        IdentifierType::LinkToken,
        form,
        api_key,
        9, // origin 9 = link token
    ).await
}

/// Record Articulate completion event
/// POST /api/participation/articulate
#[handler]
pub async fn record_event_articulate(
    Form(form): Form<ArticulateForm>,
) -> Result<Response, Error> {
    use jsonwebtoken::{decode, DecodingKey, Validation, Algorithm};

    // Get JWT secret from compile-time environment
    let jwt_secret = env!("SCISTARTER_JWT_SECRET");

    // Decode and validate JWT
    let token_data = decode::<ArticulateClaims>(
        &form.token,
        &DecodingKey::from_secret(jwt_secret.as_bytes()),
        &Validation::new(Algorithm::HS256),
    ).map_err(|e| anyhow::anyhow!("Invalid JWT token: {}", e))?;

    let claims = token_data.claims;

    // Normalize activity: lowercase and remove trailing slash
    let activity = form.activity.trim_end_matches('/').to_lowercase();

    // Extract ULID from pseudo-email format (e.g., "01HXYZ...@articulate.scistarter.org")
    let user_id_str = claims.email.split('@').next()
        .ok_or_else(|| anyhow::anyhow!("Invalid JWT email format"))?
        .to_string();

    // Validate it's a proper ULID
    let user_id = Ulid::from_string(&user_id_str)
        .map_err(|_| anyhow::anyhow!("JWT email does not contain valid ULID"))?;

    // Process based on activity type
    match activity.as_str() {
        // Foundations of Citizen Science - records participation event
        "http://nnlmhour1tutorial" | "http://_lmq5j-gx0q2iuk_qw8moe6fkvtns-_-_rise" | "http://foundationsstandardized" => {
            // Record workflow progress
            let _: WorkflowProgressResponse = request(
                "user.workflow.progress",
                WorkflowProgressRequest {
                    user_id: user_id_str.clone(),
                    workflow: WorkflowType::Foundations,
                    step: "tutorial".to_string(),
                }
            ).await?;

            // Get entity_id for foundations project
            let entity_id = get_entity_id_for_slug("foundations-of-citizen-science-training-module").await
                .map_err(|_| anyhow::anyhow!("Foundations project not found"))?;

            // Record the participation
            send(
                "log.project_participation_record",
                ProjectParticipationRecord {
                    user_id,
                    project_id: entity_id,
                    origin: Some(10), // origin 10 = Articulate
                    when: None,
                    duration: None,
                    where_point: None,
                    magnitude: None,
                    authenticated: Some(true), // JWT-authenticated
                    extra: Some(String::new()),
                    fidelity: None,
                    task_name: Some("collection".to_string()),
                }
            ).await?;
        }

        // Libraries as Community Hubs
        "http://t-ykenupqd7zp9otmd-mitgnjh9kxxq6_rise" | "http://kv84cf5vh3hzzsgplewwprpgydhuf7y6_rise" | "http://libsascommhubsstandardized" => {
            let _: WorkflowProgressResponse = request(
                "user.workflow.progress",
                WorkflowProgressRequest {
                    user_id: user_id_str,
                    workflow: WorkflowType::LibrariesHubs,
                    step: "tutorial".to_string(),
                }
            ).await?;
        }

        // Data Literacy
        "http://dataliteracywithbadge" | "http://dataliteracywithbadgestandardized" | "http://dataliteracyspanishwithbadge" => {
            let _: WorkflowProgressResponse = request(
                "user.workflow.progress",
                WorkflowProgressRequest {
                    user_id: user_id_str,
                    workflow: WorkflowType::DataLiteracy,
                    step: "tutorial".to_string(),
                }
            ).await?;
        }

        // Teaching in Higher Ed
        "http://teachinginhighered" | "http://teachinginhigheredstandardized" => {
            let _: WorkflowProgressResponse = request(
                "user.workflow.progress",
                WorkflowProgressRequest {
                    user_id: user_id_str,
                    workflow: WorkflowType::HigherEd,
                    step: "tutorial".to_string(),
                }
            ).await?;
        }

        // Data Ethics
        "http://dataethicsxapisept72022" => {
            let _: WorkflowProgressResponse = request(
                "user.workflow.progress",
                WorkflowProgressRequest {
                    user_id: user_id_str,
                    workflow: WorkflowType::DataEthics,
                    step: "tutorial".to_string(),
                }
            ).await?;
        }

        // IDEAL
        "http://wzki5lztd4rrc1tn39i4pwca_wssx1yw_rise" => {
            let _: WorkflowProgressResponse = request(
                "user.workflow.progress",
                WorkflowProgressRequest {
                    user_id: user_id_str,
                    workflow: WorkflowType::Ideal,
                    step: "tutorial".to_string(),
                }
            ).await?;
        }

        // Ambassador
        "http://ambassador_rise_xapi" | "http://re3jvlrvw-cibju89pefhvvnoma-60ig_rise" => {
            let _: WorkflowProgressResponse = request(
                "user.workflow.progress",
                WorkflowProgressRequest {
                    user_id: user_id_str,
                    workflow: WorkflowType::Ambassador,
                    step: "tutorial".to_string(),
                }
            ).await?;
        }

        _ => {
            tracing::warn!("Unknown Articulate activity: '{}'", activity);
        }
    }

    Ok(poem::web::Json(serde_json::json!({
        "result": "ok"
    })).into_response())
}

#[derive(Debug, Deserialize)]
struct ArticulateForm {
    token: String,
    activity: String,
}

#[derive(Debug, serde::Deserialize)]
struct ArticulateClaims {
    email: String,
    #[serde(default)]
    exp: Option<u64>,
}
