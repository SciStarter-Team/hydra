use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use poem::{Endpoint, Middleware, error::ResponseError, web::RealIp};

/// Rate limit exceeded error
#[derive(Debug)]
pub struct RateLimitExceeded {
    retry_after: u64,
    message: String,
}

impl RateLimitExceeded {
    pub fn new(retry_after: u64, message: String) -> Self {
        Self {
            retry_after,
            message,
        }
    }
}

impl std::fmt::Display for RateLimitExceeded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for RateLimitExceeded {}

impl ResponseError for RateLimitExceeded {
    fn status(&self) -> poem::http::StatusCode {
        poem::http::StatusCode::TOO_MANY_REQUESTS
    }

    fn as_response(&self) -> poem::Response {
        poem::Response::builder()
            .status(self.status())
            .header("Retry-After", self.retry_after.to_string())
            .body(self.message.clone())
    }
}

/// Rate limiting configuration
#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    /// Maximum number of requests allowed
    pub max_requests: u32,
    /// Time window for the rate limit
    pub window: Duration,
    /// Optional custom error message
    pub error_message: Option<String>,
}

impl RateLimitConfig {
    /// Create a new rate limit config
    pub fn new(max_requests: u32, window: Duration) -> Self {
        Self {
            max_requests,
            window,
            error_message: None,
        }
    }

    /// Set a custom error message
    pub fn with_error_message(mut self, message: impl Into<String>) -> Self {
        self.error_message = Some(message.into());
        self
    }
}

/// Default rate limits for WebAuthn endpoints
impl Default for RateLimitConfig {
    fn default() -> Self {
        // Conservative WebAuthn rate limits: 10 attempts per 5 minutes per IP
        Self::new(10, Duration::from_secs(300))
            .with_error_message("Rate limit exceeded. Please try again later.")
    }
}

/// Rate limit entry tracking requests from an IP
#[derive(Debug)]
struct RateLimitEntry {
    count: u32,
    window_start: Instant,
}

/// Rate limiting middleware that tracks requests by IP address
pub struct RateLimitMiddleware {
    config: RateLimitConfig,
    // In-memory storage of rate limit data by IP address
    // In production, consider Redis or other distributed storage
    storage: Arc<RwLock<HashMap<String, RateLimitEntry>>>,
}

impl RateLimitMiddleware {
    /// Create a new rate limiting middleware
    pub fn new(config: RateLimitConfig) -> Self {
        Self {
            config,
            storage: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Check if the request should be rate limited
    fn check_rate_limit(&self, client_ip: &str) -> Result<(), RateLimitExceeded> {
        let mut storage = self.storage.write().unwrap();
        let now = Instant::now();

        match storage.get_mut(client_ip) {
            Some(entry) => {
                // Check if the window has expired
                if now.duration_since(entry.window_start) >= self.config.window {
                    // Reset the window
                    entry.count = 1;
                    entry.window_start = now;
                    Ok(()) // Not rate limited
                } else if entry.count >= self.config.max_requests {
                    // Rate limit exceeded
                    let error_message = self
                        .config
                        .error_message
                        .as_deref()
                        .unwrap_or("Rate limit exceeded")
                        .to_string();
                    Err(RateLimitExceeded::new(
                        self.config.window.as_secs(),
                        error_message,
                    ))
                } else {
                    // Increment count
                    entry.count += 1;
                    Ok(()) // Not rate limited
                }
            }
            None => {
                // First request from this IP
                storage.insert(
                    client_ip.to_string(),
                    RateLimitEntry {
                        count: 1,
                        window_start: now,
                    },
                );
                Ok(()) // Not rate limited
            }
        }
    }

    /// Clean up expired entries (optional background task)
    pub fn cleanup_expired(&self) {
        let mut storage = self.storage.write().unwrap();
        let now = Instant::now();

        storage.retain(|_, entry| now.duration_since(entry.window_start) < self.config.window);
    }
}

/// Endpoint wrapper that applies rate limiting
pub struct RateLimitEndpoint<E> {
    middleware: RateLimitMiddleware,
    wrapped: E,
}

impl<E: Endpoint> Middleware<E> for RateLimitMiddleware {
    type Output = RateLimitEndpoint<E>;

    fn transform(&self, ep: E) -> Self::Output {
        RateLimitEndpoint {
            middleware: RateLimitMiddleware {
                config: self.config.clone(),
                storage: Arc::clone(&self.storage),
            },
            wrapped: ep,
        }
    }
}

impl<E: Endpoint> Endpoint for RateLimitEndpoint<E> {
    type Output = E::Output;

    async fn call(&self, req: poem::Request) -> poem::Result<Self::Output> {
        // Extract client IP address using RealIp extractor
        let client_ip = match req.extensions().get::<RealIp>() {
            Some(RealIp(Some(ip))) => ip.to_string(),
            Some(RealIp(None)) | None => "unknown".to_string(),
        };

        // Check rate limit
        self.middleware.check_rate_limit(&client_ip)?;

        // Continue with the request
        self.wrapped.call(req).await
    }
}
