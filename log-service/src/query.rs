//! Ad-hoc read-only SQL over the DataFusion analytics context.
//!
//! This is the Parquet-and-summaries counterpart to
//! [`common::query`], which covers the service's SQLite cache. The two
//! answer the same shape of request and return the same shape of
//! response, so the staff query tool treats them as two targets of one
//! tool rather than as two tools.
//!
//! As with the SQLite side, modifying data through this is impossible by
//! structure rather than by inspection of the SQL. Here the structure is
//! DataFusion's own: [`SQLOptions`] plans the caller's SQL, walks the
//! resulting plan tree — subqueries included — for `Ddl`, `Dml`, `Copy`
//! and `Statement` nodes, and refuses the whole plan before any of it
//! runs. `INSERT`, `COPY ... TO`, `CREATE EXTERNAL TABLE` and `SET` are
//! rejected as plan node kinds rather than as text, and DataFusion plans
//! exactly one statement per `sql` call.
//!
//! The SQLite side wraps the caller's query in a sub-select to reach the
//! same end. That is deliberately *not* done here: DataFusion is free to
//! discard a sort it does not have to honour, and a subquery's ordering
//! is not one it has to honour, so wrapping silently threw away the
//! caller's `ORDER BY` and returned an arbitrary slice of the result.
//! Leaving the caller's query as the outermost plan keeps the ordering
//! meaningful, and the row cap goes on top of it as a `LIMIT`, which
//! does force the sort beneath to be honoured.

use common::query::{ColumnInfo, QueryRequest, QueryResponse, Truncation};
use datafusion::arrow::json::writer::{LineDelimited, WriterBuilder};
use datafusion::arrow::record_batch::RecordBatch;
use datafusion::prelude::{SQLOptions, SessionContext};
use futures::StreamExt;

use crate::error::{LogServiceError, Result};

/// Run one query expression against the analytics context.
pub async fn execute(ctx: &SessionContext, request: &QueryRequest) -> Result<QueryResponse> {
    let sql = request
        .sql
        .trim_end_matches(|c: char| c == ';' || c.is_whitespace())
        .trim_start();

    if sql.is_empty() {
        return Err(LogServiceError::Validation(
            "The sql parameter is empty".to_string(),
        ));
    }

    let max_rows = request.max_rows.clamp(1, common::query::ROW_LIMIT);

    let df = ctx.sql_with_options(sql, read_only()).await?;

    let columns = df
        .schema()
        .fields()
        .iter()
        .map(|field| ColumnInfo {
            name: field.name().clone(),
            declared_type: field.data_type().to_string(),
        })
        .collect();

    // One row past the limit, so that truncation can be reported for
    // what it is rather than guessed at from a full result.
    let df = df.limit(0, Some(max_rows as usize + 1))?;

    let mut rows: Vec<String> = Vec::new();
    let mut truncated = Truncation::None;
    let mut bytes = 0;

    let mut batches = df.execute_stream().await?;

    'outer: while let Some(batch) = batches.next().await {
        for encoded in encode_rows(&batch?)? {
            if rows.len() as u32 == max_rows {
                truncated = Truncation::RowLimit;
                break 'outer;
            }

            // The first row always goes out, so that a query over rows
            // wider than the whole budget returns something to look at.
            if !rows.is_empty() && bytes + encoded.len() > common::query::BYTE_BUDGET {
                truncated = Truncation::ByteBudget;
                break 'outer;
            }

            bytes += encoded.len();
            rows.push(encoded);
        }
    }

    Ok(QueryResponse {
        columns,
        rows,
        truncated,
    })
}

/// Plan verification that refuses everything which is not a query.
///
/// Deny-by-default over the plan node kinds, so a node kind added by a
/// later DataFusion release is refused until someone decides otherwise,
/// rather than being allowed through by an allowlist that has not caught
/// up with it.
fn read_only() -> SQLOptions {
    SQLOptions::new()
        .with_allow_ddl(false)
        .with_allow_dml(false)
        .with_allow_statements(false)
}

/// Serialize each row of a batch as its own JSON object, with the fields
/// in schema order.
///
/// Arrow's line-delimited writer gives one object per line, which is
/// exactly the per-row granularity the byte budget needs. Nulls are
/// written explicitly so that every row carries every column, rather
/// than silently dropping the keys whose values happened to be null.
fn encode_rows(batch: &RecordBatch) -> Result<Vec<String>> {
    let mut buffer = Vec::new();

    let mut writer = WriterBuilder::new()
        .with_explicit_nulls(true)
        .build::<_, LineDelimited>(&mut buffer);

    writer.write(batch)?;
    writer.finish()?;

    Ok(String::from_utf8(buffer)
        .map_err(|e| LogServiceError::Validation(format!("query result is not valid UTF-8: {e}")))?
        .lines()
        .map(str::to_string)
        .collect())
}

#[cfg(test)]
mod tests;
