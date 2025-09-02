use std::time::Instant;
use serde::{Serialize, Deserialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct HealthResponse {
    pub status: String,
    pub response_time_ms: u64,
}

/// Health check endpoint for location service
pub async fn health_check(_req: &async_nats::service::Request) -> Result<HealthResponse, anyhow::Error> {
    let start_time = Instant::now();
    
    // Location service is stateless and doesn't have a database,
    // so we just return healthy if we can respond
    let response_time_ms = start_time.elapsed().as_millis() as u64;
    
    Ok(HealthResponse {
        status: "healthy".to_string(),
        response_time_ms,
    })
}