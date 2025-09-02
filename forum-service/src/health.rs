use std::time::Instant;
use common::shared::db;
use serde::{Serialize, Deserialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct HealthResponse {
    pub status: String,
    pub database: String,
    pub content_filter: String,
    pub response_time_ms: u64,
}

/// Health check endpoint that checks database connectivity and content filter status
pub async fn health_check(_req: &async_nats::service::Request) -> Result<HealthResponse, anyhow::Error> {
    let start_time = Instant::now();
    
    // Check database connectivity with SQLite-compatible query
    let database_status = match sqlx::query!("SELECT sqlite_version() as version")
        .fetch_one(db())
        .await
    {
        Ok(_) => "healthy",
        Err(e) => {
            eprintln!("Forum database health check failed: {}", e);
            "unhealthy"
        }
    };
    
    // Check content filter status (basic check that it's initialized)
    let content_filter_status = match crate::content_filter::is_initialized().await {
        true => "healthy",
        false => "degraded",
    };
    
    let response_time_ms = start_time.elapsed().as_millis() as u64;
    
    let overall_status = match (database_status, content_filter_status) {
        ("healthy", "healthy") => "healthy",
        ("healthy", "degraded") => "degraded", 
        _ => "unhealthy",
    };
    
    Ok(HealthResponse {
        status: overall_status.to_string(),
        database: database_status.to_string(),
        content_filter: content_filter_status.to_string(),
        response_time_ms,
    })
}