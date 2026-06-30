// V1 API endpoints for legacy compat
// Ported from orrery/orrery/api/views/v1.py and participation.py

use anyhow::Error;
use common::{
    message::{
        log::ProjectParticipationRecord,
        user::{
            ApiCreateUserRequest, ApiCreateUserResponse, IdentifierType, ProfileIdLookupRequest,
            ProfileIdLookupResponse, ProfileIdLookupType, UserMatchRequest, UserMatchResponse,
            ValidateApiKeyRequest, ValidateApiKeyResponse,
        },
    },
    service::{request, send},
    shared::db,
};
use poem::{
    IntoResponse, Response, handler,
    web::{Form, Json, Query},
};
use serde::Deserialize;
use ulid::Ulid;

use super::check_api_key_permission;
use crate::peering::LoadStorePeered;
use crate::project::ProjectStatus;
use crate::project::all::ProjectAll;

#[derive(Debug, Deserialize)]
struct ApiKeyQuery {
    key: Option<String>,
}

// ============================================================================
// GET /api/profile/id
// ============================================================================

#[derive(Debug, Deserialize)]
struct ProfileIdQuery {
    identifier: Option<String>,
    hashed: Option<String>,
}

#[handler]
pub async fn get_profile_id(Query(query): Query<ProfileIdQuery>) -> Result<Response, Error> {
    // orrery lowercases the hashed (hex) identifier before lookup.
    let (identifier, id_type) = if let Some(hashed) = query.hashed.filter(|s| !s.is_empty()) {
        (hashed.to_lowercase(), ProfileIdLookupType::Hashed)
    } else if let Some(identifier) = query.identifier.filter(|s| !s.is_empty()) {
        (identifier, ProfileIdLookupType::Email)
    } else {
        return Ok(Json(serde_json::json!({
            "result": "error",
            "message": "profile identifier missing"
        }))
        .into_response());
    };

    let response: ProfileIdLookupResponse = request(
        "user.profile.lookup",
        ProfileIdLookupRequest {
            identifier,
            identifier_type: id_type,
        },
    )
    .await?;

    // orrery always returns success here (it creates a placeholder profile when
    // the identifier is unknown) with a `known` flag indicating whether a real
    // user is linked. hydra has no userless-profile concept, so when the user
    // is unknown it reports known:false with null profile ids rather than the
    // old error response that callers branching on `known` would mishandle.
    if response.known {
        Ok(Json(serde_json::json!({
            "result": "success",
            "scistarter_profile_id": response.legacy_profile_id,
            "scistarter_profile_id_v2": response.opaque_id,
            "scistarter_profile_ulid": response.user_id,
            "known": true
        }))
        .into_response())
    } else {
        Ok(Json(serde_json::json!({
            "result": "success",
            "scistarter_profile_id": serde_json::Value::Null,
            "scistarter_profile_id_v2": serde_json::Value::Null,
            "scistarter_profile_ulid": "",
            "known": false
        }))
        .into_response())
    }
}

// ============================================================================
// POST /api/user/new
// ============================================================================

#[derive(Debug, Deserialize)]
struct CreateUserForm {
    user_id: Option<String>,
    email: Option<String>,
    handle: Option<String>,
    first_name: Option<String>,
    last_name: Option<String>,
    zip_code: Option<String>,
    project_id: Option<String>,
    user_type: Option<String>,
    password_cyphertext: Option<String>,
    password_cypher: Option<String>,
}

#[handler]
pub async fn create_user(
    Query(query): Query<ApiKeyQuery>,
    Form(form): Form<CreateUserForm>,
) -> Result<Response, Error> {
    // Validate API key
    let api_key = match query.key {
        Some(key) => key,
        None => {
            return Ok(Json(serde_json::json!({
                "error": "API key required"
            }))
            .into_response());
        }
    };

    let validation: ValidateApiKeyResponse = request(
        "user.api_key.validate",
        ValidateApiKeyRequest {
            api_key: api_key.to_string(),
        },
    )
    .await?;

    if !validation.valid {
        return Ok(Json(serde_json::json!({
            "error": "Invalid API key"
        }))
        .into_response());
    }

    let Some(email) = form.email.clone().or(form.user_id.clone()).filter(|e| !e.is_empty()) else {
        return Ok(Json(serde_json::json!({ "error": "Email is required" })).into_response());
    };

    // Only the "cleartext" password cypher is supported (it returns the
    // cyphertext unchanged), matching orrery's PASSWORD_DECYPHER.
    let password_cypher = form
        .password_cypher
        .clone()
        .unwrap_or_else(|| "cleartext".to_string());
    if password_cypher != "cleartext" {
        return Ok(Json(serde_json::json!({ "error": "Unknown password cypher" })).into_response());
    }

    // Resolve the referring project (by slug or legacy id) to its canonical slug.
    let referer = if let Some(project_id) = form.project_id.clone().filter(|s| !s.is_empty()) {
        let Some(entity_id) = resolve_legacy_project_id(&project_id).await else {
            return Ok(Json(serde_json::json!({
                "error": "project_id should be your project slug or integer identifier"
            }))
            .into_response());
        };
        sqlx::query_scalar!(
            "SELECT slug FROM entities WHERE target = ? AND kind = 'Project' LIMIT 1",
            entity_id
        )
        .fetch_optional(db())
        .await?
        .unwrap_or_default()
    } else {
        String::new()
    };

    let response: ApiCreateUserResponse = request(
        "user.profile.api_create",
        ApiCreateUserRequest {
            email,
            handle: form.handle,
            first_name: form.first_name,
            last_name: form.last_name,
            zipcode: form.zip_code,
            password: form.password_cyphertext,
            origin: Some("API".to_string()),
            referer: Some(referer),
            user_type: Some(form.user_type.unwrap_or_else(|| "0".to_string())),
        },
    )
    .await?;

    let success_msg = if response.already_existed {
        "Account already exists"
    } else {
        "User created"
    };

    Ok(Json(serde_json::json!({
        "success": success_msg,
        "profile_id": response.legacy_profile_id,
        "profile_id_v2": response.opaque_id,
        "user_id": response.user_id
    }))
    .into_response())
}

// ============================================================================
// POST /api/record_event
// ============================================================================

#[derive(Debug, Deserialize)]
struct RecordEventForm {
    profile_id: Option<String>,
    project_id: Option<String>,
    #[serde(rename = "type")]
    event_type: Option<String>,
    #[serde(rename = "where")]
    where_point: Option<String>,
    when: Option<String>,
    origin: Option<String>,
    duration: Option<f64>,
    magnitude: Option<i32>,
    extra: Option<String>,
    count: Option<i32>,
    #[allow(dead_code)]
    testing: Option<String>,
    identifier: Option<String>,
    hashed: Option<String>,
}

/// Truncate to at most 64 KiB by character, matching orrery's `[:64*1024]`.
fn truncate_extra(extra: Option<String>) -> String {
    extra
        .map(|e| e.chars().take(64 * 1024).collect::<String>())
        .unwrap_or_default()
}

/// Recording parameters shared by `record_event` and `record_event_by_email`.
struct RecordCore {
    event_type: Option<String>,
    where_point: Option<String>,
    when: Option<String>,
    origin: i16,
    duration: f64,
    magnitude: i32,
    extra: String,
    repetitions: i32,
    api_key: Option<String>,
}

impl RecordCore {
    fn from_form(form: &RecordEventForm, api_key: Option<String>) -> Self {
        RecordCore {
            event_type: form.event_type.clone(),
            where_point: form.where_point.clone(),
            when: form.when.clone(),
            // Default origin 3 (web); 2 for widgets (matches orrery).
            origin: if form.origin.as_deref() == Some("widget") {
                2
            } else {
                3
            },
            duration: form.duration.unwrap_or(0.0),
            magnitude: form.magnitude.unwrap_or(1),
            extra: truncate_extra(form.extra.clone()),
            repetitions: form.count.unwrap_or(1).max(0),
            api_key,
        }
    }
}

/// Record participation for one resolved user across a comma-separated list of
/// legacy project ids, collecting per-project errors/notes like orrery. The
/// `authenticated` flag is set per project, only when the API key may write to
/// that project (orrery uses `project.can_write(key)`).
async fn record_user_events(
    user_id: Ulid,
    project_ids_str: &str,
    core: &RecordCore,
    errors: &mut Vec<String>,
    notes: &mut Vec<String>,
) -> Result<(), Error> {
    let mut seen = std::collections::HashSet::new();
    for project_id_str in project_ids_str.split(',') {
        let project_id_str = project_id_str.trim();
        if project_id_str.is_empty() || !seen.insert(project_id_str.to_string()) {
            continue;
        }

        let Some(entity_id) = resolve_legacy_project_id(project_id_str).await else {
            errors.push("Invalid project id".to_string());
            continue;
        };

        // Incidental events are intentionally not recorded.
        if core.event_type.as_deref() == Some("incidental") {
            notes.push("Not saving incidental event".to_string());
            continue;
        }

        // Authenticated only when the key may write to THIS project.
        let authenticated = match &core.api_key {
            Some(key) => check_api_key_permission(key, &entity_id).await?,
            None => false,
        };

        let record = ProjectParticipationRecord {
            user_id,
            project_id: entity_id.clone(),
            origin: Some(core.origin),
            when: core.when.clone(),
            duration: Some(core.duration),
            where_point: core.where_point.clone(),
            magnitude: Some(core.magnitude),
            authenticated: Some(authenticated),
            extra: Some(core.extra.clone()),
            fidelity: None,
            task_name: core.event_type.clone(),
        };

        for _ in 0..core.repetitions {
            send("log.project_participation_record", record.clone()).await?;
        }
    }
    Ok(())
}

/// Standard success envelope for the record_event endpoints.
fn record_success(errors: Vec<String>, notes: Vec<String>) -> Response {
    Json(serde_json::json!({
        "result": "success",
        "event_id": "deprecated",
        "project_name": "deprecated",
        "errors": errors,
        "notes": notes
    }))
    .into_response()
}

fn record_error(message: &str) -> Response {
    Json(serde_json::json!({ "result": "error", "message": message })).into_response()
}

#[handler]
pub async fn record_event(
    Query(query): Query<ApiKeyQuery>,
    Form(form): Form<RecordEventForm>,
) -> Result<Response, Error> {
    let profile_ids_str = form.profile_id.clone().unwrap_or_default();
    let project_ids_str = form.project_id.clone().unwrap_or_default();

    if profile_ids_str.is_empty() {
        return Ok(record_error("profile_id required"));
    }
    if project_ids_str.is_empty() {
        return Ok(record_error("project_id required"));
    }

    let core = RecordCore::from_form(&form, query.key.clone());
    let mut errors = Vec::<String>::new();
    let mut notes = Vec::<String>::new();
    let mut seen = std::collections::HashSet::new();

    for profile_id_str in profile_ids_str.split(',') {
        let profile_id_str = profile_id_str.trim();
        if profile_id_str.is_empty() || !seen.insert(profile_id_str.to_string()) {
            continue;
        }

        // Look up user by legacy profile ID
        let user_lookup: ProfileIdLookupResponse = request(
            "user.profile.lookup",
            ProfileIdLookupRequest {
                identifier: profile_id_str.to_string(),
                identifier_type: ProfileIdLookupType::LegacyProfileId,
            },
        )
        .await?;

        let Some(user_id_str) = user_lookup.user_id else {
            // orrery records against a freshly-created placeholder profile here;
            // hydra participation requires a known user (a ULID), so an unknown
            // profile id is skipped rather than recorded.
            errors.push(format!("Profile {} not found", profile_id_str));
            continue;
        };

        // Respect the user's opt-out of contribution tracking.
        if !user_lookup.track_contributions {
            errors.push("Not recorded, per user request.".to_string());
            continue;
        }

        let user_id: Ulid = user_id_str.parse()?;
        record_user_events(user_id, &project_ids_str, &core, &mut errors, &mut notes).await?;
    }

    Ok(record_success(errors, notes))
}

// ============================================================================
// POST /api/record_event_by_email
// ============================================================================

#[handler]
pub async fn record_event_by_email(
    Query(query): Query<ApiKeyQuery>,
    Form(form): Form<RecordEventForm>,
) -> Result<Response, Error> {
    // Resolve the identifier to a user, then record exactly like record_event.
    // orrery lowercases the hashed value (hex), so we do too.
    let identifier = if let Some(hashed) = form.hashed.clone().filter(|s| !s.is_empty()) {
        Some((hashed.to_lowercase(), ProfileIdLookupType::Hashed))
    } else if let Some(ident) = form.identifier.clone().filter(|s| !s.is_empty()) {
        Some((ident, ProfileIdLookupType::Email))
    } else {
        None
    };

    let Some((ident, id_type)) = identifier else {
        return Ok(Json(serde_json::json!({
            "result": "error",
            "message": "profile identifier missing"
        }))
        .with_status(poem::http::StatusCode::BAD_REQUEST)
        .into_response());
    };

    let user_lookup: ProfileIdLookupResponse = request(
        "user.profile.lookup",
        ProfileIdLookupRequest {
            identifier: ident,
            identifier_type: id_type,
        },
    )
    .await?;

    let Some(user_id_str) = user_lookup.user_id else {
        return Ok(record_error("Not a current SciStarter user"));
    };

    let project_ids_str = form.project_id.clone().unwrap_or_default();
    if project_ids_str.is_empty() {
        return Ok(record_error("project_id required"));
    }

    let mut errors = Vec::<String>::new();
    let mut notes = Vec::<String>::new();

    // Respect the user's opt-out of contribution tracking.
    if !user_lookup.track_contributions {
        errors.push("Not recorded, per user request.".to_string());
        return Ok(record_success(errors, notes));
    }

    let core = RecordCore::from_form(&form, query.key.clone());
    let user_id: Ulid = user_id_str.parse()?;
    record_user_events(user_id, &project_ids_str, &core, &mut errors, &mut notes).await?;

    Ok(record_success(errors, notes))
}

// ============================================================================
// GET /api/project/:project_id
// ============================================================================

#[handler]
pub async fn get_project(
    poem::web::Path(project_id): poem::web::Path<String>,
    Query(query): Query<ApiKeyQuery>,
) -> Result<Response, Error> {
    // Validate API key
    let api_key = match query.key {
        Some(key) => key,
        None => {
            return Ok(Json(serde_json::json!({
                "error": "API key required"
            }))
            .into_response());
        }
    };

    let validation: ValidateApiKeyResponse =
        request("user.api_key.validate", ValidateApiKeyRequest { api_key }).await?;

    if !validation.valid {
        return Ok(Json(serde_json::json!({
            "error": "Invalid API key"
        }))
        .into_response());
    }

    // Look up project by legacy ID
    let entity_id = resolve_legacy_project_id(&project_id).await;
    let Some(entity_id) = entity_id else {
        return Ok(Json(serde_json::json!({
            "error": "Project not found"
        }))
        .into_response());
    };

    // Fetch project data
    let project = sqlx::query!(
        r#"SELECT
            p.name, p.description, p.status, p.difficulty,
            p.guid, p.search_terms,
            p.indoors, p.outdoors,
            p.presenter, p.origin,
            p."begin", p."end",
            p.country,
            p.is_affiliate AS "is_affiliate: bool",
            AsGeoJSON(p.regions) AS "regions_json: Option<String>",
            e.publish AS "publish: bool",
            e.slug AS "slug!"
           FROM projects p
           JOIN entities e ON e.target = p.project_id AND e.kind = 'Project'
           WHERE p.project_id = ?"#,
        entity_id
    )
    .fetch_optional(db())
    .await?;

    let Some(project) = project else {
        return Ok(Json(serde_json::json!({
            "error": "Project not found"
        }))
        .into_response());
    };

    // Fetch tags
    let tags: Vec<String> = sqlx::query_scalar!(
        "SELECT tag FROM project_tags WHERE project_id = ?",
        entity_id
    )
    .fetch_all(db())
    .await?;

    // Fetch topics
    let topics: Vec<String> = sqlx::query_scalar!(
        "SELECT topic FROM project_topics WHERE project_id = ?",
        entity_id
    )
    .fetch_all(db())
    .await?;

    let mut result = serde_json::Map::new();
    result.insert("title".into(), serde_json::json!(project.name));
    result.insert("description".into(), serde_json::json!(project.description));
    result.insert("publish".into(), serde_json::json!(project.publish));
    // NOTE: orrery returns topics/UN_regions as [{code, name}] from its Choice
    // vocabularies; hydra stores them as plain strings without numeric codes, so
    // `topics` is a string list here and `UN_regions` is omitted (see decisions).
    result.insert("topics".into(), serde_json::json!(topics));
    result.insert("country".into(), serde_json::json!(project.country));
    result.insert("origin".into(), serde_json::json!(project.origin));
    result.insert(
        "scistarter_affiliate".into(),
        serde_json::json!(project.is_affiliate),
    );
    if let Some(status) = project_status_code_label(&project.status) {
        result.insert("status".into(), status);
    }
    result.insert("indoors".into(), serde_json::json!(project.indoors));
    result.insert("outdoors".into(), serde_json::json!(project.outdoors));
    result.insert("guid".into(), serde_json::json!(project.guid));
    result.insert("search_terms".into(), serde_json::json!(project.search_terms));
    if let Some(difficulty) = project_difficulty_code_label(&project.difficulty) {
        result.insert("difficulty".into(), difficulty);
    }
    result.insert("begin_date".into(), serde_json::json!(project.begin));
    result.insert("end_date".into(), serde_json::json!(project.end));
    result.insert("presenter".into(), serde_json::json!(project.presenter));
    result.insert(
        "url".into(),
        serde_json::json!(format!("https://scistarter.org/{}", project.slug)),
    );
    result.insert("tags".into(), serde_json::json!(tags));

    // Region geometry, in orrery's wrapper shape.
    if let Some(geojson) = project.regions_json.flatten() {
        if let Ok(geometry) = serde_json::from_str::<serde_json::Value>(&geojson) {
            result.insert(
                "regions".into(),
                serde_json::json!([{
                    "name": "geometry",
                    "osm_id": serde_json::Value::Null,
                    "legal": serde_json::Value::Null,
                    "geometry": geometry,
                }]),
            );
        }
    }

    Ok(Json(serde_json::Value::Object(result)).into_response())
}

/// Map a project status string to orrery's `[code, label]` pair, or None for
/// statuses orrery's list doesn't cover (e.g. "Expired"), which it omits.
fn project_status_code_label(status: &str) -> Option<serde_json::Value> {
    let code = match status {
        "Pending" => 0,
        "Active" => 1,
        "Periodically Active" => 2,
        "Hiatus" => 3,
        "Complete" => 4,
        "Hidden" => 5,
        _ => return None,
    };
    Some(serde_json::json!([code, status]))
}

/// Map a project difficulty string to orrery's `[code, label]` pair (orrery
/// labels: 1 Easy, 2 medium, 3 Difficult), or None when unrecognized.
fn project_difficulty_code_label(difficulty: &str) -> Option<serde_json::Value> {
    let (code, label) = match difficulty.to_lowercase().as_str() {
        "easy" => (1, "Easy"),
        "medium" => (2, "medium"),
        "difficult" => (3, "Difficult"),
        _ => return None,
    };
    Some(serde_json::json!([code, label]))
}

// ============================================================================
// POST /api/participation/failure
// ============================================================================

#[derive(Debug, Deserialize)]
struct FailureForm {
    error: Option<String>,
}

#[handler]
pub async fn participation_failure(Form(form): Form<FailureForm>) -> Response {
    let error = form.error.unwrap_or_default();
    let error = error.trim();

    // No error text -> nothing to report (matches orrery).
    if error.is_empty() {
        return poem::http::StatusCode::OK.into();
    }

    // Email the report to the operations address (orrery: REPORTING_EMAIL,
    // default info@scistarter.org). Braces are escaped so the body is never
    // interpreted as merge fields. Failure to send must not fail the request.
    let reporting_email =
        std::env::var("REPORTING_EMAIL").unwrap_or_else(|_| "info@scistarter.org".to_string());
    let body = error.replace('{', "{{").replace('}', "}}");

    if let Err(e) = common::service::send_email(
        vec![common::message::email::EmailRecipient::Email(reporting_email)],
        "Participation error report",
        body,
        Vec::<String>::new(),
        None,
    )
    .await
    {
        tracing::warn!("Failed to send participation error report: {}", e);
    }

    poem::http::StatusCode::OK.into()
}

// acts_to_date is handled by crate::csm::acts_to_date_api (reused directly)

// ============================================================================
// POST /api/project/add
// ============================================================================

/// PPSR Core field name → hydra field name mapping
fn ppsr_map_field(name: &str) -> &str {
    match name {
        "ProjectGUID" => "guid",
        "ProjectName" => "name",
        "ProjectDataProvider" => "origin",
        "ProjectDescription" => "description",
        "ProjectContactName" => "contact_name",
        "ProjectCoordinator" => "manager_name",
        "ProjectContactEmail" => "contact_email",
        "ProjectContactPhone" => "project_owner_phone",
        "ProjectContactAddress" => "contact_address",
        "ProjectInstitutionName" | "ProjectOrganization" => "presenter",
        "ProjectURL" => "url",
        "ProjectFacebook" => "facebook_page",
        "ProjectTwitter" => "twitter_name",
        "ProjectKeywords" => "search_terms",
        other => other,
    }
}

#[handler]
pub async fn project_add(Json(mut data): Json<serde_json::Value>) -> Result<Response, Error> {
    // Validate API key from JSON body
    let api_key = data
        .get("key")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let Some(api_key) = api_key else {
        return Ok(Json(serde_json::json!({
            "result": "error",
            "error": 400,
            "message": "API key required"
        }))
        .into_response());
    };

    let validation: ValidateApiKeyResponse = request(
        "user.api_key.validate",
        ValidateApiKeyRequest {
            api_key: api_key.clone(),
        },
    )
    .await?;

    if !validation.valid {
        return Ok(Json(serde_json::json!({
            "result": "error",
            "error": 403,
            "message": "Invalid API key"
        }))
        .into_response());
    };

    // The API key's own user (owner default + image-upload attribution).
    let api_user_id = validation
        .user_id
        .as_ref()
        .and_then(|s| s.parse::<Ulid>().ok());

    // Apply PPSR field name mapping
    if let Some(obj) = data.as_object_mut() {
        let keys: Vec<String> = obj.keys().cloned().collect();
        for key in keys {
            let mapped = ppsr_map_field(&key);
            if mapped != key {
                if let Some(val) = obj.remove(&key) {
                    obj.insert(mapped.to_string(), val);
                }
            }
        }
    }

    // Goal and task are required (orrery rejects creation/update without them).
    let has_field = |k: &str| {
        data.get(k)
            .and_then(|v| v.as_str())
            .map(|s| !s.is_empty())
            .unwrap_or(false)
    };
    if !has_field("goal") || !has_field("task") {
        return Ok(Json(serde_json::json!({
            "result": "error",
            "error": 400,
            "message": "Goal and task are required."
        }))
        .with_status(poem::http::StatusCode::BAD_REQUEST)
        .into_response());
    }

    let guid = data
        .get("guid")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let scistarter_id = data
        .get("scistarter_id")
        .and_then(|v| v.as_str())
        .or_else(|| {
            data.get("scistarter_id")
                .and_then(|v| v.as_i64())
                .map(|_| "")
        });
    let scistarter_id_int = data.get("scistarter_id").and_then(|v| v.as_i64());

    if guid.is_empty() && scistarter_id.is_none() && scistarter_id_int.is_none() {
        return Ok(Json(serde_json::json!({
            "result": "error",
            "error": 400,
            "message": "guid or scistarter_id required"
        }))
        .into_response());
    }

    // Try to find existing project by GUID or legacy ID
    let existing_project_id = if !guid.is_empty() {
        sqlx::query_scalar!(
            "SELECT project_id FROM projects WHERE guid = ? LIMIT 1",
            guid
        )
        .fetch_optional(db())
        .await?
    } else if let Some(legacy_id) = scistarter_id_int {
        sqlx::query_scalar!(
            "SELECT project_id FROM projects WHERE legacy_id = ? LIMIT 1",
            legacy_id
        )
        .fetch_optional(db())
        .await?
    } else {
        None
    };

    // A scistarter_id (with no guid) must reference an existing project; orrery
    // returns 400 rather than creating a new project in that case.
    if guid.is_empty() && scistarter_id_int.is_some() && existing_project_id.is_none() {
        return Ok(Json(serde_json::json!({
            "result": "error",
            "error": 400,
            "message": "scistarter_id does not contain a valid id"
        }))
        .with_status(poem::http::StatusCode::BAD_REQUEST)
        .into_response());
    }

    let (project_ulid, is_new) = if let Some(ref pid) = existing_project_id {
        (pid.parse::<Ulid>()?, false)
    } else {
        (Ulid::new(), true)
    };

    // For an existing project, only a writer (page creator/manager or staff) may
    // update it; otherwise report that it already exists without overwriting.
    if !is_new {
        let project_id_str = project_ulid.to_string();
        if !check_api_key_permission(&api_key, &project_id_str).await? {
            let legacy_id = sqlx::query_scalar!(
                "SELECT legacy_id FROM projects WHERE project_id = ?",
                project_id_str
            )
            .fetch_optional(db())
            .await?
            .flatten();
            return Ok(Json(serde_json::json!({
                "result": "success",
                "message": "This project already exists in the database",
                "project_id": legacy_id
            }))
            .into_response());
        }
    }

    // Load existing or create new
    let mut proj = if is_new {
        let mut p = ProjectAll::default();
        p.project_id = project_ulid;
        p
    } else {
        ProjectAll::load(project_ulid).await.unwrap_or_else(|_| {
            let mut p = ProjectAll::default();
            p.project_id = project_ulid;
            p
        })
    };

    // Apply fields from request data
    let obj = data.as_object().cloned().unwrap_or_default();

    if let Some(v) = obj.get("name").and_then(|v| v.as_str()) {
        proj.name = v.to_string();
    }
    if let Some(v) = obj.get("guid").and_then(|v| v.as_str()) {
        proj.guid = v.to_string();
    }
    if let Some(v) = obj.get("description").and_then(|v| v.as_str()) {
        proj.description = v.to_string();
    }
    if let Some(v) = obj.get("url").and_then(|v| v.as_str()) {
        proj.url = v.to_string();
    }
    if let Some(v) = obj.get("origin").and_then(|v| v.as_str()) {
        proj.origin = v.to_string();
    }
    if let Some(v) = obj.get("presenter").and_then(|v| v.as_str()) {
        proj.presenter = v.to_string();
    }
    if let Some(v) = obj.get("search_terms").and_then(|v| v.as_str()) {
        proj.search_terms = v.to_string();
    }
    if let Some(v) = obj.get("contact_name").and_then(|v| v.as_str()) {
        proj.contacts = vec![crate::contact::Contact {
            contact_id: Ulid::new(),
            name: Some(v.to_string()),
            email: obj
                .get("contact_email")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            phone: obj
                .get("contact_phone")
                .and_then(|v| v.as_str())
                .or_else(|| obj.get("project_owner_phone").and_then(|v| v.as_str()))
                .map(String::from),
            affiliation: obj
                .get("contact_affiliation")
                .and_then(|v| v.as_str())
                .map(String::from),
            address: obj
                .get("contact_address")
                .and_then(|v| v.as_str())
                .map(String::from),
            role: crate::contact::ContactRole::ContactPerson,
        }]
        .into_boxed_slice();
    }
    if let Some(v) = obj.get("facebook_page").and_then(|v| v.as_str()) {
        proj.facebook_page = v.to_string();
    }
    if let Some(v) = obj.get("twitter_name").and_then(|v| v.as_str()) {
        proj.twitter_name = v.to_string();
    }
    if let Some(v) = obj.get("video_url").and_then(|v| v.as_str()) {
        proj.video_url = v.to_string();
    }
    if let Some(v) = obj.get("blog_url").and_then(|v| v.as_str()) {
        proj.blog_url = v.to_string();
    }
    if let Some(v) = obj.get("image_credit").and_then(|v| v.as_str()) {
        proj.image_credit = v.to_string();
    }
    if let Some(v) = obj.get("how_to_join").and_then(|v| v.as_str()) {
        proj.how_to_join = v.to_string();
    }
    if let Some(v) = obj.get("special_skills").and_then(|v| v.as_str()) {
        proj.special_skills = v.to_string();
    }
    if let Some(v) = obj.get("gear").and_then(|v| v.as_str()) {
        proj.gear = v.to_string();
    }
    if let Some(v) = obj.get("goal").and_then(|v| v.as_str()) {
        proj.goal = v.to_string();
    }
    if let Some(v) = obj.get("task").and_then(|v| v.as_str()) {
        proj.task = v.to_string();
    }

    // Image: re-host the supplied URL adjacent to our untracked uploads (orrery
    // does the same via url_to_file). A fetch/processing failure aborts the
    // request with the same error orrery returns.
    if let Some(image_url) = obj
        .get("image")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
    {
        let uploaded_by = api_user_id.unwrap_or_else(Ulid::nil);
        match rehost_image_url(image_url, uploaded_by).await {
            Some(hosted) => proj.image = hosted,
            None => {
                return Ok(Json(serde_json::json!({
                    "result": "error",
                    "error": 400,
                    "message": "Unable to load and process the project image"
                }))
                .with_status(poem::http::StatusCode::BAD_REQUEST)
                .into_response());
            }
        }
    }

    // Status mapping
    if let Some(v) = obj
        .get("status")
        .or(obj.get("ProjectStatus"))
        .and_then(|v| v.as_str())
    {
        proj.status = match v.to_lowercase().as_str() {
            "starting" | "pending" => ProjectStatus::Pending,
            "active" => ProjectStatus::Active,
            "complete" => ProjectStatus::Complete,
            "hiatus" => ProjectStatus::Hiatus,
            _ => ProjectStatus::Active,
        };
    }

    // Difficulty
    if let Some(v) = obj.get("difficulty").and_then(|v| v.as_str()) {
        proj.difficulty = match v.to_lowercase().as_str() {
            "hard" | "difficult" => "3".to_string(),
            "medium" => "2".to_string(),
            _ => "1".to_string(),
        };
    }

    // Boolean fields
    if let Some(v) = obj.get("outdoors").and_then(|v| v.as_bool()) {
        proj.outdoors = v;
    }
    if let Some(v) = obj.get("indoors").and_then(|v| v.as_bool()) {
        proj.indoors = v;
    }
    if let Some(v) = obj.get("entirely_online").and_then(|v| v.as_bool()) {
        proj.entirely_online = v;
    }

    // Preregistration → participation_tools
    if let Some(v) = obj.get("preregistration").and_then(|v| v.as_bool()) {
        proj.participation_tools = v;
    }

    // new_member_url → signup_url
    if let Some(v) = obj.get("new_member_url").and_then(|v| v.as_str()) {
        proj.signup_url = v.to_string();
    }

    // Location fields
    if let Some(v) = obj.get("address").and_then(|v| v.as_str()) {
        proj.address = v.to_string();
    }
    if let Some(v) = obj.get("city").and_then(|v| v.as_str()) {
        proj.city = v.to_string();
    }
    if let Some(v) = obj.get("state").and_then(|v| v.as_str()) {
        proj.state = v.to_string();
    }
    if let Some(v) = obj.get("zip").and_then(|v| v.as_str()) {
        proj.zip = v.to_string();
    }
    if let Some(v) = obj.get("country").and_then(|v| v.as_str()) {
        proj.country = v.to_string();
    }

    // Tags
    if let Some(tags_val) = obj.get("tags") {
        let tag_list: Vec<String> = if let Some(arr) = tags_val.as_array() {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        } else if let Some(s) = tags_val.as_str() {
            s.split(',')
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty())
                .collect()
        } else {
            vec![]
        };
        proj.tags = tag_list.into_iter().map(|t| t.into()).collect();
    }

    // Topics
    if let Some(topics_val) = obj.get("topics") {
        let topic_list: Vec<String> = if let Some(arr) = topics_val.as_array() {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        } else if let Some(s) = topics_val.as_str() {
            s.split(',')
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty())
                .collect()
        } else {
            vec![]
        };
        proj.topics = topic_list.into_boxed_slice();
    }

    // Audience
    if let Some(audience_val) = obj.get("audience") {
        let audience_list: Vec<String> = if let Some(arr) = audience_val.as_array() {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        } else if let Some(s) = audience_val.as_str() {
            s.split(',')
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty())
                .collect()
        } else {
            vec![]
        };
        proj.audience = audience_list.into_boxed_slice();
    }

    // Activities
    if let Some(activities_val) = obj.get("activities") {
        let activity_list: Vec<String> = if let Some(arr) = activities_val.as_array() {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        } else if let Some(s) = activities_val.as_str() {
            s.split(',')
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty())
                .collect()
        } else {
            vec![]
        };
        proj.activities = activity_list.into_boxed_slice();
    }

    // Presenting org (alternative to presenter)
    if let Some(v) = obj.get("presenting_org").and_then(|v| v.as_str()) {
        proj.presenter = v.to_string();
    }

    // Regions/border/ProjectCoverageBoundary → location_type + regions
    if let Some(regions_val) = obj
        .get("regions")
        .or(obj.get("border"))
        .or(obj.get("ProjectCoverageBoundary"))
    {
        if let Some(s) = regions_val.as_str() {
            match s.to_lowercase().as_str() {
                "" | "none" => {
                    proj.location_type = "NONE".to_string();
                }
                "address" => {
                    proj.location_type = "AT".to_string();
                }
                "online" => {
                    proj.location_type = "ANY".to_string();
                    proj.entirely_online = true;
                }
                "anywhere" => {
                    proj.location_type = "ANY".to_string();
                }
                _ => {
                    proj.location_type = "NEAR".to_string();
                    proj.regions = Some(s.to_string());
                }
            }
        } else if regions_val.is_null() {
            proj.location_type = "NONE".to_string();
        }
    } else if proj.location_type.is_empty() {
        // orrery defaults the coverage to "ANY" (global) when no boundary is
        // supplied, to accommodate CitSci.org projects.
        proj.location_type = "ANY".to_string();
    }

    // Extra
    if let Some(v) = obj.get("extra") {
        proj.extra = v.to_string();
    }

    // Mark not draft for API-created projects
    proj.draft = false;

    // Default origin if not set
    if proj.origin.is_empty() {
        proj.origin = "API import".to_string();
    }

    // Affiliate flag, keyed on the user's `automatic_affiliate` flag (synced
    // from orrery), matching orrery exactly.
    if validation.automatic_affiliate {
        proj.is_affiliate = true;
    }

    // Owner / page managers. orrery resolves admin_email_hashes to users (owner
    // and managers) and otherwise falls back to the API key's own user.
    let mut manager_ids: Vec<Ulid> = proj.page_editor_ids.to_vec();
    if let Some(hashes) = obj.get("admin_email_hashes").and_then(|v| v.as_array()) {
        for hash in hashes {
            let Some(hash) = hash.as_str().filter(|s| !s.is_empty()) else {
                continue;
            };
            let matched: UserMatchResponse = request(
                "user.match",
                UserMatchRequest {
                    identifier: hash.to_string(),
                    identifier_type: IdentifierType::Hashed,
                },
            )
            .await?;
            if let Some(uid) = matched.user_id.and_then(|s| s.parse::<Ulid>().ok()) {
                proj.page_creator_id = uid;
                if !manager_ids.contains(&uid) {
                    manager_ids.push(uid);
                }
            }
        }
    }
    if proj.page_creator_id == Ulid::default() {
        if let Some(uid) = api_user_id {
            proj.page_creator_id = uid;
            if !manager_ids.contains(&uid) {
                manager_ids.push(uid);
            }
        }
    }
    proj.page_editor_ids = manager_ids.into_boxed_slice();

    // Persist. New projects go through the shared creation path (store, page
    // entity, activity log, and submitter/reviewer notification emails); updates
    // just store the changed fields.
    let slug = if is_new {
        let creator = api_user_id.unwrap_or(proj.page_creator_id);
        crate::project::all::persist_new_project(proj, true, creator).await?
    } else {
        proj.store().await?;
        let project_id_str = project_ulid.to_string();
        sqlx::query_scalar!(
            "SELECT slug FROM entities WHERE target = ? LIMIT 1",
            project_id_str
        )
        .fetch_optional(db())
        .await?
        .unwrap_or_else(|| project_ulid.to_string())
    };

    // Look up legacy_id for response
    let project_id_str = project_ulid.to_string();
    let legacy_id = sqlx::query_scalar!(
        "SELECT legacy_id FROM projects WHERE project_id = ?",
        project_id_str
    )
    .fetch_optional(db())
    .await?
    .flatten();

    Ok(Json(serde_json::json!({
        "result": "success",
        "project_id": legacy_id.unwrap_or(0),
        "project_slug": slug
    }))
    .into_response())
}

// ============================================================================
// POST /api/event/add
// ============================================================================

#[handler]
pub async fn event_add() -> Response {
    // Event creation via the API has been retired: there are no known consumers,
    // and orrery's model (guid-based dedup and arbitrary field passthrough) does
    // not map onto hydra's typed event model. The endpoint reports that it is gone.
    Json(serde_json::json!({
        "result": "error",
        "error": 410,
        "message": "The event creation API is no longer available."
    }))
    .with_status(poem::http::StatusCode::GONE)
    .into_response()
}

// ============================================================================
// Helpers
// ============================================================================

/// Fetch a remote image and re-host it adjacent to our untracked uploads,
/// returning the hosted path (`/-uploaded/<id>`). Returns None on any
/// fetch/upload failure so the caller can report it. Mirrors orrery's
/// `url_to_file`.
async fn rehost_image_url(url: &str, uploaded_by: Ulid) -> Option<String> {
    let response = reqwest::get(url).await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let data = response.bytes().await.ok()?.to_vec();
    if data.is_empty() {
        return None;
    }

    let file_id = Ulid::new();
    let metadata = crate::upload::storage::FileMetadata {
        file_id,
        original_filename: None,
        content_type,
        size: data.len() as u64,
        uploaded_by,
        access: crate::upload::storage::FileAccess::Untracked,
        variant: None,
        force_download: false,
    };
    crate::upload::STORAGE.upload_file(metadata, data).await.ok()?;

    Some(format!("/-uploaded/{file_id}"))
}

/// Resolve a legacy project ID (integer) or slug to a project_id ULID string
async fn resolve_legacy_project_id(id_or_slug: &str) -> Option<String> {
    // Try as integer first (legacy ID)
    if let Ok(legacy_id) = id_or_slug.parse::<i64>() {
        let row = sqlx::query_scalar!(
            "SELECT project_id FROM projects WHERE legacy_id = ? LIMIT 1",
            legacy_id
        )
        .fetch_optional(db())
        .await
        .ok()?;

        if let Some(project_id) = row {
            return Some(project_id);
        }
    }

    // Try as slug
    let row = sqlx::query_scalar!(
        "SELECT target FROM entities WHERE slug = ? LIMIT 1",
        id_or_slug
    )
    .fetch_optional(db())
    .await
    .ok()?;

    row
}
