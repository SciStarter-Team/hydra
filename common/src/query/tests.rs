use sqlx::SqlitePool;
use sqlx::sqlite::SqliteConnectOptions;

use super::*;
use crate::shared::read_only_pool;

/// A populated database plus a read-only pool over it. The temporary
/// directory lives as long as the fixture, so the file outlives the
/// pool.
struct Fixture {
    _dir: tempfile::TempDir,
    pool: SqlitePool,
}

impl Fixture {
    async fn new() -> Fixture {
        let dir = tempfile::tempdir().expect("temporary directory");
        let path = dir.path().join("fixture.sqlite");

        let writable = SqlitePool::connect_with(
            SqliteConnectOptions::new()
                .filename(&path)
                .create_if_missing(true),
        )
        .await
        .expect("writable pool");

        sqlx::raw_sql(
            r#"
            CREATE TABLE "animals" (
              "id" INTEGER PRIMARY KEY,
              "name" TEXT NOT NULL,
              "mass" REAL,
              "photo" BLOB
            );

            INSERT INTO "animals" ("id", "name", "mass", "photo") VALUES
              (1, 'aardvark', 60.5, x'00ff10'),
              (2, 'zebra', 350.0, NULL),
              (3, 'quokka', NULL, NULL);
            "#,
        )
        .execute(&writable)
        .await
        .expect("fixture schema");

        writable.close().await;

        let pool = read_only_pool(&path).await.expect("read-only pool");

        Fixture { _dir: dir, pool }
    }

    async fn query(&self, sql: &str) -> Result<QueryResponse, Error> {
        self.run(QueryRequest::new(sql, None)).await
    }

    async fn run(&self, request: QueryRequest) -> Result<QueryResponse, Error> {
        execute_on(&self.pool, &request).await
    }
}

/// Parse a row back into a value, so that assertions can talk about the
/// data rather than about its encoding.
fn row(response: &QueryResponse, index: usize) -> serde_json::Value {
    serde_json::from_str(&response.rows[index]).expect("a row is valid JSON")
}

/// In production the read-only pool is a second pool over a database
/// that a service already has open in WAL mode and is writing to. A
/// read-only connection to a WAL database has to reach the shared-memory
/// index to do it, which is the one part of the arrangement that a
/// self-contained fixture would not exercise.
#[tokio::test]
async fn a_database_being_written_can_still_be_read() {
    let dir = tempfile::tempdir().expect("temporary directory");
    let path = dir.path().join("live.sqlite");

    // The same options [`crate::shared::init`] opens a service database
    // with, minus the SpatiaLite extension the fixture has no use for.
    let writable = SqlitePool::connect_with(
        SqliteConnectOptions::new()
            .filename(&path)
            .create_if_missing(true)
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
            .synchronous(sqlx::sqlite::SqliteSynchronous::Normal),
    )
    .await
    .expect("writable pool");

    sqlx::raw_sql(r#"CREATE TABLE "counter" ("n" INTEGER NOT NULL)"#)
        .execute(&writable)
        .await
        .expect("fixture schema");

    let pool = read_only_pool(&path).await.expect("read-only pool");

    // Interleaved, so that the reader sees a WAL with a writer behind it
    // rather than a quiescent file.
    for n in 1..=20 {
        sqlx::query(r#"INSERT INTO "counter" ("n") VALUES (?)"#)
            .bind(n)
            .execute(&writable)
            .await
            .expect("write");

        let response = execute_on(
            &pool,
            &QueryRequest::new(r#"SELECT MAX("n") AS n FROM "counter""#, None),
        )
        .await
        .expect("query should succeed");

        assert_eq!(row(&response, 0)["n"], n);
    }

    writable.close().await;
}

/// Every statement that is not a query expression must fail, without
/// the endpoint having inspected the SQL to decide so.
#[tokio::test]
async fn non_query_statements_are_rejected() {
    let fixture = Fixture::new().await;

    let rejected = [
        "INSERT INTO animals (name) VALUES ('badger')",
        "INSERT INTO animals (name) VALUES ('badger') RETURNING id",
        "UPDATE animals SET name = 'badger'",
        "DELETE FROM animals",
        "CREATE TABLE query_test (id INTEGER)",
        "DROP TABLE animals",
        "ALTER TABLE animals ADD COLUMN query_test INTEGER",
        "CREATE TRIGGER t AFTER INSERT ON animals BEGIN SELECT 1; END",
        // Settings that would undo the read-only mode.
        "PRAGMA query_only = OFF",
        // Reaching a second database that might not be read-only.
        "ATTACH DATABASE 'other.sqlite' AS other",
        // A second statement smuggled in after the first.
        "SELECT 1; DROP TABLE animals",
    ];

    for sql in rejected {
        assert!(
            fixture.query(sql).await.is_err(),
            "should have been rejected: {sql}"
        );
    }
}

/// The pool's own connections refuse writes, so the guarantee does not
/// rest on the shape of the wrapper query alone.
#[tokio::test]
async fn pool_connections_are_read_only() {
    let fixture = Fixture::new().await;

    let error = sqlx::query("CREATE TABLE query_test (id INTEGER)")
        .execute(&fixture.pool)
        .await
        .expect_err("a read-only connection must refuse DDL");

    let message = error.to_string();

    assert!(
        message.contains("readonly")
            || message.contains("read-only")
            || message.contains("read only"),
        "expected a read-only error, got: {message}"
    );
}

/// The wrapper is a sub-select, so the caller's own limits, ordering and
/// CTEs all survive it.
#[tokio::test]
async fn query_expressions_are_evaluated_as_written() {
    let fixture = Fixture::new().await;

    let response = fixture
        .query(
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
    let fixture = Fixture::new().await;

    let response = fixture
        .query("SELECT mass AS zebra, name AS aardvark FROM animals WHERE id = 2")
        .await
        .expect("query should succeed");

    assert_eq!(response.columns[0].name, "zebra");
    assert_eq!(response.columns[1].name, "aardvark");

    // The serialized row keeps the order of the SELECT, which ordering
    // by object key would not preserve.
    let encoded = &response.rows[0];
    assert!(
        encoded.find("\"zebra\"") < encoded.find("\"aardvark\""),
        "row keys are out of order: {encoded}"
    );
}

#[tokio::test]
async fn a_query_matching_nothing_still_reports_its_columns() {
    let fixture = Fixture::new().await;

    let response = fixture
        .query("SELECT id, name FROM animals WHERE name = 'no such animal'")
        .await
        .expect("query should succeed");

    assert!(response.rows.is_empty());
    assert_eq!(response.columns.len(), 2);
    assert_eq!(response.columns[0].name, "id");
    assert_eq!(response.columns[1].name, "name");
}

/// SQLite types a value, not a column, so each value is encoded as
/// whatever it turned out to be.
#[tokio::test]
async fn values_are_encoded_by_their_own_type() {
    let fixture = Fixture::new().await;

    let response = fixture
        .query("SELECT id, name, mass, photo FROM animals ORDER BY id")
        .await
        .expect("query should succeed");

    let aardvark = row(&response, 0);

    assert_eq!(aardvark["id"], 1);
    assert_eq!(aardvark["name"], "aardvark");
    assert_eq!(aardvark["mass"], 60.5);
    // x'00ff10', base64-encoded, since it is well under the inline limit.
    assert_eq!(aardvark["photo"], "AP8Q");

    let quokka = row(&response, 2);

    assert!(quokka["mass"].is_null());
    assert!(quokka["photo"].is_null());
}

#[tokio::test]
async fn a_blob_too_large_to_inline_is_summarized() {
    let fixture = Fixture::new().await;

    let response = fixture
        .query(&format!(
            "SELECT zeroblob({}) AS big",
            BLOB_INLINE_LIMIT + 1
        ))
        .await
        .expect("query should succeed");

    assert_eq!(
        row(&response, 0)["big"],
        format!("<blob: {} bytes>", BLOB_INLINE_LIMIT + 1)
    );
}

/// A join can select the same column name twice, which would otherwise
/// produce a JSON object with a repeated key.
#[tokio::test]
async fn repeated_column_names_are_disambiguated() {
    let fixture = Fixture::new().await;

    let response = fixture
        .query("SELECT a.id, b.id, a.id FROM animals a JOIN animals b ON b.id = a.id + 1")
        .await
        .expect("query should succeed");

    let names: Vec<&str> = response
        .columns
        .iter()
        .map(|column| column.name.as_str())
        .collect();

    assert_eq!(names, ["id", "id:1", "id:2"]);

    let first = row(&response, 0);

    assert_eq!(first["id"], 1);
    assert_eq!(first["id:1"], 2);
    assert_eq!(first["id:2"], 1);
}

/// SQLite does the renaming above itself, so this covers the backstop
/// that keeps a row encodable as a JSON object regardless.
#[test]
fn names_are_made_unique_in_sqlite_s_own_style() {
    assert_eq!(
        unique_names(["id", "id", "name", "id"].into_iter()),
        ["id", "id:1", "name", "id:2"]
    );
}

/// A column that happens to be named like a generated one still has to
/// come out distinct, whatever it ends up being called.
#[test]
fn a_name_that_collides_with_a_generated_one_is_still_made_unique() {
    let names = unique_names(["id", "id", "id:1"].into_iter());

    assert_eq!(names[0], "id");
    assert_eq!(
        names.iter().collect::<std::collections::HashSet<_>>().len(),
        3,
        "names are not distinct: {names:?}"
    );
}

#[tokio::test]
async fn results_are_capped_at_max_rows() {
    let fixture = Fixture::new().await;

    let response = fixture
        .run(QueryRequest::new(
            "SELECT id FROM animals ORDER BY id",
            Some(2),
        ))
        .await
        .expect("query should succeed");

    assert_eq!(response.rows.len(), 2);
    assert_eq!(response.truncated, Truncation::RowLimit);
}

/// A result that exactly fills `max_rows` is complete, not truncated.
#[tokio::test]
async fn a_result_that_exactly_fits_is_not_reported_as_truncated() {
    let fixture = Fixture::new().await;

    let response = fixture
        .run(QueryRequest::new(
            "SELECT id FROM animals ORDER BY id",
            Some(3),
        ))
        .await
        .expect("query should succeed");

    assert_eq!(response.rows.len(), 3);
    assert_eq!(response.truncated, Truncation::None);
}

/// Wide rows have to stop the result before the row limit does, because
/// the whole response has to fit in a NATS message.
#[tokio::test]
async fn wide_rows_stop_at_the_byte_budget() {
    let fixture = Fixture::new().await;

    let response = fixture
        .query(
            "WITH RECURSIVE counter(i) AS (
               SELECT 1 UNION ALL SELECT i + 1 FROM counter WHERE i < 50
             )
             SELECT i, printf('%.*c', 100000, 'x') AS padding FROM counter",
        )
        .await
        .expect("query should succeed");

    assert_eq!(response.truncated, Truncation::ByteBudget);

    let total: usize = response.rows.iter().map(String::len).sum();

    assert!(total <= BYTE_BUDGET, "budget exceeded: {total} bytes");
    assert!(!response.rows.is_empty());
}

/// A single row wider than the whole budget is still worth returning.
#[tokio::test]
async fn one_oversized_row_is_returned_anyway() {
    let fixture = Fixture::new().await;

    let response = fixture
        .query(&format!(
            "SELECT printf('%.*c', {}, 'x') AS padding",
            BYTE_BUDGET + 1000
        ))
        .await
        .expect("query should succeed");

    assert_eq!(response.rows.len(), 1);
    assert_eq!(response.truncated, Truncation::None);
}

#[tokio::test]
async fn a_trailing_semicolon_is_tolerated() {
    let fixture = Fixture::new().await;

    let response = fixture
        .query("  SELECT 1 AS n ;  ")
        .await
        .expect("query should succeed");

    assert_eq!(response.rows.len(), 1);
    assert_eq!(row(&response, 0)["n"], 1);
}

#[tokio::test]
async fn an_empty_query_is_refused() {
    let fixture = Fixture::new().await;

    assert!(fixture.query("   ;  ").await.is_err());
}

/// The schema is reachable through the same tool, so exploring a
/// database does not need an endpoint of its own.
#[tokio::test]
async fn the_schema_is_queryable() {
    let fixture = Fixture::new().await;

    let response = fixture
        .query("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
        .await
        .expect("query should succeed");

    assert_eq!(row(&response, 0)["name"], "animals");

    let response = fixture
        .query("SELECT name, type FROM pragma_table_info('animals') ORDER BY cid")
        .await
        .expect("query should succeed");

    assert_eq!(row(&response, 0)["name"], "id");
    assert_eq!(row(&response, 1)["name"], "name");
}

#[test]
fn truncation_survives_a_flexbuffers_round_trip() {
    for truncation in [
        Truncation::None,
        Truncation::RowLimit,
        Truncation::ByteBudget,
    ] {
        let encoded = flexbuffers::to_vec(truncation).expect("encode");
        let decoded: Truncation = flexbuffers::from_slice(&encoded).expect("decode");

        assert_eq!(decoded, truncation);
    }
}
