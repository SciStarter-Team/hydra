//! Ad-hoc read-only SQL over the DataFusion analytics context.
//!
//! This is the Parquet-and-summaries counterpart to
//! [`common::query`], which covers the service's SQLite cache. The two
//! answer the same shape of request and return the same shape of
//! response, so the staff query tool treats them as two targets of one
//! tool rather than as two tools.
//!
//! As with the SQLite side, modifying data through this is impossible by
//! structure rather than by inspection of the SQL: the caller's query is
//! interpolated as a sub-select of a wrapper query, and nothing that is
//! not a query expression parses in that position. `INSERT`, `COPY ...
//! TO`, `CREATE EXTERNAL TABLE` and the rest are statements, and
//! DataFusion plans exactly one statement per `sql` call.

use common::query::{ColumnInfo, QueryRequest, QueryResponse, Truncation};
use datafusion::arrow::json::writer::{LineDelimited, WriterBuilder};
use datafusion::arrow::record_batch::RecordBatch;
use datafusion::prelude::SessionContext;
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

    // One row past the limit, so that truncation can be reported for
    // what it is rather than guessed at from a full result.
    let wrapped = format!(
        "SELECT * FROM (\n{sql}\n) AS \"__mcp_query\" LIMIT {}",
        max_rows as u64 + 1
    );

    let df = ctx.sql(&wrapped).await?;

    let columns = df
        .schema()
        .fields()
        .iter()
        .map(|field| ColumnInfo {
            name: field.name().clone(),
            declared_type: field.data_type().to_string(),
        })
        .collect();

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
