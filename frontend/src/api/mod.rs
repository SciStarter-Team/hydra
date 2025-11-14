use poem::{Route, get, post, Endpoint};

pub mod v2;
pub mod user;
pub mod middleware;

/// External API routes - for partner projects (no session auth required)
/// These use API keys, JWT, or OAuth for authentication
pub fn external_routes() -> impl Endpoint {
    Route::new()
        // v2 Participation APIs
        .at("/participation/hashed/:slug", post(v2::record_event_hashed))
        .at("/participation/connect/:slug", get(v2::connect_opaque))
        .at("/participation/connect/:slug/:idx", get(v2::connect_opaque_with_idx))
        .at("/participation/opaque/:slug", post(v2::record_event_opaque))
        .at("/participation/email/:slug", post(v2::record_event_email))
        .at("/participation/link-token/:slug", post(v2::record_event_link_token))
        .at("/participation/articulate", post(v2::record_event_articulate))
        // OpenID endpoints require Phase 5
        // .at("/participation/openid/:slug", post(v2::record_event_openid))
        // .at("/participation/openid", post(v2::record_event_openid_no_slug))
}

/// Authenticated API routes - for logged-in users
/// User-facing pages like API key display
pub fn authenticated_routes() -> impl Endpoint {
    Route::new()
        .at("/key", get(user::api_key_page))
        .at("/delete", post(user::delete_account))
}
