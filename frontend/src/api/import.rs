use std::collections::HashSet;

use anyhow::Error;
use common::{
    insensitivestring::InsensitiveString,
    message::{
        log::PageChange,
        user::{ValidateApiKeyRequest, ValidateApiKeyResponse},
    },
    service::request,
    shared::db,
    timestring::TimeString,
};
use poem::{IntoResponse, Response, handler, http::HeaderMap, web::Json};
use serde::Deserialize;
use ulid::Ulid;

use crate::{
    entity::{Entity, EntityKind, EntityTarget},
    event::{EventInstance, all::EventAll},
    page_change::Stored,
    peering::LoadStorePeered,
};

/// A single scanner event. All fields default so that a malformed or partial
/// event fails only its own index (per-event error isolation) rather than the
/// whole batch; required fields are validated explicitly in import_single_event.
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub struct ImportEvent {
    pub name: String,
    pub description: String,
    pub start_datetime: Option<String>,
    pub end_datetime: Option<String>,
    pub image_url: Option<String>,
    pub host_organization: String,
    pub source_url: String,
    pub location: Option<ImportEventLocation>,
    pub tags: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub enum ImportEventLocation {
    Physical { address: String },
    Online { url: String },
}

/// Convert a datetime string to a TimeString.
///
/// Handles both timezone-aware strings (e.g. "2026-03-15T10:00:00-04:00")
/// and naive strings (e.g. "2026-03-15T10:00:00") by treating the latter as UTC.
fn to_timestring(s: &str) -> Result<TimeString, Error> {
    // Try parsing as-is first (handles timezone-aware strings)
    if let Ok(ts) = TimeString::try_from(s.to_string()) {
        return Ok(ts);
    }
    // Fall back: assume UTC
    Ok(TimeString::try_from(format!("{}Z", s))?)
}

#[derive(serde::Serialize)]
struct ImportResponse {
    result: String,
    imported: usize,
    errors: Vec<ImportError>,
}

#[derive(serde::Serialize)]
struct ImportError {
    index: usize,
    message: String,
}

/// A pre-loop error (auth/parse), in orrery's `{result, message}` shape.
fn import_request_error(status: poem::http::StatusCode, message: &str) -> Response {
    Json(serde_json::json!({ "result": "error", "message": message }))
        .with_status(status)
        .into_response()
}

/// POST /api/v2/events/import
///
/// Accepts a JSON array of events from the scanner and imports them. Requires a
/// valid API key (any user — orrery does not require staff). Each event is
/// validated and imported independently so one bad entry doesn't fail the batch.
#[handler]
pub async fn events_import(
    headers: &HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> Result<Response, Error> {
    // Bearer token required (401 when missing/malformed, matching orrery).
    let Some(api_key) = extract_bearer_token(headers) else {
        return Ok(import_request_error(
            poem::http::StatusCode::UNAUTHORIZED,
            "Missing or invalid Authorization header",
        ));
    };

    let validation: ValidateApiKeyResponse = request(
        "user.api_key.validate",
        ValidateApiKeyRequest {
            api_key: api_key.to_string(),
        },
    )
    .await?;

    // Any valid key may import (orrery imposes no staff requirement here).
    if !validation.valid {
        return Ok(import_request_error(
            poem::http::StatusCode::FORBIDDEN,
            "Invalid API key",
        ));
    }

    let serde_json::Value::Array(events) = body else {
        return Ok(import_request_error(
            poem::http::StatusCode::BAD_REQUEST,
            "Request body must be a JSON array",
        ));
    };

    let mut imported = 0;
    let mut errors = Vec::new();

    for (i, value) in events.into_iter().enumerate() {
        let result = match serde_json::from_value::<ImportEvent>(value) {
            Ok(event) => import_single_event(event).await,
            Err(e) => Err(anyhow::anyhow!("Invalid event: {}", e)),
        };
        match result {
            Ok(()) => imported += 1,
            Err(e) => errors.push(ImportError {
                index: i,
                message: e.to_string(),
            }),
        }
    }

    Ok(Json(ImportResponse {
        result: "success".into(),
        imported,
        errors,
    })
    .into_response())
}

async fn import_single_event(event: ImportEvent) -> Result<(), Error> {
    if event.name.is_empty() {
        return Err(anyhow::anyhow!("Event name is required"));
    }

    let Some(start_datetime) = event.start_datetime.clone().filter(|s| !s.is_empty()) else {
        return Err(anyhow::anyhow!("start_datetime is required"));
    };

    let begin = to_timestring(&start_datetime)?;

    // Dedup: check for existing event with same name and origin='scanner'
    let existing = sqlx::query!(
        r#"SELECT event_id FROM events WHERE LOWER(name) = LOWER(?) AND origin = 'scanner'"#,
        event.name
    )
    .fetch_all(db())
    .await?;

    let mut event_id = None;
    let begin_str: String = begin.clone().into();

    for row in &existing {
        // Check if any instance has a matching begin time
        let instance = sqlx::query!(
            r#"SELECT 1 as "found: bool" FROM event_instances
               WHERE event_id = ? AND begin = ?
               LIMIT 1"#,
            row.event_id,
            begin_str,
        )
        .fetch_optional(db())
        .await?;

        if instance.is_some() {
            event_id = Some(row.event_id.parse::<Ulid>()?);
            break;
        }
    }

    let ulid = event_id.unwrap_or_else(Ulid::new);
    let before = Stored::of_event(ulid).await;

    // An event met again keeps the time it first arrived.
    let created = match event_id {
        Some(existing) => EventAll::load(existing)
            .await
            .map(|event| event.created)
            .unwrap_or_else(|_| TimeString::now()),
        None => TimeString::now(),
    };

    // Determine URL and entirely_online
    let (url, entirely_online, address) = match event.location {
        Some(ImportEventLocation::Online { url }) => (url, true, String::new()),
        Some(ImportEventLocation::Physical { address }) => (event.source_url, false, address),
        None => (event.source_url, false, String::new()),
    };

    let end = match event.end_datetime {
        Some(ref s) => to_timestring(s)?,
        None => begin.clone(),
    };

    let tags: HashSet<InsensitiveString> = event
        .tags
        .into_iter()
        .map(InsensitiveString::from)
        .collect();

    let event_all = EventAll {
        event_id: ulid,
        created,
        name: event.name.clone(),
        description: event.description,
        origin: "scanner".to_string(),
        presenter: event.host_organization,
        url,
        image: event.image_url.unwrap_or_default(),
        entirely_online,
        address,
        tags,
        instances: vec![EventInstance { begin, end }].into_boxed_slice(),
        // Published as it arrives, so there is nothing for anyone to review.
        reviewed: true,
        ..Default::default()
    };

    // store() (not save_to_database) so the write is distributed to all peer
    // frontend instances, per the LoadStorePeered contract.
    event_all.store().await?;

    // Ensure entity exists for this event (publish=true)
    if event_id.is_none() {
        // New event — create entity
        let entity =
            Entity::new_by_name(&event.name, EntityKind::Event, EntityTarget::Ulid(ulid)).await?;
        entity.store().await?;
    }

    let after = Stored::of_event(ulid).await;
    let update = if event_id.is_none() {
        PageChange::EventCreate
    } else {
        PageChange::EventEdit
    };
    before
        .log_update(&after, update, None, ulid, "the event scanner")
        .await
}

fn extract_bearer_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|auth| auth.strip_prefix("Bearer "))
}
