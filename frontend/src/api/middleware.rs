// API middleware for CORS
// Uses poem's built-in CORS middleware for API endpoints

use poem::{http::Method, middleware::Cors};

/// Create CORS middleware for API endpoints
/// Allows cross-origin requests from partner projects
pub fn api_cors() -> Cors {
    Cors::new()
        .allow_method(Method::GET)
        .allow_method(Method::POST)
        .allow_method(Method::OPTIONS)
        .allow_credentials(false)
        // TODO: Configure allowed origins based on settings
        // For now, allow all origins for partner project access using wildcard
        .allow_origin_regex("*")
}
