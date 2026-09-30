//! Go link tools for the content MCP endpoint.
//!
//! A go link is a short address, `/go/<slug>`, that either redirects or serves
//! a tracked file, logging a clickthrough on the way. It can also record a
//! step of the visitor's training and stamp a value into their session. Most
//! of the ten thousand or so that exist were imported from orrery, including
//! the `wpct-…` links the WordPress pages use for click tracking, and nothing
//! re-imports them, so an edit made here is the lasting one.
//!
//! Every write goes through [`GoUrl`]'s own operations, the same ones the
//! admin Files & Links screen uses, so the two enforce identical rules and
//! both reach every frontend instance.

use anyhow::{Error, anyhow};
use serde_json::{Value, json};
use ulid::Ulid;

use crate::gourl::{GoUrl, GoUrlType};

pub async fn dispatch(name: &str, arguments: Value) -> Option<Result<String, Error>> {
    match name {
        "list_go_links" => Some(tool_list_go_links(arguments).await),
        "get_go_link" => Some(tool_get_go_link(arguments).await),
        "create_go_link" => Some(tool_create_go_link(arguments).await),
        "update_go_link" => Some(tool_update_go_link(arguments).await),
        "delete_go_link" => Some(tool_delete_go_link(arguments).await),
        _ => None,
    }
}

// ==================== Argument helpers ====================

fn string_arg(arguments: &Value, name: &str) -> Result<String, Error> {
    optional_string(arguments, name).ok_or_else(|| anyhow!("Missing required argument: {name}"))
}

fn optional_string(arguments: &Value, name: &str) -> Option<String> {
    arguments
        .get(name)
        .and_then(Value::as_str)
        .map(|value| value.trim().to_string())
}

/// The destination named by `target_url` or `file_id`, if either is given.
/// Naming both is refused rather than resolved by preference.
fn destination_arg(arguments: &Value) -> Result<Option<GoUrlType>, Error> {
    match (
        optional_string(arguments, "target_url"),
        optional_string(arguments, "file_id"),
    ) {
        (Some(_), Some(_)) => Err(anyhow!(
            "Give target_url or file_id, not both: a go link either redirects or serves a file"
        )),
        (Some(target_url), None) => Ok(Some(GoUrlType::Redirect { target_url })),
        (None, Some(file_id)) => Ok(Some(GoUrlType::TrackedFile {
            file_id: Ulid::from_string(&file_id)
                .map_err(|_| anyhow!("`file_id` is not a ULID: {file_id}"))?,
        })),
        (None, None) => Ok(None),
    }
}

/// Apply whichever of the optional fields were given. An empty string clears
/// a field.
fn apply_optional_fields(go_url: &mut GoUrl, arguments: &Value) {
    if let Some(statemachine) = optional_string(arguments, "statemachine") {
        go_url.statemachine = statemachine;
    }
    if let Some(transition) = optional_string(arguments, "transition") {
        go_url.transition = transition;
    }
    if let Some(key) = optional_string(arguments, "session_set_key") {
        go_url.session_set_key = key;
    }
    if let Some(value) = optional_string(arguments, "session_set_value") {
        go_url.session_set_value = value;
    }
}

fn go_link_json(go_url: &GoUrl) -> Value {
    let mut link = json!({
        "slug": go_url.slug,
        "path": format!("/go/{}", go_url.slug),
        "statemachine": go_url.statemachine,
        "transition": go_url.transition,
        "session_set_key": go_url.session_set_key,
        "session_set_value": go_url.session_set_value,
    });

    match &go_url.url_type {
        GoUrlType::Redirect { target_url } => {
            link["kind"] = json!("redirect");
            link["target_url"] = json!(target_url);
        }
        GoUrlType::TrackedFile { file_id } => {
            link["kind"] = json!("tracked_file");
            link["file_id"] = json!(file_id.to_string());
        }
    }

    link
}

// ==================== Tools ====================

async fn tool_list_go_links(arguments: Value) -> Result<String, Error> {
    let search = optional_string(&arguments, "search").map(|term| term.to_lowercase());
    let page = arguments
        .get("page")
        .and_then(Value::as_u64)
        .unwrap_or(1)
        .max(1) as usize;
    let per_page = arguments
        .get("per_page")
        .and_then(Value::as_u64)
        .unwrap_or(50)
        .clamp(1, 200) as usize;

    let matching: Vec<GoUrl> = GoUrl::all()
        .await?
        .into_iter()
        .filter(|go_url| match &search {
            Some(term) => {
                go_url.slug.to_lowercase().contains(term)
                    || go_url.destination().to_lowercase().contains(term)
            }
            None => true,
        })
        .collect();

    let links: Vec<Value> = matching
        .iter()
        .skip((page - 1) * per_page)
        .take(per_page)
        .map(go_link_json)
        .collect();

    Ok(serde_json::to_string_pretty(&json!({
        "total": matching.len(),
        "page": page,
        "per_page": per_page,
        "links": links,
    }))?)
}

async fn tool_get_go_link(arguments: Value) -> Result<String, Error> {
    let go_url = GoUrl::find(&string_arg(&arguments, "slug")?).await?;

    Ok(serde_json::to_string_pretty(&go_link_json(&go_url))?)
}

async fn tool_create_go_link(arguments: Value) -> Result<String, Error> {
    let slug = string_arg(&arguments, "slug")?;
    let url_type = destination_arg(&arguments)?
        .ok_or_else(|| anyhow!("Give target_url for a redirect, or file_id for a tracked file"))?;

    let mut go_url = match url_type {
        GoUrlType::Redirect { target_url } => GoUrl::new_redirect(slug, target_url),
        GoUrlType::TrackedFile { file_id } => GoUrl::new_file_url(slug, file_id),
    };
    apply_optional_fields(&mut go_url, &arguments);

    go_url.create().await?;

    Ok(serde_json::to_string_pretty(&go_link_json(&go_url))?)
}

async fn tool_update_go_link(arguments: Value) -> Result<String, Error> {
    let mut go_url = GoUrl::find(&string_arg(&arguments, "slug")?).await?;

    if let Some(url_type) = destination_arg(&arguments)? {
        go_url.url_type = url_type;
    }
    apply_optional_fields(&mut go_url, &arguments);

    go_url.save().await?;

    Ok(serde_json::to_string_pretty(&go_link_json(&go_url))?)
}

async fn tool_delete_go_link(arguments: Value) -> Result<String, Error> {
    let slug = string_arg(&arguments, "slug")?;
    GoUrl::remove(&slug).await?;

    Ok(serde_json::to_string_pretty(&json!({
        "slug": slug,
        "status": "deleted",
    }))?)
}

// ==================== Tool definitions ====================

pub fn tool_definitions() -> Vec<Value> {
    let tracking_fields = json!({
        "statemachine": { "type": "string", "description": "Training whose step a signed-in visitor's click records, by orrery state machine name (e.g. higheredstate). Set together with transition; empty clears both" },
        "transition": { "type": "string", "description": "The training step to record. Set together with statemachine" },
        "session_set_key": { "type": "string", "description": "Session key stamped on visit (signup_tags is the one read today). Keys that establish identity are refused; empty clears it" },
        "session_set_value": { "type": "string", "description": "Value stored under session_set_key" }
    });

    let with_tracking = |mut properties: Value| {
        for (name, schema) in tracking_fields.as_object().expect("an object literal") {
            properties[name] = schema.clone();
        }
        properties
    };

    vec![
        json!({
            "name": "list_go_links",
            "description": "List go links (/go/<slug>): short addresses that redirect or serve a tracked file, logging each click. There are about ten thousand, most imported from orrery; the wpct-… ones are click-tracking links used by the WordPress pages. Search matches slug and destination, so it finds every link pointing at a given path.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "search": { "type": "string", "description": "Case-insensitive substring of the slug or the destination (target URL or file id)" },
                    "page": { "type": "integer", "description": "1-based page number (default 1)" },
                    "per_page": { "type": "integer", "description": "Results per page, 1-200 (default 50)" }
                }
            }
        }),
        json!({
            "name": "get_go_link",
            "description": "Get one go link by slug (case-insensitive).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "slug": { "type": "string", "description": "The go link's slug" }
                },
                "required": ["slug"]
            }
        }),
        json!({
            "name": "create_go_link",
            "description": "Create a go link. Give target_url to redirect, or file_id to serve an uploaded tracked file. The slug may use letters, digits and hyphens, and must be unused in any capitalization.",
            "inputSchema": {
                "type": "object",
                "properties": with_tracking(json!({
                    "slug": { "type": "string", "description": "The new slug; the link is served at /go/<slug>" },
                    "target_url": { "type": "string", "description": "Redirect target: a path on this site starting with /, or an absolute URL with a scheme" },
                    "file_id": { "type": "string", "description": "ULID of an uploaded tracked file to serve instead of redirecting" }
                })),
                "required": ["slug"]
            }
        }),
        json!({
            "name": "update_go_link",
            "description": "Update a go link. Only the fields you provide change. Giving target_url or file_id replaces the destination, and can switch the link between redirecting and serving a file. The slug cannot change: to rename, create the new one and delete the old.",
            "inputSchema": {
                "type": "object",
                "properties": with_tracking(json!({
                    "slug": { "type": "string", "description": "The slug of the link to update" },
                    "target_url": { "type": "string", "description": "New redirect target" },
                    "file_id": { "type": "string", "description": "ULID of an uploaded tracked file to serve instead" }
                })),
                "required": ["slug"]
            }
        }),
        json!({
            "name": "delete_go_link",
            "description": "Permanently delete a go link. Anything still linking to /go/<slug> will get a 404, and the link cannot be restored except by recreating it. For a link that is merely out of date, update its target instead.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "slug": { "type": "string", "description": "The slug of the link to delete" }
                },
                "required": ["slug"]
            }
        }),
    ]
}
