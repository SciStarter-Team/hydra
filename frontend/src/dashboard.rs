pub mod bookmarks;
pub mod entity_messages;
pub mod entity_summary;
pub mod interests;
pub mod lists;
pub mod main;
pub mod messages;
pub mod owned;
pub mod places;
pub mod profile;
pub mod settings;

use std::collections::HashMap;
use std::time::Duration;

use anyhow::Error;
use common::message::dashboard::{EntitySummary, EntitySummaryKind};
use common::message::user::{EntityCountsRequest, EntityCountsResponse};
use common::shared::db;
use poem::{Endpoint, EndpointExt, Route, get, post};
use sailfish::TemplateOnce;

use crate::dashboard::entity_summary::entity_link;
use crate::local_time::{TimeFormat, local_time};
use crate::middleware::rate_limit::{RateLimitConfig, RateLimitMiddleware};

/// Items per page in the paginated dashboard/profile entity sections.
pub const SECTION_PAGE_SIZE: usize = 6;

/// Clamp a requested page number against the item count, returning
/// `(page, pages)`. There is always at least one page, so an empty
/// section renders its empty text rather than a phantom pager.
pub fn clamp_page(total: usize, page: usize) -> (usize, usize) {
    let pages = total.div_ceil(SECTION_PAGE_SIZE).max(1);
    (page.min(pages - 1), pages)
}

/// Previous/next controls for a paginated section. `endpoint` is the
/// HTMX fragment URL (page number appended as a query parameter) and
/// `target` is the DOM id of the section container being swapped.
#[derive(TemplateOnce)]
#[template(path = "dashboard/_partials/pager.stpl.html")]
pub struct Pager {
    pub page: usize,
    pub pages: usize,
    pub endpoint: String,
    pub target: String,
}

/// One page of a card-grid section (joined, saves, events, owned
/// grids on the profile page), swapped in place by its pager.
#[derive(TemplateOnce)]
#[template(path = "dashboard/_partials/entity_grid_section.stpl.html")]
pub struct EntityGridSection {
    pub target: String,
    pub items: Vec<EntityCardModel>,
    pub empty_text: String,
    pub pager: Pager,
}

pub fn entity_grid_section(
    summaries: &[EntitySummary],
    page: usize,
    endpoint: &str,
    target: &str,
    empty_text: &str,
) -> EntityGridSection {
    let (page, pages) = clamp_page(summaries.len(), page);
    let items = summaries
        .iter()
        .skip(page * SECTION_PAGE_SIZE)
        .take(SECTION_PAGE_SIZE)
        .map(EntityCardModel::from_summary)
        .collect();
    EntityGridSection {
        target: target.to_string(),
        items,
        empty_text: empty_text.to_string(),
        pager: Pager {
            page,
            pages,
            endpoint: endpoint.to_string(),
            target: target.to_string(),
        },
    }
}

/// The user's owned projects and events (in that order), resolved to
/// summaries.
pub async fn fetch_owned(
    user_id_str: &str,
) -> Result<(Vec<EntitySummary>, Vec<EntitySummary>), Error> {
    let ids = owned_entity_ids(user_id_str).await;
    let summaries = entity_summary::lookup_batch(&ids).await?;
    Ok(summaries
        .into_iter()
        .partition(|s| s.kind == EntitySummaryKind::Project))
}

/// The projects the user has joined, resolved to summaries.
pub async fn fetch_joined(user_id_str: &str) -> Result<Vec<EntitySummary>, Error> {
    let ids: Vec<String> =
        common::service::request("user.joins.list_by_user_id", user_id_str.to_string())
            .await
            .unwrap_or_default();
    entity_summary::lookup_batch(&ids).await
}

/// The user's bookmarked projects and events (in that order), resolved
/// to summaries.
pub async fn fetch_bookmarked(
    user_id_str: &str,
) -> Result<(Vec<EntitySummary>, Vec<EntitySummary>), Error> {
    let ids: Vec<String> =
        common::service::request("user.bookmarks.list_by_user", user_id_str.to_string())
            .await
            .unwrap_or_default();
    let summaries = entity_summary::lookup_batch(&ids).await?;
    Ok(summaries
        .into_iter()
        .partition(|s| s.kind == EntitySummaryKind::Project))
}

/// Fetch per-entity engagement counts from the given user-service
/// endpoint, tolerating failure (missing entries simply read as zero).
pub async fn entity_counts(endpoint: &'static str, entity_ids: &[String]) -> HashMap<String, i64> {
    if entity_ids.is_empty() {
        return HashMap::new();
    }
    let response: EntityCountsResponse = match common::service::request(
        endpoint,
        EntityCountsRequest {
            entity_ids: entity_ids.to_vec(),
        },
    )
    .await
    {
        Ok(response) => response,
        Err(_) => return HashMap::new(),
    };
    response.counts.into_iter().collect()
}

/// The ids of every project and event the user can edit, read from the
/// page-editor tables that the entity sync and the detail-page editor
/// management maintain. These tables are the site's authoritative
/// ownership store (the detail pages authorize edits against them).
/// Failures read as "none owned" so dashboard pages still render if
/// the database is unavailable.
pub async fn owned_entity_ids(user_id_str: &str) -> Vec<String> {
    let projects = sqlx::query_scalar!(
        r#"SELECT "project_id" FROM "project_page_editors" WHERE "user_id" = ?"#,
        user_id_str
    )
    .fetch_all(db())
    .await
    .unwrap_or_default();

    let events = sqlx::query_scalar!(
        r#"SELECT "event_id" FROM "event_page_editors" WHERE "user_id" = ?"#,
        user_id_str
    )
    .fetch_all(db())
    .await
    .unwrap_or_default();

    projects.into_iter().chain(events).collect()
}

/// Whether the user is a page editor of the given entity, per the same
/// page-editor tables `owned_entity_ids` reads.
pub async fn manages_entity(user_id_str: &str, summary: &EntitySummary) -> Result<bool, Error> {
    let entity_id = summary.id.to_string();
    let found = match summary.kind {
        EntitySummaryKind::Project => {
            sqlx::query_scalar!(
                r#"SELECT 1 as "v!: i32"
            FROM "project_page_editors"
            WHERE "project_id" = ? AND "user_id" = ?"#,
                entity_id,
                user_id_str
            )
            .fetch_optional(db())
            .await?
        }
        EntitySummaryKind::Event => {
            sqlx::query_scalar!(
                r#"SELECT 1 as "v!: i32"
            FROM "event_page_editors"
            WHERE "event_id" = ? AND "user_id" = ?"#,
                entity_id,
                user_id_str
            )
            .fetch_optional(db())
            .await?
        }
    };
    Ok(found.is_some())
}

/// Card-sized view of a project or event, shared by the dashboard panels
/// (overview, owned, bookmarks, lists, profile).
pub struct EntityCardModel {
    /// The entity ULID (string form), used to build management links such
    /// as the owned-entity message inbox.
    pub id: String,
    pub kind: EntitySummaryKind,
    pub name: String,
    pub link: String,
    pub image: Option<String>,
    pub status: String,
    pub created: String,
    pub begin: Option<String>,
    pub end: Option<String>,
}

impl EntityCardModel {
    pub fn from_summary(summary: &EntitySummary) -> Self {
        Self {
            id: summary.id.to_string(),
            kind: summary.kind,
            name: summary.name.clone(),
            link: entity_link(summary),
            image: summary.image.clone(),
            status: summary.status.clone(),
            created: summary.created.clone(),
            begin: summary.begin.clone(),
            end: summary.end.clone(),
        }
    }

    pub fn kind_label(&self) -> &'static str {
        match self.kind {
            EntitySummaryKind::Project => "Project",
            EntitySummaryKind::Event => "Event",
        }
    }
}

pub fn routes() -> impl Endpoint {
    // Per-IP write limits for direct messaging — the legitimate way to put a
    // topic in someone else's inbox. Conservative to deter mass DM spam.
    let send_limit = RateLimitConfig::new(10, Duration::from_secs(300))
        .with_error_message("Too many messages. Please wait a few minutes before sending again.");
    let reply_limit = RateLimitConfig::new(20, Duration::from_secs(300))
        .with_error_message("Too many replies. Please wait a few minutes before replying again.");

    Route::new()
        .at("/", get(main::page))
        .at("/section/:section", get(main::section))
        .at("/profile/section/:section", get(profile::section))
        .at("/owned", get(owned::page))
        .at(
            "/owned/:entity_id/broadcast",
            get(entity_messages::broadcast_form).post(entity_messages::broadcast),
        )
        .at("/owned/:entity_id/messages", get(entity_messages::inbox))
        .at(
            "/owned/:entity_id/messages/:topic_id",
            get(entity_messages::thread),
        )
        .at(
            "/owned/:entity_id/messages/:topic_id/reply",
            post(entity_messages::reply),
        )
        .at("/lists", get(lists::page).post(lists::create))
        .at("/lists/:list_id/update", post(lists::update))
        .at("/lists/:list_id/delete", post(lists::delete))
        .at("/lists/:list_id/entries", post(lists::add_entry))
        .at(
            "/lists/:list_id/entries/:entry_id/remove",
            post(lists::remove_entry),
        )
        .at(
            "/lists/:list_id/entries/:entry_id/note",
            post(lists::set_entry_note),
        )
        .at("/lists/:list_id/share", post(lists::share))
        .at("/lists/:list_id/unshare", post(lists::unshare))
        .at("/bookmarks", get(bookmarks::page))
        .at("/messages", get(messages::page))
        .at("/messages/compose", get(messages::compose))
        .at(
            "/messages/send",
            post(messages::send).with(RateLimitMiddleware::new(send_limit)),
        )
        .at("/messages/:topic_id", get(messages::thread))
        .at(
            "/messages/:topic_id/reply",
            post(messages::reply).with(RateLimitMiddleware::new(reply_limit)),
        )
        .at("/messages/:topic_id/discard", post(messages::discard))
        .at("/profile", get(profile::page))
        .at("/interests", get(interests::page).post(interests::save))
        .at("/places", get(places::page).post(places::add))
        .at("/places/:place_id/delete", post(places::delete))
        .at("/settings", get(settings::page))
        .at("/settings/general", post(settings::save_general))
        .at("/settings/ambassador", post(settings::save_ambassador))
}
