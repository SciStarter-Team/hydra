use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserServiceHealthResponse {
    pub status: String,
    pub database: String,
    pub response_time_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocationServiceHealthResponse {
    pub status: String,
    pub response_time_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogServiceHealthResponse {
    pub status: String,
    pub database: String,
    pub wal_status: String,
    pub response_time_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrontendServiceHealthResponse {
    pub status: String,
    pub database: String,
    pub response_time_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForumServiceHealthResponse {
    pub status: String,
    pub database: String,
    pub content_filter: String,
    pub response_time_ms: u64,
}

impl Default for UserServiceHealthResponse {
    fn default() -> Self {
        Self {
            status: "unknown".to_string(),
            database: "unknown".to_string(),
            response_time_ms: 0,
        }
    }
}

impl Default for LocationServiceHealthResponse {
    fn default() -> Self {
        Self {
            status: "unknown".to_string(),
            response_time_ms: 0,
        }
    }
}

impl Default for LogServiceHealthResponse {
    fn default() -> Self {
        Self {
            status: "unknown".to_string(),
            database: "unknown".to_string(),
            wal_status: "unknown".to_string(),
            response_time_ms: 0,
        }
    }
}

impl Default for FrontendServiceHealthResponse {
    fn default() -> Self {
        Self {
            status: "unknown".to_string(),
            database: "unknown".to_string(),
            response_time_ms: 0,
        }
    }
}

impl Default for ForumServiceHealthResponse {
    fn default() -> Self {
        Self {
            status: "unknown".to_string(),
            database: "unknown".to_string(),
            content_filter: "unknown".to_string(),
            response_time_ms: 0,
        }
    }
}
