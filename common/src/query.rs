//! Ad-hoc read-only `SELECT` access to a service's own SQLite database.
//!
//! Every service that owns a database registers [`serve`] on its NATS
//! service, which makes the database reachable as one of the targets of
//! the staff query tool in the frontend's MCP endpoint. The caller's SQL
//! runs here, on the machine that holds the database file, and only the
//! resulting rows travel back over NATS.
//!
//! Modifying data through this endpoint is impossible by structure
//! rather than by inspection of the SQL — nothing here tries to
//! recognise a dangerous statement:
//!
//! 1. Every statement runs on a dedicated pool
//!    ([`crate::shared::read_only_db`]) whose connections are opened
//!    with `SQLITE_OPEN_READONLY` and put into `query_only` mode.
//!    SQLite itself returns `SQLITE_READONLY` for anything that would
//!    change the database, whatever the statement looks like.
//! 2. The caller's SQL is never a statement in its own right: it is
//!    interpolated as a sub-select of a wrapper query. Anything that is
//!    not a query expression — `INSERT`, `UPDATE`, `DELETE`, DDL,
//!    `PRAGMA`, `ATTACH`, or a trailing second statement — fails to
//!    parse in that position.
//!
//! Each layer is sufficient on its own; together they leave no path from
//! this endpoint to a write.

use anyhow::{Error, anyhow, bail};
use async_nats::service::{Group, Request};
use base64::engine::{Engine, general_purpose::STANDARD as BASE64};
use futures::{StreamExt, TryStreamExt};
use serde::{Deserialize, Serialize};
use sqlx::{
    AssertSqlSafe, Column, Executor, Row, SqlSafeStr, SqlitePool, Statement, TypeInfo, ValueRef,
    sqlite::SqliteRow,
};
use std::time::Duration;

use crate::{handler, service::decode_structured_message, shared::read_only_db};

/// Rows returned when the caller does not ask for a specific number.
pub const DEFAULT_MAX_ROWS: u32 = 100;

/// Ceiling on [`QueryRequest::max_rows`], so a careless `SELECT *`
/// cannot pull a whole table through the tool result.
pub const ROW_LIMIT: u32 = 1000;

/// Ceiling on the total size of the returned rows. A response that
/// crosses NATS has to fit in one message, and `max_payload` is 1MB by
/// default, so a query over wide rows has to stop well before the row
/// limit does. The same ceiling keeps a result readable as a tool
/// result when it does not cross NATS at all.
pub const BYTE_BUDGET: usize = 700_000;

/// Blobs at or under this size are returned base64-encoded; larger ones
/// are summarized, since an encoded megabyte of geometry helps nobody.
const BLOB_INLINE_LIMIT: usize = 1024;

/// How long a query may run before the endpoint gives up on it.
///
/// Nothing upstream imposes a deadline — the frontend serves HTTP
/// directly, with no proxy in front of it and no request timeout of its
/// own — so this is chosen for the tool's own sake rather than to stay
/// under someone else's limit. It is generous because the cost of
/// cutting a query short is not symmetric:
///
/// - On the SQLite targets the timeout bounds how long the caller
///   waits, not how long the query runs. sqlx registers no progress
///   handler, so dropping the future leaves `sqlite3_step` to finish the
///   statement on its worker thread. The work is paid for whether or not
///   anyone is still waiting for the answer, which is a reason to wait
///   for it rather than to throw it away.
/// - On the analytics target the timeout does stop the work: dropping a
///   DataFusion stream stops it being polled and the plan winds down.
///
/// Two minutes is past the point where a staff member would rather
/// narrow the query than keep waiting, while still leaving room for a
/// full scan of the larger Parquet tables or the multi-gigabyte user
/// database.
pub const QUERY_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryRequest {
    /// A single SQL query expression, without a trailing semicolon.
    pub sql: String,
    pub max_rows: u32,
}

impl QueryRequest {
    pub fn new(sql: impl Into<String>, max_rows: Option<u32>) -> QueryRequest {
        QueryRequest {
            sql: sql.into(),
            max_rows: max_rows.unwrap_or(DEFAULT_MAX_ROWS).clamp(1, ROW_LIMIT),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnInfo {
    pub name: String,
    /// The type SQLite declares for the column. SQLite is dynamically
    /// typed, so an individual value may not match it; the JSON type of
    /// the value in each row is what that value actually was.
    pub declared_type: String,
}

/// Why a result stopped short of the whole query.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Truncation {
    /// The result is complete.
    None,
    /// There were more rows than the request's `max_rows`.
    RowLimit,
    /// The rows returned so far filled [`BYTE_BUDGET`].
    ByteBudget,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResponse {
    pub columns: Vec<ColumnInfo>,
    /// Each row, already serialized as a JSON object whose keys are in
    /// the order the query selected them.
    pub rows: Vec<String>,
    pub truncated: Truncation,
}

/// Run [`handle_select`] as the `select` endpoint of `group`.
///
/// A service registers the endpoint with a single line in its `main`:
///
/// ```ignore
/// toplevel.spawn(common::query::serve(service.group("forum.query")));
/// ```
pub async fn serve(group: Group) {
    handler!(group, "select", handle_select).await
}

pub async fn handle_select(req: &Request) -> Result<QueryResponse, Error> {
    execute(&decode_structured_message(req.message.clone())?).await
}

/// Run one query expression against this service's read-only pool.
///
/// The timeout lives here rather than in [`handle_select`] so that it
/// applies however the query arrived — the frontend answers queries
/// against its own database in process, without going through NATS.
pub async fn execute(request: &QueryRequest) -> Result<QueryResponse, Error> {
    tokio::time::timeout(QUERY_TIMEOUT, execute_on(read_only_db().await?, request))
        .await
        .map_err(|_| anyhow!("query timed out after {} seconds", QUERY_TIMEOUT.as_secs()))?
}

/// Run one query expression against `pool`, which must be a pool opened
/// with [`crate::shared::read_only_pool`].
async fn execute_on(pool: &SqlitePool, request: &QueryRequest) -> Result<QueryResponse, Error> {
    let sql = request
        .sql
        .trim_end_matches(|c: char| c == ';' || c.is_whitespace())
        .trim_start();

    if sql.is_empty() {
        bail!("The sql parameter is empty");
    }

    let max_rows = request.max_rows.clamp(1, ROW_LIMIT);

    let mut conn = pool.acquire().await?;

    // Describing the wrapper's projection gives the caller the column
    // names and types even when the query matches no rows.
    let describe_sql = format!("SELECT * FROM (\n{sql}\n) AS \"__mcp_query\" LIMIT 0");

    let columns = {
        // The caller's SQL is deliberately dynamic: running it is what
        // this endpoint is for. What keeps that safe is not the shape of
        // the string but the read-only connection it runs on, and the
        // sub-select position, which leaves no room for a statement.
        let statement = (&mut *conn)
            .prepare(AssertSqlSafe(describe_sql).into_sql_str())
            .await?;

        let names = unique_names(statement.columns().iter().map(|column| column.name()));

        names
            .into_iter()
            .zip(statement.columns())
            .map(|(name, column)| ColumnInfo {
                name,
                declared_type: column.type_info().name().to_string(),
            })
            .collect::<Vec<_>>()
    };

    // One row past the limit, so that truncation can be reported for
    // what it is rather than guessed at from a full result.
    let select_sql = format!(
        "SELECT * FROM (\n{sql}\n) AS \"__mcp_query\" LIMIT {}",
        max_rows as u64 + 1
    );

    let mut rows: Vec<String> = Vec::new();
    let mut truncated = Truncation::None;
    let mut bytes = 0;

    let mut stream = sqlx::query(AssertSqlSafe(select_sql)).fetch(&mut *conn);

    while let Some(row) = stream.try_next().await? {
        if rows.len() as u32 == max_rows {
            truncated = Truncation::RowLimit;
            break;
        }

        let encoded = row_to_json(&row, &columns)?;

        // The first row always goes out, so that a query over rows wider
        // than the whole budget returns something to look at.
        if !rows.is_empty() && bytes + encoded.len() > BYTE_BUDGET {
            truncated = Truncation::ByteBudget;
            break;
        }

        bytes += encoded.len();
        rows.push(encoded);
    }

    Ok(QueryResponse {
        columns,
        rows,
        truncated,
    })
}

/// Serialize one row as a JSON object, keeping the columns in the order
/// the query selected them.
///
/// SQLite stores a type per value rather than per column, so each value
/// is encoded as whatever it turned out to be, not as whatever the
/// column was declared to hold.
fn row_to_json(row: &SqliteRow, columns: &[ColumnInfo]) -> Result<String, Error> {
    let mut out = String::from("{");

    for (index, column) in columns.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }

        out.push_str(&serde_json::to_string(&column.name)?);
        out.push(':');

        let value = row.try_get_raw(index)?;

        if value.is_null() {
            out.push_str("null");
            continue;
        }

        match value.type_info().name() {
            "INTEGER" => out.push_str(&row.try_get::<i64, _>(index)?.to_string()),
            // Non-finite floats have no JSON representation, and
            // serde_json writes them as null.
            "REAL" => out.push_str(&serde_json::to_string(&row.try_get::<f64, _>(index)?)?),
            "BLOB" => {
                let blob = row.try_get::<Vec<u8>, _>(index)?;

                let rendered = if blob.len() <= BLOB_INLINE_LIMIT {
                    BASE64.encode(&blob)
                } else {
                    format!("<blob: {} bytes>", blob.len())
                };

                out.push_str(&serde_json::to_string(&rendered)?);
            }
            _ => out.push_str(&serde_json::to_string(&row.try_get::<String, _>(index)?)?),
        }
    }

    out.push('}');

    Ok(out)
}

/// Give every column a distinct name, so that a query selecting `id`
/// from two joined tables cannot produce a JSON object with the same key
/// twice.
///
/// Expanding the wrapper's `SELECT *` already makes SQLite rename such
/// columns to `id`, `id:1`, `id:2`, so in practice there is nothing left
/// to do here. This keeps the encoding a valid JSON object even for a
/// result where it did not, and follows the same naming so that the two
/// cannot be told apart.
fn unique_names<'a>(names: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut seen: Vec<String> = Vec::new();

    for name in names {
        let mut candidate = name.to_string();
        let mut suffix = 1;

        while seen.contains(&candidate) {
            candidate = format!("{name}:{suffix}");
            suffix += 1;
        }

        seen.push(candidate);
    }

    seen
}

#[cfg(test)]
mod tests;
