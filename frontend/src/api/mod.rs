use poem::{Endpoint, Route, get, post};

pub mod analytics;
pub mod import;
pub mod middleware;
pub mod misc;
pub mod user;
pub mod v1;
pub mod v2;

/// Check if an API key is valid and has permission to write to a project
/// (project page creator, a page manager, or staff/superuser). Shared by the
/// v1 and v2 participation endpoints to decide the `authenticated` flag.
pub(crate) async fn check_api_key_permission(
    api_key: &str,
    project_id: &str,
) -> Result<bool, anyhow::Error> {
    use common::message::user::{ValidateApiKeyRequest, ValidateApiKeyResponse};
    use common::service::request;
    use common::shared::db;

    let validation: ValidateApiKeyResponse = request(
        "user.api_key.validate",
        ValidateApiKeyRequest {
            api_key: api_key.to_string(),
        },
    )
    .await?;

    if !validation.valid {
        return Ok(false);
    }

    // Staff/superuser can always write
    if validation.is_staff || validation.is_superuser {
        return Ok(true);
    }

    let Some(user_id) = validation.user_id else {
        return Ok(false);
    };

    // Check if user is the project page creator or a page manager
    let has_permission = sqlx::query_scalar!(
        r#"SELECT 1 as "has_permission: bool" FROM projects
         WHERE project_id = ? AND page_creator_id = ?
         UNION
         SELECT 1 FROM project_page_editors
         WHERE project_id = ? AND user_id = ?
         LIMIT 1"#,
        project_id,
        user_id,
        project_id,
        user_id
    )
    .fetch_optional(db())
    .await?;

    Ok(has_permission.is_some())
}

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
        .at("/participation/ulid/:slug", post(v2::record_event_ulid))
        .at("/participation/connect/:slug", get(v2::connect_opaque))
        .at(
            "/participation/connect/:slug/:idx",
            get(v2::connect_opaque_with_idx),
        )
        .at("/participation/opaque/:slug", post(v2::record_event_opaque))
        .at("/participation/email/:slug", post(v2::record_event_email))
        .at(
            "/participation/link-token/:slug",
            post(v2::record_event_link_token),
        )
        .at(
            "/participation/articulate",
            post(v2::record_event_articulate),
        )
        .at("/participation/openid/:slug", post(v2::record_event_openid))
        .at("/participation/openid", post(v2::record_event_openid))
        // Analytics endpoints
        .at("/affiliated", get(analytics::affiliated))
        .at("/ai-opted-out", get(analytics::ai_opted_out))
        .at("/check-join", get(analytics::check_join))
        .at("/project-region-name", get(analytics::project_region_name))
        // Event import
        .at("/v2/events/import", post(import::events_import))
        // Misc
        .at(
            "/library-network-signup",
            post(misc::library_network_signup),
        )
        .at("/training-state.json", get(misc::training_state))
        .at("/ambassador_map.geojson", get(misc::ambassador_map))
        .at("/login-for-snm", post(misc::snm_login))
}

/// Authenticated API routes - for logged-in users
/// User-facing pages like API key display
pub fn authenticated_routes() -> impl Endpoint {
    Route::new()
        .at("/key", get(user::api_key_page))
        .at("/delete", post(user::delete_account))
}
