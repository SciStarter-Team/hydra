use std::time::Instant;
use serde::{Serialize, Deserialize};
use common::shared::db;

#[derive(Debug, Serialize, Deserialize)]
pub struct HealthResponse {
    pub status: String,
    pub database: String,
    pub response_time_ms: u64,
}

/// NATS health check endpoint that checks database connectivity and response time
pub async fn health_check(_req: &async_nats::service::Request) -> Result<HealthResponse, anyhow::Error> {
    let start_time = Instant::now();
    
    // Check database connectivity with SQLite-compatible query
    let database_status = match sqlx::query!("SELECT sqlite_version() as version")
        .fetch_one(db())
        .await
    {
        Ok(_) => "healthy",
        Err(e) => {
            eprintln!("Frontend database health check failed: {}", e);
            "unhealthy"
        }
    };
    
    let response_time_ms = start_time.elapsed().as_millis() as u64;
    
    let overall_status = match database_status {
        "healthy" => "healthy",
        _ => "unhealthy",
    };
    
    Ok(HealthResponse {
        status: overall_status.to_string(),
        database: database_status.to_string(),
        response_time_ms,
    })
}