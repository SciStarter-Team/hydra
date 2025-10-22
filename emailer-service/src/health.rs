use common::shared::db;
use serde::{Deserialize, Serialize};
use std::time::Instant;

#[derive(Debug, Serialize, Deserialize)]
pub struct HealthResponse {
    pub status: String,
    pub database: String,
    pub queue_status: String,
    pub response_time_ms: u64,
}

/// Health check endpoint that checks database connectivity, queue status, and response time
pub async fn health_check(
    _req: &async_nats::service::Request,
) -> Result<HealthResponse, anyhow::Error> {
    let start_time = Instant::now();

    // Check database connectivity with SQLite-compatible query
    let database_status = match sqlx::query!("SELECT sqlite_version() as version")
        .fetch_one(db())
        .await
    {
        Ok(_) => "healthy",
        Err(e) => {
            eprintln!("Emailer database health check failed: {}", e);
            "unhealthy"
        }
    };

    // Check queue status - see if we can query the send_queue table
    let queue_status = match sqlx::query!("SELECT COUNT(*) as count FROM send_queue")
        .fetch_one(db())
        .await
    {
        Ok(_) => "healthy",
        Err(e) => {
            eprintln!("Queue status check failed: {}", e);
            "unhealthy"
        }
    };

    let response_time_ms = start_time.elapsed().as_millis() as u64;

    let overall_status = match (database_status, queue_status) {
        ("healthy", "healthy") => "healthy",
        _ => "unhealthy",
    };

    Ok(HealthResponse {
        status: overall_status.to_string(),
        database: database_status.to_string(),
        queue_status: queue_status.to_string(),
        response_time_ms,
    })
}
