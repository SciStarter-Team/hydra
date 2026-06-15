use crate::error::Result;
use datafusion::arrow::array::StringArray;
use datafusion::arrow::datatypes::{DataType, Field, Schema, TimeUnit};
use datafusion::arrow::record_batch::RecordBatch;
use datafusion::catalog::Session;
use datafusion::datasource::file_format::json::JsonFormat;
use datafusion::datasource::file_format::parquet::ParquetFormat;
use datafusion::datasource::listing::{
    ListingOptions, ListingTable, ListingTableConfig, ListingTableUrl,
};
use datafusion::datasource::{MemTable, TableProvider, TableType};
use datafusion::error::{DataFusionError, Result as DataFusionResult};
use datafusion::execution::context::SessionContext;
use datafusion::execution::memory_pool::FairSpillPool;
use datafusion::execution::runtime_env::RuntimeEnvBuilder;
use datafusion::logical_expr::{Expr, TableProviderFilterPushDown};
use datafusion::physical_plan::ExecutionPlan;
use datafusion::prelude::SessionConfig;
use object_store::local::LocalFileSystem;
use serde::{Deserialize, Serialize};
use sqlx::{QueryBuilder, Row, Sqlite};
use std::any::Any;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use crate::database::table::Table;
use crate::database::tables::{
    ActivityLogRow, BookmarkRow, ClickthroughRow, EmailEventRow, ExchangeEventRow, JoinRow,
    PageViewRow, ParticipationRow, SearchEventRow,
};
use std::path::PathBuf;
use url::Url;

/// Default memory limit for DataFusion queries (2GB)
const DEFAULT_MEMORY_LIMIT_BYTES: usize = 2 * 1024 * 1024 * 1024;

/// Default batch size for DataFusion operations
const DEFAULT_BATCH_SIZE: usize = 8192;

/// SQLite table provider for DataFusion
#[derive(Debug)]
pub struct SqliteTableProvider {
    table_name: String,
    schema: Arc<Schema>,
}

impl SqliteTableProvider {
    pub fn new(table_name: String, schema: Arc<Schema>) -> Self {
        Self { table_name, schema }
    }
}

impl TableProvider for SqliteTableProvider {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn schema(&self) -> Arc<Schema> {
        self.schema.clone()
    }

    fn table_type(&self) -> TableType {
        TableType::Base
    }

    fn supports_filters_pushdown(
        &self,
        filters: &[&Expr],
    ) -> DataFusionResult<Vec<TableProviderFilterPushDown>> {
        // For each filter, determine if we can push it down to SQLite
        let mut pushdowns = Vec::new();
        for filter in filters {
            let pushdown = if self.can_pushdown_filter(filter) {
                TableProviderFilterPushDown::Exact
            } else {
                TableProviderFilterPushDown::Unsupported
            };
            pushdowns.push(pushdown);
        }
        Ok(pushdowns)
    }

    fn scan<'life0, 'life1, 'life2, 'life3, 'async_trait>(
        &'life0 self,
        state: &'life1 dyn Session,
        projection: Option<&'life2 Vec<usize>>,
        filters: &'life3 [Expr],
        limit: Option<usize>,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = DataFusionResult<Arc<dyn ExecutionPlan>>>
                + Send
                + 'async_trait,
        >,
    >
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        'life3: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move {
            // Query SQLite with pushed-down filters and limit
            let data = self.query_sqlite_table_with_filters(filters, limit).await?;

            // Create a MemTable with the data and get its execution plan
            let mem_table = MemTable::try_new(self.schema.clone(), vec![vec![data]])?;
            mem_table.scan(state, projection, &[], limit).await // No filters since we already applied them
        })
    }
}

/// A literal value bound into a query as a parameter rather than interpolated
/// into the SQL text.
enum FilterValue {
    Text(String),
    Int(i64),
    Float(f64),
}

/// A single pushed-down filter expressed as a raw SQL fragment plus an optional
/// bound value.
///
/// `sql` contains only identifiers and operators, which cannot be parameterized;
/// any literal value travels in `value` so it is bound rather than interpolated.
struct SqlCondition {
    sql: String,
    value: Option<FilterValue>,
}

impl SqliteTableProvider {
    fn can_pushdown_filter(&self, filter: &Expr) -> bool {
        use datafusion::logical_expr::Expr;

        match filter {
            // Simple equality comparisons and LIKE operations
            Expr::BinaryExpr(binary_expr) => {
                use datafusion::logical_expr::Operator;
                matches!(
                    binary_expr.op,
                    Operator::Eq
                        | Operator::NotEq
                        | Operator::Lt
                        | Operator::LtEq
                        | Operator::Gt
                        | Operator::GtEq
                        | Operator::LikeMatch
                        | Operator::NotLikeMatch
                        | Operator::ILikeMatch
                        | Operator::NotILikeMatch
                ) && self.is_simple_column_expr(&binary_expr.left)
                    && self.is_literal_expr(&binary_expr.right)
            }
            // IN clauses
            Expr::InList(in_list_expr) => self.is_simple_column_expr(&in_list_expr.expr),
            // IS NULL / IS NOT NULL
            Expr::IsNull(expr) | Expr::IsNotNull(expr) => self.is_simple_column_expr(expr),
            _ => false,
        }
    }

    fn is_simple_column_expr(&self, expr: &Expr) -> bool {
        matches!(expr, Expr::Column(_))
    }

    fn is_literal_expr(&self, expr: &Expr) -> bool {
        matches!(expr, Expr::Literal(_, _))
    }

    fn push_where_clause(
        &self,
        builder: &mut QueryBuilder<Sqlite>,
        filters: &[Expr],
    ) -> DataFusionResult<()> {
        let mut first = true;

        for filter in filters {
            let Some(condition) = self.filter_to_condition(filter)? else {
                continue;
            };

            builder.push(if first { " WHERE " } else { " AND " });
            first = false;

            builder.push(condition.sql);
            match condition.value {
                Some(FilterValue::Text(s)) => {
                    builder.push_bind(s);
                }
                Some(FilterValue::Int(i)) => {
                    builder.push_bind(i);
                }
                Some(FilterValue::Float(f)) => {
                    builder.push_bind(f);
                }
                None => {}
            }
        }

        Ok(())
    }

    /// Translate a single pushed-down filter into a SQL fragment plus an optional
    /// bound value.
    ///
    /// Column names and operators are emitted as raw SQL because SQL identifiers
    /// cannot be parameterized; they originate from the DataFusion query plan and
    /// this table's own schema, not from untrusted input. Literal values are
    /// always returned as bound parameters so they are never interpolated into
    /// the query text.
    fn filter_to_condition(&self, filter: &Expr) -> DataFusionResult<Option<SqlCondition>> {
        use datafusion::common::ScalarValue;
        use datafusion::logical_expr::{Expr, Operator};

        match filter {
            Expr::BinaryExpr(binary_expr) => {
                if let (Expr::Column(col), Expr::Literal(literal, _)) =
                    (&*binary_expr.left, &*binary_expr.right)
                {
                    let column_name = &col.name;

                    // Handle ILIKE specially since SQLite doesn't have native ILIKE
                    match binary_expr.op {
                        Operator::ILikeMatch | Operator::NotILikeMatch => {
                            if let ScalarValue::Utf8(Some(s)) = literal {
                                let not_prefix =
                                    if matches!(binary_expr.op, Operator::NotILikeMatch) {
                                        "NOT "
                                    } else {
                                        ""
                                    };
                                Ok(Some(SqlCondition {
                                    sql: format!("{}UPPER({}) LIKE ", not_prefix, column_name),
                                    value: Some(FilterValue::Text(s.to_uppercase())),
                                }))
                            } else {
                                Ok(None)
                            }
                        }
                        _ => {
                            let op_str = match binary_expr.op {
                                Operator::Eq => "=",
                                Operator::NotEq => "!=",
                                Operator::Lt => "<",
                                Operator::LtEq => "<=",
                                Operator::Gt => ">",
                                Operator::GtEq => ">=",
                                Operator::LikeMatch => "LIKE",
                                Operator::NotLikeMatch => "NOT LIKE",
                                _ => return Ok(None),
                            };

                            let value = match literal {
                                ScalarValue::Utf8(Some(s)) => FilterValue::Text(s.clone()),
                                ScalarValue::Int32(Some(i)) => FilterValue::Int(*i as i64),
                                ScalarValue::Int64(Some(i)) => FilterValue::Int(*i),
                                ScalarValue::Float64(Some(f)) => FilterValue::Float(*f),
                                ScalarValue::Boolean(Some(b)) => {
                                    FilterValue::Int(if *b { 1 } else { 0 })
                                }
                                _ => return Ok(None),
                            };

                            Ok(Some(SqlCondition {
                                sql: format!("{} {} ", column_name, op_str),
                                value: Some(value),
                            }))
                        }
                    }
                } else {
                    Ok(None)
                }
            }
            Expr::IsNull(expr) => {
                if let Expr::Column(col) = &**expr {
                    Ok(Some(SqlCondition {
                        sql: format!("{} IS NULL", col.name),
                        value: None,
                    }))
                } else {
                    Ok(None)
                }
            }
            Expr::IsNotNull(expr) => {
                if let Expr::Column(col) = &**expr {
                    Ok(Some(SqlCondition {
                        sql: format!("{} IS NOT NULL", col.name),
                        value: None,
                    }))
                } else {
                    Ok(None)
                }
            }
            _ => Ok(None),
        }
    }

    async fn query_sqlite_table_with_filters(
        &self,
        filters: &[Expr],
        limit: Option<usize>,
    ) -> DataFusionResult<RecordBatch> {
        // Build dynamic SELECT query based on schema
        let columns_str = self
            .schema
            .fields()
            .iter()
            .map(|f| f.name().clone())
            .collect::<Vec<_>>()
            .join(", ");

        let mut builder = QueryBuilder::new("SELECT ");
        builder.push(columns_str);
        builder.push(" FROM ");
        builder.push(&self.table_name);

        self.push_where_clause(&mut builder, filters)?;

        // Add LIMIT clause if provided to reduce memory usage
        if let Some(n) = limit {
            builder.push(" LIMIT ");
            builder.push_bind(n as i64);
        }

        self.execute_sqlite_query(builder).await
    }

    async fn execute_sqlite_query(
        &self,
        mut builder: QueryBuilder<Sqlite>,
    ) -> DataFusionResult<RecordBatch> {
        let db = common::shared::db();
        let rows = builder
            .build()
            .fetch_all(db)
            .await
            .map_err(|e| DataFusionError::External(Box::new(e)))?;

        // Build column arrays dynamically based on schema
        let mut column_arrays: Vec<Arc<dyn datafusion::arrow::array::Array>> = Vec::new();

        for field in self.schema.fields() {
            let mut column_data: Vec<String> = Vec::new();

            for row in &rows {
                // For now, convert all SQLite data to strings
                // This could be enhanced to handle proper types based on field.data_type()
                let value: Option<String> = row.try_get(field.name().as_str()).ok();
                column_data.push(value.unwrap_or_else(|| "NULL".to_string()));
            }

            column_arrays.push(Arc::new(StringArray::from(column_data)));
        }

        let batch = RecordBatch::try_new(self.schema.clone(), column_arrays)?;
        Ok(batch)
    }
}

/// Main database instance that contains tables and session context
pub struct Database {
    path: PathBuf,
    session_context: SessionContext,
    pub activity_log: Table<ActivityLogRow>,
    pub page_views: Table<PageViewRow>,
    pub clickthroughs: Table<ClickthroughRow>,
    pub join_table: Table<JoinRow>,
    pub participation: Table<ParticipationRow>,
    pub bookmark: Table<BookmarkRow>,
    pub email_events: Table<EmailEventRow>,
    pub exchange_events: Table<ExchangeEventRow>,
    pub search_events: Table<SearchEventRow>,
}

impl Database {
    /// Create a new database instance with the given path
    pub async fn new<P: Into<PathBuf>>(database_path: P) -> Result<Self> {
        let path = database_path.into();

        // Create base directory structure
        tokio::fs::create_dir_all(&path).await?;

        // Configure memory limits from environment or use defaults
        let memory_limit = std::env::var("DATAFUSION_MEMORY_LIMIT_MB")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .map(|mb| mb * 1024 * 1024)
            .unwrap_or(DEFAULT_MEMORY_LIMIT_BYTES);

        let batch_size = std::env::var("DATAFUSION_BATCH_SIZE")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(DEFAULT_BATCH_SIZE);

        // Create memory pool with spill-to-disk capability
        let memory_pool = Arc::new(FairSpillPool::new(memory_limit));

        // Create runtime environment with memory pool
        let runtime_env = RuntimeEnvBuilder::new()
            .with_memory_pool(memory_pool)
            .build_arc()?;

        // Configure session with batch size limits
        let session_config = SessionConfig::new()
            .with_batch_size(batch_size)
            .with_coalesce_batches(true)
            .with_target_partitions(2); // Limit parallelism to reduce memory

        // Create session context with configured runtime and config
        let session_context = SessionContext::new_with_config_rt(session_config, runtime_env);

        let database = Self {
            path: path.clone(),
            session_context,
            activity_log: Table::new(&path).await?,
            page_views: Table::new(&path).await?,
            clickthroughs: Table::new(&path).await?,
            join_table: Table::new(&path).await?,
            participation: Table::new(&path).await?,
            bookmark: Table::new(&path).await?,
            email_events: Table::new(&path).await?,
            exchange_events: Table::new(&path).await?,
            search_events: Table::new(&path).await?,
        };

        // Register all tables with DataFusion
        database.register_all_tables().await?;

        Ok(database)
    }

    /// Get the database path
    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    /// Get the session context
    pub fn context(&self) -> &SessionContext {
        &self.session_context
    }

    /// Gracefully shutdown all tables
    pub async fn shutdown(self) -> Result<()> {
        let results = tokio::join!(
            self.activity_log.shutdown(),
            self.page_views.shutdown(),
            self.clickthroughs.shutdown(),
            self.join_table.shutdown(),
            self.participation.shutdown(),
            self.bookmark.shutdown(),
            self.email_events.shutdown(),
            self.exchange_events.shutdown(),
            self.search_events.shutdown(),
        );

        // Check if any shutdown failed
        for result in [
            results.0, results.1, results.2, results.3, results.4, results.5, results.6, results.7,
            results.8,
        ] {
            result?;
        }

        Ok(())
    }

    /// Register all tables with DataFusion
    async fn register_all_tables(&self) -> Result<()> {
        use crate::database::table::get_registered_tables;

        // Register local file system with DataFusion
        let local_fs = Arc::new(LocalFileSystem::new());
        let file_url = Url::parse("file:///")?;
        self.session_context
            .register_object_store(&file_url, local_fs);

        // Automatically register all TableRow types from inventory
        for table_registration in get_registered_tables() {
            let schema = Arc::new((table_registration.schema)());
            self.create_parquet_table(
                table_registration.name,
                schema,
                &self.path.join("parquet").join(table_registration.name),
            )
            .await?;
        }

        // Register summary and cache tables (these don't use TableRow trait)
        self.register_summary_tables().await?;
        self.register_sqlite_cache_tables().await?;

        // DataFusion database initialized successfully
        Ok(())
    }

    /// Create and register both parquet-only and realtime (parquet+WAL) tables
    async fn create_parquet_table(
        &self,
        table_name: &str,
        schema: Arc<Schema>,
        table_path: &Path,
    ) -> Result<()> {
        // Ensure the parquet directory exists
        tokio::fs::create_dir_all(table_path).await?;

        // Create parquet-only table
        let parquet_absolute_path = table_path.canonicalize()?;
        let parquet_table_url = format!("file:///{}/", parquet_absolute_path.to_string_lossy());

        let parquet_listing_options =
            ListingOptions::new(Arc::new(ParquetFormat::default())).with_file_extension(".parquet");

        let parquet_config = ListingTableConfig::new(ListingTableUrl::parse(&parquet_table_url)?)
            .with_listing_options(parquet_listing_options)
            .with_schema(schema.clone());

        let parquet_table = ListingTable::try_new(parquet_config)?;

        // Register parquet-only table with original name
        self.session_context
            .register_table(table_name, Arc::new(parquet_table.clone()))?;

        // Create WAL table (JSONL files) and union table
        let wal_table_path = self.path.join("wal").join(table_name);
        tokio::fs::create_dir_all(&wal_table_path).await?;

        if let Ok(wal_absolute_path) = wal_table_path.canonicalize() {
            let wal_table_url = format!("file:///{}/", wal_absolute_path.to_string_lossy());

            let wal_listing_options =
                ListingOptions::new(Arc::new(JsonFormat::default())).with_file_extension(".jsonl");

            let wal_config = ListingTableConfig::new(ListingTableUrl::parse(&wal_table_url)?)
                .with_listing_options(wal_listing_options)
                .with_schema(schema.clone());

            if let Ok(wal_table) = ListingTable::try_new(wal_config) {
                // Register WAL table separately with "_wal" suffix
                self.session_context
                    .register_table(&format!("{}_wal", table_name), Arc::new(wal_table))?;

                // Create a SQL view that unions both tables for realtime queries
                let realtime_view_sql = format!(
                    "SELECT * FROM {} UNION ALL SELECT * FROM {}_wal",
                    table_name, table_name
                );

                // Register the view - this gives us the "_realtime" table automatically
                let _ = self
                    .session_context
                    .sql(&format!(
                        "CREATE VIEW {}_realtime AS {}",
                        table_name, realtime_view_sql
                    ))
                    .await;

                // Available tables:
                // - `table_name`: parquet only (fast historical queries)
                // - `table_name_wal`: WAL only (fresh data)
                // - `table_name_realtime`: combined view (complete data)
            }
        }

        Ok(())
    }

    /// Summary tables
    async fn register_summary_tables(&self) -> Result<()> {
        // Daily summary schema
        let daily_schema = Arc::new(Schema::new(vec![
            Field::new("date", DataType::Date32, false),
            Field::new("count", DataType::Int64, false),
            Field::new(
                "updated_at",
                DataType::Timestamp(TimeUnit::Microsecond, None),
                false,
            ),
        ]));

        // Weekly summary schema
        let weekly_schema = Arc::new(Schema::new(vec![
            Field::new("week_start", DataType::Date32, false),
            Field::new("count", DataType::Int64, false),
            Field::new(
                "updated_at",
                DataType::Timestamp(TimeUnit::Microsecond, None),
                false,
            ),
        ]));

        // Register summary tables as parquet-only (no WAL variants)
        self.create_summary_table(
            "clickthrough_daily_summary",
            daily_schema.clone(),
            &self.path.join("clickthrough_daily_summary"),
        )
        .await?;

        self.create_summary_table(
            "clickthrough_weekly_summary",
            weekly_schema.clone(),
            &self.path.join("clickthrough_weekly_summary"),
        )
        .await?;

        self.create_summary_table(
            "join_daily_summary",
            daily_schema,
            &self.path.join("join_daily_summary"),
        )
        .await?;

        self.create_summary_table(
            "join_weekly_summary",
            weekly_schema,
            &self.path.join("join_weekly_summary"),
        )
        .await?;

        Ok(())
    }

    /// Create a summary table (parquet-only, no WAL variants)
    async fn create_summary_table(
        &self,
        table_name: &str,
        schema: Arc<Schema>,
        table_path: &Path,
    ) -> Result<()> {
        // Ensure the summary table directory exists
        tokio::fs::create_dir_all(table_path).await?;

        // Create parquet-only table (no WAL variants needed for summary tables)
        let parquet_absolute_path = table_path.canonicalize()?;
        let parquet_table_url = format!("file:///{}/", parquet_absolute_path.to_string_lossy());

        let parquet_listing_options =
            ListingOptions::new(Arc::new(ParquetFormat::default())).with_file_extension(".parquet");

        let parquet_config = ListingTableConfig::new(ListingTableUrl::parse(&parquet_table_url)?)
            .with_listing_options(parquet_listing_options)
            .with_schema(schema);

        let parquet_table = ListingTable::try_new(parquet_config)?;

        // Register summary table (parquet-only)
        self.session_context
            .register_table(table_name, Arc::new(parquet_table))?;

        Ok(())
    }

    /// Register SQLite cache tables with DataFusion
    async fn register_sqlite_cache_tables(&self) -> Result<()> {
        // Define schema for user_tags_cache
        let user_tags_cache_schema = Arc::new(Schema::new(vec![
            Field::new("user_id", DataType::Utf8, false),
            Field::new("tag", DataType::Utf8, false),
            Field::new("updated_at", DataType::Utf8, false),
        ]));

        // Define schema for user_emails_cache
        let user_emails_cache_schema = Arc::new(Schema::new(vec![
            Field::new("user_id", DataType::Utf8, false),
            Field::new("email", DataType::Utf8, false),
            Field::new("updated_at", DataType::Utf8, false),
        ]));

        // Define schema for user_emails_cache
        let user_tags_emails_cache_schema = Arc::new(Schema::new(vec![
            Field::new("user_id", DataType::Utf8, false),
            Field::new("tag", DataType::Utf8, false),
            Field::new("email", DataType::Utf8, false),
        ]));

        // Register both cache tables
        let user_tags_provider =
            SqliteTableProvider::new("user_tags_cache".to_string(), user_tags_cache_schema);

        let user_emails_provider =
            SqliteTableProvider::new("user_emails_cache".to_string(), user_emails_cache_schema);

        let user_tags_emails_provider = SqliteTableProvider::new(
            "user_tags_emails_cache".to_string(),
            user_tags_emails_cache_schema,
        );

        self.session_context
            .register_table("user_tags_cache", Arc::new(user_tags_provider))?;

        self.session_context
            .register_table("user_emails_cache", Arc::new(user_emails_provider))?;

        self.session_context.register_table(
            "user_tags_emails_cache",
            Arc::new(user_tags_emails_provider),
        )?;

        // SQLite cache tables registered successfully
        Ok(())
    }

    /// Health check for the database and WAL systems
    pub async fn health_check(&self) -> Result<(String, String)> {
        // Check if we can query the session context (basic health)
        let database_status = match self.session_context.sql("SELECT 1").await {
            Ok(_) => "healthy".to_string(),
            Err(e) => {
                eprintln!("Database session context health check failed: {}", e);
                "unhealthy".to_string()
            }
        };

        // Check WAL directory accessibility
        let wal_path = self.path.join("wal");
        let wal_status = if wal_path.exists() && wal_path.is_dir() {
            // Try to read the WAL directory
            match tokio::fs::read_dir(&wal_path).await {
                Ok(_) => "healthy".to_string(),
                Err(e) => {
                    eprintln!("WAL directory health check failed: {}", e);
                    "degraded".to_string()
                }
            }
        } else {
            "initializing".to_string()
        };

        Ok((database_status, wal_status))
    }
}

// Bookmark storage functions (store in simple JSON file for now)

#[derive(Serialize, Deserialize, Default)]
struct BookmarkStorage {
    bookmarks: HashMap<String, String>,
}

async fn load_bookmarks(database_path: &Path) -> Result<BookmarkStorage> {
    let bookmark_file = database_path.join("sync_bookmarks.json");

    if bookmark_file.exists() {
        // Retry lock acquisition with exponential backoff
        let mut retry_delay = std::time::Duration::from_millis(1);
        loop {
            // Open file fresh each time to ensure we see latest data
            let file = std::fs::File::open(&bookmark_file)?;

            match file.try_lock_shared() {
                Ok(_) => {
                    // Read immediately while we have the lock and fresh file handle
                    let content = std::fs::read_to_string(&bookmark_file)?;
                    // Lock is automatically released when file goes out of scope
                    return Ok(serde_json::from_str(&content)?);
                }
                Err(_e) => {
                    // Retry after delay - lock contention is expected
                    tokio::time::sleep(retry_delay).await;
                    retry_delay =
                        std::cmp::min(retry_delay * 2, std::time::Duration::from_millis(500));
                }
            }
        }
    } else {
        Ok(BookmarkStorage::default())
    }
}

pub async fn get_sync_bookmark(database: &Database, key: &str) -> Result<String> {
    let storage = load_bookmarks(&database.path).await?;
    Ok(storage
        .bookmarks
        .get(key)
        .cloned()
        .unwrap_or_else(|| String::from("1900-01-01T00:00:00Z")))
}

pub async fn set_sync_bookmark(database: &Database, key: &str, value: &str) -> Result<()> {
    // Perform atomic read-modify-write with single exclusive lock
    atomic_bookmark_update(&database.path, |storage| {
        storage.bookmarks.insert(key.to_string(), value.to_string());
    })
    .await
}

pub async fn get_sync_last_id(database: &Database, key: &str) -> Result<i32> {
    let storage = load_bookmarks(&database.path).await?;
    match storage.bookmarks.get(key) {
        Some(value) => Ok(value.parse::<i32>().unwrap_or(0)),
        None => Ok(0),
    }
}

pub async fn set_sync_last_id(database: &Database, key: &str, last_id: i32) -> Result<()> {
    // Perform atomic read-modify-write with single exclusive lock
    atomic_bookmark_update(&database.path, |storage| {
        storage
            .bookmarks
            .insert(key.to_string(), last_id.to_string());
    })
    .await
}

/// Perform atomic read-modify-write operation on bookmarks with single exclusive lock
/// This prevents race conditions where concurrent operations could overwrite each other's changes
async fn atomic_bookmark_update<F>(database_path: &Path, update_fn: F) -> Result<()>
where
    F: FnOnce(&mut BookmarkStorage),
{
    let bookmark_file = database_path.join("sync_bookmarks.json");

    // Retry with exponential backoff for lock acquisition (infinite retries for data integrity)
    let mut retry_delay = std::time::Duration::from_millis(1);

    loop {
        // Open file fresh each time for exclusive access
        let file = std::fs::OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(&bookmark_file)?;

        match file.try_lock() {
            Ok(_) => {
                // Successfully acquired exclusive lock - perform atomic read-modify-write

                // Step 1: Read current state while holding lock
                let mut storage = if bookmark_file.exists() {
                    let content = std::fs::read_to_string(&bookmark_file)?;
                    if content.trim().is_empty() {
                        BookmarkStorage::default()
                    } else {
                        serde_json::from_str(&content)?
                    }
                } else {
                    BookmarkStorage::default()
                };

                // Step 2: Apply modification while holding lock
                update_fn(&mut storage);

                // Step 3: Write back while still holding lock
                file.set_len(0)?; // Truncate after acquiring lock
                let content = serde_json::to_string_pretty(&storage)?;
                tokio::fs::write(&bookmark_file, &content).await?;

                // Lock automatically released when file goes out of scope
                return Ok(());
            }
            Err(_e) => {
                // Retry after delay - lock contention is expected
                tokio::time::sleep(retry_delay).await;
                retry_delay = std::cmp::min(retry_delay * 2, std::time::Duration::from_millis(500));
            }
        }
    }
}
