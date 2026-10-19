use crate::database::storage::Database;
use crate::error::Result;
use datafusion::arrow::record_batch::RecordBatch;
use datafusion::functions::expr_fn::date_trunc;
use datafusion::functions_aggregate::expr_fn::{count, min};
use datafusion::parquet::arrow::ArrowWriter;
use datafusion::parquet::file::properties::WriterProperties;
use datafusion::prelude::*;
use jiff::{Span, Timestamp, Zoned, civil::Date};
use sqlx::SqlitePool;
use std::fs::File;
use std::path::Path;
use std::sync::Arc;

/// Generate and write summary tables incrementally from parquet data
pub struct SummaryGenerator {
    database: Arc<Database>,
    /// The log cache, where how far each summary has got is kept.
    log_cache: SqlitePool,
}

impl SummaryGenerator {
    pub fn new(database: Arc<Database>, log_cache: SqlitePool) -> Self {
        Self {
            database,
            log_cache,
        }
    }

    /// Get the last processed date for a summary table
    async fn get_last_processed_date(&self, table_name: &str) -> Result<Option<Date>> {
        let db = &self.log_cache;

        let row = sqlx::query!(
            "SELECT last_processed_date FROM summary_metadata WHERE table_name = ?",
            table_name
        )
        .fetch_optional(db)
        .await?;

        if let Some(row) = row {
            Ok(row.last_processed_date.parse().ok())
        } else {
            Ok(None)
        }
    }

    /// Update the last processed date for a summary table
    async fn set_last_processed_date(&self, table_name: &str, date: Date) -> Result<()> {
        let db = &self.log_cache;
        let now = Timestamp::now().as_microsecond();
        let date_string = date.to_string();

        sqlx::query!(
            r#"
            INSERT OR REPLACE INTO summary_metadata 
            (table_name, last_processed_date, last_updated_at)
            VALUES (?, ?, ?)
            "#,
            table_name,
            date_string,
            now
        )
        .execute(db)
        .await?;

        Ok(())
    }

    /// Get the earliest date from a source table to determine full range
    async fn get_earliest_date(&self, source_table: &str) -> Result<Option<Date>> {
        let ctx = self.database.context();

        let df = ctx
            .table(source_table)
            .await?
            .aggregate(vec![], vec![min(col("when")).alias("earliest_date")])?;

        let batches = df.collect().await?;

        if let Some(batch) = batches.first() {
            if batch.num_rows() > 0 {
                if let Some(array) = batch.column_by_name("earliest_date") {
                    if let Some(timestamp_array) = array
                        .as_any()
                        .downcast_ref::<datafusion::arrow::array::StringArray>(
                    ) {
                        if let Some(earliest_str) = timestamp_array.value(0).split('T').next() {
                            return Ok(earliest_str.parse().ok());
                        }
                    }
                }
            }
        }

        Ok(None)
    }

    /// Update all summary tables with latest parquet data (incremental)
    pub async fn update_all_summaries(&self) -> Result<()> {
        // Get yesterday's date (since today's data is still in WAL)
        let yesterday = Zoned::now()
            .date()
            .checked_sub(Span::new().days(1))
            .unwrap();

        self.update_table_summaries("clickthroughs", "clickthrough", yesterday)
            .await?;
        self.update_table_summaries("join_table", "join", yesterday)
            .await?;

        Ok(())
    }

    /// Regenerate all summary tables from scratch using ALL available parquet data
    pub async fn regenerate_all_summaries(&self) -> Result<()> {
        println!("Starting full regeneration of all summary tables...");

        // Clear existing summary metadata to force full rebuild
        self.clear_summary_metadata().await?;

        // Use yesterday as the target date (same logic as incremental updates)
        let yesterday = Zoned::now()
            .date()
            .checked_sub(Span::new().days(1))
            .unwrap();

        println!(
            "Regenerating summaries from earliest data up to {}",
            yesterday
        );

        self.update_table_summaries("clickthroughs", "clickthrough", yesterday)
            .await?;
        self.update_table_summaries("join_table", "join", yesterday)
            .await?;

        println!("Summary regeneration completed successfully");

        Ok(())
    }

    /// Update both daily and weekly summaries for a table
    async fn update_table_summaries(
        &self,
        source_table: &str,
        table_prefix: &str,
        up_to_date: Date,
    ) -> Result<()> {
        let daily_table = format!("{}_daily_summary", table_prefix);
        let weekly_table = format!("{}_weekly_summary", table_prefix);

        // Update daily summary
        self.update_daily_summary(source_table, &daily_table, up_to_date)
            .await?;

        // Update weekly summary
        self.update_weekly_summary(source_table, &weekly_table, up_to_date)
            .await?;

        Ok(())
    }

    /// Generate daily summary data, creating one file per day to prevent duplication
    async fn update_daily_summary(
        &self,
        source_table: &str,
        summary_table: &str,
        up_to_date: Date,
    ) -> Result<()> {
        let last_processed = match self.get_last_processed_date(summary_table).await? {
            Some(date) => date,
            None => {
                // No previous processing - start from the beginning of the dataset
                match self.get_earliest_date(source_table).await? {
                    Some(earliest) => earliest.checked_sub(Span::new().days(1)).unwrap(), // Start day before earliest data
                    None => {
                        println!("No data found in {} - skipping summary", source_table);
                        return Ok(());
                    }
                }
            }
        };

        // Only process if there are new dates to process
        if last_processed >= up_to_date {
            println!(
                "Daily summary {} is up to date (last: {}, up_to: {})",
                summary_table, last_processed, up_to_date
            );
            return Ok(());
        }

        // Process each day individually to avoid duplication
        // Start from last_processed_date itself to update it with any new data
        let mut current_date = last_processed;
        let summary_dir = self.database.path().join(summary_table);

        while current_date <= up_to_date {
            let next_date = current_date.checked_add(Span::new().days(1)).unwrap();

            let ctx = self.database.context();

            // Generate complete aggregation for this entire day from ALL available parquet data
            let df = ctx
                .table(source_table)
                .await?
                .filter(
                    col("when")
                        .gt_eq(lit(current_date.to_string()))
                        .and(col("when").lt(lit(next_date.to_string()))),
                )?
                .aggregate(vec![], vec![count(lit(1)).alias("count")])?
                .with_column("date", lit(current_date.to_string()))?
                .with_column("updated_at", lit(Timestamp::now().as_microsecond()))?;

            let batches = df.collect().await?;

            if !batches.is_empty() && batches[0].num_rows() > 0 {
                // Create one file per day to ensure no duplication
                let filename = format!("{}_{}.parquet", summary_table, current_date);
                self.write_summary_parquet(&batches, &summary_dir, &filename)
                    .await?;

                println!(
                    "Updated daily summary: {} for date {}",
                    summary_table, current_date
                );
            }

            current_date = next_date;
        }

        // Update metadata in SQLite
        self.set_last_processed_date(summary_table, up_to_date)
            .await?;

        Ok(())
    }

    /// Generate weekly summary data, creating one file per week to prevent duplication
    async fn update_weekly_summary(
        &self,
        source_table: &str,
        summary_table: &str,
        up_to_date: Date,
    ) -> Result<()> {
        let last_processed = match self.get_last_processed_date(summary_table).await? {
            Some(date) => date,
            None => {
                // No previous processing - start from the beginning of the dataset
                match self.get_earliest_date(source_table).await? {
                    Some(earliest) => earliest.checked_sub(Span::new().days(1)).unwrap(), // Start day before earliest data
                    None => {
                        println!("No data found in {} - skipping summary", source_table);
                        return Ok(());
                    }
                }
            }
        };

        // Only process if there are new dates to process
        if last_processed >= up_to_date {
            println!(
                "Weekly summary {} is up to date (last: {}, up_to: {})",
                summary_table, last_processed, up_to_date
            );
            return Ok(());
        }

        // Find all weeks that need to be updated/created
        // Start from last_processed_date itself to ensure overlapping weeks get updated
        let from_date = last_processed;

        let ctx = self.database.context();

        // Get all unique weeks in the date range to process each week separately
        let weeks_df = ctx
            .table(source_table)
            .await?
            .filter(
                col("when").gt_eq(lit(from_date.to_string())).and(
                    col("when").lt(lit(up_to_date
                        .checked_add(Span::new().days(1))
                        .unwrap()
                        .to_string())),
                ),
            )?
            .aggregate(
                vec![date_trunc(lit("week"), col("when")).alias("week_start")],
                vec![], // No aggregation yet, just get unique weeks
            )?
            .sort(vec![col("week_start").sort(true, true)])?;

        let weeks_batches = weeks_df.collect().await?;
        let summary_dir = self.database.path().join(summary_table);

        // Process each week individually to avoid duplication
        for batch in weeks_batches {
            if batch.num_rows() == 0 {
                continue;
            }

            let week_start_array = batch.column(0);

            for i in 0..batch.num_rows() {
                // Extract week start date
                let week_start_str = if let Some(string_array) =
                    week_start_array
                        .as_any()
                        .downcast_ref::<datafusion::arrow::array::StringArray>()
                {
                    string_array
                        .value(i)
                        .split('T')
                        .next()
                        .unwrap_or("")
                        .to_string()
                } else {
                    continue; // Skip if not string format
                };

                let week_start_date: Date = if let Ok(date) = week_start_str.parse() {
                    date
                } else {
                    continue; // Skip invalid dates
                };

                // Calculate week end (Sunday)
                let week_end = week_start_date.checked_add(Span::new().days(6)).unwrap();

                // Generate complete aggregation for this entire week from ALL available parquet data
                let week_df = ctx
                    .table(source_table)
                    .await?
                    .filter(
                        col("when").gt_eq(lit(week_start_date.to_string())).and(
                            col("when").lt(lit(week_end
                                .checked_add(Span::new().days(1))
                                .unwrap()
                                .to_string())),
                        ),
                    )?
                    .aggregate(vec![], vec![count(lit(1)).alias("count")])?
                    .with_column("week_start", lit(week_start_str))?
                    .with_column("updated_at", lit(Timestamp::now().as_microsecond()))?;

                let week_batches = week_df.collect().await?;

                if !week_batches.is_empty() && week_batches[0].num_rows() > 0 {
                    // Create one file per week to ensure no duplication
                    let filename = format!(
                        "{}_{}_to_{}.parquet",
                        summary_table, week_start_date, week_end
                    );
                    self.write_summary_parquet(&week_batches, &summary_dir, &filename)
                        .await?;

                    println!(
                        "Updated weekly summary: {} for week {} to {}",
                        summary_table, week_start_date, week_end
                    );
                }
            }
        }

        // Update metadata in SQLite
        self.set_last_processed_date(summary_table, up_to_date)
            .await?;

        Ok(())
    }

    /// Write summary data to parquet file
    async fn write_summary_parquet(
        &self,
        batches: &[RecordBatch],
        summary_dir: &Path,
        filename: &str,
    ) -> Result<()> {
        if batches.is_empty() {
            return Ok(());
        }

        // Ensure directory exists
        tokio::fs::create_dir_all(summary_dir).await?;

        let file_path = summary_dir.join(filename);

        // Write using tokio spawn_blocking to avoid blocking async executor
        let file_path_clone = file_path.clone();
        let batches_clone = batches.to_vec();
        let schema = batches[0].schema();

        tokio::task::spawn_blocking(move || -> Result<()> {
            let file = File::create(&file_path_clone)?;
            let props = WriterProperties::builder()
                .set_compression(datafusion::parquet::basic::Compression::ZSTD(
                    datafusion::parquet::basic::ZstdLevel::default(),
                ))
                .build();

            let mut writer = ArrowWriter::try_new(file, schema, Some(props))?;

            for batch in batches_clone {
                writer.write(&batch)?;
            }

            writer.close()?;
            Ok(())
        })
        .await??;

        println!("Wrote summary file: {}", file_path.display());
        Ok(())
    }

    /// Clear all summary metadata to force full regeneration
    async fn clear_summary_metadata(&self) -> Result<()> {
        let db = &self.log_cache;

        sqlx::query!("DELETE FROM summary_metadata")
            .execute(db)
            .await?;

        println!("Cleared all summary metadata");
        Ok(())
    }
}

/// Update summary tables (typically called after compaction runs)
pub async fn update_summary_tables(database: Arc<Database>, log_cache: SqlitePool) -> Result<()> {
    let generator = SummaryGenerator::new(database, log_cache);
    generator.update_all_summaries().await?;
    Ok(())
}

/// Regenerate all summary tables from scratch using all available parquet data
pub async fn regenerate_all_summaries(
    database: Arc<Database>,
    log_cache: SqlitePool,
) -> Result<()> {
    let generator = SummaryGenerator::new(database, log_cache);
    generator.regenerate_all_summaries().await?;
    Ok(())
}
