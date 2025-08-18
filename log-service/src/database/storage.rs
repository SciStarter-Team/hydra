use crate::error::Result;
use arrow::datatypes::{DataType, Field, Schema, TimeUnit};
use datafusion::arrow::array::StringArray;
use datafusion::arrow::record_batch::RecordBatch;
use datafusion::catalog::Session;
use datafusion::datasource::file_format::parquet::ParquetFormat;
use datafusion::datasource::listing::{
    ListingOptions, ListingTable, ListingTableConfig, ListingTableUrl,
};
use datafusion::datasource::{MemTable, TableProvider, TableType};
use datafusion::error::{DataFusionError, Result as DataFusionResult};
use datafusion::execution::context::SessionContext;
use datafusion::logical_expr::{Expr, TableProviderFilterPushDown};
use datafusion::physical_plan::ExecutionPlan;
use object_store::local::LocalFileSystem;
use serde::{Deserialize, Serialize};
use sqlx::Row;
use std::any::Any;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use crate::database::table::Table;
use crate::database::tables::{
    ActivityLogRow, BookmarkRow, ClickthroughRow, JoinRow, PageViewRow, ParticipationRow,
};
use std::path::PathBuf;
use url::Url;

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
            // Query SQLite with pushed-down filters
            let data = self.query_sqlite_table_with_filters(filters).await?;

            // Create a MemTable with the data and get its execution plan
            let mem_table = MemTable::try_new(self.schema.clone(), vec![vec![data]])?;
            mem_table.scan(state, projection, &[], limit).await // No filters since we already applied them
        })
    }
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

    fn build_where_clause(&self, filters: &[Expr]) -> DataFusionResult<String> {
        if filters.is_empty() {
            return Ok(String::new());
        }

        let mut conditions = Vec::new();

        for filter in filters {
            if let Some(condition) = self.filter_to_sql(filter)? {
                conditions.push(condition);
            }
        }

        if conditions.is_empty() {
            Ok(String::new())
        } else {
            Ok(format!(" WHERE {}", conditions.join(" AND ")))
        }
    }

    fn filter_to_sql(&self, filter: &Expr) -> DataFusionResult<Option<String>> {
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
                                let escaped_value =
                                    format!("'{}'", s.to_uppercase().replace('\'', "''"));
                                let not_prefix =
                                    if matches!(binary_expr.op, Operator::NotILikeMatch) {
                                        "NOT "
                                    } else {
                                        ""
                                    };
                                Ok(Some(format!(
                                    "{}UPPER({}) LIKE {}",
                                    not_prefix, column_name, escaped_value
                                )))
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

                            let value_str = match literal {
                                ScalarValue::Utf8(Some(s)) => {
                                    format!("'{}'", s.replace('\'', "''"))
                                } // SQL escape
                                ScalarValue::Int32(Some(i)) => i.to_string(),
                                ScalarValue::Int64(Some(i)) => i.to_string(),
                                ScalarValue::Float64(Some(f)) => f.to_string(),
                                ScalarValue::Boolean(Some(b)) => {
                                    if *b {
                                        "1".to_string()
                                    } else {
                                        "0".to_string()
                                    }
                                }
                                _ => return Ok(None),
                            };

                            Ok(Some(format!("{} {} {}", column_name, op_str, value_str)))
                        }
                    }
                } else {
                    Ok(None)
                }
            }
            Expr::IsNull(expr) => {
                if let Expr::Column(col) = &**expr {
                    Ok(Some(format!("{} IS NULL", col.name)))
                } else {
                    Ok(None)
                }
            }
            Expr::IsNotNull(expr) => {
                if let Expr::Column(col) = &**expr {
                    Ok(Some(format!("{} IS NOT NULL", col.name)))
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
    ) -> DataFusionResult<RecordBatch> {
        let where_clause = self.build_where_clause(filters)?;

        // Build dynamic SELECT query based on schema
        let column_names: Vec<String> = self
            .schema
            .fields()
            .iter()
            .map(|f| f.name().clone())
            .collect();
        let columns_str = column_names.join(", ");
        let sql = format!(
            "SELECT {} FROM {}{}",
            columns_str, self.table_name, where_clause
        );

        self.execute_sqlite_query(&sql).await
    }

    async fn execute_sqlite_query(&self, sql: &str) -> DataFusionResult<RecordBatch> {
        let db = common::shared::db();
        let rows = sqlx::query(&sql)
            .fetch_all(db)
            .await
            .map_err(|e| DataFusionError::External(Box::new(e)))?;

        // Build column arrays dynamically based on schema
        let mut column_arrays: Vec<Arc<dyn arrow::array::Array>> = Vec::new();

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
}

impl Database {
    /// Create a new database instance with the given path
    pub async fn new<P: Into<PathBuf>>(database_path: P) -> Result<Self> {
        let path = database_path.into();

        // Create directory structure for parquet files
        tokio::fs::create_dir_all(&path).await?;

        // Create subdirectories for each table
        for table_name in &[
            "activity_log",
            "page_views",
            "clickthroughs",
            "participation",
            "bookmark",
            "join_table",
            "clickthrough_daily_summary",
            "clickthrough_weekly_summary",
            "join_daily_summary",
            "join_weekly_summary",
        ] {
            tokio::fs::create_dir_all(path.join(table_name)).await?;
        }

        // Create session context and register tables
        let session_context = SessionContext::new();

        let database = Self {
            path: path.clone(),
            session_context,
            activity_log: Table::new(&path).await?,
            page_views: Table::new(&path).await?,
            clickthroughs: Table::new(&path).await?,
            join_table: Table::new(&path).await?,
            participation: Table::new(&path).await?,
            bookmark: Table::new(&path).await?,
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
        );

        // Check if any shutdown failed
        for result in [
            results.0, results.1, results.2, results.3, results.4, results.5,
        ] {
            result?;
        }

        Ok(())
    }

    /// Register all tables with DataFusion
    async fn register_all_tables(&self) -> Result<()> {
        // Register local file system with DataFusion
        let local_fs = Arc::new(LocalFileSystem::new());
        let file_url = Url::parse("file:///")?;
        self.session_context
            .register_object_store(&file_url, local_fs);

        // Create/register all tables
        self.register_activity_log_table().await?;
        self.register_page_views_table().await?;
        self.register_clickthroughs_table().await?;
        self.register_participation_table().await?;
        self.register_bookmark_table().await?;
        self.register_join_table().await?;

        // Register summary tables
        self.register_summary_tables().await?;

        // Register SQLite cache tables
        self.register_sqlite_cache_tables().await?;

        // DataFusion database initialized successfully
        Ok(())
    }

    /// Helper function to create a listing table for Parquet files
    async fn create_parquet_table(
        &self,
        table_name: &str,
        schema: Arc<Schema>,
        table_path: &Path,
    ) -> Result<()> {
        // Ensure the parquet directory exists
        tokio::fs::create_dir_all(table_path).await?;

        // ListingTable expects a directory path that it will scan for files
        // Convert to absolute path and use proper file:/// URL format
        let absolute_path = table_path.canonicalize()?;
        let table_url = format!("file:///{}/", absolute_path.to_string_lossy());

        let listing_options =
            ListingOptions::new(Arc::new(ParquetFormat::default())).with_file_extension(".parquet");

        let config = ListingTableConfig::new(ListingTableUrl::parse(&table_url)?)
            .with_listing_options(listing_options)
            .with_schema(schema);

        let table = ListingTable::try_new(config)?;
        self.session_context
            .register_table(table_name, Arc::new(table))?;

        Ok(())
    }

    /// Activity log table
    async fn register_activity_log_table(&self) -> Result<()> {
        let schema = Arc::new(Schema::new(vec![
            Field::new(
                "when",
                DataType::Timestamp(TimeUnit::Microsecond, None),
                false,
            ),
            Field::new("action", DataType::Utf8, false),
            Field::new("user_id", DataType::Utf8, true),
            Field::new("target", DataType::Utf8, true),
            Field::new("extra", DataType::Utf8, true),
        ]));

        self.create_parquet_table(
            "activity_log",
            schema,
            &self.path.join("parquet").join("activity_log"),
        )
        .await
    }

    /// Page views table
    async fn register_page_views_table(&self) -> Result<()> {
        let schema = Arc::new(Schema::new(vec![
            Field::new(
                "when",
                DataType::Timestamp(TimeUnit::Microsecond, None),
                false,
            ),
            Field::new("ip_address", DataType::Utf8, false),
            Field::new("session_id", DataType::Utf8, false),
            Field::new("interaction_id", DataType::Utf8, false),
            Field::new("user_id", DataType::Utf8, true),
            Field::new("path", DataType::Utf8, false),
            Field::new("referrer", DataType::Utf8, true),
            Field::new("query", DataType::Utf8, true),
            Field::new("geolocation", DataType::Utf8, false),
        ]));

        self.create_parquet_table(
            "page_views",
            schema,
            &self.path.join("parquet").join("page_views"),
        )
        .await
    }

    /// Clickthroughs table
    async fn register_clickthroughs_table(&self) -> Result<()> {
        let schema = Arc::new(Schema::new(vec![
            Field::new(
                "when",
                DataType::Timestamp(TimeUnit::Microsecond, None),
                false,
            ),
            Field::new("ip_address", DataType::Utf8, false),
            Field::new("session_id", DataType::Utf8, false),
            Field::new("interaction_id", DataType::Utf8, false),
            Field::new("user_id", DataType::Utf8, true),
            Field::new("from_path", DataType::Utf8, false),
            Field::new("to_url", DataType::Utf8, false),
            Field::new("link_type", DataType::Utf8, false),
            Field::new("program_id", DataType::Utf8, true),
            Field::new("geolocation", DataType::Utf8, false),
        ]));

        self.create_parquet_table(
            "clickthroughs",
            schema,
            &self.path.join("parquet").join("clickthroughs"),
        )
        .await
    }

    /// Participation table
    async fn register_participation_table(&self) -> Result<()> {
        let schema = Arc::new(Schema::new(vec![
            Field::new("id", DataType::Utf8, false),
            Field::new("origin", DataType::Int16, false),
            Field::new(
                "when",
                DataType::Timestamp(TimeUnit::Microsecond, None),
                false,
            ),
            Field::new("duration", DataType::Float64, false),
            Field::new("where_point", DataType::Utf8, true),
            Field::new("magnitude", DataType::Int32, false),
            Field::new("authenticated", DataType::Boolean, false),
            Field::new("extra", DataType::Utf8, false),
            Field::new("fidelity", DataType::Float64, false),
            Field::new("entity_id", DataType::Utf8, false),
            Field::new("user_id", DataType::Utf8, false),
            Field::new("task_name", DataType::Utf8, true),
        ]));

        self.create_parquet_table(
            "participation",
            schema,
            &self.path.join("parquet").join("participation"),
        )
        .await
    }

    /// Bookmark table
    async fn register_bookmark_table(&self) -> Result<()> {
        let schema = Arc::new(Schema::new(vec![
            Field::new("id", DataType::Utf8, false),
            Field::new(
                "when",
                DataType::Timestamp(TimeUnit::Microsecond, None),
                false,
            ),
            Field::new("entity_id", DataType::Utf8, false),
            Field::new("user_id", DataType::Utf8, false),
            Field::new("active", DataType::Boolean, false),
            Field::new(
                "activity",
                DataType::Timestamp(TimeUnit::Microsecond, None),
                false,
            ),
            Field::new(
                "contacted",
                DataType::Timestamp(TimeUnit::Microsecond, None),
                true,
            ),
            Field::new("current", DataType::Boolean, false),
        ]));

        self.create_parquet_table(
            "bookmark",
            schema,
            &self.path.join("parquet").join("bookmark"),
        )
        .await
    }

    /// Join table
    async fn register_join_table(&self) -> Result<()> {
        let schema = Arc::new(Schema::new(vec![
            Field::new("id", DataType::Utf8, false),
            Field::new(
                "when",
                DataType::Timestamp(TimeUnit::Microsecond, None),
                false,
            ),
            Field::new("entity_id", DataType::Utf8, false),
            Field::new("user_id", DataType::Utf8, false),
            Field::new("active", DataType::Boolean, false),
            Field::new(
                "consented",
                DataType::Timestamp(TimeUnit::Microsecond, None),
                true,
            ),
            Field::new(
                "contacted",
                DataType::Timestamp(TimeUnit::Microsecond, None),
                true,
            ),
            Field::new("explicit", DataType::Boolean, false),
            Field::new("notified", DataType::Boolean, false),
            Field::new(
                "activity",
                DataType::Timestamp(TimeUnit::Microsecond, None),
                false,
            ),
            Field::new("reason", DataType::Utf8, false),
            Field::new("current", DataType::Boolean, false),
        ]));

        self.create_parquet_table(
            "join_table",
            schema,
            &self.path.join("parquet").join("join_table"),
        )
        .await
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

        // Register all summary tables
        self.create_parquet_table(
            "clickthrough_daily_summary",
            daily_schema.clone(),
            &self.path.join("clickthrough_daily_summary"),
        )
        .await?;
        self.create_parquet_table(
            "clickthrough_weekly_summary",
            weekly_schema.clone(),
            &self.path.join("clickthrough_weekly_summary"),
        )
        .await?;
        self.create_parquet_table(
            "join_daily_summary",
            daily_schema,
            &self.path.join("join_daily_summary"),
        )
        .await?;
        self.create_parquet_table(
            "join_weekly_summary",
            weekly_schema,
            &self.path.join("join_weekly_summary"),
        )
        .await?;

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
