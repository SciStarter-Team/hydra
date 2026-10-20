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
    timestring::TimeString,
};
use poem::{
    IntoResponse, Response, handler,
    web::{Form, Json, Query},
};
use serde::Deserialize;
use ulid::Ulid;

use super::check_api_key_permission;
use crate::contact::{edit_contact_person, edit_manager};
use crate::peering::LoadStorePeered;
use crate::project::ProjectStatus;
use crate::project::all::{ProjectAll, Submission, persist_new_project};
use crate::project::choice_ids;
use crate::project::field_source::FieldSource;
use crate::project::location::{ANYWHERE, AT_AN_ADDRESS, IN_A_RANGE};

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

    let Some(email) = form
        .email
        .clone()
        .or(form.user_id.clone())
        .filter(|e| !e.is_empty())
    else {
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
    result.insert(
        "search_terms".into(),
        serde_json::json!(project.search_terms),
    );
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
        vec![common::message::email::EmailRecipient::Email(
            reporting_email,
        )],
        "Participation error report",
        body,
        Vec::<String>::new(),
        None,
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

/// PPSR Core fields, and the project fields they write. A field named twice
/// takes the later one's value.
const PPSR_FIELDS: &[(&str, PpsrField)] = &[
    ("ProjectDataProvider", PpsrField::Origin),
    ("ProjectDescription", PpsrField::Description),
    ("ProjectContactName", PpsrField::ContactName),
    ("ProjectCoordinator", PpsrField::ManagerName),
    ("ProjectContactEmail", PpsrField::ContactEmail),
    ("ProjectContactPhone", PpsrField::ContactPhone),
    ("ProjectContactAddress", PpsrField::ContactAddress),
    ("ProjectInstitutionName", PpsrField::Presenter),
    ("ProjectOrganization", PpsrField::Presenter),
    ("ProjectURL", PpsrField::Url),
    ("ProjectFacebook", PpsrField::FacebookPage),
    ("ProjectTwitter", PpsrField::TwitterName),
    ("ProjectKeywords", PpsrField::SearchTerms),
];

#[derive(Debug, Clone, Copy)]
enum PpsrField {
    Origin,
    Description,
    ContactName,
    ManagerName,
    ContactEmail,
    ContactPhone,
    ContactAddress,
    Presenter,
    Url,
    FacebookPage,
    TwitterName,
    SearchTerms,
}

impl PpsrField {
    fn assign(self, proj: &mut ProjectAll, value: String) {
        match self {
            PpsrField::Origin => proj.origin = value,
            PpsrField::Description => proj.description = value,
            PpsrField::ContactName => {
                edit_contact_person(&mut proj.contacts, |c| c.name = Some(value.clone()))
            }
            PpsrField::ManagerName => {
                edit_manager(&mut proj.contacts, |m| m.name = Some(value.clone()))
            }
            PpsrField::ContactEmail => {
                edit_contact_person(&mut proj.contacts, |c| c.email = value.clone())
            }
            PpsrField::ContactPhone => {
                edit_contact_person(&mut proj.contacts, |c| c.phone = Some(value.clone()))
            }
            PpsrField::ContactAddress => {
                edit_contact_person(&mut proj.contacts, |c| c.address = Some(value.clone()))
            }
            PpsrField::Presenter => proj.presenter = value,
            PpsrField::Url => proj.url = value,
            PpsrField::FacebookPage => proj.facebook_page = value,
            PpsrField::TwitterName => proj.twitter_name = value,
            PpsrField::SearchTerms => proj.search_terms = value,
        }
    }
}

/// The project status a PPSR status name stands for, or Hidden for a name it
/// does not know.
fn ppsr_status(name: &str) -> ProjectStatus {
    match name.to_lowercase().as_str() {
        "starting" | "pending" => ProjectStatus::Pending,
        "active" => ProjectStatus::Active,
        "hiatus" => ProjectStatus::Hiatus,
        "complete" => ProjectStatus::Complete,
        _ => ProjectStatus::Hidden,
    }
}

/// The difficulty a partner's difficulty name stands for.
fn api_difficulty(name: &str) -> &'static str {
    match name.to_lowercase().as_str() {
        "hard" => "Difficult",
        "medium" => "Medium",
        _ => "Easy",
    }
}

/// Whether text is an ISO 8601 date, or date and time, as Python's
/// `datetime.fromisoformat` reads them.
fn is_iso_time(text: &str) -> bool {
    text.parse::<jiff::Timestamp>().is_ok()
        || text.parse::<jiff::civil::DateTime>().is_ok()
        || text.parse::<jiff::civil::Date>().is_ok()
}

/// A partner's value for a text field: text as given, a number written out,
/// and null as empty. Anything else is no value for a text field.
fn api_text(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(text) => Some(text.clone()),
        serde_json::Value::Number(number) => Some(number.to_string()),
        serde_json::Value::Null => Some(String::new()),
        _ => None,
    }
}

/// Whether a partner's value is true, as Python's `bool()` decides.
fn api_truthy(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Null => false,
        serde_json::Value::Bool(flag) => *flag,
        serde_json::Value::Number(number) => number.as_f64().is_some_and(|n| n != 0.0),
        serde_json::Value::String(text) => !text.is_empty(),
        serde_json::Value::Array(items) => !items.is_empty(),
        serde_json::Value::Object(fields) => !fields.is_empty(),
    }
}

/// A partner's `scistarter_id`, as Python's `int()` reads it, or 0 for one
/// that is not a number.
fn api_scistarter_id(value: Option<&serde_json::Value>) -> i64 {
    match value {
        Some(serde_json::Value::Number(number)) => number
            .as_i64()
            .or_else(|| number.as_f64().map(|n| n.trunc() as i64))
            .unwrap_or(0),
        Some(serde_json::Value::String(text)) => text.trim().parse().unwrap_or(0),
        Some(serde_json::Value::Bool(flag)) => *flag as i64,
        _ => 0,
    }
}

fn api_refusal(status: poem::http::StatusCode, message: &str) -> Response {
    Json(serde_json::json!({
        "result": "error",
        "error": status.as_u16(),
        "message": message,
    }))
    .with_status(status)
    .into_response()
}

/// Where a partner says a project happens.
enum Border {
    Nowhere,
    Address,
    Online,
    Anywhere,
    Regions(String),
}

/// Add a project, or update one the key may write, from a partner's record:
/// SciStarter's own field names, or PPSR Core's. A port of orrery's
/// `project_add`.
///
/// Everything that can refuse the request, the network work among it (the
/// image and the coverage), is settled before anything is written. New
/// projects are created unpublished, for the reviewers to look over.
#[handler]
pub async fn project_add(body: String) -> Result<Response, Error> {
    use poem::http::StatusCode;

    let Ok(serde_json::Value::Object(mut obj)) = serde_json::from_str::<serde_json::Value>(&body)
    else {
        return Ok(Json(serde_json::json!({
            "result": "error",
            "message": "Unable to parse JSON"
        }))
        .with_status(StatusCode::BAD_REQUEST)
        .into_response());
    };

    let api_key = obj
        .get("key")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    if api_key.is_empty() {
        return Ok(api_refusal(StatusCode::FORBIDDEN, "Invalid key"));
    }

    let validation: ValidateApiKeyResponse =
        request("user.api_key.validate", ValidateApiKeyRequest { api_key }).await?;

    let Some(api_user_id) = validation
        .user_id
        .as_deref()
        .and_then(|id| id.parse::<Ulid>().ok())
        .filter(|_| validation.valid)
    else {
        return Ok(api_refusal(StatusCode::FORBIDDEN, "Invalid key"));
    };

    let mut take = |key: &str| obj.remove(key);

    let name_fields = [take("name"), take("ProjectName")];
    let name = name_fields
        .iter()
        .flatten()
        .next()
        .and_then(api_text)
        .unwrap_or_default();

    // PPSR Core spells it ProjectGUID; ProjectGuid is accepted as well,
    // because callers have long sent it that way.
    let guid_fields = [take("guid"), take("ProjectGUID"), take("ProjectGuid")];
    let guid = guid_fields
        .iter()
        .flatten()
        .filter_map(api_text)
        .find(|guid| !guid.is_empty())
        .unwrap_or_default();

    let scistarter_id = api_scistarter_id(take("scistarter_id").as_ref());

    let obj = obj;

    let present = |key: &str| obj.get(key).is_some_and(api_truthy);
    if !present("goal") || !present("task") {
        return Ok(api_refusal(
            StatusCode::BAD_REQUEST,
            "Goal and task are required.",
        ));
    }

    let status_key = if obj.contains_key("status") {
        "status"
    } else {
        "ProjectStatus"
    };
    for key in [status_key, "difficulty"] {
        if obj.get(key).is_some_and(|value| !value.is_string()) {
            return Ok(api_refusal(
                StatusCode::BAD_REQUEST,
                &format!("{key} must be a string."),
            ));
        }
    }

    // Checked as orrery checks it, though the project keeps its own record of
    // when it was last edited rather than the partner's.
    if let Some(last_updated) = obj.get("ProjectDataLastUpdated") {
        if !last_updated.as_str().is_some_and(is_iso_time) {
            return Ok(api_refusal(
                StatusCode::BAD_REQUEST,
                "ProjectDataLastUpdated must be an ISO 8601 date and time.",
            ));
        }
    }

    // Find the project the record describes.
    let existing = if !guid.is_empty() {
        sqlx::query_scalar!(
            r#"SELECT project_id AS "project_id!" FROM projects WHERE guid = ?
               ORDER BY created, project_id LIMIT 1"#,
            guid
        )
        .fetch_optional(db())
        .await?
    } else if scistarter_id != 0 {
        let found = sqlx::query_scalar!(
            r#"SELECT project_id AS "project_id!" FROM projects WHERE legacy_id = ? LIMIT 1"#,
            scistarter_id
        )
        .fetch_optional(db())
        .await?;

        if found.is_none() {
            return Ok(api_refusal(
                StatusCode::BAD_REQUEST,
                "scistarter_id does not contain a valid id",
            ));
        }

        found
    } else {
        return Ok(api_refusal(
            StatusCode::BAD_REQUEST,
            "Project GUID is required for new records",
        ));
    };

    let is_new = existing.is_none();

    let mut proj = match existing {
        Some(project_id) => {
            if !super::key_may_write_project(&validation, &project_id).await? {
                let legacy_id = sqlx::query_scalar!(
                    "SELECT legacy_id FROM projects WHERE project_id = ?",
                    project_id
                )
                .fetch_optional(db())
                .await?
                .flatten();

                return Ok(Json(serde_json::json!({
                    "result": "error",
                    "error": 403,
                    "message": "This project already exists in the database, and this key may not change it",
                    "project_id": legacy_id,
                }))
                .with_status(StatusCode::FORBIDDEN)
                .into_response());
            }

            ProjectAll::load(project_id.parse()?).await?
        }
        None => {
            let namesakes = sqlx::query_scalar!(
                r#"SELECT count(*) AS "count!: i64" FROM projects WHERE lower(name) = lower(?)"#,
                name
            )
            .fetch_one(db())
            .await?;

            let mut proj = ProjectAll::default();
            proj.project_id = Ulid::new();
            proj.created = TimeString::now();
            proj.name = name.clone();
            proj.guid = guid.clone();
            proj.duplicate = namesakes > 1;
            proj.page_creator_id = api_user_id;
            proj
        }
    };

    // The image and the coverage, which can refuse the request.
    let image = match obj.get("image") {
        None => None,
        Some(image_url) => {
            let hosted = match image_url.as_str().filter(|url| !url.is_empty()) {
                Some(url) => crate::upload::rehost::rehost_image_url(url, api_user_id).await,
                None => None,
            };

            match hosted {
                Some(hosted) => Some(hosted),
                None => {
                    return Ok(api_refusal(
                        StatusCode::BAD_REQUEST,
                        "Unable to load and process the project image",
                    ));
                }
            }
        }
    };

    let border = obj
        .get("regions")
        .or_else(|| obj.get("border"))
        .or_else(|| obj.get("ProjectCoverageBoundary"));

    let border = match border {
        None => None,
        Some(serde_json::Value::Null) => Some(Border::Nowhere),
        Some(serde_json::Value::String(text)) if text.is_empty() || text == "none" => {
            Some(Border::Nowhere)
        }
        Some(serde_json::Value::String(text)) if text == "address" => Some(Border::Address),
        Some(serde_json::Value::String(text)) if text == "online" => Some(Border::Online),
        Some(serde_json::Value::String(text)) if text == "anywhere" => Some(Border::Anywhere),
        Some(value) if crate::coverage::is_global(value) => Some(Border::Anywhere),
        Some(value) => match crate::coverage::multipolygon(db(), value).await {
            Ok(regions) => Some(Border::Regions(regions)),
            Err(crate::coverage::CoverageError::Database(err)) => {
                return Err(anyhow::anyhow!(
                    "examining a partner's coverage failed: {err}"
                ));
            }
            Err(err) => return Ok(api_refusal(StatusCode::BAD_REQUEST, &err.to_string())),
        },
    };

    // The record's fields.
    let text = |key: &str| obj.get(key).and_then(api_text);
    let truthy = |key: &str| obj.get(key).map(api_truthy);

    for (key, field) in PPSR_FIELDS {
        if let Some(value) = text(key) {
            field.assign(&mut proj, value);
        }
    }

    if let Some(status) = obj.get(status_key).and_then(|v| v.as_str()) {
        proj.status = ppsr_status(status);
    }
    if let Some(difficulty) = obj.get("difficulty").and_then(|v| v.as_str()) {
        proj.difficulty = api_difficulty(difficulty).to_string();
    }
    if let Some(flag) = truthy("preregistration") {
        proj.participation_tools = flag;
    }
    if let Some(value) = text("new_member_url") {
        proj.signup_url = value;
    }
    if let Some(value) = text("goal") {
        proj.goal = value;
    }
    if let Some(value) = text("task") {
        proj.task = value;
    }

    proj.draft = false;

    if let Some(value) = text("image_credit") {
        proj.image_credit = value;
    }
    if let Some(value) = text("how_to_join") {
        proj.how_to_join = value;
    }
    if let Some(value) = text("special_skills") {
        proj.special_skills = value;
    }
    if let Some(flag) = truthy("outdoors") {
        proj.outdoors = flag;
    }
    if let Some(flag) = truthy("indoors") {
        proj.indoors = flag;
    }
    if let Some(value) = text("gear") {
        proj.gear = value;
    }

    if !name.is_empty() {
        proj.name = name;
    }

    for (key, field) in [
        ("description", &mut proj.description),
        ("url", &mut proj.url),
        ("video_url", &mut proj.video_url),
        ("blog_url", &mut proj.blog_url),
        ("twitter_name", &mut proj.twitter_name),
        ("facebook_page", &mut proj.facebook_page),
        ("presenting_org", &mut proj.presenter),
    ] {
        if let Some(value) = text(key) {
            *field = value;
        }
    }

    if let Some(value) = text("contact_name") {
        edit_contact_person(&mut proj.contacts, |c| c.name = Some(value.clone()));
    }
    if let Some(value) = text("contact_affiliation") {
        edit_contact_person(&mut proj.contacts, |c| c.affiliation = Some(value.clone()));
    }
    if let Some(value) = text("contact_email") {
        edit_contact_person(&mut proj.contacts, |c| c.email = value.clone());
    }
    if let Some(value) = text("contact_phone") {
        edit_contact_person(&mut proj.contacts, |c| c.phone = Some(value.clone()));
    }
    if let Some(value) = text("contact_address") {
        edit_contact_person(&mut proj.contacts, |c| c.address = Some(value.clone()));
    }

    if let Some(value) = text("address") {
        proj.location_type = AT_AN_ADDRESS.to_string();
        proj.address = value;
    }
    for (key, field) in [
        ("city", &mut proj.city),
        ("state", &mut proj.state),
        ("zip", &mut proj.zip),
        ("country", &mut proj.country),
    ] {
        if let Some(value) = text(key) {
            *field = value;
        }
    }

    // `origin` takes precedence over PPSR's ProjectDataProvider, which
    // PPSR_FIELDS has already written. A new project named by neither is
    // labelled with the account that imported it.
    let imported_by = || {
        format!(
            "API import by {}",
            validation
                .email
                .clone()
                .unwrap_or_else(|| api_user_id.to_string())
        )
    };
    if let Some(origin) = text("origin") {
        proj.origin = if origin.is_empty() {
            imported_by()
        } else {
            origin
        };
    } else if is_new && !obj.contains_key("ProjectDataProvider") {
        proj.origin = imported_by();
    }

    if let Some(umbrella) = validation
        .umbrella_project_id
        .as_deref()
        .and_then(|id| id.parse::<Ulid>().ok())
    {
        proj.part_of = Some(umbrella);
    }

    if let Some(flag) = truthy("entirely_online") {
        proj.entirely_online = flag;
    }

    if let Some(image) = image {
        proj.image = image;
    }

    match obj.get("extra") {
        Some(serde_json::Value::String(extra)) => proj.extra = extra.clone(),
        Some(extra) => proj.extra = extra.to_string(),
        None if is_new => proj.extra = "{}".to_string(),
        None => {}
    }

    match obj.get("tags") {
        Some(serde_json::Value::String(tags)) => {
            for tag in tags.split(',').map(str::trim).filter(|tag| !tag.is_empty()) {
                proj.tags.insert(tag.into());
            }
        }
        Some(serde_json::Value::Array(tags)) => {
            for tag in tags
                .iter()
                .filter_map(|tag| tag.as_str())
                .map(str::trim)
                .filter(|tag| !tag.is_empty())
            {
                proj.tags.insert(tag.into());
            }
        }
        _ => {}
    }

    if validation.automatic_affiliate {
        proj.is_affiliate = true;
    }

    if let Some(audience) = obj.get("audience") {
        proj.audience = choice_ids::all_named(choice_ids::AUDIENCE, audience);
    }
    if let Some(topics) = obj.get("topics") {
        proj.topics = choice_ids::all_named(choice_ids::TOPICS, topics);
    }
    if let Some(activities) = obj.get("activities") {
        proj.activities = choice_ids::all_named(choice_ids::ACTIVITIES, activities);
    }

    match border {
        // A new project that says nothing of where it happens is taken to
        // happen anywhere, as CitSci.org's projects need.
        None => {
            if is_new && !obj.contains_key("address") {
                proj.location_type = ANYWHERE.to_string();
            }
        }
        Some(Border::Nowhere) => proj.location_type = String::new(),
        Some(Border::Address) => proj.location_type = AT_AN_ADDRESS.to_string(),
        Some(Border::Online) => {
            proj.location_type = ANYWHERE.to_string();
            proj.entirely_online = true;
        }
        Some(Border::Anywhere) => proj.location_type = ANYWHERE.to_string(),
        Some(Border::Regions(regions)) => {
            proj.location_type = IN_A_RANGE.to_string();
            proj.regions = Some(regions);
        }
    }

    // Owner and page managers: whoever the record's admin_email_hashes name,
    // or failing them the key's own account.
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
        proj.page_creator_id = api_user_id;
        if !manager_ids.contains(&api_user_id) {
            manager_ids.push(api_user_id);
        }
    }
    proj.page_editor_ids = manager_ids.into_boxed_slice();

    proj.update_source = Some(FieldSource::Api);

    let project_id = proj.project_id;

    let slug = if is_new {
        let submission = Submission::Api {
            key_email: validation.email.clone().unwrap_or_default(),
            notify_reviewers: validation.api_projects_notify_reviewers,
        };
        persist_new_project(proj, false, api_user_id, submission).await?
    } else {
        proj.store().await?;
        let project_id_str = project_id.to_string();
        sqlx::query_scalar!(
            "SELECT slug FROM entities WHERE target = ? LIMIT 1",
            project_id_str
        )
        .fetch_optional(db())
        .await?
        .unwrap_or_else(|| project_id.to_string())
    };

    let project_id_str = project_id.to_string();
    let legacy_id = sqlx::query_scalar!(
        "SELECT legacy_id FROM projects WHERE project_id = ?",
        project_id_str
    )
    .fetch_optional(db())
    .await?
    .flatten();

    Ok(Json(serde_json::json!({
        "result": "success",
        "project_id": legacy_id,
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

#[cfg(test)]
mod tests {
    use super::*;
    use assert2::check;
    use serde_json::json;

    #[test]
    fn ppsr_statuses_map_onto_ours() {
        check!(matches!(ppsr_status("Starting"), ProjectStatus::Pending));
        check!(matches!(ppsr_status("active"), ProjectStatus::Active));
        check!(matches!(ppsr_status("HIATUS"), ProjectStatus::Hiatus));
        check!(matches!(ppsr_status("complete"), ProjectStatus::Complete));
        check!(matches!(ppsr_status("retired"), ProjectStatus::Hidden));
    }

    #[test]
    fn difficulty_is_easy_unless_said_otherwise() {
        check!(api_difficulty("Hard") == "Difficult");
        check!(api_difficulty("medium") == "Medium");
        check!(api_difficulty("difficult") == "Easy");
        check!(api_difficulty("") == "Easy");
    }

    #[test]
    fn iso_times_are_dates_or_dates_and_times() {
        check!(is_iso_time("2026-10-20T12:30:00Z"));
        check!(is_iso_time("2026-10-20T12:30:00+10:00"));
        check!(is_iso_time("2026-10-20T12:30:00"));
        check!(is_iso_time("2026-10-20"));
        check!(!is_iso_time("20 October 2026"));
        check!(!is_iso_time(""));
    }

    #[test]
    fn truth_is_pythons() {
        check!(!api_truthy(&json!(null)));
        check!(!api_truthy(&json!(0)));
        check!(!api_truthy(&json!("")));
        check!(!api_truthy(&json!([])));
        check!(api_truthy(&json!("no")));
        check!(api_truthy(&json!(2)));
        check!(api_truthy(&json!({"a": 1})));
    }

    #[test]
    fn scistarter_ids_are_read_as_int_reads_them() {
        check!(api_scistarter_id(Some(&json!(42))) == 42);
        check!(api_scistarter_id(Some(&json!(" 42 "))) == 42);
        check!(api_scistarter_id(Some(&json!(42.9))) == 42);
        check!(api_scistarter_id(Some(&json!("forty-two"))) == 0);
        check!(api_scistarter_id(Some(&json!(null))) == 0);
        check!(api_scistarter_id(None) == 0);
    }

    #[test]
    fn text_fields_take_text_numbers_and_null() {
        check!(api_text(&json!("words")) == Some("words".to_string()));
        check!(api_text(&json!(7)) == Some("7".to_string()));
        check!(api_text(&json!(null)) == Some(String::new()));
        check!(api_text(&json!(["words"])) == None);
    }

    #[test]
    fn a_ppsr_contact_field_edits_the_contact_person() {
        let mut proj = ProjectAll::default();

        PpsrField::ContactEmail.assign(&mut proj, "ada@example.org".to_string());
        PpsrField::ContactName.assign(&mut proj, "Ada".to_string());
        PpsrField::ManagerName.assign(&mut proj, "Grace".to_string());

        check!(proj.contacts.len() == 2);
        check!(proj.contacts[0].email == "ada@example.org");
        check!(proj.contacts[0].name.as_deref() == Some("Ada"));
        check!(proj.contacts[1].name.as_deref() == Some("Grace"));
    }
}
