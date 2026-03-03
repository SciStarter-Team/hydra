use std::collections::HashSet;

use anyhow::Error;
use common::{
    insensitivestring::InsensitiveString,
    message::user::{ValidateApiKeyRequest, ValidateApiKeyResponse},
    service::request,
    shared::db,
    timestring::TimeString,
};
use poem::{handler, http::HeaderMap, web::Json, IntoResponse, Response};
use serde::Deserialize;
use ulid::Ulid;

use crate::{
    entity::{Entity, EntityKind, EntityTarget},
    event::{all::EventAll, EventInstance},
    peering::LoadStorePeered,
};

#[derive(Debug, Deserialize)]
pub struct ImportEvent {
    pub name: String,
    pub description: String,
    pub start_datetime: String,
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

/// POST /api/v2/events/import
///
/// Accepts a JSON array of events from the scanner and imports them.
/// Requires a valid API key with staff/superuser permissions.
#[handler]
pub async fn events_import(
    headers: &HeaderMap,
    Json(events): Json<Vec<ImportEvent>>,
) -> Result<Response, Error> {
    // Extract and validate API key from Authorization header
    let api_key = extract_bearer_token(headers)
        .ok_or_else(|| anyhow::anyhow!("Missing or invalid Authorization header"))?;

    let validation: ValidateApiKeyResponse = request(
        "user.api_key.validate",
        ValidateApiKeyRequest {
            api_key: api_key.to_string(),
        },
    )
    .await?;

    if !validation.valid {
        return Ok(Json(ImportResponse {
            result: "error".into(),
            imported: 0,
            errors: vec![ImportError {
                index: 0,
                message: "Invalid API key".into(),
            }],
        })
        .with_status(poem::http::StatusCode::FORBIDDEN)
        .into_response());
    }

    if !validation.is_staff && !validation.is_superuser {
        return Ok(Json(ImportResponse {
            result: "error".into(),
            imported: 0,
            errors: vec![ImportError {
                index: 0,
                message: "Staff or superuser permission required".into(),
            }],
        })
        .with_status(poem::http::StatusCode::FORBIDDEN)
        .into_response());
    }

    let mut imported = 0;
    let mut errors = Vec::new();

    for (i, event) in events.into_iter().enumerate() {
        match import_single_event(event).await {
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

    let begin = to_timestring(&event.start_datetime)?;

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

    // Determine URL and entirely_online
    let (url, entirely_online, address) = match event.location {
        Some(ImportEventLocation::Online { url }) => (url, true, String::new()),
        Some(ImportEventLocation::Physical { address }) => {
            (event.source_url, false, address)
        }
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
        created: TimeString::now(),
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
        ..Default::default()
    };

    event_all.save_to_database().await?;

    // Ensure entity exists for this event (publish=true)
    if event_id.is_none() {
        // New event — create entity
        let entity =
            Entity::new_by_name(&event.name, EntityKind::Event, EntityTarget::Ulid(ulid)).await?;
        entity.save_to_database().await?;
    }

    Ok(())
}

fn extract_bearer_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|auth| auth.strip_prefix("Bearer "))
}
