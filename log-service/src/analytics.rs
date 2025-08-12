use anyhow::Result;
use common::message::log::{AnalyticsQuery, AnalyticsResponse, WeeklyCount};
use duckdb::params;

use crate::database::create_connection;

pub fn get_clickthrough_analytics(query: AnalyticsQuery) -> Result<AnalyticsResponse> {
    let conn = create_connection()?;
    
    // Query pre-computed weekly summary table for much faster results
    let mut stmt = conn.prepare(r#"
        SELECT 
            week_start,
            count
        FROM clickthrough_weekly_summary 
        WHERE week_start >= ? AND week_start <= ?
        ORDER BY week_start
    "#)?;
    
    let rows = stmt.query_map(params![query.start_date, query.end_date], |row| {
        let week_start: String = row.get(0)?;
        let count: i64 = row.get(1)?;
        Ok(WeeklyCount {
            week_start,
            count,
        })
    })?;
    
    let weekly_counts: Result<Vec<WeeklyCount>, _> = rows.collect();
    Ok(AnalyticsResponse::WeeklyCounts(weekly_counts?))
}

pub fn get_join_analytics(query: AnalyticsQuery) -> Result<AnalyticsResponse> {
    let conn = create_connection()?;
    
    // Query pre-computed weekly summary table for much faster results
    let mut stmt = conn.prepare(r#"
        SELECT 
            week_start,
            count
        FROM join_weekly_summary 
        WHERE week_start >= ? AND week_start <= ?
        ORDER BY week_start
    "#)?;
    
    let rows = stmt.query_map(params![query.start_date, query.end_date], |row| {
        let week_start: String = row.get(0)?;
        let count: i64 = row.get(1)?;
        Ok(WeeklyCount {
            week_start,
            count,
        })
    })?;
    
    let weekly_counts: Result<Vec<WeeklyCount>, _> = rows.collect();
    Ok(AnalyticsResponse::WeeklyCounts(weekly_counts?))
}