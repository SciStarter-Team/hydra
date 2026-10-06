//! Email tools for the content MCP endpoint: the stored emails the site sends
//! automatically, and the triggers that decide when.
//!
//! An automatic email is two records. A **stored email** holds the subject and
//! the body; a **trigger** names the event that sends it and points at the
//! stored email. A [`common::service::TriggerEvent`] whose name no active
//! trigger carries sends nothing at all -- not an error, not a fallback, just
//! silence -- so a training that grants a badge without its trigger row looks
//! like it worked and tells nobody.
//!
//! Bodies are held as Quill Delta, the format the Engine Room's editor reads
//! and writes. These tools take and return HTML instead, converting with the
//! same [`common::quill`] pair the emailer's own orrery import uses, so copy
//! can be moved across without hand-editing Delta.
//!
//! Everything here goes through the emailer service's own messages -- the same
//! ones the admin screens use -- rather than reaching into its database, which
//! belongs to that service and is not reachable from here anyway.

use anyhow::{Error, anyhow};
use common::message::email::*;
use common::service::{TriggerEvent, request as nats_request};
use serde_json::{Value, json};
use strum::IntoEnumIterator;
use ulid::Ulid;

pub async fn dispatch(name: &str, arguments: Value) -> Option<Result<String, Error>> {
    match name {
        "list_stored_emails" => Some(tool_list_stored_emails(arguments).await),
        "get_stored_email" => Some(tool_get_stored_email(arguments).await),
        "create_stored_email" => Some(tool_create_stored_email(arguments).await),
        "update_stored_email" => Some(tool_update_stored_email(arguments).await),
        "list_email_triggers" => Some(tool_list_email_triggers(arguments).await),
        "create_email_trigger" => Some(tool_create_email_trigger(arguments).await),
        "update_email_trigger" => Some(tool_update_email_trigger(arguments).await),
        "list_trigger_events" => Some(tool_list_trigger_events().await),
        "list_imported_orrery_emails" => Some(tool_list_imported(arguments).await),
        "get_imported_orrery_email" => Some(tool_get_imported(arguments).await),
        _ => None,
    }
}

// ==================== Argument helpers ====================

fn string_arg(arguments: &Value, name: &str) -> Result<String, Error> {
    arguments
        .get(name)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| anyhow!("Missing required argument: {name}"))
}

fn optional_string(arguments: &Value, name: &str) -> Option<String> {
    arguments
        .get(name)
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn ulid_arg(arguments: &Value, name: &str) -> Result<Ulid, Error> {
    let raw = string_arg(arguments, name)?;
    Ulid::from_string(&raw).map_err(|_| anyhow!("`{name}` is not a ULID: {raw}"))
}

fn datasets_arg(arguments: &Value) -> Vec<String> {
    arguments
        .get("datasets")
        .and_then(Value::as_array)
        .map(|names| {
            names
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// The body as it is stored. Callers hand over HTML because that is what the
/// copy being ported is written in.
fn body_delta_from(arguments: &Value) -> Result<String, Error> {
    let html = string_arg(arguments, "body_html")?;
    common::quill::html_to_delta(&html)
        .map_err(|error| anyhow!("Could not convert the body HTML to Quill Delta: {error}"))
}

/// The body as it reads. A stored Delta this cannot render is reported rather
/// than hidden: it means the record is not editable in the Engine Room either.
fn body_html_of(body_delta: &str) -> Value {
    match common::quill::delta_to_html(body_delta) {
        Ok(html) => json!(html),
        Err(error) => json!(format!("<!-- unrenderable body: {error} -->")),
    }
}

fn stored_email_json(email: &StoredEmail) -> Value {
    json!({
        "stored_email_id": email.stored_email_id.to_string(),
        "slug": email.slug,
        "subject": email.subject,
        "datasets": email.datasets,
        "reply_to_noreply": email.reply_to_noreply,
        "no_unsubscribe": email.no_unsubscribe,
        "updated_at": email.updated_at.to_string(),
    })
}

// ==================== Stored emails ====================

async fn tool_list_stored_emails(arguments: Value) -> Result<String, Error> {
    let response: ListStoredEmailsResponse = nats_request(
        "emailer.stored.list",
        ListStoredEmailsRequest {
            page: arguments
                .get("page")
                .and_then(Value::as_u64)
                .unwrap_or(1)
                .max(1) as u32,
            per_page: arguments
                .get("per_page")
                .and_then(Value::as_u64)
                .unwrap_or(50)
                .clamp(1, 200) as u32,
            search: optional_string(&arguments, "search"),
        },
    )
    .await?;

    let emails: Vec<Value> = response.emails.iter().map(stored_email_json).collect();

    Ok(serde_json::to_string_pretty(
        &json!({ "total": response.total, "emails": emails }),
    )?)
}

async fn tool_get_stored_email(arguments: Value) -> Result<String, Error> {
    let response: GetStoredEmailResponse = nats_request(
        "emailer.stored.get",
        GetStoredEmailRequest {
            stored_email_id: ulid_arg(&arguments, "stored_email_id")?,
        },
    )
    .await?;

    let mut email = stored_email_json(&response.email);
    email["body_html"] = body_html_of(&response.email.body_delta);

    Ok(serde_json::to_string_pretty(&email)?)
}

async fn tool_create_stored_email(arguments: Value) -> Result<String, Error> {
    let response: CreateStoredEmailResponse = nats_request(
        "emailer.stored.create",
        CreateStoredEmailRequest {
            slug: string_arg(&arguments, "slug")?,
            subject: string_arg(&arguments, "subject")?,
            body_delta: body_delta_from(&arguments)?,
            datasets: datasets_arg(&arguments),
            reply_to_noreply: arguments
                .get("reply_to_noreply")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            no_unsubscribe: arguments
                .get("no_unsubscribe")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        },
    )
    .await?;

    Ok(serde_json::to_string_pretty(&json!({
        "stored_email_id": response.stored_email_id.to_string(),
        "note": "Created, but nothing sends it yet. Point a trigger at it with create_email_trigger.",
    }))?)
}

async fn tool_update_stored_email(arguments: Value) -> Result<String, Error> {
    let body_delta = match arguments.get("body_html") {
        Some(_) => Some(body_delta_from(&arguments)?),
        None => None,
    };

    let response: UpdateStoredEmailResponse = nats_request(
        "emailer.stored.update",
        UpdateStoredEmailRequest {
            stored_email_id: ulid_arg(&arguments, "stored_email_id")?,
            slug: optional_string(&arguments, "slug"),
            subject: optional_string(&arguments, "subject"),
            body_delta,
            datasets: arguments.get("datasets").map(|_| datasets_arg(&arguments)),
            reply_to_noreply: arguments.get("reply_to_noreply").and_then(Value::as_bool),
            no_unsubscribe: arguments.get("no_unsubscribe").and_then(Value::as_bool),
        },
    )
    .await?;

    Ok(serde_json::to_string_pretty(
        &json!({ "success": response.success }),
    )?)
}

// ==================== Triggers ====================

fn action_type_arg(arguments: &Value) -> Result<TriggerActionType, Error> {
    match arguments.get("action_type").and_then(Value::as_str) {
        None | Some("immediate") => Ok(TriggerActionType::Immediate),
        Some("delayed") => Ok(TriggerActionType::Delayed),
        Some(other) => Err(anyhow!(
            "`action_type` must be \"immediate\" or \"delayed\", not {other:?}"
        )),
    }
}

/// A trigger fired by an event nothing raises will never send. Checked here
/// rather than left to discovery in production, where the symptom is silence.
fn check_trigger_name(name: &str) -> Result<(), Error> {
    if TriggerEvent::iter().any(|event| event.as_ref() == name) {
        return Ok(());
    }

    Err(anyhow!(
        "No code raises the event {name:?}. Call list_trigger_events for the events that exist."
    ))
}

async fn tool_list_email_triggers(arguments: Value) -> Result<String, Error> {
    let response: ListTriggersResponse = nats_request(
        "emailer.triggers.list",
        ListTriggersRequest {
            active_only: arguments
                .get("active_only")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        },
    )
    .await?;

    let triggers: Vec<Value> = response
        .triggers
        .iter()
        .map(|trigger| {
            json!({
                "trigger_id": trigger.trigger_id.to_string(),
                "trigger_name": trigger.trigger_name,
                "stored_email_id": trigger.stored_email_id.to_string(),
                "action_type": trigger.action_type.as_str(),
                "delay_seconds": trigger.delay_seconds,
                "recipients": trigger.recipients,
                "active": trigger.active,
            })
        })
        .collect();

    Ok(serde_json::to_string_pretty(
        &json!({ "triggers": triggers }),
    )?)
}

async fn tool_create_email_trigger(arguments: Value) -> Result<String, Error> {
    let trigger_name = string_arg(&arguments, "trigger_name")?;
    check_trigger_name(&trigger_name)?;

    let response: CreateTriggerResponse = nats_request(
        "emailer.triggers.create",
        CreateTriggerRequest {
            trigger_name,
            stored_email_id: ulid_arg(&arguments, "stored_email_id")?,
            action_type: action_type_arg(&arguments)?,
            delay_seconds: arguments.get("delay_seconds").and_then(Value::as_i64),
            cancellation_trigger_name: optional_string(&arguments, "cancellation_trigger_name"),
            recipients: optional_string(&arguments, "recipients"),
        },
    )
    .await?;

    Ok(serde_json::to_string_pretty(&json!({
        "trigger_id": response.trigger_id.to_string(),
    }))?)
}

async fn tool_update_email_trigger(arguments: Value) -> Result<String, Error> {
    let stored_email_id = match arguments.get("stored_email_id") {
        Some(_) => Some(ulid_arg(&arguments, "stored_email_id")?),
        None => None,
    };

    let action_type = match arguments.get("action_type") {
        Some(_) => Some(action_type_arg(&arguments)?),
        None => None,
    };

    let response: UpdateTriggerResponse = nats_request(
        "emailer.triggers.update",
        UpdateTriggerRequest {
            trigger_id: ulid_arg(&arguments, "trigger_id")?,
            stored_email_id,
            action_type,
            delay_seconds: arguments.get("delay_seconds").and_then(Value::as_i64),
            cancellation_trigger_name: optional_string(&arguments, "cancellation_trigger_name"),
            recipients: optional_string(&arguments, "recipients"),
            active: arguments.get("active").and_then(Value::as_bool),
        },
    )
    .await?;

    Ok(serde_json::to_string_pretty(
        &json!({ "success": response.success }),
    )?)
}

async fn tool_list_trigger_events() -> Result<String, Error> {
    let events: Vec<Value> = TriggerEvent::iter()
        .map(|event| {
            json!({
                "trigger_name": event.as_ref(),
                "category": event.category(),
                "description": event.description(),
            })
        })
        .collect();

    Ok(serde_json::to_string_pretty(&json!({ "events": events }))?)
}

// ==================== Orrery's imported templates ====================

async fn tool_list_imported(arguments: Value) -> Result<String, Error> {
    let response: ListOneoffEmailsResponse = nats_request(
        "emailer.oneoff.list",
        ListOneoffEmailsRequest {
            page: 1,
            per_page: 500,
            include_archived: true,
        },
    )
    .await?;

    let search = optional_string(&arguments, "search").map(|term| term.to_lowercase());

    let emails: Vec<Value> = response
        .emails
        .iter()
        .filter(|email| email.legacy_slug.is_some())
        .filter(|email| match &search {
            Some(term) => {
                email.subject.to_lowercase().contains(term)
                    || email
                        .legacy_slug
                        .as_deref()
                        .is_some_and(|slug| slug.to_lowercase().contains(term))
            }
            None => true,
        })
        .map(|email| {
            json!({
                "oneoff_email_id": email.oneoff_email_id.to_string(),
                "legacy_slug": email.legacy_slug,
                "subject": email.subject,
            })
        })
        .collect();

    Ok(serde_json::to_string_pretty(&json!({ "emails": emails }))?)
}

async fn tool_get_imported(arguments: Value) -> Result<String, Error> {
    let response: GetOneoffEmailResponse = nats_request(
        "emailer.oneoff.get",
        GetOneoffEmailRequest {
            oneoff_email_id: ulid_arg(&arguments, "oneoff_email_id")?,
        },
    )
    .await?;

    Ok(serde_json::to_string_pretty(&json!({
        "oneoff_email_id": response.email.oneoff_email_id.to_string(),
        "legacy_slug": response.email.legacy_slug,
        "subject": response.email.subject,
        "body_html": body_html_of(&response.email.body_delta),
    }))?)
}

// ==================== Tool definitions ====================

pub fn tool_definitions() -> Vec<Value> {
    vec![
        json!({
            "name": "list_trigger_events",
            "description": "List the events the code can raise, each with the trigger_name an email trigger must carry to be sent by it. An event with no active trigger sends nothing at all -- no error, no fallback -- so this is the authoritative list of what an email can be attached to.",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        json!({
            "name": "list_stored_emails",
            "description": "List the stored emails the site sends automatically. Returns id, slug, subject and datasets, but not bodies; use get_stored_email for one body.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "search": { "type": "string", "description": "Optional substring to match against slug and subject" },
                    "page": { "type": "integer", "description": "1-based page number (default 1)" },
                    "per_page": { "type": "integer", "description": "Results per page, 1-200 (default 50)" }
                }
            }
        }),
        json!({
            "name": "get_stored_email",
            "description": "Get one stored email, including its body rendered back to HTML.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "stored_email_id": { "type": "string", "description": "The stored email's ULID" }
                },
                "required": ["stored_email_id"]
            }
        }),
        json!({
            "name": "create_stored_email",
            "description": "Create a stored email. The body is given as HTML and converted to the Quill Delta the Engine Room editor reads. Creating it does not send it: attach it to an event with create_email_trigger. Merge fields are written {{LIKE_THIS}}; orrery's templates spell them {like_this}.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "slug": { "type": "string", "description": "Short identifying name, unique among stored emails" },
                    "subject": { "type": "string", "description": "Subject line" },
                    "body_html": { "type": "string", "description": "Body as HTML" },
                    "datasets": { "type": "array", "items": { "type": "string" }, "description": "Names of datasets whose variables the body may use" },
                    "reply_to_noreply": { "type": "boolean", "description": "Send replies to the no-reply address (default false)" },
                    "no_unsubscribe": { "type": "boolean", "description": "Send without an Unsubscribe link or one-click header, for an email its recipient cannot opt out of (default false)" }
                },
                "required": ["slug", "subject", "body_html"]
            }
        }),
        json!({
            "name": "update_stored_email",
            "description": "Update a stored email. Only the fields you provide are changed.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "stored_email_id": { "type": "string", "description": "The stored email's ULID" },
                    "slug": { "type": "string" },
                    "subject": { "type": "string" },
                    "body_html": { "type": "string", "description": "Replacement body as HTML" },
                    "datasets": { "type": "array", "items": { "type": "string" } },
                    "reply_to_noreply": { "type": "boolean" },
                    "no_unsubscribe": { "type": "boolean" }
                },
                "required": ["stored_email_id"]
            }
        }),
        json!({
            "name": "list_email_triggers",
            "description": "List the triggers that attach stored emails to events.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "active_only": { "type": "boolean", "description": "Only triggers that are currently active (default false)" }
                }
            }
        }),
        json!({
            "name": "create_email_trigger",
            "description": "Send a stored email whenever an event is raised. The trigger_name must be one list_trigger_events reports, or nothing will ever fire it; this is checked before the trigger is created.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "trigger_name": { "type": "string", "description": "The event's trigger_name, from list_trigger_events" },
                    "stored_email_id": { "type": "string", "description": "ULID of the stored email to send" },
                    "action_type": { "type": "string", "enum": ["immediate", "delayed"], "description": "Send at once, or after delay_seconds (default immediate)" },
                    "delay_seconds": { "type": "integer", "description": "Required for action_type 'delayed'" },
                    "cancellation_trigger_name": { "type": "string", "description": "Optional event that cancels a pending delayed send" },
                    "recipients": { "type": "string", "description": "Optional recipient override; omit to send to the user the event names" }
                },
                "required": ["trigger_name", "stored_email_id"]
            }
        }),
        json!({
            "name": "update_email_trigger",
            "description": "Update a trigger. Only the fields you provide are changed. Set active to false to stop it sending without deleting it.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "trigger_id": { "type": "string", "description": "The trigger's ULID" },
                    "stored_email_id": { "type": "string" },
                    "action_type": { "type": "string", "enum": ["immediate", "delayed"] },
                    "delay_seconds": { "type": "integer" },
                    "cancellation_trigger_name": { "type": "string" },
                    "recipients": { "type": "string" },
                    "active": { "type": "boolean" }
                },
                "required": ["trigger_id"]
            }
        }),
        json!({
            "name": "list_imported_orrery_emails",
            "description": "List orrery's message templates as the emailer imported them: one-off emails carrying the orrery slug they came from, with merge fields already converted and bodies already in Delta. This is where to find the copy for an email being ported, rather than retyping it.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "search": { "type": "string", "description": "Optional substring to match against the orrery slug and the subject" }
                }
            }
        }),
        json!({
            "name": "get_imported_orrery_email",
            "description": "Get one imported orrery template, with its body rendered back to HTML -- ready to hand to create_stored_email.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "oneoff_email_id": { "type": "string", "description": "The imported email's ULID, from list_imported_orrery_emails" }
                },
                "required": ["oneoff_email_id"]
            }
        }),
    ]
}
