use crate::Database;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Instant;

#[derive(Debug, Serialize, Deserialize)]
pub struct HealthResponse {
    pub status: String,
    pub database: String,
    pub wal_status: String,
    pub response_time_ms: u64,
}

/// Health check endpoint that checks database connectivity and WAL system
pub async fn health_check(
    database: Arc<Database>,
    _req: &async_nats::service::Request,
) -> Result<HealthResponse, anyhow::Error> {
    let start_time = Instant::now();

    // Check database/WAL system health
    let (database_status, wal_status) = match database.health_check().await {
        Ok((db, wal)) => (db, wal),
        Err(e) => {
            eprintln!("Database health check failed: {}", e);
            ("unhealthy".to_string(), "unhealthy".to_string())
        }
    };

    let response_time_ms = start_time.elapsed().as_millis() as u64;

    let overall_status = if database_status == "healthy" && wal_status == "healthy" {
        "healthy"
    } else {
        "degraded"
    };

    Ok(HealthResponse {
        status: overall_status.to_string(),
        database: database_status,
        wal_status,
        response_time_ms,
    })
}
