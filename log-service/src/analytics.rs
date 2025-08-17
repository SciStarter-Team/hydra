use anyhow::Result;
use common::message::log::{AnalyticsQuery, AnalyticsResponse, WeeklyCount};
use datafusion::arrow::record_batch::RecordBatch;
use datafusion::functions_aggregate::expr_fn::count;
use datafusion::functions::expr_fn::date_trunc;
use datafusion::prelude::*;

use crate::database::storage::Database;
use std::sync::Arc;

/// Get clickthrough analytics using DataFusion with safe parameter binding on pre-computed summary tables
pub async fn get_clickthrough_analytics(database: Arc<Database>, query: AnalyticsQuery) -> Result<AnalyticsResponse> {
    let ctx = database.context();
    
    // Use safe parameter binding with DataFusion expressions
    let df = ctx.table("clickthrough_weekly_summary").await?
        .filter(col("week_start").gt_eq(lit(&query.start_date)))?
        .filter(col("week_start").lt_eq(lit(&query.end_date)))?
        .select(vec![col("week_start"), col("count")])?
        .sort(vec![col("week_start").sort(true, true)])?;
    
    let batches = df.collect().await?;
    let weekly_counts = extract_weekly_counts_from_batches(batches)?;
    
    Ok(AnalyticsResponse::WeeklyCounts(weekly_counts))
}

/// Get join analytics using DataFusion with safe parameter binding on pre-computed summary tables
pub async fn get_join_analytics(database: Arc<Database>, query: AnalyticsQuery) -> Result<AnalyticsResponse> {
    let ctx = database.context();
    
    // Use safe parameter binding with DataFusion expressions
    let df = ctx.table("join_weekly_summary").await?
        .filter(col("week_start").gt_eq(lit(&query.start_date)))?
        .filter(col("week_start").lt_eq(lit(&query.end_date)))?
        .select(vec![col("week_start"), col("count")])?
        .sort(vec![col("week_start").sort(true, true)])?;
    
    let batches = df.collect().await?;
    let weekly_counts = extract_weekly_counts_from_batches(batches)?;
    
    Ok(AnalyticsResponse::WeeklyCounts(weekly_counts))
}

/// Helper function to extract WeeklyCount data from Arrow RecordBatches
fn extract_weekly_counts_from_batches(batches: Vec<RecordBatch>) -> Result<Vec<WeeklyCount>> {
    use datafusion::arrow::array::{Array, Date32Array, Int64Array, StringArray};
    let mut weekly_counts = Vec::new();
    
    for batch in batches {
        if batch.num_rows() == 0 {
            continue;
        }
        
        let week_start_array = batch.column(0);
        let count_array = batch.column(1);
        
        // Convert Arrow arrays to WeeklyCount structs
        for i in 0..batch.num_rows() {
            let week_start = if let Some(string_array) = week_start_array.as_any().downcast_ref::<StringArray>() {
                string_array.value(i).to_string()
            } else if let Some(date_array) = week_start_array.as_any().downcast_ref::<Date32Array>() {
                // Convert Date32 to string format (YYYY-MM-DD) using jiff
                let days_since_epoch = date_array.value(i);
                // Date32 is days since Unix epoch (1970-01-01)
                let unix_epoch = jiff::civil::Date::constant(1970, 1, 1);
                let date = unix_epoch.checked_add(jiff::Span::new().days(days_since_epoch as i64))
                    .map_err(|_| anyhow::anyhow!("Invalid date value: {}", days_since_epoch))?;
                date.to_string()
            } else {
                return Err(anyhow::anyhow!("Unsupported week_start column type: {:?}", week_start_array.data_type()));
            };
                
            let count = count_array
                .as_any()
                .downcast_ref::<Int64Array>()
                .ok_or_else(|| anyhow::anyhow!("Failed to cast count column to Int64Array"))?
                .value(i);
                
            weekly_counts.push(WeeklyCount {
                week_start,
                count,
            });
        }
    }
    
    Ok(weekly_counts)
}

/// Generate clickthrough analytics for real-time data directly from main tables
/// This can be used for more detailed queries when summary tables are not sufficient
pub async fn get_clickthrough_analytics_realtime(database: Arc<Database>, query: AnalyticsQuery) -> Result<AnalyticsResponse> {
    let ctx = database.context();
    
    // Use DataFrame API for complex aggregations with safe parameter binding
    let df = ctx.table("clickthroughs").await?
        .filter(col("when").gt_eq(lit(&query.start_date)))?
        .filter(col("when").lt_eq(lit(&query.end_date)))?
        .aggregate(
            vec![date_trunc(lit("week"), col("when")).alias("week_start")],
            vec![count(lit(1)).alias("count")]
        )?
        .sort(vec![col("week_start").sort(true, true)])?;
    
    let batches = df.collect().await?;
    let weekly_counts = extract_weekly_counts_from_batches(batches)?;
    
    Ok(AnalyticsResponse::WeeklyCounts(weekly_counts))
}

/// Generate join analytics for real-time data directly from main tables
pub async fn get_join_analytics_realtime(database: Arc<Database>, query: AnalyticsQuery) -> Result<AnalyticsResponse> {
    let ctx = database.context();
    
    // Use DataFrame API for complex aggregations with safe parameter binding
    let df = ctx.table("join_table").await?
        .filter(col("when").gt_eq(lit(&query.start_date)))?
        .filter(col("when").lt_eq(lit(&query.end_date)))?
        .aggregate(
            vec![date_trunc(lit("week"), col("when")).alias("week_start")],
            vec![count(lit(1)).alias("count")]
        )?
        .sort(vec![col("week_start").sort(true, true)])?;
    
    let batches = df.collect().await?;
    let weekly_counts = extract_weekly_counts_from_batches(batches)?;
    
    Ok(AnalyticsResponse::WeeklyCounts(weekly_counts))
}