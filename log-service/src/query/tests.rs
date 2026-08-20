use std::sync::Arc;

use datafusion::arrow::array::{Float64Array, Int64Array, StringArray};
use datafusion::arrow::datatypes::{DataType, Field, Schema};
use datafusion::prelude::SessionConfig;

use super::*;

/// A context holding one small table, configured the way
/// [`crate::database::storage::Database`] configures its own.
fn fixture() -> SessionContext {
    let ctx = SessionContext::new_with_config(SessionConfig::new().with_information_schema(true));

    let schema = Arc::new(Schema::new(vec![
        Field::new("id", DataType::Int64, false),
        Field::new("name", DataType::Utf8, false),
        Field::new("mass", DataType::Float64, true),
    ]));

    let batch = RecordBatch::try_new(
        schema,
        vec![
            Arc::new(Int64Array::from(vec![1, 2, 3])),
            Arc::new(StringArray::from(vec!["aardvark", "zebra", "quokka"])),
            Arc::new(Float64Array::from(vec![Some(60.5), Some(350.0), None])),
        ],
    )
    .expect("fixture batch");

    ctx.register_batch("animals", batch)
        .expect("register fixture table");

    ctx
}

/// A context whose table is wide enough to exercise the byte budget.
fn wide_fixture() -> SessionContext {
    let ctx = SessionContext::new_with_config(SessionConfig::new().with_information_schema(true));

    let schema = Arc::new(Schema::new(vec![Field::new("id", DataType::Int64, false)]));

    let batch = RecordBatch::try_new(
        schema,
        vec![Arc::new(Int64Array::from((1..=50).collect::<Vec<i64>>()))],
    )
    .expect("fixture batch");

    ctx.register_batch("counter", batch)
        .expect("register fixture table");

    ctx
}

async fn query(ctx: &SessionContext, sql: &str) -> Result<QueryResponse> {
    run(ctx, QueryRequest::new(sql, None)).await
}

async fn run(ctx: &SessionContext, request: QueryRequest) -> Result<QueryResponse> {
    execute(ctx, &request).await
}

/// Parse a row back into a value, so that assertions can talk about the
/// data rather than about its encoding.
fn row(response: &QueryResponse, index: usize) -> serde_json::Value {
    serde_json::from_str(&response.rows[index]).expect("a row is valid JSON")
}

/// Every statement that is not a query expression must fail, without
/// the endpoint having inspected the SQL to decide so.
#[tokio::test]
async fn non_query_statements_are_rejected() {
    let ctx = fixture();

    let rejected = [
        "INSERT INTO animals VALUES (4, 'badger', 12.0)",
        "CREATE TABLE query_test AS SELECT 1",
        "CREATE EXTERNAL TABLE query_test STORED AS PARQUET LOCATION '/tmp/query_test'",
        "CREATE VIEW query_test AS SELECT 1",
        "DROP TABLE animals",
        "COPY animals TO '/tmp/query_test.parquet'",
        "SET datafusion.execution.batch_size = 1",
        // A second statement smuggled in after the first.
        "SELECT 1; DROP TABLE animals",
    ];

    for sql in rejected {
        assert!(
            query(&ctx, sql).await.is_err(),
            "should have been rejected: {sql}"
        );
    }
}

#[tokio::test]
async fn query_expressions_are_evaluated_as_written() {
    let ctx = fixture();

    let response = query(
        &ctx,
        "WITH heavy AS (SELECT name, mass FROM animals WHERE mass IS NOT NULL)
         SELECT name FROM heavy ORDER BY mass DESC LIMIT 1",
    )
    .await
    .expect("query should succeed");

    assert_eq!(response.rows.len(), 1);
    assert_eq!(row(&response, 0)["name"], "zebra");
}

#[tokio::test]
async fn columns_are_reported_in_the_order_they_were_selected() {
    let ctx = fixture();

    let response = query(
        &ctx,
        "SELECT mass AS zebra, name AS aardvark FROM animals WHERE id = 2",
    )
    .await
    .expect("query should succeed");

    assert_eq!(response.columns[0].name, "zebra");
    assert_eq!(response.columns[1].name, "aardvark");

    let encoded = &response.rows[0];
    assert!(
        encoded.find("\"zebra\"") < encoded.find("\"aardvark\""),
        "row keys are out of order: {encoded}"
    );
}

#[tokio::test]
async fn a_query_matching_nothing_still_reports_its_columns() {
    let ctx = fixture();

    let response = query(
        &ctx,
        "SELECT id, name FROM animals WHERE name = 'no such animal'",
    )
    .await
    .expect("query should succeed");

    assert!(response.rows.is_empty());
    assert_eq!(response.columns.len(), 2);
    assert_eq!(response.columns[0].name, "id");
    assert_eq!(response.columns[1].declared_type, "Utf8");
}

/// A null has to appear as a null, rather than as a missing key, so that
/// every row of a result has the same shape.
#[tokio::test]
async fn null_values_are_written_explicitly() {
    let ctx = fixture();

    let response = query(&ctx, "SELECT id, mass FROM animals ORDER BY id")
        .await
        .expect("query should succeed");

    let quokka = row(&response, 2);

    assert_eq!(quokka["id"], 3);
    assert!(quokka["mass"].is_null());
    assert!(
        response.rows[2].contains("\"mass\""),
        "the null column was dropped: {}",
        response.rows[2]
    );
}

#[tokio::test]
async fn results_are_capped_at_max_rows() {
    let ctx = fixture();

    let response = run(
        &ctx,
        QueryRequest::new("SELECT id FROM animals ORDER BY id", Some(2)),
    )
    .await
    .expect("query should succeed");

    assert_eq!(response.rows.len(), 2);
    assert_eq!(response.truncated, Truncation::RowLimit);
}

/// A result that exactly fills `max_rows` is complete, not truncated.
#[tokio::test]
async fn a_result_that_exactly_fits_is_not_reported_as_truncated() {
    let ctx = fixture();

    let response = run(
        &ctx,
        QueryRequest::new("SELECT id FROM animals ORDER BY id", Some(3)),
    )
    .await
    .expect("query should succeed");

    assert_eq!(response.rows.len(), 3);
    assert_eq!(response.truncated, Truncation::None);
}

/// Wide rows have to stop the result before the row limit does, because
/// the whole response has to fit in a NATS message.
#[tokio::test]
async fn wide_rows_stop_at_the_byte_budget() {
    let ctx = wide_fixture();

    let response = query(
        &ctx,
        "SELECT id, repeat('x', 100000) AS padding FROM counter ORDER BY id",
    )
    .await
    .expect("query should succeed");

    assert_eq!(response.truncated, Truncation::ByteBudget);

    let total: usize = response.rows.iter().map(String::len).sum();

    assert!(
        total <= common::query::BYTE_BUDGET,
        "budget exceeded: {total} bytes"
    );
    assert!(!response.rows.is_empty());
}

/// A single row wider than the whole budget is still worth returning.
#[tokio::test]
async fn one_oversized_row_is_returned_anyway() {
    let ctx = fixture();

    let response = query(
        &ctx,
        &format!(
            "SELECT repeat('x', {}) AS padding FROM animals WHERE id = 1",
            common::query::BYTE_BUDGET + 1000
        ),
    )
    .await
    .expect("query should succeed");

    assert_eq!(response.rows.len(), 1);
    assert_eq!(response.truncated, Truncation::None);
}

#[tokio::test]
async fn a_trailing_semicolon_is_tolerated() {
    let ctx = fixture();

    let response = query(&ctx, "  SELECT 1 AS n ;  ")
        .await
        .expect("query should succeed");

    assert_eq!(response.rows.len(), 1);
    assert_eq!(row(&response, 0)["n"], 1);
}

#[tokio::test]
async fn an_empty_query_is_refused() {
    let ctx = fixture();

    assert!(query(&ctx, "   ;  ").await.is_err());
}

/// The catalog is reachable through the same tool, so exploring the
/// analytics tables does not need an endpoint of its own.
#[tokio::test]
async fn the_catalog_is_queryable() {
    let ctx = fixture();

    let response = query(
        &ctx,
        "SELECT table_name FROM information_schema.tables
         WHERE table_schema = 'public' ORDER BY table_name",
    )
    .await
    .expect("query should succeed");

    assert_eq!(row(&response, 0)["table_name"], "animals");

    let response = query(
        &ctx,
        "SELECT column_name, data_type FROM information_schema.columns
         WHERE table_name = 'animals' ORDER BY ordinal_position",
    )
    .await
    .expect("query should succeed");

    assert_eq!(row(&response, 0)["column_name"], "id");
    assert_eq!(row(&response, 1)["column_name"], "name");
}
