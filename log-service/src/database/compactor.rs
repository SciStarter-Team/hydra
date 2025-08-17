use anyhow::Result;
// Arrow array types imported only when needed for building
use arrow::array::builder::{BooleanBuilder, Float64Builder, Int16Builder, Int32Builder, StringBuilder, ListBuilder};
use arrow::datatypes::{DataType, Schema};
use arrow::record_batch::RecordBatch;
use parquet::arrow::ArrowWriter;
use parquet::basic::{Compression, ZstdLevel};
use parquet::file::properties::WriterProperties;
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use tokio::time::{interval, Duration};
use tokio_util::sync::CancellationToken;

use crate::database::storage::Database;
use crate::database::tables::{
    ActivityLogRow, PageViewRow, ClickthroughRow, UserTagsRow, 
    JoinRow, ParticipationRow, BookmarkRow
};
use crate::database::table::TableRow;

/// Background service that compacts WAL files to Parquet
pub struct Compactor {
    base_path: std::path::PathBuf,
    cycle_interval: Duration,
    retention_days: u64,
}

impl Compactor {
    pub fn new(database: &Database) -> Self {
        // Load configuration from environment variables with defaults
        let cycle_seconds = std::env::var("COMPACTOR_CYCLE_SECONDS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(300); // 5 minutes default
            
        let retention_days = std::env::var("COMPACTOR_RETENTION_DAYS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(30); // 30 days default
            
        Self::with_config(database.path().clone(), Duration::from_secs(cycle_seconds), retention_days)
    }
    
    pub fn with_config(base_path: impl AsRef<Path>, cycle_interval: Duration, retention_days: u64) -> Self {
        Self {
            base_path: base_path.as_ref().to_path_buf(),
            cycle_interval,
            retention_days,
        }
    }
    
    /// Main compaction loop
    pub async fn run(&self, cancellation_token: CancellationToken) -> Result<()> {
        let mut interval = interval(self.cycle_interval);
        
        println!("WAL compactor started (cycle: {:?}, retention: {} days)", 
                self.cycle_interval, self.retention_days);
        
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    // Check for cancellation before starting work
                    if cancellation_token.is_cancelled() {
                        println!("WAL compactor cancelled, exiting gracefully");
                        return Ok(());
                    }
                    
                    if let Err(e) = self.compact_all_tables().await {
                        eprintln!("Compaction error: {}", e);
                    }
                    
                    if let Err(e) = self.cleanup_old_processed_files().await {
                        eprintln!("Cleanup error: {}", e);
                    }
                }
                _ = cancellation_token.cancelled() => {
                    println!("WAL compactor cancelled during interval, exiting gracefully");
                    return Ok(());
                }
            }
        }
    }
    
    /// Compact WAL files for all table types
    async fn compact_all_tables(&self) -> Result<()> {
        // Define table mappings
        let tables = [
            ("activity_log", ActivityLogRow::schema()),
            ("page_views", PageViewRow::schema()),
            ("clickthroughs", ClickthroughRow::schema()),
            ("user_tags", UserTagsRow::schema()),
            ("join_table", JoinRow::schema()),
            ("participation", ParticipationRow::schema()),
            ("bookmark", BookmarkRow::schema()),
        ];
        
        for (table_name, schema) in tables {
            if let Err(e) = self.compact_table(table_name, &schema).await {
                eprintln!("Failed to compact table {}: {}", table_name, e);
            }
        }
        
        Ok(())
    }
    
    /// Compact WAL files for a specific table
    async fn compact_table(&self, table_name: &str, schema: &Schema) -> Result<()> {
        let wal_dir = self.base_path.join("wal");
        let parquet_dir = self.base_path.join("parquet").join(table_name);
        
        // Create parquet directory if it doesn't exist
        fs::create_dir_all(&parquet_dir)?;
        
        // Find WAL files for this table
        let wal_pattern = format!("{}_", table_name);
        let mut wal_files = Vec::new();
        
        if wal_dir.exists() {
            for entry in fs::read_dir(&wal_dir)? {
                let entry = entry?;
                let file_name = entry.file_name().to_string_lossy().to_string();
                
                if file_name.starts_with(&wal_pattern) && file_name.ends_with(".jsonl") {
                    // Check if this file has already been processed
                    let parquet_name = file_name.replace(".jsonl", ".parquet");
                    let parquet_path = parquet_dir.join(&parquet_name);
                    
                    if !parquet_path.exists() {
                        wal_files.push(entry.path());
                    }
                }
            }
        }
        
        // Process each WAL file
        for wal_file in wal_files {
            if let Err(e) = self.process_wal_file_atomically(&wal_file, &parquet_dir, schema).await {
                eprintln!("Failed to process WAL file {}: {}", wal_file.display(), e);
            }
        }
        
        Ok(())
    }
    
    /// Atomically process a WAL file: read, convert to Parquet, and move to processed
    /// Holds exclusive lock throughout the entire operation to prevent race conditions
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
                    
                    // Step 1: Read WAL file content
                    let content = fs::read_to_string(wal_file)?;
                    
                    // Step 2: Convert to Parquet (but don't release lock yet)
                    if let Err(e) = self.convert_content_to_parquet(&content, wal_file, parquet_dir, schema).await {
                        return Err(e);
                    }
                    
                    // Step 3: Move to processed directory while still holding lock
                    let processed_dir = self.base_path.join("processed");
                    fs::create_dir_all(&processed_dir)?;
                    
                    let file_name = wal_file.file_name()
                        .ok_or_else(|| anyhow::anyhow!("Invalid WAL file path"))?;
                    let dest_path = processed_dir.join(file_name);
                    
                    fs::rename(wal_file, &dest_path)?;
                    println!("Atomically processed WAL file: {} -> Parquet + moved to {}", 
                           wal_file.display(), dest_path.display());
                    
                    // Lock automatically released when file handle drops
                    return Ok(());
                }
                Err(e) => {
                    attempt += 1;
                    eprintln!("Failed to acquire exclusive lock on WAL file {} (attempt {}): {}, retrying in {:?}", 
                             wal_file.display(), attempt, e, retry_delay);
                    tokio::time::sleep(retry_delay).await;
                    retry_delay = std::cmp::min(retry_delay * 2, tokio::time::Duration::from_millis(500));
                }
            }
        }
    }
    
    /// Convert WAL content to Parquet (extracted from convert_wal_to_parquet)
    async fn convert_content_to_parquet(
        &self,
        content: &str,
        wal_file: &std::path::Path,
        parquet_dir: &std::path::Path,
        schema: &Schema,
    ) -> Result<()> {
        let file_name = wal_file.file_name()
            .ok_or_else(|| anyhow::anyhow!("Invalid file name"))?
            .to_string_lossy();
        
        let parquet_name = file_name.replace(".jsonl", ".parquet");
        let parquet_path = parquet_dir.join(&parquet_name);
        
        // Parse JSONL content
        let mut records = Vec::new();
        
        for line in content.lines() {
            if line.trim().is_empty() {
                continue;
            }
            
            match serde_json::from_str::<Value>(line) {
                Ok(value) => records.push(value),
                Err(e) => {
                    eprintln!("Failed to parse JSON line in {}: {}", wal_file.display(), e);
                    continue;
                }
            }
        }
        
        if records.is_empty() {
            println!("No records to convert in {}", wal_file.display());
            return Ok(());
        }
        
        // Convert to Arrow RecordBatch
        let batch = self.json_to_record_batch(&records, schema)?;
        
        // Write to Parquet
        let file = fs::File::create(&parquet_path)?;
        let props = WriterProperties::builder()
            .set_compression(Compression::ZSTD(ZstdLevel::try_new(3)?))
            .build();
        
        let mut writer = ArrowWriter::try_new(file, batch.schema(), Some(props))?;
        writer.write(&batch)?;
        writer.close()?;
        
        println!("Converted {} to {}", wal_file.display(), parquet_path.display());
        Ok(())
    }
    
    
    /// Convert JSON records to Arrow RecordBatch
    fn json_to_record_batch(&self, records: &[Value], schema: &Schema) -> Result<RecordBatch> {
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
                    let list_builder = ListBuilder::new(StringBuilder::new()).with_field(inner_field.clone());
                    Box::new(list_builder)
                }
                _ => return Err(anyhow::anyhow!("Unsupported field type: {:?}", field.data_type())),
            };
            builders.insert(field.name().clone(), builder);
        }
        
        // Process each record
        for record in records {
            for field in schema.fields() {
                let field_name = field.name();
                let builder = builders.get_mut(field_name)
                    .ok_or_else(|| anyhow::anyhow!("Missing builder for field: {}", field_name))?;
                
                let value = record.get(field_name);
                
                match field.data_type() {
                    DataType::Utf8 => {
                        let string_builder = builder.as_any_mut()
                            .downcast_mut::<StringBuilder>()
                            .unwrap();
                        
                        match value {
                            Some(Value::String(s)) => string_builder.append_value(s),
                            Some(v) => string_builder.append_value(&v.to_string()),
                            None => string_builder.append_null(),
                        }
                    }
                    DataType::Boolean => {
                        let bool_builder = builder.as_any_mut()
                            .downcast_mut::<BooleanBuilder>()
                            .unwrap();
                        
                        match value {
                            Some(Value::Bool(b)) => bool_builder.append_value(*b),
                            None => bool_builder.append_null(),
                            _ => bool_builder.append_null(),
                        }
                    }
                    DataType::Int16 => {
                        let int_builder = builder.as_any_mut()
                            .downcast_mut::<Int16Builder>()
                            .unwrap();
                        
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
                        let int_builder = builder.as_any_mut()
                            .downcast_mut::<Int32Builder>()
                            .unwrap();
                        
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
                        let float_builder = builder.as_any_mut()
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
                    DataType::List(inner_field) if matches!(inner_field.data_type(), DataType::Utf8) => {
                        let list_builder = builder.as_any_mut()
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
                    _ => return Err(anyhow::anyhow!("Unsupported field type: {:?}", field.data_type())),
                }
            }
        }
        
        // Build arrays
        let mut arrays = Vec::new();
        for field in schema.fields() {
            let builder = builders.remove(field.name())
                .ok_or_else(|| anyhow::anyhow!("Missing builder for field: {}", field.name()))?;
            arrays.push(builder.finish());
        }
        
        RecordBatch::try_new(Arc::new(schema.clone()), arrays)
            .map_err(|e| anyhow::anyhow!("Failed to create RecordBatch: {}", e))
    }
    
    
    /// Clean up old processed WAL files
    async fn cleanup_old_processed_files(&self) -> Result<()> {
        let processed_dir = self.base_path.join("processed");
        
        if !processed_dir.exists() {
            return Ok(());
        }
        
        let cutoff = std::time::SystemTime::now() - Duration::from_secs(self.retention_days * 24 * 3600);
        
        for entry in fs::read_dir(&processed_dir)? {
            let entry = entry?;
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
                            Ok(true) => println!("Removed old processed file: {}", file_path.display()),
                            Ok(false) => {
                                // File is still being read, skip deletion
                                println!("Skipping deletion of processed file {} - still in use", file_path.display());
                            }
                            Err(e) => eprintln!("Failed to remove old processed file {}: {}", file_path.display(), e),
                        }
                    }
                }
            }
        }
        
        Ok(())
    }
    
    /// Safely delete a processed WAL file by acquiring exclusive lock first
    /// Returns Ok(true) if deleted, Ok(false) if still in use, Err if error
    async fn safely_delete_processed_file(&self, processed_file_path: &std::path::Path) -> Result<bool> {
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
                            match fs::remove_file(processed_file_path) {
                                Ok(_) => return Ok(true), // Successfully deleted
                                Err(e) => return Err(anyhow::anyhow!("Failed to delete processed file: {}", e)),
                            }
                            // Lock automatically released when file handle drops
                        }
                        Err(_) if attempt < MAX_RETRIES - 1 => {
                            // Lock contention - file might still be accessed
                            eprintln!("Processed file {} is in use (attempt {}), retrying in {:?}", 
                                     processed_file_path.display(), attempt + 1, retry_delay);
                            tokio::time::sleep(retry_delay).await;
                            retry_delay = std::cmp::min(retry_delay * 2, tokio::time::Duration::from_millis(50));
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

/// Trait for dynamic array building
trait ArrayBuilder: Send {
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;
    fn finish(self: Box<Self>) -> Arc<dyn arrow::array::Array>;
}

impl ArrayBuilder for StringBuilder {
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }
    fn finish(mut self: Box<Self>) -> Arc<dyn arrow::array::Array> {
        Arc::new(StringBuilder::finish(&mut *self))
    }
}

impl ArrayBuilder for BooleanBuilder {
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }
    fn finish(mut self: Box<Self>) -> Arc<dyn arrow::array::Array> {
        Arc::new(BooleanBuilder::finish(&mut *self))
    }
}

impl ArrayBuilder for Int16Builder {
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }
    fn finish(mut self: Box<Self>) -> Arc<dyn arrow::array::Array> {
        Arc::new(Int16Builder::finish(&mut *self))
    }
}

impl ArrayBuilder for Int32Builder {
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }
    fn finish(mut self: Box<Self>) -> Arc<dyn arrow::array::Array> {
        Arc::new(Int32Builder::finish(&mut *self))
    }
}

impl ArrayBuilder for Float64Builder {
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }
    fn finish(mut self: Box<Self>) -> Arc<dyn arrow::array::Array> {
        Arc::new(Float64Builder::finish(&mut *self))
    }
}

impl ArrayBuilder for ListBuilder<StringBuilder> {
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }
    fn finish(mut self: Box<Self>) -> Arc<dyn arrow::array::Array> {
        Arc::new(ListBuilder::finish(&mut *self))
    }
}

/// Run the compactor service
pub async fn run_compactor_service(database: &Database, cancellation_token: CancellationToken) -> Result<()> {
    let compactor = Compactor::new(database);
    compactor.run(cancellation_token).await
}