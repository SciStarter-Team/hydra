//! Token-authenticated MCP endpoint for viewing and editing site
//! content.
//!
//! Mounted at `/-content-mcp`, this endpoint requires a bearer token
//! issued from the admin Engine Room (see [`crate::mcp_token`]). It
//! exposes tools for reading and writing the same content that staff
//! manage through the admin interface:
//!
//! - **Page content** (`list_content` / `get_content` / `create_content`
//!   / `update_content`) covering four `content_type`s:
//!   - `raw_html_page` — a [`PageContent`] of kind `RawHtml`
//!   - `simple_page` — a [`PageContent`] of kind `Wysiwyg` (body is
//!     Quill Delta JSON)
//!   - `component_page` — a [`PageBuilderPage`]
//!   - `component` — a [`PageBuilderComponent`]
//! - **Entity records** (`list_entities` / `get_entity` /
//!   `create_entity` / `update_entity`) — the slug-to-target routing
//!   layer ([`Entity`]).
//!
//! All writes go through the `LoadStorePeered::store()` path so changes
//! propagate to every frontend instance, exactly as the admin handlers
//! do.

use std::str::FromStr;

use anyhow::{Error, anyhow, bail};
use common::{shared::db, timestring::TimeString};
use poem::{
    IntoResponse, Response, handler,
    http::{HeaderMap, StatusCode},
    web::Json,
};
use serde_json::{Value, json};
use ulid::Ulid;

use super::{
    INVALID_PARAMS, INVALID_REQUEST, JsonRpcRequest, JsonRpcResponse, METHOD_NOT_FOUND,
    PROTOCOL_VERSION, SERVER_VERSION, tool_result,
};
use crate::builder::context::{Context, ContextMode};
use crate::builder::render::{CompiledTemplate, render};
use crate::entity::{Entity, EntityKind, EntityTarget};
use crate::mcp_token::McpToken;
use crate::pagebuilder::source::{ComposeMode, compose_component_into_slot, extract_slots};
use crate::pagebuilder::{PageBuilderComponent, PageBuilderPage};
use crate::pagecontent::{PageContent, PageContentKind};
use crate::peering::LoadStorePeered;

const SERVER_NAME: &str = "SciStarter Content MCP";

/// Handle JSON-RPC requests for the content-management endpoint. Every
/// request must carry a valid `Authorization: Bearer <token>` header.
#[handler]
pub async fn content_mcp_handler(
    headers: &HeaderMap,
    Json(request): Json<JsonRpcRequest>,
) -> Response {
    if let Err(response) = authenticate(headers).await {
        return response;
    }

    if request.jsonrpc != "2.0" {
        return Json(JsonRpcResponse::error(
            request.id,
            INVALID_REQUEST,
            "Invalid JSON-RPC version",
        ))
        .into_response();
    }

    let response = match request.method.as_str() {
        "initialize" => handle_initialize(request.id),
        "initialized" | "notifications/initialized" => {
            JsonRpcResponse::success(request.id, json!({}))
        }
        "tools/list" => JsonRpcResponse::success(request.id, json!({ "tools": get_tools() })),
        "tools/call" => handle_tools_call(request.id, request.params).await,
        _ => JsonRpcResponse::error(request.id, METHOD_NOT_FOUND, "Method not found"),
    };

    Json(response).into_response()
}

/// Verify the request carries a valid MCP bearer token. Returns a 401
/// response if the token is missing or unrecognized.
async fn authenticate(headers: &HeaderMap) -> Result<(), Response> {
    let presented = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|auth| auth.strip_prefix("Bearer "));

    if let Some(token) = presented {
        if McpToken::validate(token).await.is_ok() {
            return Ok(());
        }
    }

    Err(Response::builder()
        .status(StatusCode::UNAUTHORIZED)
        .header("WWW-Authenticate", "Bearer")
        .content_type("application/json")
        .body(
            json!({
                "error": "Unauthorized: a valid MCP bearer token is required"
            })
            .to_string(),
        ))
}

fn handle_initialize(id: Option<Value>) -> JsonRpcResponse {
    JsonRpcResponse::success(
        id,
        json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": { "tools": {} },
            "serverInfo": {
                "name": SERVER_NAME,
                "version": SERVER_VERSION
            }
        }),
    )
}

async fn handle_tools_call(id: Option<Value>, params: Option<Value>) -> JsonRpcResponse {
    let Some(params) = params else {
        return JsonRpcResponse::error(id, INVALID_PARAMS, "Missing params");
    };

    let name = params.get("name").and_then(|v| v.as_str());
    let arguments = params.get("arguments").cloned().unwrap_or(json!({}));

    let Some(name) = name else {
        return JsonRpcResponse::error(id, INVALID_PARAMS, "Missing tool name");
    };

    let result = match name {
        "list_content" => tool_list_content(arguments).await,
        "get_content" => tool_get_content(arguments).await,
        "create_content" => tool_create_content(arguments).await,
        "update_content" => tool_update_content(arguments).await,
        "insert_component" => tool_insert_component(arguments).await,
        "render_preview" => tool_render_preview(arguments).await,
        "list_entities" => tool_list_entities(arguments).await,
        "get_entity" => tool_get_entity(arguments).await,
        "create_entity" => tool_create_entity(arguments).await,
        "update_entity" => tool_update_entity(arguments).await,
        _ => Err(anyhow!("Unknown tool: {}", name)),
    };

    tool_result(id, result)
}

// ==================== Content type dispatch ====================

/// The four kinds of editable page content this endpoint exposes.
#[derive(Clone, Copy)]
enum ContentType {
    RawHtmlPage,
    SimplePage,
    ComponentPage,
    Component,
}

impl ContentType {
    fn parse(args: &Value) -> Result<Self, Error> {
        match required_str(args, "content_type")? {
            "raw_html_page" => Ok(Self::RawHtmlPage),
            "simple_page" => Ok(Self::SimplePage),
            "component_page" => Ok(Self::ComponentPage),
            "component" => Ok(Self::Component),
            other => Err(anyhow!(
                "Unknown content_type '{other}'. Expected one of: raw_html_page, simple_page, component_page, component"
            )),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::RawHtmlPage => "raw_html_page",
            Self::SimplePage => "simple_page",
            Self::ComponentPage => "component_page",
            Self::Component => "component",
        }
    }

    /// For the two `PageContent`-backed types, the underlying kind.
    fn pagecontent_kind(self) -> Option<PageContentKind> {
        match self {
            Self::RawHtmlPage => Some(PageContentKind::RawHtml),
            Self::SimplePage => Some(PageContentKind::Wysiwyg),
            Self::ComponentPage | Self::Component => None,
        }
    }

    /// A short description of how the `body` field is interpreted, for
    /// the benefit of the calling agent.
    fn body_format(self) -> &'static str {
        match self {
            Self::RawHtmlPage => "raw HTML",
            Self::SimplePage => "Quill Delta JSON",
            Self::ComponentPage | Self::Component => "page-builder template source",
        }
    }
}

// ==================== Content tools ====================

async fn tool_list_content(args: Value) -> Result<String, Error> {
    let content_type = ContentType::parse(&args)?;

    let items: Vec<Value> = match content_type {
        ContentType::RawHtmlPage | ContentType::SimplePage => {
            let kind = content_type.pagecontent_kind().unwrap();
            let kind_str = kind.as_ref();
            let rows = sqlx::query!(
                r#"SELECT "pagecontent_id", "name" FROM "pagecontent"
                   WHERE "kind" = ? ORDER BY "pagecontent_id" DESC"#,
                kind_str
            )
            .fetch_all(db())
            .await?;

            rows.into_iter()
                .map(|row| json!({ "id": row.pagecontent_id, "name": row.name }))
                .collect()
        }
        ContentType::ComponentPage => PageBuilderPage::list()
            .await?
            .into_iter()
            .map(|page| {
                json!({
                    "id": page.page_id.to_string(),
                    "name": page.name,
                    "description": page.description,
                    "updated": page.updated.to_string(),
                })
            })
            .collect(),
        ContentType::Component => PageBuilderComponent::list(None)
            .await?
            .into_iter()
            .map(|component| {
                json!({
                    "id": component.component_id.to_string(),
                    "name": component.name,
                    "description": component.description,
                    "category": component.category,
                    "updated": component.updated.to_string(),
                })
            })
            .collect(),
    };

    Ok(serde_json::to_string_pretty(&items)?)
}

async fn tool_get_content(args: Value) -> Result<String, Error> {
    let content_type = ContentType::parse(&args)?;
    let id = parse_ulid(&args, "id")?;

    let result = match content_type {
        ContentType::RawHtmlPage | ContentType::SimplePage => {
            let kind = content_type.pagecontent_kind().unwrap();
            let content = PageContent::load((kind, id)).await?;
            json!({
                "id": id.to_string(),
                "content_type": content_type.label(),
                "name": content.name(),
                "body": content.as_utf8()?,
                "body_format": content_type.body_format(),
            })
        }
        ContentType::ComponentPage => {
            let page = PageBuilderPage::load(id).await?;
            json!({
                "id": page.page_id.to_string(),
                "content_type": content_type.label(),
                "name": page.name,
                "description": page.description,
                "body": page.source,
                "body_format": content_type.body_format(),
                "slots": slots_json(&page.source),
                "created": page.created.to_string(),
                "updated": page.updated.to_string(),
            })
        }
        ContentType::Component => {
            let component = PageBuilderComponent::load(id).await?;
            json!({
                "id": component.component_id.to_string(),
                "content_type": content_type.label(),
                "name": component.name,
                "description": component.description,
                "category": component.category,
                "body": component.source,
                "body_format": content_type.body_format(),
                "slots": slots_json(&component.source),
                "created": component.created.to_string(),
                "updated": component.updated.to_string(),
            })
        }
    };

    Ok(serde_json::to_string_pretty(&result)?)
}

async fn tool_create_content(args: Value) -> Result<String, Error> {
    let content_type = ContentType::parse(&args)?;
    let name = required_str(&args, "name")?.to_string();
    let body = required_str(&args, "body")?.to_string();

    let id = match content_type {
        ContentType::RawHtmlPage | ContentType::SimplePage => {
            reject_field(&args, "description", content_type)?;
            reject_field(&args, "category", content_type)?;

            let kind = content_type.pagecontent_kind().unwrap();
            let id = Ulid::new();
            PageContent::new_with_id(id, kind, name, body.as_bytes())
                .store()
                .await?;
            id
        }
        ContentType::ComponentPage => {
            reject_field(&args, "category", content_type)?;
            validate_template_source(&body)?;

            let mut page = PageBuilderPage::new(name, body);
            if let Some(description) = optional_str(&args, "description") {
                page = page.with_description(description);
            }
            page.store().await?;
            page.page_id
        }
        ContentType::Component => {
            validate_template_source(&body)?;

            let mut component = PageBuilderComponent::new(name, body);
            if let Some(description) = optional_str(&args, "description") {
                component = component.with_description(description);
            }
            if let Some(category) = optional_str(&args, "category") {
                component = component.with_category(category);
            }
            component.store().await?;
            component.component_id
        }
    };

    Ok(serde_json::to_string_pretty(&json!({
        "id": id.to_string(),
        "content_type": content_type.label(),
        "status": "created",
    }))?)
}

async fn tool_update_content(args: Value) -> Result<String, Error> {
    let content_type = ContentType::parse(&args)?;
    let id = parse_ulid(&args, "id")?;

    match content_type {
        ContentType::RawHtmlPage | ContentType::SimplePage => {
            reject_field(&args, "description", content_type)?;
            reject_field(&args, "category", content_type)?;

            let kind = content_type.pagecontent_kind().unwrap();
            let mut content = PageContent::load((kind, id)).await?;
            if let Some(name) = optional_str(&args, "name") {
                content.set_name(name);
            }
            if let Some(body) = optional_str(&args, "body") {
                content.clear();
                content.extend_from_slice(body.as_bytes());
            }
            content.store().await?;
        }
        ContentType::ComponentPage => {
            reject_field(&args, "category", content_type)?;

            let mut page = PageBuilderPage::load(id).await?;
            if let Some(name) = optional_str(&args, "name") {
                page.name = name.to_string();
            }
            if let Some(description) = optional_str(&args, "description") {
                page.description = description.to_string();
            }
            if let Some(body) = optional_str(&args, "body") {
                validate_template_source(body)?;
                page.set_source(body);
            }
            page.updated = TimeString::now();
            page.store().await?;
        }
        ContentType::Component => {
            let mut component = PageBuilderComponent::load(id).await?;
            if let Some(name) = optional_str(&args, "name") {
                component.name = name.to_string();
            }
            if let Some(description) = optional_str(&args, "description") {
                component.description = description.to_string();
            }
            if let Some(category) = optional_str(&args, "category") {
                component.category = category.to_string();
            }
            if let Some(body) = optional_str(&args, "body") {
                validate_template_source(body)?;
                component.set_source(body);
            }
            component.updated = TimeString::now();
            component.store().await?;
        }
    }

    Ok(serde_json::to_string_pretty(&json!({
        "id": id.to_string(),
        "content_type": content_type.label(),
        "status": "updated",
    }))?)
}

// ==================== Page composition tools ====================

/// The two content types backed by page-builder source, which is what
/// the composition and preview tools operate on.
fn require_pagebuilder_type(content_type: ContentType) -> Result<(), Error> {
    match content_type {
        ContentType::ComponentPage | ContentType::Component => Ok(()),
        _ => bail!(
            "This tool applies only to component_page and component, not {}",
            content_type.label()
        ),
    }
}

async fn tool_insert_component(args: Value) -> Result<String, Error> {
    let content_type = ContentType::parse(&args)?;
    require_pagebuilder_type(content_type)?;

    let id = parse_ulid(&args, "id")?;
    let slot_path = required_str(&args, "slot_path")?.to_string();
    let component_id = parse_ulid(&args, "component_id")?;
    let child_name = optional_str(&args, "name");
    let params = object_to_pairs(&args, "params")?;
    let slot_contents = object_to_pairs(&args, "slots")?;
    let mode = match optional_str(&args, "mode") {
        None | Some("append") => ComposeMode::Append,
        Some("replace") => ComposeMode::Replace,
        Some(other) => bail!("Unknown mode '{other}'. Expected 'append' or 'replace'"),
    };

    let component = PageBuilderComponent::load(component_id).await?;

    let child_slot = match content_type {
        ContentType::ComponentPage => {
            let mut page = PageBuilderPage::load(id).await?;
            let (new_source, child) = compose_component_into_slot(
                &page.source,
                &slot_path,
                &component,
                child_name,
                &params,
                &slot_contents,
                mode,
                db(),
            )
            .await?;
            validate_template_source(&new_source)?;
            page.set_source(&new_source);
            page.store().await?;
            child
        }
        ContentType::Component => {
            let mut target = PageBuilderComponent::load(id).await?;
            let (new_source, child) = compose_component_into_slot(
                &target.source,
                &slot_path,
                &component,
                child_name,
                &params,
                &slot_contents,
                mode,
                db(),
            )
            .await?;
            validate_template_source(&new_source)?;
            target.set_source(&new_source);
            target.store().await?;
            child
        }
        _ => unreachable!("guarded by require_pagebuilder_type"),
    };

    let resulting_path = match mode {
        ComposeMode::Append => format!("{slot_path}.{child_slot}"),
        ComposeMode::Replace => slot_path,
    };

    Ok(serde_json::to_string_pretty(&json!({
        "status": "inserted",
        "content_type": content_type.label(),
        "id": id.to_string(),
        "component_id": component_id.to_string(),
        "slot_path": resulting_path,
    }))?)
}

async fn tool_render_preview(args: Value) -> Result<String, Error> {
    let content_type = ContentType::parse(&args)?;
    require_pagebuilder_type(content_type)?;
    let id = parse_ulid(&args, "id")?;

    // For a component, bind its declared parameter defaults so the
    // preview shows representative content rather than empty values.
    let (source, bind_defaults) = match content_type {
        ContentType::ComponentPage => (PageBuilderPage::load(id).await?.source, false),
        ContentType::Component => (PageBuilderComponent::load(id).await?.source, true),
        _ => unreachable!("guarded by require_pagebuilder_type"),
    };

    let ctx = Context::new(db(), &None, ContextMode::Presenting)?;
    if bind_defaults {
        if let Ok(compiled) = CompiledTemplate::compile(&source) {
            for param in compiled.extract_params() {
                if let Some(default) = param.default {
                    ctx.bind_string(&param.name, &default.to_string())?;
                }
            }
        }
    }

    let html = render(&ctx, &source).await?;
    Ok(truncate_on_char_boundary(html, 60_000))
}

/// Truncate a string to at most `max` bytes, ending on a UTF-8 char
/// boundary, appending a note when truncation occurs.
fn truncate_on_char_boundary(s: String, max: usize) -> String {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n\n…[truncated, {} bytes total]", &s[..end], s.len())
}

// ==================== Entity tools ====================

async fn tool_list_entities(args: Value) -> Result<String, Error> {
    let kind_filter = match optional_str(&args, "kind") {
        Some(kind) => Some(parse_entity_kind(kind)?.as_ref().to_string()),
        None => None,
    };

    let rows = sqlx::query!(
        r#"SELECT "slug", "kind", "target", "publish" FROM "entities" ORDER BY "created" DESC"#
    )
    .fetch_all(db())
    .await?;

    let items: Vec<Value> = rows
        .into_iter()
        .filter(|row| kind_filter.as_deref().is_none_or(|kind| row.kind == kind))
        .map(|row| {
            json!({
                "slug": row.slug,
                "kind": row.kind,
                "target": row.target,
                "published": row.publish,
            })
        })
        .collect();

    Ok(serde_json::to_string_pretty(&items)?)
}

async fn tool_get_entity(args: Value) -> Result<String, Error> {
    let slug = required_str(&args, "slug")?.to_string();
    let entity = Entity::load(slug).await?;

    Ok(serde_json::to_string_pretty(&json!({
        "slug": entity.slug(),
        "kind": entity.kind().as_ref(),
        "target": entity.target().value_string(),
        "published": entity.publish(),
        "password": entity.password(),
        "created": entity.created().to_string(),
    }))?)
}

async fn tool_create_entity(args: Value) -> Result<String, Error> {
    let slug = required_str(&args, "slug")?.trim().to_string();
    if slug.is_empty() {
        bail!("slug is required");
    }
    if Entity::slug_exists(&slug).await? {
        bail!("An entity with slug '{slug}' already exists");
    }

    let kind = parse_entity_kind(required_str(&args, "kind")?)?;
    let target = EntityTarget::parse_for_kind(kind, optional_str(&args, "target"))?;
    let publish = args
        .get("publish")
        .and_then(|value| value.as_bool())
        .unwrap_or(true);

    Entity::new_with_publish(&slug, kind, target, publish)
        .store()
        .await?;

    Ok(serde_json::to_string_pretty(&json!({
        "slug": slug,
        "status": "created",
    }))?)
}

async fn tool_update_entity(args: Value) -> Result<String, Error> {
    let slug = required_str(&args, "slug")?.to_string();
    let existing = Entity::load(slug.clone()).await?;

    // Determine the (possibly new) kind, then resolve the target under
    // that kind. If the kind changed but no new target was supplied,
    // reinterpret the existing target string under the new kind.
    let kind = match optional_str(&args, "kind") {
        Some(kind) => parse_entity_kind(kind)?,
        None => *existing.kind(),
    };

    let existing_target = existing.target().value_string();
    let target = if field_present(&args, "target") {
        EntityTarget::parse_for_kind(kind, optional_str(&args, "target"))?
    } else if optional_str(&args, "kind").is_some() {
        EntityTarget::parse_for_kind(kind, Some(existing_target.as_str()))?
    } else {
        existing.target().clone()
    };

    let publish = args
        .get("publish")
        .and_then(|value| value.as_bool())
        .unwrap_or(existing.publish());

    let password = match args.get("password") {
        None => existing.password().map(|s| s.to_string()),
        Some(Value::Null) => None,
        Some(value) => {
            let s = value
                .as_str()
                .ok_or_else(|| anyhow!("password must be a string"))?;
            if s.trim().is_empty() {
                None
            } else {
                Some(s.to_string())
            }
        }
    };

    existing
        .with_kind(kind)
        .with_target(target)
        .with_publish(publish)
        .with_password(password)
        .store()
        .await?;

    Ok(serde_json::to_string_pretty(&json!({
        "slug": slug,
        "status": "updated",
    }))?)
}

// ==================== Argument helpers ====================

fn optional_str<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(|value| value.as_str())
}

fn required_str<'a>(args: &'a Value, key: &str) -> Result<&'a str, Error> {
    optional_str(args, key).ok_or_else(|| anyhow!("Missing required parameter: {key}"))
}

/// Whether `key` is present with a non-null value.
fn field_present(args: &Value, key: &str) -> bool {
    args.get(key).is_some_and(|value| !value.is_null())
}

/// Reject a field that does not apply to the given content type, so a
/// caller never silently loses data they expected to set.
fn reject_field(args: &Value, key: &str, content_type: ContentType) -> Result<(), Error> {
    if field_present(args, key) {
        bail!(
            "The '{key}' field does not apply to {}",
            content_type.label()
        );
    }
    Ok(())
}

/// Extract a JSON object argument as an ordered list of string pairs.
/// Scalar JSON values (string/number/bool) are accepted and stringified;
/// an absent or null key yields an empty list.
fn object_to_pairs(args: &Value, key: &str) -> Result<Vec<(String, String)>, Error> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Object(map)) => map
            .iter()
            .map(|(k, v)| {
                let value = match v {
                    Value::String(s) => s.clone(),
                    Value::Number(n) => n.to_string(),
                    Value::Bool(b) => b.to_string(),
                    Value::Null => String::new(),
                    _ => {
                        bail!("Value for '{k}' in '{key}' must be a string, number, or boolean")
                    }
                };
                Ok((k.clone(), value))
            })
            .collect(),
        Some(_) => bail!("'{key}' must be an object"),
    }
}

/// Summarize a page-builder source's slots for the `get_content`
/// response, so callers can discover insertion points.
fn slots_json(source: &str) -> Value {
    match extract_slots(source) {
        Ok(slots) => Value::Array(
            slots
                .iter()
                .map(|slot| {
                    json!({
                        "path": slot.path,
                        "component_id": slot.component_id.map(|c| c.to_string()),
                        "filled": !slot.is_empty,
                    })
                })
                .collect(),
        ),
        Err(_) => Value::Array(Vec::new()),
    }
}

fn parse_ulid(args: &Value, key: &str) -> Result<Ulid, Error> {
    let value = required_str(args, key)?;
    value
        .parse::<Ulid>()
        .map_err(|_| anyhow!("Invalid {key} (expected a ULID): {value}"))
}

/// Validate that page-builder template source compiles, mirroring the
/// check the admin editor performs before saving a component or page.
fn validate_template_source(source: &str) -> Result<(), Error> {
    crate::builder::render::CompiledTemplate::compile(source)
        .map_err(|e| anyhow!("Invalid template source: {e}"))?;
    Ok(())
}

fn parse_entity_kind(kind: &str) -> Result<EntityKind, Error> {
    EntityKind::from_str(kind).map_err(|_| {
        anyhow!(
            "Unknown entity kind '{kind}'. Expected one of: Redirect, RawHtml, DressedHtml, WordPress, Wysiwyg, PageBuilder, Form, Project, Event, Placeholder"
        )
    })
}

// ==================== Tool schemas ====================

fn get_tools() -> Value {
    json!([
        {
            "name": "list_content",
            "description": "List page content of a given type. Returns id, name, and (where applicable) description/category for each item.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "content_type": content_type_schema(),
                },
                "required": ["content_type"]
            }
        },
        {
            "name": "get_content",
            "description": "Get a single content item by id, including its full body. For simple_page the body is Quill Delta JSON; for raw_html_page it is HTML; for component_page and component it is page-builder template source. For component_page and component the response also includes a 'slots' list (path, component_id, filled) describing insertion points for insert_component.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "content_type": content_type_schema(),
                    "id": { "type": "string", "description": "The ULID of the content item" }
                },
                "required": ["content_type", "id"]
            }
        },
        {
            "name": "create_content",
            "description": "Create a new content item and return its id. 'description' applies only to component_page and component; 'category' applies only to component.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "content_type": content_type_schema(),
                    "name": { "type": "string", "description": "Human-readable name/title" },
                    "body": { "type": "string", "description": "The content body. HTML for raw_html_page, Quill Delta JSON for simple_page, template source for component_page and component." },
                    "description": { "type": "string", "description": "Optional description (component_page and component only)" },
                    "category": { "type": "string", "description": "Optional category (component only; defaults to 'general')" }
                },
                "required": ["content_type", "name", "body"]
            }
        },
        {
            "name": "update_content",
            "description": "Update fields of an existing content item. Only the fields you provide are changed. 'description' applies only to component_page and component; 'category' applies only to component.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "content_type": content_type_schema(),
                    "id": { "type": "string", "description": "The ULID of the content item" },
                    "name": { "type": "string", "description": "New name/title" },
                    "body": { "type": "string", "description": "New body (see create_content for the expected format)" },
                    "description": { "type": "string", "description": "New description (component_page and component only)" },
                    "category": { "type": "string", "description": "New category (component only)" }
                },
                "required": ["content_type", "id"]
            }
        },
        {
            "name": "insert_component",
            "description": "Compose a component into a slot of a component_page or component, returning the resulting slot path. The component's source is inlined so its inner slots stay editable for further composition. 'params' sets the component's scalar parameters; 'slots' fills the component's named inner slots with HTML/source content (use get_content on the component to see its parameters and inner slots). mode 'append' (default) adds the component as a new child slot inside the target slot — use it to stack several components inside one container slot; 'replace' makes the target slot itself become the component.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "content_type": { "type": "string", "enum": ["component_page", "component"], "description": "The kind of target that contains the slot" },
                    "id": { "type": "string", "description": "ULID of the target component_page or component" },
                    "slot_path": { "type": "string", "description": "Path of the existing slot to insert into, as reported by get_content's 'slots' (e.g. 'content' or 'content.cards')" },
                    "component_id": { "type": "string", "description": "ULID of the component to insert" },
                    "name": { "type": "string", "description": "Base name for the new child slot in append mode; defaults to a slug of the component name" },
                    "params": { "type": "object", "description": "Scalar parameter values for the component, e.g. {\"title\": \"...\", \"image_url\": \"...\"}", "additionalProperties": { "type": "string" } },
                    "slots": { "type": "object", "description": "Content for the component's named inner slots, e.g. {\"description\": \"<p>...</p>\"}", "additionalProperties": { "type": "string" } },
                    "mode": { "type": "string", "enum": ["append", "replace"], "description": "append (default) adds a new child slot; replace makes the target slot become the component" }
                },
                "required": ["content_type", "id", "slot_path", "component_id"]
            }
        },
        {
            "name": "render_preview",
            "description": "Render a component_page or component to HTML in presenting mode so you can preview the output before publishing. For a component, its declared parameter defaults are applied. Returns the inner content HTML, without the surrounding site layout.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "content_type": { "type": "string", "enum": ["component_page", "component"], "description": "What to render" },
                    "id": { "type": "string", "description": "ULID of the component_page or component" }
                },
                "required": ["content_type", "id"]
            }
        },
        {
            "name": "list_entities",
            "description": "List entity (slug routing) records, optionally filtered by kind. Returns slug, kind, target, and published status.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "kind": entity_kind_schema("Optional kind to filter by")
                }
            }
        },
        {
            "name": "get_entity",
            "description": "Get a single entity record by slug, including its kind, target, published status, and password (if any).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "slug": { "type": "string", "description": "The entity slug" }
                },
                "required": ["slug"]
            }
        },
        {
            "name": "create_entity",
            "description": "Create a new entity (slug) pointing at a target. The target's meaning depends on kind: a URL for Redirect, a ULID for content/page/project/event kinds, a page URL string for WordPress, and empty for Placeholder.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "slug": { "type": "string", "description": "The slug to create (must be unique)" },
                    "kind": entity_kind_schema("The entity kind"),
                    "target": { "type": "string", "description": "The target value, interpreted according to kind" },
                    "publish": { "type": "boolean", "description": "Whether the entity is published (default true)" }
                },
                "required": ["slug", "kind", "target"]
            }
        },
        {
            "name": "update_entity",
            "description": "Update an existing entity identified by slug. Only the fields you provide are changed. Set 'password' to null or an empty string to remove an existing password.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "slug": { "type": "string", "description": "The slug identifying the entity to update" },
                    "kind": entity_kind_schema("New kind"),
                    "target": { "type": "string", "description": "New target value, interpreted according to kind" },
                    "publish": { "type": "boolean", "description": "New published status" },
                    "password": { "type": ["string", "null"], "description": "New access password, or null/empty to clear" }
                },
                "required": ["slug"]
            }
        }
    ])
}

fn content_type_schema() -> Value {
    json!({
        "type": "string",
        "enum": ["raw_html_page", "simple_page", "component_page", "component"],
        "description": "The kind of content: raw_html_page, simple_page (Quill Delta JSON), component_page, or component"
    })
}

fn entity_kind_schema(description: &str) -> Value {
    json!({
        "type": "string",
        "enum": [
            "Redirect", "RawHtml", "DressedHtml", "WordPress", "Wysiwyg",
            "PageBuilder", "Form", "Project", "Event", "Placeholder"
        ],
        "description": description
    })
}
