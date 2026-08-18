// Admin module - routes and submodule declarations

pub mod ambassadors;
pub mod analytics;
pub mod archive;
pub mod autocomplete;
pub mod badges;
pub mod communities;
pub mod content;
pub mod dashboard;
pub mod dashboard_settings;
pub mod email;
pub mod entities;
pub mod events;
pub mod files;
pub mod girlscouts;
pub mod integrations;
pub mod libraries;
pub mod mcp_tokens;
pub mod messages;
pub mod news;
pub mod pagebuilder;
pub mod projects;
pub mod schedules;
pub mod sets;
pub mod staff_picks;
pub mod trainings;
pub mod types;
pub mod users;
pub mod webinars;
pub mod wiki;

// Re-exports for templates and other modules that reference crate::admin::*
pub use analytics::{AnalyticsRecentActivityTable, TopPerformersBox};
pub use badges::RecipientBadge;
pub use email::{RecipientPickerPartial, TriggerList, TriggerRow};
pub use files::{GoUrlRow, GoUrlWithFileInfo, UploadSuccessResponse, get_go_urls_with_file_info};
pub use projects::{ContactWidget, ParticipantFileWidget, TagsDisplay};

use crate::middleware::authorization::{Authenticated, StaffOnly};
use poem::{
    Endpoint, EndpointExt, Route, delete, get, handler, post,
    web::{Form, Query},
};
use sailfish::TemplateOnce;
use serde::Deserialize;

pub fn routes() -> impl Endpoint {
    Route::new()
        // Dashboard and help
        .nest("/", dashboard::routes())
        // Home Page / Dashboard settings
        .nest("/dashboard-settings", dashboard_settings::routes())
        // Users
        .nest("/users", users::routes())
        // Direct-message staff review queue
        .nest("/messages", messages::routes())
        // Ambassador application processing (staff-facing)
        .nest("/ambassadors", ambassadors::routes())
        // Girl Scouts Take Action submissions (staff-facing)
        .nest("/girlscouts", girlscouts::routes())
        .nest("/libraries", libraries::routes())
        // Projects
        .nest("/projects", projects::routes())
        // Events
        .nest("/events", events::routes())
        // Analytics
        .nest("/analytics", analytics::routes())
        // Files & Links
        .nest("/files-links", files::routes())
        // Entities
        .nest("/entities", entities::routes())
        // Content management
        .nest("/wysiwyg", content::wysiwyg_routes())
        .nest("/pagecontent", content::pagecontent_routes())
        .at("/clear-modal", post(content::clear_modal))
        // Schedules
        .nest("/schedules", schedules::routes())
        // News
        .nest("/news", news::routes())
        // Staff Picks (CSM)
        .nest("/staff-picks", staff_picks::routes())
        // Webinars
        .nest("/webinars", webinars::routes())
        // Communities
        .nest("/communities", communities::routes())
        // Email
        .nest("/email", email::routes())
        .nest("/email/address-sets", sets::address_sets_routes())
        .nest("/email/user-sets", sets::user_sets_routes())
        // Integrations
        .nest("/exchanges", integrations::exchange_routes())
        .nest("/oauth2-clients", integrations::oauth2_routes())
        // Wiki
        .nest("/wiki", wiki::routes())
        // Badges
        .nest("/badges", badges::routes())
        // Archive
        .nest("/archive", archive::routes())
        // Page Builder
        .nest("/engine-room", pagebuilder::engine_room_routes())
        .nest("/component-pages", pagebuilder::pages_routes())
        // MCP content-management tokens (shown under Engine Room in the sidebar)
        .nest("/mcp-tokens", mcp_tokens::routes())
        // Training-page text content (shown under Engine Room in the sidebar)
        .nest("/trainings", trainings::routes())
        // API endpoints for recipient search (used by badge granting)
        .at("/api/recipient-search", get(api_recipient_search))
        .at("/api/recipient-select", post(api_recipient_select))
        .at("/api/recipient-remove", delete(api_recipient_remove))
        // Autocomplete
        .nest("/autocomplete", autocomplete::routes())
        .with(StaffOnly)
}

// ============================================================================
// API Recipient Search (for badge granting)
// These handlers will eventually be moved to badges.rs
// ============================================================================

#[derive(Debug, Deserialize)]
struct RecipientSearchQuery {
    q: String,
    #[serde(default)]
    filter: Option<RecipientFilterQuery>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum RecipientFilterQuery {
    Users,
}

#[handler]
async fn api_recipient_search(
    Query(query): Query<RecipientSearchQuery>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    if query.q.trim().len() < 2 {
        return Ok(poem::Response::builder().content_type("text/html").body(""));
    }

    let request = common::message::user::RecipientSearchRequest {
        query: query.q,
        limit: Some(10),
        filter: query.filter.map(|f| match f {
            RecipientFilterQuery::Users => common::message::user::RecipientFilter::Users,
        }),
    };

    let response: common::message::user::RecipientSearchResponse =
        common::service::request("user.search.recipients", request).await?;

    use common::message::user::RecipientSearchResult;

    #[derive(TemplateOnce)]
    #[template(path = "admin/partials/recipient_search_results.stpl.html")]
    struct RecipientSearchResults {
        results: Vec<RecipientSearchResult>,
    }

    let html = RecipientSearchResults {
        results: response.results,
    }
    .render_once()?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(html))
}

#[derive(Debug, Deserialize)]
struct RecipientSelectForm {
    q: String,
    badgeclass: String,
    #[serde(default)]
    recipient_types: Vec<String>,
    #[serde(default)]
    recipient_values: Vec<String>,
}

#[derive(Clone)]
struct RecipientInfo {
    display: String,
    recipient_type: String,
    recipient_value: String,
}

#[derive(TemplateOnce)]
#[template(path = "admin/partials/grant_badge_form.stpl.html")]
struct GrantBadgeForm {
    badge: common::badge::Badge,
    recipients: Vec<RecipientInfo>,
}

#[handler]
async fn api_recipient_select(
    Form(form): Form<RecipientSelectForm>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let badge = common::badge::Badge::from_badgeclass(&form.badgeclass)
        .ok_or_else(|| anyhow::anyhow!("Invalid badge class"))?;

    // Build current recipients list
    let mut recipients = Vec::new();
    for (recipient_type, recipient_value) in form
        .recipient_types
        .iter()
        .zip(form.recipient_values.iter())
    {
        // Look up display name for existing recipients
        let display = if recipient_type == "User" {
            if let Ok(user_id) = recipient_value.parse::<ulid::Ulid>() {
                match common::service::request::<common::message::user::UserEmailInfo>(
                    "user.info.email_info",
                    common::message::user::UserEmailInfoRequest { user_id },
                )
                .await
                {
                    Ok(user) => {
                        if !user.first_name.is_empty() || !user.last_name.is_empty() {
                            format!("{} {}", user.first_name, user.last_name)
                                .trim()
                                .to_string()
                        } else {
                            user.handle
                        }
                    }
                    Err(_) => recipient_value.clone(),
                }
            } else {
                recipient_value.clone()
            }
        } else {
            recipient_value.clone()
        };

        recipients.push(RecipientInfo {
            display,
            recipient_type: recipient_type.clone(),
            recipient_value: recipient_value.clone(),
        });
    }

    // Add new recipient
    let (display, recipient_type, recipient_value) = if let Ok(user_id) =
        form.q.parse::<ulid::Ulid>()
    {
        // It's a user ID - look up the user
        match common::service::request::<common::message::user::UserEmailInfo>(
            "user.info.email_info",
            common::message::user::UserEmailInfoRequest { user_id },
        )
        .await
        {
            Ok(user) => {
                let display_name = if !user.first_name.is_empty() || !user.last_name.is_empty() {
                    format!("{} {}", user.first_name, user.last_name)
                        .trim()
                        .to_string()
                } else {
                    user.handle
                };
                (display_name, "User", user_id.to_string())
            }
            Err(_) => {
                return Ok(poem::Response::builder()
                    .status(poem::http::StatusCode::BAD_REQUEST)
                    .body("User not found"));
            }
        }
    } else {
        return Ok(poem::Response::builder()
            .status(poem::http::StatusCode::BAD_REQUEST)
            .body("Invalid user ID"));
    };

    recipients.push(RecipientInfo {
        display,
        recipient_type: recipient_type.to_string(),
        recipient_value,
    });

    // Return the entire form
    let form_html = GrantBadgeForm { badge, recipients }.render_once()?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(format!(r#"<div id="grant-badge-form-container" class="px-6 py-4 border-b border-gray-200" hx-swap-oob="true">{}</div>"#, form_html)))
}

#[derive(Debug, Deserialize)]
struct RecipientRemoveForm {
    remove_id: String,
    badgeclass: String,
    #[serde(default)]
    recipient_types: Vec<String>,
    #[serde(default)]
    recipient_values: Vec<String>,
}

#[handler]
async fn api_recipient_remove(
    Form(form): Form<RecipientRemoveForm>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let badge = common::badge::Badge::from_badgeclass(&form.badgeclass)
        .ok_or_else(|| anyhow::anyhow!("Invalid badge class"))?;

    // Build recipients list, filtering out all recipients at the position matching remove_id
    let mut recipients = Vec::new();
    let remove_idx = form.remove_id.parse::<usize>().unwrap_or(usize::MAX);

    for (idx, (recipient_type, recipient_value)) in form
        .recipient_types
        .iter()
        .zip(form.recipient_values.iter())
        .enumerate()
    {
        // Skip the one being removed
        if idx == remove_idx {
            continue;
        }

        // Look up display name
        let display = if recipient_type == "User" {
            if let Ok(user_id) = recipient_value.parse::<ulid::Ulid>() {
                match common::service::request::<common::message::user::UserEmailInfo>(
                    "user.info.email_info",
                    common::message::user::UserEmailInfoRequest { user_id },
                )
                .await
                {
                    Ok(user) => {
                        if !user.first_name.is_empty() || !user.last_name.is_empty() {
                            format!("{} {}", user.first_name, user.last_name)
                                .trim()
                                .to_string()
                        } else {
                            user.handle
                        }
                    }
                    Err(_) => recipient_value.clone(),
                }
            } else {
                recipient_value.clone()
            }
        } else {
            recipient_value.clone()
        };

        recipients.push(RecipientInfo {
            display,
            recipient_type: recipient_type.clone(),
            recipient_value: recipient_value.clone(),
        });
    }

    // Return the entire form
    let form_html = GrantBadgeForm { badge, recipients }.render_once()?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(format!(r#"<div id="grant-badge-form-container" class="px-6 py-4 border-b border-gray-200" hx-swap-oob="true">{}</div>"#, form_html)))
}
