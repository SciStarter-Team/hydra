use poem::{Route, get, post, Endpoint};

pub mod analytics;
pub mod v1;
pub mod v2;
pub mod user;
pub mod middleware;
pub mod import;

/// External API routes - for partner projects (no session auth required)
/// These use API keys, JWT, or OAuth for authentication
pub fn external_routes() -> impl Endpoint {
    Route::new()
        // v1 Legacy APIs
        .at("/profile/id", get(v1::get_profile_id))
        .at("/user/new", post(v1::create_user))
        .at("/record_event", post(v1::record_event))
        .at("/record_event_by_email", post(v1::record_event_by_email))
        .at("/project/:project_id", get(v1::get_project))
        .at("/participation/failure", post(v1::participation_failure))
        .at("/project/add", post(v1::project_add))
        .at("/event/add", post(v1::event_add))
        .at("/acts-to-date", get(crate::csm::acts_to_date_api))
        // v2 Participation APIs
        .at("/participation/hashed/:slug", post(v2::record_event_hashed))
        .at("/participation/connect/:slug", get(v2::connect_opaque))
        .at("/participation/connect/:slug/:idx", get(v2::connect_opaque_with_idx))
        .at("/participation/opaque/:slug", post(v2::record_event_opaque))
        .at("/participation/email/:slug", post(v2::record_event_email))
        .at("/participation/link-token/:slug", post(v2::record_event_link_token))
        .at("/participation/articulate", post(v2::record_event_articulate))
        .at("/participation/openid/:slug", post(v2::record_event_openid))
        .at("/participation/openid", post(v2::record_event_openid))
        // Analytics endpoints
        .at("/affiliated", get(analytics::affiliated))
        .at("/ai-opted-out", get(analytics::ai_opted_out))
        .at("/check-join", get(analytics::check_join))
        .at("/project-region-name", get(analytics::project_region_name))
        // Event import
        .at("/v2/events/import", post(import::events_import))
}

/// Authenticated API routes - for logged-in users
/// User-facing pages like API key display
pub fn authenticated_routes() -> impl Endpoint {
    Route::new()
        .at("/key", get(user::api_key_page))
        .at("/delete", post(user::delete_account))
}
