use crate::error::{LogServiceError, Result};
// Arrow array types imported only when needed for building
use datafusion::arrow::array::builder::{
    BooleanBuilder, Float64Builder, Int16Builder, Int32Builder, ListBuilder, StringBuilder,
};
use datafusion::arrow::datatypes::{DataType, Schema};
use datafusion::arrow::record_batch::RecordBatch;
use serde_json::Value;
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::sync::Arc;
use tokio::time::{Duration, interval};
use tokio_util::sync::CancellationToken;

use crate::database::retraction;
use crate::database::rewrite::{self, ParquetReplacement};
use crate::database::storage::Database;

/// Background service that compacts WAL files to Parquet
pub struct Compactor {
    base_path: std::path::PathBuf,
    cycle_interval: Duration,
    retention_days: u64,
    batch_size: usize,
    database: Arc<Database>,
    /// The log cache, where summaries keep track of how far they have got
    /// and participation retractions are queued (see [`retraction`]).
    /// Without it the compactor compacts, and maintains neither.
    log_cache: Option<SqlitePool>,
}

impl Compactor {
    pub fn new(database: Arc<Database>) -> Self {
        // Load configuration from environment variables with defaults
        let cycle_seconds = std::env::var("COMPACTOR_CYCLE_SECONDS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(300); // 5 minutes default

        let retention_days = std::env::var("COMPACTOR_RETENTION_DAYS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(30); // 30 days default

        let batch_size = std::env::var("COMPACTOR_BATCH_SIZE")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(rewrite::DEFAULT_ROW_GROUP_SIZE);

        Self::with_config(
            database.clone(),
            Duration::from_secs(cycle_seconds),
            retention_days,
            batch_size,
        )
    }

    pub fn with_config(
        database: Arc<Database>,
        cycle_interval: Duration,
        retention_days: u64,
        batch_size: usize,
    ) -> Self {
        Self {
            base_path: database.path().clone(),
            cycle_interval,
            retention_days,
            batch_size,
            database,
            log_cache: None,
        }
    }

    /// Also maintain the summaries and carry out the participation
    /// retractions kept in `log_cache`.
    pub fn with_log_cache(mut self, log_cache: SqlitePool) -> Self {
        self.log_cache = Some(log_cache);
        self
    }

    /// Main compaction loop
    pub async fn run(&self, cancellation_token: CancellationToken) -> Result<()> {
        let mut interval = interval(self.cycle_interval);

        // WAL compactor started successfully

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    // Check for cancellation before starting work
                    if cancellation_token.is_cancelled() {
                        // WAL compactor cancelled gracefully
                        return Ok(());
                    }

                    // A replacement recorded for a rename that an interrupted
                    // cycle did not make would otherwise be discarded as
                    // abandoned when its table is compacted.
                    if let Some(log_cache) = &self.log_cache
                        && let Err(e) = retraction::finish_moves(log_cache).await
                    {
                        eprintln!("Unable to finish an interrupted file replacement: {e}");
                        continue;
                    }

                    // Only run summary generation if compaction processed any files
                    match self.compact_all_tables().await {
                        Ok(files_processed) => {
                            if files_processed {
                                // Update summary tables after successful compaction of new data
                                if let Err(e) = self.update_summary_tables().await {
                                    eprintln!("Summary generation error: {}", e);
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("Compaction error: {}", e);
                        }
                    }

                    if let Some(log_cache) = &self.log_cache
                        && let Err(e) = retraction::apply_pending(log_cache, &self.base_path).await
                    {
                        eprintln!("Participation retraction error: {e:#}");
                    }

                    if let Err(e) = self.cleanup_old_processed_files().await {
                        eprintln!("Cleanup error: {}", e);
                    }
                }
                _ = cancellation_token.cancelled() => {
                    // WAL compactor cancelled during interval
                    return Ok(());
                }
            }
        }
    }

    /// Update summary tables after compaction
    async fn update_summary_tables(&self) -> Result<()> {
        use crate::database::summaries::update_summary_tables;

        match &self.log_cache {
            Some(log_cache) => {
                update_summary_tables(self.database.clone(), log_cache.clone()).await
            }
            None => Ok(()),
        }
    }

    /// Compact WAL files for all table types
    /// Returns true if any table processed files, false otherwise
    async fn compact_all_tables(&self) -> Result<bool> {
        use crate::database::table::get_registered_tables;

        let mut any_files_processed = false;

        // Use inventory system to get all registered tables
        for table_registration in get_registered_tables() {
            let schema = (table_registration.schema)();
            match self.compact_table(table_registration.name, &schema).await {
                Ok(files_processed) => {
                    if files_processed {
                        any_files_processed = true;
                    }
                }
                Err(e) => {
                    eprintln!("Failed to compact table {}: {}", table_registration.name, e);
                }
            }
        }

        Ok(any_files_processed)
    }

    /// Compact WAL files for a specific table
    /// Returns true if any files were processed, false otherwise
    async fn compact_table(&self, table_name: &str, schema: &Schema) -> Result<bool> {
        let parquet_dir = self.base_path.join("parquet").join(table_name);

        // Create parquet directory if it doesn't exist
        tokio::fs::create_dir_all(&parquet_dir).await?;

        // A conversion that was interrupted left its Parquet unfinished, and
        // its WAL file where it was; converting that again below replaces it.
        rewrite::discard_unfinished(&parquet_dir)?;

        // Every finished day's WAL file is compacted, whether or not a Parquet
        // of that day exists: one that does was converted by a pass that
        // stopped before it could move the WAL file out of the way, and the
        // WAL file is the whole of that day, so it is converted again.
        let today = jiff::Timestamp::now().strftime("%Y-%m-%d").to_string();
        let wal_files: Vec<_> = rewrite::wal_files(&self.base_path, table_name)?
            .into_iter()
            .filter(|path| !path.to_string_lossy().ends_with(&format!("_{today}.jsonl")))
            .collect();

        // Process each WAL file
        let files_processed = !wal_files.is_empty();
        for wal_file in wal_files {
            if let Err(e) = self
                .process_wal_file_atomically(&wal_file, &parquet_dir, schema)
                .await
            {
                eprintln!("Failed to process WAL file {}: {}", wal_file.display(), e);
            }
        }

        Ok(files_processed)
    }

    /// Atomically process a WAL file: read, convert to Parquet, and move to processed
    /// Uses streaming to process large files without loading entirely into memory
    async fn process_wal_file_atomically(
        &self,
        wal_file: &std::path::Path,
        parquet_dir: &std::path::Path,
        schema: &Schema,
    ) -> Result<()> {
        // Retry with exponential backoff for lock acquisition (infinite retries for data integrity)
        let mut retry_delay = tokio::time::Duration::from_millis(1);
        let mut attempt = 0;

        loop {
            // Open file fresh each time to ensure we see latest data
            let file = fs::File::open(wal_file)?;

            match file.try_lock() {
                Ok(_) => {
                    // Successfully acquired exclusive lock - now perform entire operation atomically

                    // Step 1: Stream WAL file and convert to Parquet in batches
                    let wal_file_path = wal_file.to_path_buf();
                    let parquet_dir = parquet_dir.to_path_buf();
                    let schema = schema.clone();
                    let batch_size = self.batch_size;

                    tokio::task::spawn_blocking(move || {
                        Self::stream_wal_to_parquet(
                            &wal_file_path,
                            &parquet_dir,
                            &schema,
                            batch_size,
                        )
                    })
                    .await??;

                    // Step 2: Move to processed directory while still holding lock
                    let processed_dir = self.base_path.join("processed");
                    tokio::fs::create_dir_all(&processed_dir).await?;

                    let file_name = wal_file
                        .file_name()
                        .ok_or_else(|| LogServiceError::InvalidFilePath(wal_file.to_path_buf()))?;
                    let dest_path = processed_dir.join(file_name);

                    rewrite::move_into_place(wal_file, &dest_path)?;

                    // Lock automatically released when file handle drops
                    return Ok(());
                }
                Err(e) => {
                    attempt += 1;
                    eprintln!(
                        "Failed to acquire exclusive lock on WAL file {} (attempt {}): {}, retrying in {:?}",
                        wal_file.display(),
                        attempt,
                        e,
                        retry_delay
                    );
                    tokio::time::sleep(retry_delay).await;
                    retry_delay =
                        std::cmp::min(retry_delay * 2, tokio::time::Duration::from_millis(500));
                }
            }
        }
    }

    /// Stream WAL file line by line and write to Parquet in batches
    /// This is the memory-efficient version that processes large files without loading them entirely
    ///
    /// The Parquet appears only once it is complete, so a conversion that
    /// stops part-way leaves no file a query could count from.
    fn stream_wal_to_parquet(
        wal_file: &std::path::Path,
        parquet_dir: &std::path::Path,
        schema: &Schema,
        batch_size: usize,
    ) -> Result<()> {
        let file_name = wal_file
            .file_name()
            .ok_or_else(|| LogServiceError::InvalidFilePath(wal_file.to_path_buf()))?
            .to_string_lossy();

        let parquet_name = file_name.replace(".jsonl", ".parquet");
        let parquet_path = parquet_dir.join(&parquet_name);

        // Open WAL file for streaming read
        let file = File::open(wal_file)?;
        let reader = BufReader::with_capacity(64 * 1024, file); // 64KB buffer

        let mut writer = ParquetReplacement::create(&parquet_path, schema, batch_size)?;

        // Process file in batches
        let mut records: Vec<Value> = Vec::with_capacity(batch_size);
        let mut total_records = 0;

        for line_result in reader.lines() {
            let line = match line_result {
                Ok(l) => l,
                Err(e) => {
                    eprintln!("Error reading line from {}: {}", wal_file.display(), e);
                    continue;
                }
            };

            if line.trim().is_empty() {
                continue;
            }

            match serde_json::from_str::<Value>(&line) {
                Ok(value) => {
                    records.push(value);
                    total_records += 1;

                    // When batch is full, write it to Parquet
                    if records.len() >= batch_size {
                        let batch = json_records_to_batch(&records, schema)?;
                        writer.write(&batch)?;
                        records.clear();
                    }
                }
                Err(e) => {
                    eprintln!("Failed to parse JSON line in {}: {}", wal_file.display(), e);
                    continue;
                }
            }
        }

        // Write remaining records
        if !records.is_empty() {
            let batch = json_records_to_batch(&records, schema)?;
            writer.write(&batch)?;
        }

        let replacement = writer.finish()?;

        // A day with no records has no Parquet: an empty one would only slow
        // later reads.
        if total_records == 0 {
            replacement.abandon()?;

            if parquet_path.exists() {
                fs::remove_file(&parquet_path)?;
            }
        } else {
            replacement.commit()?;
        }

        Ok(())
    }

    /// Clean up old processed WAL files
    async fn cleanup_old_processed_files(&self) -> Result<()> {
        let processed_dir = self.base_path.join("processed");

        if !processed_dir.exists() {
            return Ok(());
        }

        let cutoff =
            std::time::SystemTime::now() - Duration::from_secs(self.retention_days * 24 * 3600);

        let mut read_dir = tokio::fs::read_dir(&processed_dir).await?;
        while let Some(entry) = read_dir.next_entry().await? {
            let file_path = entry.path();

            if !file_path.extension().map_or(false, |ext| ext == "jsonl") {
                continue;
            }

            // Check if processed file is old enough to delete
            if let Ok(metadata) = fs::metadata(&file_path) {
                if let Ok(modified) = metadata.modified() {
                    if modified < cutoff {
                        // Acquire exclusive lock before deletion to ensure no active readers
                        match self.safely_delete_processed_file(&file_path).await {
                            Ok(true) => {} // File removed successfully
                            Ok(false) => {
                                // File still in use - skip deletion
                            }
                            Err(e) => eprintln!(
                                "Failed to remove old processed file {}: {}",
                                file_path.display(),
                                e
                            ),
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Safely delete a processed WAL file by acquiring exclusive lock first
    /// Returns Ok(true) if deleted, Ok(false) if still in use, Err if error
    async fn safely_delete_processed_file(
        &self,
        processed_file_path: &std::path::Path,
    ) -> Result<bool> {
        // Try to acquire exclusive lock with more aggressive retry for cleanup
        let mut retry_delay = tokio::time::Duration::from_millis(1);
        const MAX_RETRIES: u32 = 10; // More aggressive retry for cleanup operations

        for attempt in 0..MAX_RETRIES {
            match fs::File::open(processed_file_path) {
                Ok(file) => {
                    match file.try_lock() {
                        Ok(_) => {
                            // Successfully acquired exclusive lock - safe to delete
                            // On Unix, we can delete while holding the lock for maximum safety
                            fs::remove_file(processed_file_path)?;
                            return Ok(true); // Successfully deleted
                            // Lock automatically released when file handle drops
                        }
                        Err(_) if attempt < MAX_RETRIES - 1 => {
                            // Lock contention - file might still be accessed
                            eprintln!(
                                "Processed file {} is in use (attempt {}), retrying in {:?}",
                                processed_file_path.display(),
                                attempt + 1,
                                retry_delay
                            );
                            tokio::time::sleep(retry_delay).await;
                            retry_delay = std::cmp::min(
                                retry_delay * 2,
                                tokio::time::Duration::from_millis(50),
                            );
                        }
                        Err(_) => {
                            // Still locked after retries - file might be accessed by external process
                            return Ok(false); // Don't delete, try again later
                        }
                    }
                }
                Err(_) => {
                    // File doesn't exist anymore (race condition) - that's fine
                    return Ok(true);
                }
            }
        }

        // Should not reach here, but handle gracefully
        Ok(false)
    }
}

/// Convert JSON records to an Arrow RecordBatch of `schema`.
///
/// The one way a row becomes Arrow, for the WAL and for every rewrite.
pub(crate) fn json_records_to_batch(records: &[Value], schema: &Schema) -> Result<RecordBatch> {
    let mut builders: HashMap<String, Box<dyn ArrayBuilder>> = HashMap::new();

    // Initialize builders for each field
    for field in schema.fields() {
        let builder: Box<dyn ArrayBuilder> = match field.data_type() {
            DataType::Utf8 => Box::new(StringBuilder::new()),
            DataType::Boolean => Box::new(BooleanBuilder::new()),
            DataType::Int16 => Box::new(Int16Builder::new()),
            DataType::Int32 => Box::new(Int32Builder::new()),
            DataType::Float64 => Box::new(Float64Builder::new()),
            DataType::List(inner_field) if matches!(inner_field.data_type(), DataType::Utf8) => {
                // Create ListBuilder with the exact same field schema as expected
                let list_builder =
                    ListBuilder::new(StringBuilder::new()).with_field(inner_field.clone());
                Box::new(list_builder)
            }
            _ => {
                return Err(LogServiceError::UnsupportedDataType(format!(
                    "{:?}",
                    field.data_type()
                )));
            }
        };
        builders.insert(field.name().clone(), builder);
    }

    // Process each record
    for record in records {
        for field in schema.fields() {
            let field_name = field.name();
            let builder = builders
                .get_mut(field_name)
                .ok_or_else(|| LogServiceError::MissingField(field_name.clone()))?;

            let value = record.get(field_name);

            match field.data_type() {
                DataType::Utf8 => {
                    let string_builder = builder
                        .as_any_mut()
                        .downcast_mut::<StringBuilder>()
                        .unwrap();

                    // A JSON null is an absent value, not the word "null":
                    // spelled out, it reads as a user, a place or a task
                    // named "null" to everything that tests for NULL.
                    match value {
                        Some(Value::String(s)) => string_builder.append_value(s),
                        Some(Value::Null) | None => string_builder.append_null(),
                        Some(v) => string_builder.append_value(&v.to_string()),
                    }
                }
                DataType::Boolean => {
                    let bool_builder = builder
                        .as_any_mut()
                        .downcast_mut::<BooleanBuilder>()
                        .unwrap();

                    match value {
                        Some(Value::Bool(b)) => bool_builder.append_value(*b),
                        None => bool_builder.append_null(),
                        _ => bool_builder.append_null(),
                    }
                }
                DataType::Int16 => {
                    let int_builder = builder.as_any_mut().downcast_mut::<Int16Builder>().unwrap();

                    match value {
                        Some(Value::Number(n)) => {
                            if let Some(i) = n.as_i64() {
                                int_builder.append_value(i as i16);
                            } else {
                                int_builder.append_null();
                            }
                        }
                        None => int_builder.append_null(),
                        _ => int_builder.append_null(),
                    }
                }
                DataType::Int32 => {
                    let int_builder = builder.as_any_mut().downcast_mut::<Int32Builder>().unwrap();

                    match value {
                        Some(Value::Number(n)) => {
                            if let Some(i) = n.as_i64() {
                                int_builder.append_value(i as i32);
                            } else {
                                int_builder.append_null();
                            }
                        }
                        None => int_builder.append_null(),
                        _ => int_builder.append_null(),
                    }
                }
                DataType::Float64 => {
                    let float_builder = builder
                        .as_any_mut()
                        .downcast_mut::<Float64Builder>()
                        .unwrap();

                    match value {
                        Some(Value::Number(n)) => {
                            if let Some(f) = n.as_f64() {
                                float_builder.append_value(f);
                            } else {
                                float_builder.append_null();
                            }
                        }
                        None => float_builder.append_null(),
                        _ => float_builder.append_null(),
                    }
                }
                DataType::List(inner_field)
                    if matches!(inner_field.data_type(), DataType::Utf8) =>
                {
                    let list_builder = builder
                        .as_any_mut()
                        .downcast_mut::<ListBuilder<StringBuilder>>()
                        .unwrap();

                    match value {
                        Some(Value::Array(arr)) => {
                            for item in arr {
                                if let Value::String(s) = item {
                                    list_builder.values().append_value(s);
                                } else {
                                    list_builder.values().append_value(&item.to_string());
                                }
                            }
                            list_builder.append(true);
                        }
                        None => list_builder.append_null(),
                        _ => list_builder.append_null(),
                    }
                }
                _ => {
                    return Err(LogServiceError::UnsupportedDataType(format!(
                        "{:?}",
                        field.data_type()
                    )));
                }
            }
        }
    }

    // Build arrays
    let mut arrays = Vec::new();
    for field in schema.fields() {
        let builder = builders
            .remove(field.name())
            .ok_or_else(|| LogServiceError::MissingField(field.name().clone()))?;
        arrays.push(builder.finish());
    }

    Ok(RecordBatch::try_new(Arc::new(schema.clone()), arrays)?)
}

/// Trait for dynamic array building
trait ArrayBuilder: Send {
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;
    fn finish(self: Box<Self>) -> Arc<dyn datafusion::arrow::array::Array>;
}

impl ArrayBuilder for StringBuilder {
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
    fn finish(mut self: Box<Self>) -> Arc<dyn datafusion::arrow::array::Array> {
        Arc::new(StringBuilder::finish(&mut *self))
    }
}

impl ArrayBuilder for BooleanBuilder {
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
    fn finish(mut self: Box<Self>) -> Arc<dyn datafusion::arrow::array::Array> {
        Arc::new(BooleanBuilder::finish(&mut *self))
    }
}

impl ArrayBuilder for Int16Builder {
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
    fn finish(mut self: Box<Self>) -> Arc<dyn datafusion::arrow::array::Array> {
        Arc::new(Int16Builder::finish(&mut *self))
    }
}

impl ArrayBuilder for Int32Builder {
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
    fn finish(mut self: Box<Self>) -> Arc<dyn datafusion::arrow::array::Array> {
        Arc::new(Int32Builder::finish(&mut *self))
    }
}

impl ArrayBuilder for Float64Builder {
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
    fn finish(mut self: Box<Self>) -> Arc<dyn datafusion::arrow::array::Array> {
        Arc::new(Float64Builder::finish(&mut *self))
    }
}

impl ArrayBuilder for ListBuilder<StringBuilder> {
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
    fn finish(mut self: Box<Self>) -> Arc<dyn datafusion::arrow::array::Array> {
        Arc::new(ListBuilder::finish(&mut *self))
    }
}

/// Run the compactor service
pub async fn run_compactor_service(
    database: Arc<Database>,
    log_cache: SqlitePool,
    cancellation_token: CancellationToken,
) -> Result<()> {
    let compactor = Compactor::new(database).with_log_cache(log_cache);
    compactor.run(cancellation_token).await
}

#[cfg(test)]
mod tests {
    use assert2::check;
    use datafusion::arrow::array::{Array, StringArray};
    use datafusion::arrow::datatypes::{DataType, Field};

    use super::*;

    #[test]
    fn an_absent_string_compacts_to_null_not_to_the_word() {
        let schema = Schema::new(vec![Field::new("task_name", DataType::Utf8, true)]);
        let records = vec![
            serde_json::json!({"task_name": null}),
            serde_json::json!({}),
            serde_json::json!({"task_name": "Data collection"}),
        ];

        let batch = json_records_to_batch(&records, &schema).unwrap();
        let tasks = batch
            .column(0)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();

        check!(tasks.is_null(0));
        check!(tasks.is_null(1));
        check!(tasks.value(2) == "Data collection");
    }
}
