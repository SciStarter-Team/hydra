// V1 API endpoints for legacy compat
// Ported from orrery/orrery/api/views/v1.py and participation.py

use anyhow::Error;
use common::{
    message::{
        log::ProjectParticipationRecord,
        user::{
            ApiCreateUserRequest, ApiCreateUserResponse, ProfileIdLookupRequest,
            ProfileIdLookupResponse, ProfileIdLookupType, ValidateApiKeyRequest,
            ValidateApiKeyResponse,
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

use crate::entity::{Entity, EntityKind, EntityTarget};
use crate::event::EventInstance;
use crate::event::all::EventAll;
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
    let (identifier, id_type) = if let Some(hashed) = query.hashed {
        (hashed, ProfileIdLookupType::Hashed)
    } else if let Some(identifier) = query.identifier {
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

    if response.known {
        Ok(Json(serde_json::json!({
            "result": "success",
            "scistarter_profile_id": response.legacy_profile_id,
            "scistarter_profile_id_v2": response.opaque_id,
            "known": true
        }))
        .into_response())
    } else {
        Ok(Json(serde_json::json!({
            "result": "error",
            "message": "Not a current SciStarter user"
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
    #[allow(dead_code)]
    project_id: Option<String>,
    #[allow(dead_code)]
    user_type: Option<String>,
    password_cyphertext: Option<String>,
    #[allow(dead_code)]
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

    let email = form
        .email
        .or(form.user_id.clone())
        .ok_or_else(|| anyhow::anyhow!("email or user_id required"))?;

    let response: ApiCreateUserResponse = request(
        "user.profile.api_create",
        ApiCreateUserRequest {
            email,
            handle: form.handle,
            first_name: form.first_name,
            last_name: form.last_name,
            zipcode: form.zip_code,
            password: form.password_cyphertext,
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

#[handler]
pub async fn record_event(
    Query(query): Query<ApiKeyQuery>,
    Form(form): Form<RecordEventForm>,
) -> Result<Response, Error> {
    let mut errors = Vec::<String>::new();

    // Resolve profile IDs to user ULIDs
    let profile_ids_str = form.profile_id.unwrap_or_default();
    let project_ids_str = form.project_id.unwrap_or_default();

    if profile_ids_str.is_empty() {
        return Ok(Json(serde_json::json!({
            "result": "error",
            "message": "profile_id required"
        }))
        .into_response());
    }

    if project_ids_str.is_empty() {
        return Ok(Json(serde_json::json!({
            "result": "error",
            "message": "project_id required"
        }))
        .into_response());
    }

    // Determine origin
    let origin: i16 = match form.origin.as_deref() {
        Some("widget") => 2,
        _ => 1,
    };

    let repetitions = form.count.unwrap_or(1);

    // API key check (optional for legacy compat)
    let authenticated = if let Some(ref key) = query.key {
        let validation: ValidateApiKeyResponse = request(
            "user.api_key.validate",
            ValidateApiKeyRequest {
                api_key: key.to_string(),
            },
        )
        .await?;
        validation.valid
    } else {
        false
    };

    // Handle comma-separated profile_id and project_id lists
    for profile_id_str in profile_ids_str.split(',') {
        let profile_id_str = profile_id_str.trim();
        if profile_id_str.is_empty() {
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
            errors.push(format!("Profile {} not found", profile_id_str));
            continue;
        };

        let user_id: ulid::Ulid = user_id_str.parse()?;

        for project_id_str in project_ids_str.split(',') {
            let project_id_str = project_id_str.trim();
            if project_id_str.is_empty() {
                continue;
            }

            // Resolve legacy project ID to entity target (ULID)
            let entity_id = resolve_legacy_project_id(project_id_str).await;

            let Some(entity_id) = entity_id else {
                errors.push(format!("Project {} not found", project_id_str));
                continue;
            };

            for _ in 0..repetitions {
                send(
                    "log.project_participation_record",
                    ProjectParticipationRecord {
                        user_id,
                        project_id: entity_id.clone(),
                        origin: Some(origin),
                        when: form.when.clone(),
                        duration: form.duration,
                        where_point: form.where_point.clone(),
                        magnitude: form.magnitude,
                        authenticated: Some(authenticated),
                        extra: form.extra.clone().or(Some(String::new())),
                        fidelity: None,
                        task_name: form.event_type.clone(),
                    },
                )
                .await?;
            }
        }
    }

    Ok(Json(serde_json::json!({
        "result": "success",
        "event_id": "deprecated",
        "project_name": "deprecated",
        "errors": errors,
        "notes": []
    }))
    .into_response())
}

// ============================================================================
// POST /api/record_event_by_email
// ============================================================================

#[handler]
pub async fn record_event_by_email(
    Query(query): Query<ApiKeyQuery>,
    Form(form): Form<RecordEventForm>,
) -> Result<Response, Error> {
    // Resolve identifier to profile_id, then delegate to record_event logic
    let identifier = if let Some(ref hashed) = form.hashed {
        Some((hashed.clone(), ProfileIdLookupType::Hashed))
    } else if let Some(ref identifier) = form.identifier {
        Some((identifier.clone(), ProfileIdLookupType::Email))
    } else {
        None
    };

    let Some((ident, id_type)) = identifier else {
        return Ok(Json(serde_json::json!({
            "result": "error",
            "message": "identifier or hashed required"
        }))
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
        return Ok(Json(serde_json::json!({
            "result": "success",
            "event_id": "deprecated",
            "errors": [],
            "notes": ["User not found, discarding for privacy"]
        }))
        .into_response());
    };

    let user_id: ulid::Ulid = user_id_str.parse()?;

    let project_ids_str = form.project_id.clone().unwrap_or_default();
    if project_ids_str.is_empty() {
        return Ok(Json(serde_json::json!({
            "result": "error",
            "message": "project_id required"
        }))
        .into_response());
    }

    let origin: i16 = match form.origin.as_deref() {
        Some("widget") => 2,
        _ => 1,
    };

    let repetitions = form.count.unwrap_or(1);
    let mut errors = Vec::<String>::new();

    let authenticated = if let Some(ref key) = query.key {
        let validation: ValidateApiKeyResponse = request(
            "user.api_key.validate",
            ValidateApiKeyRequest {
                api_key: key.to_string(),
            },
        )
        .await?;
        validation.valid
    } else {
        false
    };

    for project_id_str in project_ids_str.split(',') {
        let project_id_str = project_id_str.trim();
        if project_id_str.is_empty() {
            continue;
        }

        let entity_id = resolve_legacy_project_id(project_id_str).await;

        let Some(entity_id) = entity_id else {
            errors.push(format!("Project {} not found", project_id_str));
            continue;
        };

        for _ in 0..repetitions {
            send(
                "log.project_participation_record",
                ProjectParticipationRecord {
                    user_id,
                    project_id: entity_id.clone(),
                    origin: Some(origin),
                    when: form.when.clone(),
                    duration: form.duration,
                    where_point: form.where_point.clone(),
                    magnitude: form.magnitude,
                    authenticated: Some(authenticated),
                    extra: form.extra.clone().or(Some(String::new())),
                    fidelity: None,
                    task_name: form.event_type.clone(),
                },
            )
            .await?;
        }
    }

    Ok(Json(serde_json::json!({
        "result": "success",
        "event_id": "deprecated",
        "errors": errors,
        "notes": []
    }))
    .into_response())
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
            name, description, status, difficulty,
            url, guid, search_terms,
            indoors, outdoors,
            presenter, origin,
            "begin", "end"
           FROM projects WHERE project_id = ?"#,
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

    Ok(Json(serde_json::json!({
        "title": project.name,
        "description": project.description,
        "status": project.status,
        "difficulty": project.difficulty,
        "url": project.url,
        "guid": project.guid,
        "search_terms": project.search_terms,
        "indoors": project.indoors,
        "outdoors": project.outdoors,
        "presenter": project.presenter,
        "origin": project.origin,
        "begin_date": project.begin,
        "end_date": project.end,
        "tags": tags,
        "topics": topics
    }))
    .into_response())
}

// ============================================================================
// POST /api/participation/failure
// ============================================================================

#[handler]
pub async fn participation_failure(Form(_form): Form<serde_json::Value>) -> Response {
    // Just log and return 200 — this endpoint exists for error reporting
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

    let validation: ValidateApiKeyResponse =
        request("user.api_key.validate", ValidateApiKeyRequest { api_key }).await?;

    if !validation.valid {
        return Ok(Json(serde_json::json!({
            "result": "error",
            "error": 403,
            "message": "Invalid API key"
        }))
        .into_response());
    };

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

    let (project_ulid, is_new) = if let Some(ref pid) = existing_project_id {
        (pid.parse::<Ulid>()?, false)
    } else {
        (Ulid::new(), true)
    };

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

    // Set affiliate flag if the API key user has automatic_affiliate
    // (simplified: just mark it based on the user being staff)
    if validation.is_staff {
        proj.is_affiliate = true;
    }

    // Save project
    proj.store().await?;

    // Create or update entity slug
    let slug = if is_new {
        let slug = generate_slug(&proj.name).await;
        let entity = Entity::new(&slug, EntityKind::Project, EntityTarget::Ulid(project_ulid));
        entity.store().await?;
        slug
    } else {
        // Find existing slug
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
pub async fn event_add(Json(mut data): Json<serde_json::Value>) -> Result<Response, Error> {
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

    let validation: ValidateApiKeyResponse =
        request("user.api_key.validate", ValidateApiKeyRequest { api_key }).await?;

    if !validation.valid {
        return Ok(Json(serde_json::json!({
            "result": "error",
            "error": 401,
            "message": "Invalid API key"
        }))
        .into_response());
    };

    let guid = data
        .get("guid")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if guid.is_empty() {
        return Ok(Json(serde_json::json!({
            "result": "error",
            "error": 400,
            "message": "guid required"
        }))
        .into_response());
    }

    // Remove internal-only fields that shouldn't be set via API
    if let Some(obj) = data.as_object_mut() {
        for key in &[
            "slug",
            "value",
            "id",
            "legacy_id",
            "box",
            "link",
            "type",
            "publish",
            "created",
            "updated",
            "component",
            "_metadata",
            "authoritative",
            "redirect",
            "circuit_id",
            "key",
        ] {
            obj.remove(*key);
        }
    }

    // Try to find existing event by GUID (stored in admin_notes or a guid-like field)
    // Events don't have a guid column, so we match by name+origin or create new
    let existing_event_id = sqlx::query_scalar!(
        "SELECT e.event_id FROM events e
         JOIN entities ent ON ent.target = e.event_id
         WHERE ent.slug = ? LIMIT 1",
        guid
    )
    .fetch_optional(db())
    .await?;

    let (event_ulid, is_new) = if let Some(ref eid) = existing_event_id {
        (eid.parse::<Ulid>()?, false)
    } else {
        (Ulid::new(), true)
    };

    let mut evt = if is_new {
        let mut e = EventAll::default();
        e.event_id = event_ulid;
        e
    } else {
        EventAll::load(event_ulid).await.unwrap_or_else(|_| {
            let mut e = EventAll::default();
            e.event_id = event_ulid;
            e
        })
    };

    let obj = data.as_object().cloned().unwrap_or_default();

    if let Some(v) = obj.get("name").and_then(|v| v.as_str()) {
        evt.name = v.to_string();
    }
    if let Some(v) = obj.get("description").and_then(|v| v.as_str()) {
        evt.description = v.to_string();
    }
    if let Some(v) = obj.get("url").and_then(|v| v.as_str()) {
        evt.url = v.to_string();
    }
    if let Some(v) = obj.get("presenter").and_then(|v| v.as_str()) {
        evt.presenter = v.to_string();
    }
    if let Some(v) = obj
        .get("image_url")
        .or(obj.get("image"))
        .and_then(|v| v.as_str())
    {
        evt.image = v.to_string();
    }
    if let Some(v) = obj.get("address").and_then(|v| v.as_str()) {
        evt.address = v.to_string();
    }
    if let Some(v) = obj.get("city").and_then(|v| v.as_str()) {
        evt.city = v.to_string();
    }
    if let Some(v) = obj.get("state").and_then(|v| v.as_str()) {
        evt.state = v.to_string();
    }
    if let Some(v) = obj.get("zip").and_then(|v| v.as_str()) {
        evt.zip = v.to_string();
    }
    if let Some(v) = obj.get("country").and_then(|v| v.as_str()) {
        evt.country = v.to_string();
    }
    if let Some(v) = obj.get("how_to_join").and_then(|v| v.as_str()) {
        evt.how_to_join = v.to_string();
    }
    if let Some(v) = obj.get("image_credit").and_then(|v| v.as_str()) {
        evt.image_credit = v.to_string();
    }

    // Origin
    evt.origin = obj
        .get("origin")
        .and_then(|v| v.as_str())
        .unwrap_or("API")
        .to_string();

    // Location type
    if let Some(lt) = obj.get("location_type") {
        if let Some(lt_obj) = lt.as_object() {
            evt.location_type = lt_obj
                .get("value")
                .and_then(|v| v.as_str())
                .unwrap_or("ANY")
                .to_string();
        } else if let Some(s) = lt.as_str() {
            evt.location_type = s.to_string();
        }
    }

    // Regions (GeoJSON)
    if let Some(v) = obj.get("regions").and_then(|v| v.as_str()) {
        evt.regions = Some(v.to_string());
    }

    // Point (GeoJSON)
    if let Some(v) = obj.get("point").and_then(|v| v.as_str()) {
        evt.point = Some(v.to_string());
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
        evt.tags = tag_list.into_iter().map(|t| t.into()).collect();
    }

    // Begin/end as event instances
    let begin: Option<TimeString> = obj
        .get("begin")
        .and_then(|v| v.as_str())
        .and_then(|s| TimeString::try_from(s.to_string()).ok());
    let end: Option<TimeString> = obj
        .get("end")
        .and_then(|v| v.as_str())
        .and_then(|s| TimeString::try_from(s.to_string()).ok());
    if begin.is_some() || end.is_some() {
        evt.instances = vec![EventInstance {
            begin: begin.unwrap_or_default(),
            end: end.unwrap_or_default(),
        }]
        .into_boxed_slice();
    }

    // Entirely online
    if let Some(v) = obj.get("entirely_online").and_then(|v| v.as_bool()) {
        evt.entirely_online = v;
    }

    // Save event
    evt.store().await?;

    // Create or find entity slug
    let slug = if is_new {
        // Use guid as slug for events
        let slug = if !guid.is_empty() {
            guid.clone()
        } else {
            generate_slug(&evt.name).await
        };
        let entity = Entity::new(&slug, EntityKind::Event, EntityTarget::Ulid(event_ulid));
        entity.store().await?;
        slug
    } else {
        let event_id_str = event_ulid.to_string();
        sqlx::query_scalar!(
            "SELECT slug FROM entities WHERE target = ? LIMIT 1",
            event_id_str
        )
        .fetch_optional(db())
        .await?
        .unwrap_or_else(|| event_ulid.to_string())
    };

    Ok(Json(serde_json::json!({
        "result": "success",
        "slug": slug
    }))
    .into_response())
}

// ============================================================================
// Helpers
// ============================================================================

/// Generate a URL-safe slug from a name, ensuring uniqueness
async fn generate_slug(name: &str) -> String {
    let base_slug: String = name
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-");

    let base_slug = if base_slug.is_empty() {
        "project".to_string()
    } else {
        base_slug
    };

    // Check for uniqueness, append suffix if needed
    if !Entity::slug_exists(&base_slug).await.unwrap_or(true) {
        return base_slug;
    }

    for i in 2..1000 {
        let candidate = format!("{}-{}", base_slug, i);
        if !Entity::slug_exists(&candidate).await.unwrap_or(true) {
            return candidate;
        }
    }

    // Fallback: use ULID
    format!("{}-{}", base_slug, Ulid::new())
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
