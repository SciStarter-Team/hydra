//! Staff-only ad-hoc `SELECT` access to every SciStarter database.
//!
//! SciStarter's data is spread across one SQLite database per service,
//! each on the machine that runs that service, plus the log service's
//! DataFusion context over its Parquet files. This module presents all
//! of them as targets of a single tool: the caller names a database and
//! writes a query, the query runs where the data lives, and the rows
//! come back over NATS.
//!
//! Nothing here inspects the SQL. Writing through this tool is
//! impossible because of how the query is run at the far end — see
//! [`common::query`] for the SQLite targets and
//! [`log_service::query`]'s counterpart for the analytics one — not
//! because of anything this module recognises or rejects.

use anyhow::{Error, anyhow};
use common::{
    query::{ColumnInfo, DEFAULT_MAX_ROWS, QueryRequest, QueryResponse, ROW_LIMIT, Truncation},
    service::request_timeout,
};
use futures::future::join_all;
use serde::Serialize;
use serde_json::{Value, json, value::RawValue};
use std::time::Duration;

/// Margin on top of the endpoint's own deadline, for the round trip and
/// for carrying back the error a timed-out query reports.
const REQUEST_MARGIN: Duration = Duration::from_secs(5);

/// How long to wait for a service to answer. Derived from the deadline
/// the endpoints hold themselves to rather than chosen alongside it: if
/// the caller gave up first, a query that ran its full time would have
/// its answer thrown away by the one who asked for it.
const REQUEST_TIMEOUT: Duration =
    Duration::from_secs(common::query::QUERY_TIMEOUT.as_secs() + REQUEST_MARGIN.as_secs());

/// Tables listed for one database by `list_databases`.
const SCHEMA_MAX_ROWS: u32 = 500;

/// What SQL dialect a target speaks, and therefore how to ask it what
/// tables it has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Dialect {
    Sqlite,
    DataFusion,
}

impl Dialect {
    fn label(self) -> &'static str {
        match self {
            Dialect::Sqlite => "sqlite",
            Dialect::DataFusion => "datafusion",
        }
    }

    /// A query listing the target's tables, and the column its name
    /// arrives in.
    fn table_listing(self) -> (&'static str, &'static str) {
        match self {
            // Shadow tables belonging to an FTS5 index or a SpatiaLite
            // spatial index are left out: they are storage for the table
            // they belong to, not data anyone means to query.
            Dialect::Sqlite => (
                r#"SELECT "name" FROM "sqlite_master"
                   WHERE "type" IN ('table', 'view')
                     AND "name" NOT LIKE 'sqlite_%'
                     AND "name" NOT LIKE 'idx_%'
                     AND "name" NOT LIKE '%_fts_config'
                     AND "name" NOT LIKE '%_fts_content'
                     AND "name" NOT LIKE '%_fts_data'
                     AND "name" NOT LIKE '%_fts_docsize'
                     AND "name" NOT LIKE '%_fts_idx'
                   ORDER BY "name""#,
                "name",
            ),
            Dialect::DataFusion => (
                "SELECT table_name FROM information_schema.tables
                 WHERE table_schema = 'public' ORDER BY table_name",
                "table_name",
            ),
        }
    }
}

/// How a target is reached.
#[derive(Debug, Clone, Copy)]
enum Source {
    /// Answered in this process, against the database this instance
    /// holds. Going through NATS instead would hand the query to
    /// whichever frontend instance the Service API happened to pick,
    /// which is the wrong answer twice over: the instances are only
    /// eventually consistent with each other, and an edit made through
    /// this same MCP endpoint is applied here first, so a query that
    /// landed elsewhere might not see it.
    Local,
    /// Answered by the service that owns the database, over NATS. Each
    /// of these databases has exactly one owner, so there is no question
    /// of which copy answered.
    Nats(&'static str),
}

/// A queryable database, and where the query for it runs.
#[derive(Debug)]
struct Target {
    name: &'static str,
    source: Source,
    dialect: Dialect,
    description: &'static str,
}

impl Target {
    fn find(name: &str) -> Result<&'static Target, Error> {
        TARGETS
            .iter()
            .find(|target| target.name == name)
            .ok_or_else(|| {
                anyhow!(
                    "Unknown database '{name}'. Expected one of: {}",
                    TARGETS
                        .iter()
                        .map(|target| target.name)
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })
    }

    async fn run(&self, request: QueryRequest) -> Result<QueryResponse, Error> {
        match self.source {
            Source::Local => common::query::execute(&request).await,
            Source::Nats(subject) => request_timeout(subject, request, REQUEST_TIMEOUT).await,
        }
        .map_err(|e| anyhow!("{} database: {e}", self.name))
    }

    /// Which copy of the database answered, where there is more than one
    /// copy for it to have been.
    fn answered_by(&self) -> Option<&'static str> {
        match self.source {
            Source::Local => Some(crate::peering::SERVER_ID_STR.as_str()),
            Source::Nats(_) => None,
        }
    }

    /// The names of the tables this database holds.
    async fn tables(&self) -> Result<Vec<String>, Error> {
        let (sql, column) = self.dialect.table_listing();

        let response = self
            .run(QueryRequest::new(sql, Some(SCHEMA_MAX_ROWS)))
            .await?;

        response
            .rows
            .iter()
            .map(|row| {
                let row: Value = serde_json::from_str(row)?;

                row.get(column)
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .ok_or_else(|| anyhow!("table listing has no '{column}' column"))
            })
            .collect()
    }
}

const TARGETS: &[Target] = &[
    Target {
        name: "frontend",
        source: Source::Local,
        dialect: Dialect::Sqlite,
        description: "The web frontend's own database: entities (the slug routing layer), page \
             content and page-builder pages and components, projects and events with their many \
             detail tables, entity lists, exchanges, go URLs, news, HTML blocks, OAuth2 clients \
             and tokens, and concierge conversations. Spatially enabled, so project, event and \
             region geometry lives in SpatiaLite columns and the SpatiaLite metadata tables are \
             present alongside the application's own. Every frontend instance keeps its own copy \
             of this database, kept in step by peer messages rather than shared, so the copies \
             are only eventually consistent: a query answers from the instance serving this \
             request — reported as `answered_by` — and a write made moments ago on another \
             instance may not have arrived there yet.",
    },
    Target {
        name: "user",
        source: Source::Nats("user.query.select"),
        dialect: Dialect::Sqlite,
        description: "Accounts and profiles, emails, sessions, permissions, badges and \
             certificates, bookmarks, project joins, communities and memberships, user sets and \
             tags, WebAuthn credentials, ambassador applications and messages, and one progress \
             table per training program. Spatially enabled, as above.",
    },
    Target {
        name: "forum",
        source: Source::Nats("forum.query.select"),
        dialect: Dialect::Sqlite,
        description: "Forums, categories, topics and posts, moderators and the moderation log, \
             the content filter's word list, and the message status and reply tables behind \
             user-to-user messaging, which is stored in each user's personal forum.",
    },
    Target {
        name: "emailer",
        source: Source::Nats("emailer.query.select"),
        dialect: Dialect::Sqlite,
        description: "Outgoing mail: the send queue and its per-recipient targets, sendings and \
             send history, stored and one-off emails, scheduled sends, triggers, and the \
             do-not-send list.",
    },
    Target {
        name: "location",
        source: Source::Nats("location.query.select"),
        dialect: Dialect::Sqlite,
        description: "The IP2Location range table behind IP geolocation. Static reference data, \
             replaced wholesale by its own deploy step rather than written to by the service.",
    },
    Target {
        name: "log_cache",
        source: Source::Nats("log.query.select"),
        dialect: Dialect::Sqlite,
        description: "The log service's SQLite cache, for lookups too operational to go through \
             Parquet: acts of science, external links, participation, rating and review caches, \
             Articulate completions, the user tag and email caches, and the iNaturalist and \
             Zooniverse sync tables.",
    },
    Target {
        name: "analytics",
        source: Source::Nats("log.query.analytics"),
        dialect: Dialect::DataFusion,
        description: "The log service's DataFusion context: the activity log, page views, \
             clickthroughs, joins, participation, bookmarks, email events, exchange, search and \
             share events, plus weekly summaries and the SQLite cache tables registered \
             alongside them. Each event table comes in three forms: `<name>` is the compacted \
             Parquet, `<name>_wal` is what has been written since the last compaction, and \
             `<name>_realtime` is the union of the two — query the realtime view unless you \
             specifically want one half. This target speaks DataFusion SQL, not SQLite SQL.",
    },
];

pub fn tool_definitions() -> Vec<Value> {
    let databases = TARGETS
        .iter()
        .map(|target| target.name)
        .collect::<Vec<_>>()
        .join(", ");

    vec![
        json!({
            "name": "list_databases",
            "description": "\
        List the SciStarter databases the run_select_query tool can reach, with what each one holds and \
        what tables it has. Call this first when you do not already know which database a piece of data \
        lives in. Shadow tables belonging to full-text or spatial indexes are left out of the listing; \
        so are databases whose service is not answering, which are reported with an error instead.",
            "inputSchema": {
                "type": "object",
                "properties": {}
            }
        }),
        json!({
            "name": "run_select_query",
            "description": format!("\
Run a read-only SQL query against one of the SciStarter databases and return the rows as JSON.

SciStarter's data is split across one database per service ({databases}); use list_databases to \
        see what each holds. A query reaches exactly one of them — there are no cross-database joins, so \
        correlating data from two services means running one query against each and joining the results \
yourself.

The query is evaluated as a sub-select of a wrapper query, on a connection that cannot write, so \
        only query expressions are accepted: INSERT/UPDATE/DELETE/DDL, PRAGMA, ATTACH, and extra \
statements after a semicolon are all rejected. There is no way to modify data with this tool.

Write the query as a bare SELECT (or a WITH ... SELECT), with no trailing semicolon and no bind \
        parameters — write literal values inline. Column order is preserved. Results are capped at \
max_rows and at roughly 700KB; a `truncated` field says whether, and why, a result stopped short.

All targets except 'analytics' are SQLite. To explore one: \
        SELECT name, sql FROM sqlite_master WHERE type = 'table' ORDER BY name, or \
SELECT * FROM pragma_table_info('users').

The 'analytics' target is DataFusion over Parquet. To explore it: \
        SELECT table_name FROM information_schema.tables WHERE table_schema = 'public' ORDER BY 1, or \
        SELECT column_name, data_type FROM information_schema.columns WHERE table_name = 'page_views' \
        ORDER BY ordinal_position."),
            "inputSchema": {
                "type": "object",
                "properties": {
                    "database": {
                        "type": "string",
                        "enum": TARGETS.iter().map(|target| target.name).collect::<Vec<_>>(),
                        "description": "Which database to query."
                    },
                    "sql": {
                        "type": "string",
                        "description": "A single SQL query expression, without a trailing semicolon."
                    },
                    "max_rows": {
                        "type": "integer",
                        "description": format!("Maximum rows to return (default: {DEFAULT_MAX_ROWS}, max: {ROW_LIMIT})."),
                        "default": DEFAULT_MAX_ROWS
                    }
                },
                "required": ["database", "sql"]
            }
        }),
    ]
}

pub async fn dispatch(name: &str, arguments: Value) -> Option<Result<String, Error>> {
    match name {
        "list_databases" => Some(tool_list_databases().await),
        "run_select_query" => Some(tool_run_select_query(arguments).await),
        _ => None,
    }
}

#[derive(Serialize)]
struct DatabaseListing {
    database: &'static str,
    dialect: &'static str,
    description: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    tables: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

async fn tool_list_databases() -> Result<String, Error> {
    let listings = join_all(TARGETS.iter().map(|target| async move {
        match target.tables().await {
            Ok(tables) => DatabaseListing {
                database: target.name,
                dialect: target.dialect.label(),
                description: target.description,
                tables: Some(tables),
                error: None,
            },
            Err(e) => DatabaseListing {
                database: target.name,
                dialect: target.dialect.label(),
                description: target.description,
                tables: None,
                error: Some(e.to_string()),
            },
        }
    }))
    .await;

    Ok(serde_json::to_string_pretty(&listings)?)
}

#[derive(Serialize)]
struct ColumnListing<'a> {
    name: &'a str,
    #[serde(rename = "type")]
    type_name: &'a str,
}

#[derive(Serialize)]
struct QueryResult<'a> {
    database: &'static str,
    /// Which copy of the database answered, for a database that has more
    /// than one copy. Absent where the question does not arise.
    #[serde(skip_serializing_if = "Option::is_none")]
    answered_by: Option<&'static str>,
    columns: Vec<ColumnListing<'a>>,
    row_count: usize,
    truncated: Value,
    rows: Vec<Box<RawValue>>,
}

async fn tool_run_select_query(args: Value) -> Result<String, Error> {
    let database = args
        .get("database")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("Missing required parameter: database"))?;

    let sql = args
        .get("sql")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("Missing required parameter: sql"))?;

    let max_rows = args.get("max_rows").and_then(Value::as_u64);

    let target = Target::find(database)?;

    let response = target
        .run(QueryRequest::new(sql, max_rows.map(|rows| rows as u32)))
        .await?;

    render(target, &response)
}

fn render(target: &Target, response: &QueryResponse) -> Result<String, Error> {
    // The rows arrive already serialized, which is what keeps their keys
    // in the order the query selected them; a serde_json::Map would sort
    // them instead.
    let rows = response
        .rows
        .iter()
        .map(|row| RawValue::from_string(row.clone()))
        .collect::<Result<Vec<_>, _>>()?;

    Ok(serde_json::to_string_pretty(&QueryResult {
        database: target.name,
        answered_by: target.answered_by(),
        columns: response.columns.iter().map(column_listing).collect(),
        row_count: rows.len(),
        truncated: truncation(response.truncated),
        rows,
    })?)
}

fn column_listing(column: &ColumnInfo) -> ColumnListing<'_> {
    ColumnListing {
        name: &column.name,
        type_name: &column.declared_type,
    }
}

/// Say whether the result is complete, and if not, what to do about it.
fn truncation(truncation: Truncation) -> Value {
    match truncation {
        Truncation::None => json!(false),
        Truncation::RowLimit => json!(
            "row_limit: more rows matched than max_rows. Raise max_rows, or narrow the query."
        ),
        Truncation::ByteBudget => json!(
            "byte_budget: the rows returned filled the response size limit. Select fewer columns, \
             or narrow the query."
        ),
    }
}

#[cfg(test)]
mod tests;
