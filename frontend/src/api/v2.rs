// V2 API endpoints for participation recording
// Ported from orrery/orrery/api/views/v2.py

use crate::middleware::authorization::Authenticated;
use crate::oauth2::types::AccessToken;
use crate::peering::LoadStorePeered;
use anyhow::Error;
use common::message::log::ProjectParticipationRecord;
use common::message::user::{
    IdentifierType, UserMatchRequest, UserMatchResponse, UserOpaqueIdRequest, UserOpaqueIdResponse,
    WorkflowProgressRequest, WorkflowProgressResponse, WorkflowType,
};
use common::service::{request, send};
use common::shared::db;
use poem::{
    IntoResponse, Response, handler,
    http::HeaderMap,
    web::{Form, Path, Query},
};
use serde::Deserialize;
use ulid::Ulid;

/// Form data for participation recording
#[derive(Debug, Clone, Deserialize)]
struct ParticipationForm {
    #[serde(rename = "where")]
    where_point: Option<String>, // GeoJSON point
    when: Option<String>, // ISO datetime
    #[serde(rename = "type")]
    event_type: Option<String>,
    /// Only the literal string "widget" is meaningful here (forces origin 2);
    /// any other value is ignored and the endpoint's own origin is used.
    origin: Option<String>,
    duration: Option<f64>,
    magnitude: Option<i32>,
    fidelity: Option<f64>,
    extra: Option<String>,
    /// Number of repetitions; recorded as that many participation rows.
    count: Option<i32>,

    // Identifier fields (depend on endpoint)
    hashed: Option<String>,
    opaque: Option<String>,
    ulid: Option<String>,
    email: Option<String>,
    #[serde(rename = "Email")] // Handle both cases
    email_cap: Option<String>,
    // Note: the link-token endpoint accepts repeated `token`/`Token` values and
    // parses them via its own LinkTokenBody extractor, so they are not fields
    // here.

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
    let row = sqlx::query!("SELECT target FROM entities WHERE slug = ? LIMIT 1", slug)
        .fetch_one(db())
        .await?;

    Ok(row.target)
}

use super::check_api_key_permission;

/// 404 JSON response for an unknown project slug (matches orrery).
fn project_not_found() -> Response {
    poem::web::Json(serde_json::json!({
        "error": "Project slug does not identify a project"
    }))
    .with_status(poem::http::StatusCode::NOT_FOUND)
    .into_response()
}

/// 200 discard response when the identifier doesn't resolve to a user. `field`
/// is the identifier's field name (e.g. "hashed"), echoed back like orrery.
fn discard_success(field: &str, value: &str) -> Response {
    poem::web::Json(serde_json::json!({
        "result": "success",
        "operation": "discard",
        field: value,
    }))
    .into_response()
}

/// 400 JSON response for a missing required field (matches orrery).
fn required_field_error(message: &str) -> Response {
    poem::web::Json(serde_json::json!({ "error": message }))
        .with_status(poem::http::StatusCode::BAD_REQUEST)
        .into_response()
}

/// JSON error response with an explicit status (OAuth token/scope failures on
/// the openid endpoint, mirroring orrery's protected_resource_view 401/403).
fn oauth_error(status: poem::http::StatusCode, message: &str) -> Response {
    poem::web::Json(serde_json::json!({ "error": message }))
        .with_status(status)
        .into_response()
}

/// Record participation event helper. Resolves the project and user, then
/// delegates to `emit_participation`. Returns a discard response (echoing
/// `field`) when the identifier doesn't resolve to a user.
async fn record_participation(
    slug: &str,
    identifier: &str,
    field: &str,
    identifier_type: IdentifierType,
    form: ParticipationForm,
    api_key: Option<String>,
    origin_override: i16,
) -> Result<Response, Error> {
    // Verify project exists and get entity_id
    let Ok(entity_id) = get_entity_id_for_slug(slug).await else {
        return Ok(project_not_found());
    };

    // Match user by identifier
    let user_match: UserMatchResponse = request(
        "user.match",
        UserMatchRequest {
            identifier: identifier.to_string(),
            identifier_type,
        },
    )
    .await?;

    let Some(user_id_str) = user_match.user_id else {
        // User doesn't exist - return success but don't record (privacy)
        return Ok(discard_success(field, identifier));
    };

    let user_id = Ulid::from_string(&user_id_str)?;

    // Check API key if provided
    let authenticated = if let Some(ref key) = api_key {
        check_api_key_permission(key, &entity_id).await?
    } else {
        false
    };

    emit_participation(user_id, entity_id, authenticated, form, origin_override).await
}

/// Shared participation-recording core used by every v2 endpoint (identifier
/// resolution happens in the callers). Mirrors orrery's `_record_event`: skips
/// "incidental" events, applies the v2 defaults (duration 0.0, magnitude 1,
/// fidelity 0.5), honors only the "widget" origin override, truncates `extra`
/// to 64 KiB, and records `count` repetitions as that many participation rows.
async fn emit_participation(
    user_id: Ulid,
    entity_id: String,
    authenticated: bool,
    form: ParticipationForm,
    origin_override: i16,
) -> Result<Response, Error> {
    // "incidental" events are intentionally not recorded.
    if form.event_type.as_deref() == Some("incidental") {
        return Ok(poem::web::Json(serde_json::json!({
            "result": "Not saving incidental event"
        }))
        .into_response());
    }

    // The only meaningful caller-supplied origin is "widget" (forces origin 2);
    // otherwise the endpoint's own origin is used. Numeric overrides are ignored.
    let origin = if form.origin.as_deref() == Some("widget") {
        2
    } else {
        origin_override
    };

    // Apply orrery's v2 defaults at this (API) layer, since the log-service
    // handler's defaults differ (duration 1.0 / fidelity 1.0).
    let duration = form.duration.unwrap_or(0.0);
    let magnitude = form.magnitude.unwrap_or(1);
    let fidelity = form.fidelity.unwrap_or(0.5);
    let extra = form
        .extra
        .map(|e| e.chars().take(64 * 1024).collect::<String>())
        .unwrap_or_default();

    // Number of repetitions -> that many participation rows (hydra's data model
    // represents orrery's `repetitions` as one row per repetition).
    let count = form.count.unwrap_or(1).max(0);

    let record = ProjectParticipationRecord {
        user_id,
        project_id: entity_id,
        origin: Some(origin),
        when: form.when,
        duration: Some(duration),
        where_point: form.where_point,
        magnitude: Some(magnitude),
        authenticated: Some(authenticated),
        extra: Some(extra),
        fidelity: Some(fidelity),
        task_name: form.event_type,
    };

    for _ in 0..count {
        send("log.project_participation_record", record.clone()).await?;
    }

    Ok(poem::web::Json(serde_json::json!({
        "result": "success",
        "operation": "record"
    }))
    .into_response())
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

    // A missing hashed value resolves to no user -> discard (orrery behavior).
    let hashed = form.hashed.clone().unwrap_or_default();

    let api_key = form.key.clone();

    record_participation(
        &slug,
        &hashed,
        "hashed",
        IdentifierType::Hashed,
        form,
        api_key,
        3, // origin 3 = hashed email
    )
    .await
}

/// Record participation event via the user's ULID
/// POST /api/participation/ulid/:slug
#[handler]
pub async fn record_event_ulid(
    Path(slug): Path<String>,
    Query(query): Query<ApiKeyQuery>,
    Form(mut form): Form<ParticipationForm>,
) -> Result<Response, Error> {
    // Merge query key into form key
    if form.key.is_none() {
        form.key = query.key;
    }

    // A missing ulid resolves to no user -> discard (orrery behavior).
    let ulid = form.ulid.clone().unwrap_or_default();

    let api_key = form.key.clone();

    record_participation(
        &slug,
        &ulid,
        "ulid",
        IdentifierType::Ulid,
        form,
        api_key,
        3, // origin 3 = same as hashed/ulid identification
    )
    .await
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
    let Ok(entity_id) = get_entity_id_for_slug(&slug).await else {
        return Ok(project_not_found());
    };

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
        return Ok(poem::web::Json(serde_json::json!({
            "error": "Project is not set up for this feature"
        }))
        .with_status(poem::http::StatusCode::NOT_FOUND)
        .into_response());
    }

    // If user is authenticated, append opaque_id
    if let Some(user) = user_info.as_ref() {
        // Get opaque_id from user-service
        let opaque_response: UserOpaqueIdResponse = request(
            "user.opaque_id.get",
            UserOpaqueIdRequest {
                user_id: user.user_id.to_string(),
            },
        )
        .await?;

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
        // Not authenticated and not allowing anonymous - redirect to login,
        // preserving the original path (including the idx variant) so the user
        // returns to the same connect endpoint after authenticating.
        let next = if idx == 0 {
            format!("/api/participation/connect/{}", slug)
        } else {
            format!("/api/participation/connect/{}/{}", slug, idx)
        };
        Ok(poem::web::Redirect::see_other(format!("/login?next={}", next)).into_response())
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

    // A missing opaque value resolves to no user -> discard (orrery behavior).
    let opaque = form.opaque.clone().unwrap_or_default();

    let api_key = form.key.clone();

    record_participation(
        &slug,
        &opaque,
        "opaque",
        IdentifierType::Opaque,
        form,
        api_key,
        7, // origin 7 = opaque ID
    )
    .await
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

    // Handle both 'email' and 'Email' fields; a missing email is a 400 (orrery).
    let email = form
        .email
        .clone()
        .or(form.email_cap.clone())
        .filter(|e| !e.is_empty());
    let Some(email) = email else {
        return Ok(required_field_error("The email value is required"));
    };

    let api_key = form.key.clone();

    record_participation(
        &slug,
        &email,
        "email",
        IdentifierType::Email,
        form,
        api_key,
        8, // origin 8 = plaintext email
    )
    .await
}

/// Body extractor for the link-token endpoint, which accepts multiple repeated
/// `token`/`Token` values (orrery reads them with `getlist`). serde form
/// extractors collapse repeated keys, so we parse the raw body: collect every
/// token value, and reparse the remaining (single-valued) fields as a
/// `ParticipationForm`.
struct LinkTokenBody {
    form: ParticipationForm,
    tokens: Vec<String>,
}

impl<'a> poem::FromRequest<'a> for LinkTokenBody {
    async fn from_request(
        _req: &'a poem::Request,
        body: &mut poem::RequestBody,
    ) -> poem::Result<Self> {
        let data = body.take()?.into_vec().await?;
        let pairs: Vec<(String, String)> =
            url::form_urlencoded::parse(&data).into_owned().collect();

        let tokens: Vec<String> = pairs
            .iter()
            .filter(|(k, _)| k == "token" || k == "Token")
            .map(|(_, v)| v.clone())
            .filter(|v| !v.is_empty())
            .collect();

        // Reparse everything except the multi-valued token fields into the form.
        let rest = url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs(
                pairs
                    .iter()
                    .filter(|(k, _)| k != "token" && k != "Token")
                    .map(|(k, v)| (k.as_str(), v.as_str())),
            )
            .finish();
        let form: ParticipationForm = serde_urlencoded::from_str(&rest).map_err(|e| {
            poem::Error::from_string(
                format!("Failed to parse form data: {}", e),
                poem::http::StatusCode::BAD_REQUEST,
            )
        })?;

        Ok(LinkTokenBody { form, tokens })
    }
}

/// Record participation event via link token
/// POST /api/participation/link-token/:slug
#[handler]
pub async fn record_event_link_token(
    Path(slug): Path<String>,
    Query(query): Query<ApiKeyQuery>,
    LinkTokenBody { mut form, tokens }: LinkTokenBody,
) -> Result<Response, Error> {
    // Merge query key into form key
    if form.key.is_none() {
        form.key = query.key;
    }

    if tokens.is_empty() {
        return Ok(poem::web::Json(serde_json::json!({
            "error": "The token value is required"
        }))
        .with_status(poem::http::StatusCode::BAD_REQUEST)
        .into_response());
    }

    let entity_id = get_entity_id_for_slug(&slug)
        .await
        .map_err(|_| anyhow::anyhow!("Project slug does not identify a project"))?;

    let authenticated = if let Some(ref key) = form.key {
        check_api_key_permission(key, &entity_id).await?
    } else {
        false
    };

    // Record for every valid token's user (orrery records each in turn). The
    // response mirrors orrery's: the last recording result, or "invalid" if no
    // token matched a user.
    let mut response = poem::web::Json(serde_json::json!({
        "error": "Provided token is invalid"
    }))
    .with_status(poem::http::StatusCode::BAD_REQUEST)
    .into_response();

    for token in tokens {
        let user_match: UserMatchResponse = request(
            "user.match",
            UserMatchRequest {
                identifier: token,
                identifier_type: IdentifierType::LinkToken,
            },
        )
        .await?;

        let Some(user_id_str) = user_match.user_id else {
            continue;
        };
        let user_id = Ulid::from_string(&user_id_str)?;

        response =
            emit_participation(user_id, entity_id.clone(), authenticated, form.clone(), 9).await?;
    }

    Ok(response)
}

/// Record Articulate completion event
/// POST /api/participation/articulate
#[handler]
pub async fn record_event_articulate(Form(form): Form<ArticulateForm>) -> Result<Response, Error> {
    use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode};

    // Get JWT secret from compile-time environment
    let jwt_secret = env!("SCISTARTER_JWT_SECRET");

    // Decode and validate JWT (validates expiration automatically)
    let token_data = decode::<ArticulateClaims>(
        &form.token,
        &DecodingKey::from_secret(jwt_secret.as_bytes()),
        &Validation::new(Algorithm::HS256),
    )
    .map_err(|e| anyhow::anyhow!("Invalid JWT token: {}", e))?;

    let claims = token_data.claims;

    // Normalize activity: lowercase and remove trailing slash
    let activity = form.activity.trim_end_matches('/').to_lowercase();

    // Extract ULID from pseudo-email format (e.g., "01HXYZ...@articulate.scistarter.org")
    let user_id_str = claims
        .email
        .split('@')
        .next()
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

            // Record the participation (orrery records a fixed 300s/1.0 event)
            send(
                "log.project_participation_record",
                ProjectParticipationRecord {
                    user_id,
                    project_id: entity_id,
                    origin: Some(10), // origin 10 = Articulate
                    when: None,
                    duration: Some(300.0),
                    where_point: None,
                    magnitude: Some(1),
                    authenticated: Some(true), // JWT-authenticated
                    extra: Some(String::new()),
                    fidelity: Some(1.0),
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
        "http://dataethicsxapisept72022" | /* spanish */ "http://4xi0y6kwb69qegbwdc8i-kkmgeymlbul_rise" | /* portuguese */ "http://k632u-oqbl5kbcd5jfnucao1cr2sekoh_rise" => {
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
        "http://ambassador_rise_xapi"
        | "http://re3jvlrvw-cibju89pefhvvnoma-60ig_rise"
        | "http://cd2gspumolo6vyxczli9dnavy4skhgne_rise"
        | "http://yybhofihblvakm-vuhtjy0funt-jclwi_rise" => {
            let _: WorkflowProgressResponse = request(
                "user.workflow.progress",
                WorkflowProgressRequest {
                    user_id: user_id_str,
                    workflow: WorkflowType::Ambassador,
                    step: "tutorial".to_string(),
                }
            ).await?;
        }

        // Citizen Science for K-12 Educators
        "http://fr0w9cmlh3blsc9ykp_zbzherpeldg6t_rise" => {
            let _: WorkflowProgressResponse = request(
                "user.workflow.progress",
                WorkflowProgressRequest {
                    user_id: user_id_str,
                    workflow: WorkflowType::K12,
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
    }))
    .into_response())
}

#[derive(Debug, Deserialize)]
struct ArticulateForm {
    token: String,
    activity: String,
}

#[derive(Debug, serde::Deserialize)]
struct ArticulateClaims {
    email: String,
    /// JWT expiration timestamp - validated automatically by jsonwebtoken library
    #[serde(default)]
    #[allow(dead_code)]
    exp: Option<u64>,
}

/// Record participation event via OAuth2 access token
/// POST /api/participation/openid/:slug
/// POST /api/participation/openid
#[handler]
pub async fn record_event_openid(
    slug: Option<Path<String>>,
    headers: &HeaderMap,
    Form(form): Form<ParticipationForm>,
) -> Result<Response, Error> {
    // Extract bearer token from Authorization header
    let Some(token) = extract_bearer_token(headers) else {
        return Ok(oauth_error(
            poem::http::StatusCode::UNAUTHORIZED,
            "Missing or invalid Authorization header",
        ));
    };

    // Load and validate access token
    let Ok(access_token) = AccessToken::load(token.to_string()).await else {
        return Ok(oauth_error(
            poem::http::StatusCode::UNAUTHORIZED,
            "Invalid or expired access token",
        ));
    };

    // Check if token has expired
    if access_token.is_expired() {
        AccessToken::delete(access_token.token.clone()).await?;
        return Ok(oauth_error(
            poem::http::StatusCode::UNAUTHORIZED,
            "Access token has expired",
        ));
    }

    // Check if token has participation scope
    let scopes: Vec<&str> = access_token.scope.split_whitespace().collect();
    if !scopes.contains(&"participation") {
        return Ok(oauth_error(
            poem::http::StatusCode::FORBIDDEN,
            "Access token does not have participation scope",
        ));
    }

    // Parse user_id from access token
    let user_id = Ulid::from_string(&access_token.user_id)
        .map_err(|_| anyhow::anyhow!("Invalid user_id in access token"))?;

    // Determine project ID
    let project_id = if let Some(Path(project_slug)) = slug {
        // Slug provided: look up entity and get its target (project_id)
        match get_entity_id_for_slug(&project_slug).await {
            Ok(id) => id,
            Err(_) => return Ok(project_not_found()),
        }
    } else {
        // No slug: find the unique project owned by the client owner
        use crate::oauth2::types::Client;
        let client = Client::load(access_token.client_id.clone())
            .await
            .map_err(|_| anyhow::anyhow!("Could not find OAuth2 client"))?;

        let owner_user_id = client.owner_user_id.ok_or_else(|| {
            anyhow::anyhow!("Token does not identify a project and no project slug provided")
        })?;

        // Query for projects created by the client owner
        let projects = sqlx::query_scalar!(
            "SELECT project_id FROM projects WHERE page_creator_id = ?",
            owner_user_id
        )
        .fetch_all(db())
        .await?;

        match projects.len() {
            0 => return Err(anyhow::anyhow!("Client owner has no projects")),
            1 => projects[0].clone(),
            _ => {
                return Err(anyhow::anyhow!(
                    "Client owner has multiple projects, slug required"
                ));
            }
        }
    };

    // OAuth2-authenticated; origin 6 (OpenID) unless the caller marks "widget".
    emit_participation(user_id, project_id, true, form, 6).await
}

/// Helper to extract Bearer token from Authorization header
fn extract_bearer_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|auth| {
            if auth.starts_with("Bearer ") {
                Some(&auth[7..])
            } else {
                None
            }
        })
}
