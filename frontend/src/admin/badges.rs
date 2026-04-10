// Badge management for admin

use crate::component::{AdminLayout, Component};
use crate::form::QsForm;
use crate::local_time::{TimeFormat, local_time};
use crate::middleware::authorization::Authenticated;
use poem::{
    Endpoint, Route, get, handler, post,
    web::{Path, Query},
};
use sailfish::TemplateOnce;
use serde::Deserialize;
use ulid::Ulid;

// Templates
#[derive(TemplateOnce)]
#[template(path = "admin/badges.stpl.html")]
struct BadgesListPage {}

#[derive(TemplateOnce)]
#[template(path = "admin/badges_holders.stpl.html")]
struct BadgeHoldersPage {
    badge: common::badge::Badge,
    search_term: String,
    holders: Vec<common::message::user::BadgeHolderInfo>,
    current_page: u32,
    total_count: u32,
    has_previous: bool,
    has_next: bool,
    per_page: u32,
    start_item: u32,
    end_item: u32,
}

#[derive(TemplateOnce)]
#[template(path = "admin/partials/badge_holders_content.stpl.html")]
struct BadgeHoldersContentPartial {
    badge: common::badge::Badge,
    search_term: String,
    holders: Vec<common::message::user::BadgeHolderInfo>,
    current_page: u32,
    total_count: u32,
    has_previous: bool,
    has_next: bool,
    per_page: u32,
    start_item: u32,
    end_item: u32,
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

#[derive(TemplateOnce)]
#[template(path = "admin/partials/recipient_badge.stpl.html")]
pub struct RecipientBadge {
    pub display: String,
    pub id: String,
}

// Query and form types
#[derive(Debug, Deserialize)]
struct BadgesHoldersQuery {
    search: Option<String>,
    page: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct BadgeGrantForm {
    #[serde(default)]
    recipient_types: Vec<String>,
    #[serde(default)]
    recipient_values: Vec<String>,
}

// Handlers
#[handler]
async fn badges_list(
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let content = BadgesListPage {};

    let prepared = AdminLayout::new("Badge Management", content)
        .with_sidebar_active("engine-room-badges")
        .prepare(user_info)
        .await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(prepared.render_once()?))
}

#[handler]
async fn badges_holders(
    Path(badgeclass): Path<String>,
    Query(query): Query<BadgesHoldersQuery>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let badge = common::badge::Badge::from_badgeclass(&badgeclass)
        .ok_or_else(|| anyhow::anyhow!("Invalid badge class"))?;

    let page = query.page.unwrap_or(1);
    let limit = 50;

    let request = common::message::user::BadgeHoldersRequest {
        badge,
        search: query.search.clone(),
        page: Some(page),
        limit: Some(limit),
    };

    let response = common::service::request::<common::message::user::BadgeHoldersResponse>(
        "user.badges.holders",
        request,
    )
    .await?;

    let total_pages = (response.total_count + limit - 1) / limit;
    let has_previous = page > 1;
    let has_next = page < total_pages;
    let start_item = if response.holders.is_empty() {
        0
    } else {
        ((page - 1) * limit) + 1
    };
    let end_item = std::cmp::min(page * limit, response.total_count);

    let content = BadgeHoldersPage {
        badge,
        search_term: query.search.clone().unwrap_or_default(),
        holders: response.holders,
        current_page: page,
        total_count: response.total_count,
        has_previous,
        has_next,
        per_page: limit,
        start_item,
        end_item,
    };

    let prepared = AdminLayout::new(&format!("{} - Badge Holders", badge.name()), content)
        .with_sidebar_active("engine-room-badges")
        .with_breadcrumb("Badges", Some("/-admin/badges"))
        .with_breadcrumb(badge.name(), None)
        .prepare(user_info)
        .await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(prepared.render_once()?))
}

#[handler]
async fn badges_holders_content(
    Path(badgeclass): Path<String>,
    Query(query): Query<BadgesHoldersQuery>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let badge = common::badge::Badge::from_badgeclass(&badgeclass)
        .ok_or_else(|| anyhow::anyhow!("Invalid badge class"))?;

    let page = query.page.unwrap_or(1);
    let limit = 50;

    let request = common::message::user::BadgeHoldersRequest {
        badge,
        search: query.search.clone(),
        page: Some(page),
        limit: Some(limit),
    };

    let response = common::service::request::<common::message::user::BadgeHoldersResponse>(
        "user.badges.holders",
        request,
    )
    .await?;

    let total_pages = (response.total_count + limit - 1) / limit;
    let has_previous = page > 1;
    let has_next = page < total_pages;
    let start_item = if response.holders.is_empty() {
        0
    } else {
        ((page - 1) * limit) + 1
    };
    let end_item = std::cmp::min(page * limit, response.total_count);

    let partial = BadgeHoldersContentPartial {
        badge,
        search_term: query.search.clone().unwrap_or_default(),
        holders: response.holders,
        current_page: page,
        total_count: response.total_count,
        has_previous,
        has_next,
        per_page: limit,
        start_item,
        end_item,
    };

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(partial.render_once()?))
}

#[handler]
async fn badges_grant(
    Path(badgeclass): Path<String>,
    QsForm(form): QsForm<BadgeGrantForm>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let badge = common::badge::Badge::from_badgeclass(&badgeclass)
        .ok_or_else(|| anyhow::anyhow!("Invalid badge class"))?;

    // Validate parallel arrays
    if form.recipient_types.len() != form.recipient_values.len() {
        return Err(anyhow::anyhow!("Mismatched recipient data"));
    }

    if form.recipient_types.is_empty() {
        return Err(anyhow::anyhow!("No recipients specified"));
    }

    // Grant badge to all selected users
    for (recipient_type, recipient_value) in form
        .recipient_types
        .iter()
        .zip(form.recipient_values.iter())
    {
        if recipient_type == "User" {
            let user_id = recipient_value
                .parse::<Ulid>()
                .map_err(|_| anyhow::anyhow!("Invalid user ID format"))?;

            let request = common::message::user::UserAndBadge {
                user_id,
                badge: badge.clone(),
            };

            common::service::send("user.badges.grant", request).await?;

            // Log the activity
            if let Some(user_info) = &user_info {
                common::service::log_activity(
                    None,
                    "badge-grant",
                    user_info.user_id,
                    user_id,
                    Some(badgeclass.clone()),
                )
                .await?;
            }
        }
    }

    // Return empty form (cleared state) after successful grant
    let form_html = GrantBadgeForm {
        badge,
        recipients: Vec::new(),
    }
    .render_once()?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(format!(r#"<div id="grant-badge-form-container" class="px-6 py-4 border-b border-gray-200" hx-swap-oob="true">{}</div>"#, form_html)))
}

#[handler]
async fn badges_revoke(
    Path((badgeclass, user_id_str)): Path<(String, String)>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let badge = common::badge::Badge::from_badgeclass(&badgeclass)
        .ok_or_else(|| anyhow::anyhow!("Invalid badge class"))?;

    let user_id = user_id_str
        .parse::<Ulid>()
        .map_err(|_| anyhow::anyhow!("Invalid user ID format"))?;

    let request = common::message::user::UserAndBadge { user_id, badge };

    common::service::send("user.badges.revoke", request).await?;

    // Log the activity
    if let Some(user_info) = &user_info {
        common::service::log_activity(
            None,
            "badge-revoke",
            user_info.user_id,
            user_id,
            Some(badgeclass.clone()),
        )
        .await?;
    }

    // Return empty response to delete the row via HTMX
    Ok(poem::Response::builder()
        .status(poem::http::StatusCode::OK)
        .finish())
}

// Routes export
pub fn routes() -> impl Endpoint {
    Route::new()
        .at("/", get(badges_list))
        .at("/:badgeclass/holders", get(badges_holders))
        .at("/:badgeclass/holders-content", get(badges_holders_content))
        .at("/:badgeclass/grant", post(badges_grant))
        .at("/:badgeclass/:user_id/revoke", post(badges_revoke))
}
