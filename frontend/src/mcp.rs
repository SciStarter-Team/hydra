//! Lightweight MCP (Model Context Protocol) implementation for SciStarter.
//!
//! This module provides read-only access to SciStarter data for AI agents.
//! It implements a minimal subset of MCP over HTTP using JSON-RPC 2.0.
//!
//! The handler in this file (`mcp_handler`, mounted at `/-mcp`) is
//! public and read-only. The [`content`] submodule provides a separate,
//! token-authenticated endpoint for staff, serving both its own content
//! editing tools, the email tools in [`email`], and the database query
//! tools in [`query`]. It reuses the JSON-RPC scaffolding defined here.

pub mod content;
pub mod email;
pub mod golinks;
pub mod query;

use anyhow::Error;
use common::shared::db;
use poem::{IntoResponse, Response, handler, web::Json};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use ulid::Ulid;

use crate::project::{ProjectStatus, push_listing_rule};

/// Base URL for SciStarter
const SCISTARTER_BASE_URL: &str = "https://scistarter.org";

/// JSON-RPC 2.0 request
#[derive(Debug, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub method: String,
    #[serde(default)]
    pub params: Option<Value>,
    pub id: Option<Value>,
}

/// JSON-RPC 2.0 response
#[derive(Debug, Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
    pub id: Option<Value>,
}

impl JsonRpcResponse {
    pub fn success(id: Option<Value>, result: Value) -> Self {
        Self {
            jsonrpc: "2.0",
            result: Some(result),
            error: None,
            id,
        }
    }

    pub fn error(id: Option<Value>, code: i32, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0",
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.into(),
                data: None,
            }),
            id,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

// Standard JSON-RPC error codes
pub(crate) const INVALID_REQUEST: i32 = -32600;
pub(crate) const METHOD_NOT_FOUND: i32 = -32601;
pub(crate) const INVALID_PARAMS: i32 = -32602;
pub(crate) const INTERNAL_ERROR: i32 = -32603;

/// MCP protocol version we support
pub(crate) const PROTOCOL_VERSION: &str = "2024-11-05";

/// Server information
const SERVER_NAME: &str = "SciStarter MCP";
pub(crate) const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Tool definitions for MCP
pub(crate) fn get_tools() -> Value {
    json!([
        {
            "name": "search_projects",
            "description": "Search for citizen science projects by keyword. Returns matching projects with basic info.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search query (keywords to match against project names and descriptions)"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of results to return (default: 10, max: 50)",
                        "default": 10
                    }
                },
                "required": ["query"]
            }
        },
        {
            "name": "get_project",
            "description": "Get detailed information about a specific citizen science project.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "project_id": {
                        "type": "string",
                        "description": "The project's ULID identifier"
                    }
                },
                "required": ["project_id"]
            }
        },
        {
            "name": "search_events",
            "description": "Search for citizen science events by keyword.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search query (keywords to match against event names)"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of results to return (default: 10, max: 50)",
                        "default": 10
                    }
                },
                "required": ["query"]
            }
        },
        {
            "name": "get_event",
            "description": "Get detailed information about a specific citizen science event.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "event_id": {
                        "type": "string",
                        "description": "The event's ULID identifier"
                    }
                },
                "required": ["event_id"]
            }
        },
        {
            "name": "list_topics",
            "description": "List all available project topics/categories.",
            "inputSchema": {
                "type": "object",
                "properties": {}
            }
        },
        {
            "name": "list_activities",
            "description": "List all available project activity types.",
            "inputSchema": {
                "type": "object",
                "properties": {}
            }
        }
    ])
}

/// Handle MCP JSON-RPC requests
#[handler]
pub async fn mcp_handler(Json(request): Json<JsonRpcRequest>) -> Response {
    if request.jsonrpc != "2.0" {
        return Json(JsonRpcResponse::error(
            request.id,
            INVALID_REQUEST,
            "Invalid JSON-RPC version",
        ))
        .into_response();
    }

    let response = match request.method.as_str() {
        "initialize" => handle_initialize(request.id).await,
        "initialized" => handle_initialized(request.id).await,
        "tools/list" => handle_tools_list(request.id).await,
        "tools/call" => handle_tools_call(request.id, request.params).await,
        _ => JsonRpcResponse::error(request.id, METHOD_NOT_FOUND, "Method not found"),
    };

    Json(response).into_response()
}

async fn handle_initialize(id: Option<Value>) -> JsonRpcResponse {
    JsonRpcResponse::success(
        id,
        json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": {
                "tools": {}
            },
            "serverInfo": {
                "name": SERVER_NAME,
                "version": SERVER_VERSION
            }
        }),
    )
}

async fn handle_initialized(id: Option<Value>) -> JsonRpcResponse {
    JsonRpcResponse::success(id, json!({}))
}

async fn handle_tools_list(id: Option<Value>) -> JsonRpcResponse {
    JsonRpcResponse::success(
        id,
        json!({
            "tools": get_tools()
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
        "search_projects" => tool_search_projects(arguments).await,
        "get_project" => tool_get_project(arguments).await,
        "search_events" => tool_search_events(arguments).await,
        "get_event" => tool_get_event(arguments).await,
        "list_topics" => tool_list_topics().await,
        "list_activities" => tool_list_activities().await,
        _ => Err(anyhow::anyhow!("Unknown tool: {}", name)),
    };

    tool_result(id, result)
}

/// Wrap a tool's textual result into a JSON-RPC `tools/call` response,
/// or an internal error response if the tool failed. Shared with the
/// authenticated [`content`] endpoint.
pub(crate) fn tool_result(id: Option<Value>, result: Result<String, Error>) -> JsonRpcResponse {
    match result {
        Ok(content) => JsonRpcResponse::success(
            id,
            json!({
                "content": [
                    {
                        "type": "text",
                        "text": content
                    }
                ]
            }),
        ),
        Err(e) => JsonRpcResponse::error(id, INTERNAL_ERROR, e.to_string()),
    }
}

// Tool implementations

/// A project as `search_projects` lists it.
#[derive(sqlx::FromRow)]
struct ProjectSearchRow {
    project_id: String,
    name: String,
    description: String,
    status: String,
    slug: String,
}

async fn tool_search_projects(args: Value) -> Result<String, Error> {
    let query = args
        .get("query")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing required parameter: query"))?;

    let limit = args
        .get("limit")
        .and_then(|v| v.as_i64())
        .unwrap_or(10)
        .min(50) as i32;

    if query.trim().is_empty() {
        return Ok("[]".to_string());
    }

    let search_term = crate::search::fts5_prefix_query(query);

    let mut qb = sqlx::QueryBuilder::new(
        "SELECT p.project_id, p.name, p.description, p.status, ent.slug \
         FROM projects_fts f \
         JOIN projects p ON f.project_id = p.project_id \
         JOIN entities ent ON ent.target = p.project_id AND ent.kind = 'Project' \
         WHERE f.name MATCH ",
    );
    qb.push_bind(search_term);
    qb.push(" AND ent.publish = 1");
    push_listing_rule(&mut qb, &ProjectStatus::LISTED_WHEN_INACTIVE);
    qb.push(" ORDER BY rank LIMIT ");
    qb.push_bind(limit);

    let rows = qb
        .build_query_as::<ProjectSearchRow>()
        .fetch_all(db())
        .await?;

    let results: Vec<Value> = rows
        .into_iter()
        .map(|row| {
            json!({
                "project_id": row.project_id,
                "name": row.name,
                "description": truncate_string(&row.description, 200),
                "status": row.status,
                "url": format!("{}/{}", SCISTARTER_BASE_URL, row.slug)
            })
        })
        .collect();

    Ok(serde_json::to_string_pretty(&results)?)
}

/// A project as `get_project` describes it.
#[derive(sqlx::FromRow)]
struct ProjectRow {
    project_id: String,
    name: String,
    description: String,
    status: String,
    goal: String,
    task: String,
    difficulty: String,
    time_required: String,
    location_type: String,
    city: String,
    state: String,
    country: String,
    begin: Option<String>,
    end: Option<String>,
    image: String,
    slug: String,
}

async fn tool_get_project(args: Value) -> Result<String, Error> {
    let project_id_str = args
        .get("project_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing required parameter: project_id"))?;

    let _project_id: Ulid = project_id_str
        .parse()
        .map_err(|_| anyhow::anyhow!("Invalid project_id format"))?;

    let mut qb = sqlx::QueryBuilder::new(
        "SELECT p.project_id, p.name, p.description, p.status, p.goal, p.task, \
                p.difficulty, p.time_required, p.location_type, p.city, p.state, p.country, \
                p.begin, p.end, p.image, ent.slug \
         FROM projects p \
         JOIN entities ent ON ent.target = p.project_id AND ent.kind = 'Project' \
         WHERE p.project_id = ",
    );
    qb.push_bind(project_id_str);
    qb.push(" AND ent.publish = 1");
    push_listing_rule(&mut qb, &ProjectStatus::LISTED_WHEN_INACTIVE);

    let row = qb
        .build_query_as::<ProjectRow>()
        .fetch_optional(db())
        .await?;

    let Some(row) = row else {
        return Err(anyhow::anyhow!("Project not found or not available"));
    };

    // Get topics
    let topics: Vec<String> = sqlx::query_scalar!(
        "SELECT topic FROM project_topics WHERE project_id = ?",
        project_id_str
    )
    .fetch_all(db())
    .await?;

    // Get activities
    let activities: Vec<String> = sqlx::query_scalar!(
        "SELECT activity FROM project_activities WHERE project_id = ?",
        project_id_str
    )
    .fetch_all(db())
    .await?;

    let result = json!({
        "project_id": row.project_id,
        "name": row.name,
        "url": format!("{}/{}", SCISTARTER_BASE_URL, row.slug),
        "description": row.description,
        "goal": row.goal,
        "task": row.task,
        "status": row.status,
        "image": row.image,
        "difficulty": row.difficulty,
        "time_required": row.time_required,
        "location_type": row.location_type,
        "city": row.city,
        "state": row.state,
        "country": row.country,
        "begin": row.begin,
        "end": row.end,
        "topics": topics,
        "activities": activities
    });

    Ok(serde_json::to_string_pretty(&result)?)
}

async fn tool_search_events(args: Value) -> Result<String, Error> {
    let query = args
        .get("query")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing required parameter: query"))?;

    let limit = args
        .get("limit")
        .and_then(|v| v.as_i64())
        .unwrap_or(10)
        .min(50) as i32;

    if query.trim().is_empty() {
        return Ok("[]".to_string());
    }

    let search_term = crate::search::fts5_prefix_query(query);

    let rows = sqlx::query!(
        r#"
        SELECT e.event_id, e.name, e.description, e.status, ent.slug
        FROM events_fts f
        JOIN events e ON f.event_id = e.event_id
        JOIN entities ent ON ent.target = e.event_id AND ent.kind = 'Event'
        WHERE f.name MATCH ?
        AND ent.publish = 1
        ORDER BY rank
        LIMIT ?
        "#,
        search_term,
        limit
    )
    .fetch_all(db())
    .await?;

    let results: Vec<Value> = rows
        .into_iter()
        .map(|row| {
            json!({
                "event_id": row.event_id,
                "name": row.name,
                "description": truncate_string(&row.description, 200),
                "status": row.status,
                "url": format!("{}/{}", SCISTARTER_BASE_URL, row.slug)
            })
        })
        .collect();

    Ok(serde_json::to_string_pretty(&results)?)
}

async fn tool_get_event(args: Value) -> Result<String, Error> {
    let event_id_str = args
        .get("event_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing required parameter: event_id"))?;

    let _event_id: Ulid = event_id_str
        .parse()
        .map_err(|_| anyhow::anyhow!("Invalid event_id format"))?;

    let row = sqlx::query!(
        r#"
        SELECT e.event_id, e.name, e.description, e.status, e.image,
               e.city, e.state, e.country, ent.slug
        FROM events e
        JOIN entities ent ON ent.target = e.event_id AND ent.kind = 'Event'
        WHERE e.event_id = ?
        AND ent.publish = 1
        "#,
        event_id_str
    )
    .fetch_optional(db())
    .await?;

    let Some(row) = row else {
        return Err(anyhow::anyhow!("Event not found or not published"));
    };

    // Get event instances (timing)
    let instances = sqlx::query!(
        "SELECT begin, end FROM event_instances WHERE event_id = ? ORDER BY begin",
        event_id_str
    )
    .fetch_all(db())
    .await?;

    let instances_json: Vec<Value> = instances
        .into_iter()
        .map(|i| json!({"begin": i.begin, "end": i.end}))
        .collect();

    let result = json!({
        "event_id": row.event_id,
        "name": row.name,
        "url": format!("{}/{}", SCISTARTER_BASE_URL, row.slug),
        "description": row.description,
        "status": row.status,
        "image": row.image,
        "city": row.city,
        "state": row.state,
        "country": row.country,
        "instances": instances_json
    });

    Ok(serde_json::to_string_pretty(&result)?)
}

async fn tool_list_topics() -> Result<String, Error> {
    let topics: Vec<String> =
        sqlx::query_scalar!("SELECT DISTINCT topic FROM project_topics ORDER BY topic")
            .fetch_all(db())
            .await?;

    Ok(serde_json::to_string_pretty(&topics)?)
}

async fn tool_list_activities() -> Result<String, Error> {
    let activities: Vec<String> =
        sqlx::query_scalar!("SELECT DISTINCT activity FROM project_activities ORDER BY activity")
            .fetch_all(db())
            .await?;

    Ok(serde_json::to_string_pretty(&activities)?)
}

/// Truncate a string to a maximum length, adding "..." if truncated
fn truncate_string(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len.saturating_sub(3)])
    }
}
