//! Event management handlers for the admin interface.
//!
//! This module handles event listing, search, editing, deletion,
//! and field updates.

use super::types::lookup_user_email;
use crate::component::{AdminLayout, Component, admin_layout::AdminLayoutPrepared};
use crate::middleware::authorization::Authenticated;
use crate::peering::LoadStorePeered;
use common::message::user::BasicInfo;
use common::shared::db;
use common::timestring::TimeString;
use poem::handler;
use poem::web::{Form, Path, Query};
use poem::{Route, get, put};
use sailfish::TemplateOnce;
use serde::Deserialize;
use std::sync::Arc;
use ulid::Ulid;

pub fn routes() -> Route {
    Route::new()
        .at("/", get(events))
        .at("/search", get(events_search))
        .at("/:id", get(events_edit).delete(events_delete))
        .at("/:id/field/:field", put(events_update_field))
}

#[derive(Debug, Clone)]
pub struct AdminEvent {
    pub id: String,
    pub title: String,
    pub instances: u32, // Number of scheduled instances
    pub event_date: String,
    pub location: String,
    pub event_type: String,
}

#[derive(Debug, Clone)]
pub struct AdminEventFull {
    pub id: String,
    pub name: String,
    pub created_date: String,
    pub page_creator_id: String,
    pub page_creator_email: Option<String>,
    pub description: String,
    pub presenter: String,
    pub url: String,
    pub image: String,
    pub image_credit: String,
    pub how_to_join: String,
    pub admin_notes: String,
    pub flagged: bool,
    pub event_type: String,
    pub location_type: String,
    pub entirely_online: bool,
    pub location_name: String,
    pub location_description: String,
    pub address: String,
    pub city: String,
    pub state: String,
    pub zip: String,
    pub country: String,
    pub origin: String,
}

impl AdminEventFull {
    pub async fn from_event_all(event: crate::event::all::EventAll) -> Result<Self, anyhow::Error> {
        let email = lookup_user_email(Some(event.page_creator_id)).await;
        let page_creator_email = if email.is_empty() { None } else { Some(email) };

        Ok(AdminEventFull {
            id: event.event_id.to_string(),
            name: event.name,
            created_date: event.created.to_string(),
            page_creator_id: event.page_creator_id.to_string(),
            page_creator_email,
            description: event.description,
            presenter: event.presenter,
            url: event.url,
            image: event.image,
            image_credit: event.image_credit,
            how_to_join: event.how_to_join,
            admin_notes: event.admin_notes,
            flagged: event.flagged,
            event_type: event.event_type.to_string(),
            location_type: event.location_type,
            entirely_online: event.entirely_online,
            location_name: event.location_name,
            location_description: event.location_description,
            address: event.address,
            city: event.city,
            state: event.state,
            zip: event.zip,
            country: event.country,
            origin: event.origin,
        })
    }
}

fn humanize_event_date(datetime_str: Option<String>) -> String {
    match datetime_str {
        Some(date_str) => match TimeString::try_from(date_str) {
            Ok(time_string) => time_string.format_display(),
            Err(_) => "Invalid date".to_string(),
        },
        None => "No upcoming instances".to_string(),
    }
}

async fn fetch_events_page(
    search: Option<&str>,
    status_filter: Option<&str>,
    page: Option<u32>,
    per_page: Option<u32>,
) -> Result<Vec<AdminEvent>, anyhow::Error> {
    use sqlx::QueryBuilder;

    let page = page.unwrap_or(1);
    let per_page = per_page.unwrap_or(25);
    let offset = (page - 1) * per_page;

    #[derive(sqlx::FromRow)]
    struct EventRow {
        event_id: String,
        name: String,
        #[allow(dead_code)]
        created: String,
        location_name: String,
        event_type: String,
        entirely_online: bool,
        next_instance_begin: Option<String>,
        instance_count: i64,
        is_currently_ongoing: i32,
    }

    let mut query = QueryBuilder::new("");

    if let Some(search_term) = search.filter(|s| !s.trim().is_empty()) {
        query.push(
            "SELECT e.event_id, e.name, e.created, e.location_name, e.event_type, e.entirely_online,
                    MIN(CASE WHEN ei.begin > datetime('now') THEN ei.begin END) as next_instance_begin,
                    COUNT(ei.begin) as instance_count,
                    CASE WHEN EXISTS(SELECT 1 FROM event_instances ei2 WHERE ei2.event_id = e.event_id AND ei2.begin <= datetime('now') AND ei2.end > datetime('now')) THEN 1 ELSE 0 END as is_currently_ongoing
             FROM events e
             JOIN events_fts f ON e.event_id = f.event_id
             LEFT JOIN event_instances ei ON e.event_id = ei.event_id
             WHERE events_fts MATCH "
        );
        query.push_bind(search_term);
    } else {
        query.push(
            "SELECT e.event_id, e.name, e.created, e.location_name, e.event_type, e.entirely_online,
                    MIN(CASE WHEN ei.begin > datetime('now') THEN ei.begin END) as next_instance_begin,
                    COUNT(ei.begin) as instance_count,
                    CASE WHEN EXISTS(SELECT 1 FROM event_instances ei2 WHERE ei2.event_id = e.event_id AND ei2.begin <= datetime('now') AND ei2.end > datetime('now')) THEN 1 ELSE 0 END as is_currently_ongoing
             FROM events e
             LEFT JOIN event_instances ei ON e.event_id = ei.event_id
             WHERE 1=1"
        );
    }

    if let Some(status) = status_filter.filter(|s| !s.is_empty()) {
        match status {
            "Upcoming" => {
                query.push(" AND EXISTS (SELECT 1 FROM event_instances ei2 WHERE ei2.event_id = e.event_id AND ei2.end > datetime('now'))");
            }
            "Past" => {
                query.push(" AND NOT EXISTS (SELECT 1 FROM event_instances ei2 WHERE ei2.event_id = e.event_id AND ei2.end > datetime('now'))");
            }
            _ => {
                // "All Status" or any other value - no additional filtering
            }
        }
    }

    query.push(
        " GROUP BY e.event_id, e.name, e.created, e.location_name, e.event_type, e.entirely_online",
    );
    query.push(" ORDER BY e.created DESC LIMIT ");
    query.push_bind(per_page);
    query.push(" OFFSET ");
    query.push_bind(offset);

    let event_rows = query.build_query_as::<EventRow>().fetch_all(db()).await?;

    let admin_events = event_rows
        .into_iter()
        .map(|row| {
            let location = if row.entirely_online {
                "Online".to_string()
            } else {
                row.location_name
            };

            // Check if event is currently ongoing
            let event_date = if row.is_currently_ongoing != 0 {
                "Currently ongoing".to_string()
            } else {
                humanize_event_date(row.next_instance_begin)
            };

            AdminEvent {
                id: row.event_id,
                title: row.name,
                instances: row.instance_count as u32,
                event_date,
                location,
                event_type: row.event_type,
            }
        })
        .collect();

    Ok(admin_events)
}

async fn count_events(
    search: Option<&str>,
    status_filter: Option<&str>,
) -> Result<u64, anyhow::Error> {
    use sqlx::QueryBuilder;

    let mut query = QueryBuilder::new("");

    if let Some(search_term) = search.filter(|s| !s.trim().is_empty()) {
        query.push(
            "SELECT COUNT(*) FROM events e
             JOIN events_fts f ON e.event_id = f.event_id
             WHERE events_fts MATCH ",
        );
        query.push_bind(search_term);
    } else {
        query.push("SELECT COUNT(*) FROM events e WHERE 1=1");
    }

    if let Some(status) = status_filter.filter(|s| !s.is_empty()) {
        match status {
            "Upcoming" => {
                query.push(" AND EXISTS (SELECT 1 FROM event_instances ei WHERE ei.event_id = e.event_id AND ei.end > datetime('now'))");
            }
            "Past" => {
                query.push(" AND NOT EXISTS (SELECT 1 FROM event_instances ei WHERE ei.event_id = e.event_id AND ei.end > datetime('now'))");
            }
            _ => {
                // "All Status" or any other value - no additional filtering
            }
        }
    }

    let count: (i64,) = query.build_query_as::<(i64,)>().fetch_one(db()).await?;

    Ok(count.0 as u64)
}

#[derive(Debug)]
pub struct EventsPage;

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/events.stpl.html")]
pub struct EventsPagePrepared {
    events: Vec<AdminEvent>,
    current_page: u32,
    total_events: u32,
    has_previous: bool,
    has_next: bool,
    start_item: u32,
    end_item: u32,
    search_term: String,
    selected_status: String,
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/events_content.stpl.html")]
pub struct EventsContent {
    pub events: Vec<AdminEvent>,
    pub current_page: u32,
    pub total_events: u32,
    pub has_previous: bool,
    pub has_next: bool,
    pub per_page: u32,
    pub start_item: u32,
    pub end_item: u32,
    pub pagination_url: String,
    pub pagination_include: String,
    pub search_term: String,
    pub selected_status: String,
}

impl Component for EventsPage {
    type Param = Option<Arc<BasicInfo>>;
    type Prepared = AdminLayoutPrepared<EventsPagePrepared>;

    async fn prepare(self, param: Self::Param) -> Result<Self::Prepared, anyhow::Error> {
        let page = 1u32; // For now, just show first page on dashboard
        let per_page = 10u32; // Show top 10 recent events

        let events_list = match fetch_events_page(None, None, Some(page), Some(per_page)).await {
            Ok(event_results) => event_results,
            Err(e) => {
                tracing::warn!("Failed to fetch events from database: {}", e);
                vec![]
            }
        };

        let total_events = count_events(None, None).await.unwrap_or(0);

        let events_content = EventsPagePrepared {
            events: events_list.clone(),
            current_page: page,
            total_events: total_events as u32,
            has_previous: false,
            has_next: total_events > per_page as u64,
            start_item: 1,
            end_item: std::cmp::min(per_page, total_events as u32),
            search_term: "".to_string(),
            selected_status: "".to_string(),
        };

        AdminLayout::new("Events", events_content)
            .with_breadcrumb("Events", None)
            .with_sidebar_active("events")
            .prepare(param)
            .await
    }
}

#[derive(Debug, Deserialize, Clone)]
struct EventSearchQuery {
    search: Option<String>,
    status: Option<String>,
    page: Option<u32>,
}

#[handler]
async fn events(Authenticated(user_info): Authenticated) -> Result<poem::Response, anyhow::Error> {
    EventsPage.into_response(user_info).await
}

async fn events_search_internal(
    query: EventSearchQuery,
) -> Result<EventsPagePrepared, anyhow::Error> {
    let page = query.page.unwrap_or(1);
    let per_page = 25u32;

    let events_list = match fetch_events_page(
        query.search.as_deref(),
        query.status.as_deref(),
        Some(page),
        Some(per_page),
    )
    .await
    {
        Ok(event_results) => event_results,
        Err(e) => {
            eprintln!("Error fetching events: {}", e);
            vec![]
        }
    };

    // Get accurate total count using the count_events function
    let total_events = count_events(query.search.as_deref(), query.status.as_deref())
        .await
        .unwrap_or(0) as u32;

    let has_previous = page > 1;
    let has_next = (page * per_page) < total_events;

    let start_item = if total_events == 0 {
        0
    } else {
        ((page - 1) * per_page) + 1
    };
    let end_item = std::cmp::min(page * per_page, total_events as u32);

    Ok(EventsPagePrepared {
        events: events_list,
        current_page: page,
        total_events,
        has_previous,
        has_next,
        start_item,
        end_item,
        search_term: query.search.as_deref().unwrap_or("").to_string(),
        selected_status: query.status.as_deref().unwrap_or("").to_string(),
    })
}

#[handler]
async fn events_search(
    Query(query): Query<EventSearchQuery>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let events_page = events_search_internal(query.clone()).await?;

    // For HTMX requests, render just the events content and set URL push header
    let events_content = EventsContent {
        events: events_page.events,
        current_page: events_page.current_page,
        total_events: events_page.total_events,
        has_previous: events_page.has_previous,
        has_next: events_page.has_next,
        per_page: 25,
        start_item: events_page.start_item,
        end_item: events_page.end_item,
        pagination_url: "/-admin/events/search".to_string(),
        pagination_include: "#event-search-form".to_string(),
        search_term: query.search.as_deref().unwrap_or("").to_string(),
        selected_status: query.status.as_deref().unwrap_or("").to_string(),
    };

    let html = events_content.render_once()?;

    // Build URL parameters for HX-Push-Url
    let mut url_params = vec![];
    if let Some(search) = &query.search {
        if !search.trim().is_empty() {
            url_params.push(format!("search={}", search));
        }
    }
    if let Some(status) = &query.status {
        if !status.trim().is_empty() {
            url_params.push(format!("status={}", status));
        }
    }
    let page = query.page.unwrap_or(1);
    if page > 1 {
        url_params.push(format!("page={}", page));
    }

    let url = if url_params.is_empty() {
        "/-admin/events".to_string()
    } else {
        format!("/-admin/events?{}", url_params.join("&"))
    };

    Ok(poem::Response::builder()
        .content_type("text/html")
        .header("HX-Push-Url", url)
        .body(html))
}

#[derive(Debug)]
struct EditEventPage {
    event: AdminEventFull,
}

impl Component for EditEventPage {
    type Param = Option<Arc<BasicInfo>>;
    type Prepared = AdminLayoutPrepared<EditEventContent>;

    async fn prepare(self, user_info: Self::Param) -> anyhow::Result<Self::Prepared> {
        let title = self.event.name.clone();
        let content = EditEventContent { event: self.event };

        AdminLayout::new(&format!("Edit Event: {}", title), content)
            .with_breadcrumb("Events", Some("/-admin/events"))
            .with_breadcrumb_short(&title, "Event", None)
            .with_sidebar_active("events")
            .prepare(user_info)
            .await
    }
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/edit_event_content.stpl.html")]
struct EditEventContent {
    event: AdminEventFull,
}

#[handler]
async fn events_edit(
    Path(event_id): Path<String>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    // Parse event ID and load real event data
    let event_ulid = event_id
        .parse::<Ulid>()
        .map_err(|_| anyhow::anyhow!("Invalid event ID format"))?;

    // Load EventAll from database and convert to AdminEventFull
    let event_all = crate::event::all::EventAll::load(event_ulid).await?;
    let event = AdminEventFull::from_event_all(event_all).await?;

    let edit_event_page = EditEventPage { event };

    Ok(edit_event_page.into_response(user_info).await?)
}

#[handler]
async fn events_delete(
    Path(event_id): Path<String>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let event_ulid = event_id
        .parse::<Ulid>()
        .map_err(|_| anyhow::anyhow!("Invalid event ID format"))?;
    let event_id_str = event_ulid.to_string();

    sqlx::query!(r#"DELETE FROM events WHERE event_id = ?"#, event_id_str)
        .execute(db())
        .await?;

    // Log the activity
    if let Some(user_info) = &user_info {
        common::service::log_activity(None, "event-delete", user_info.user_id, event_ulid, None)
            .await?;
    }

    // Check if there are any events left
    let exists: bool =
        sqlx::query_scalar!(r#"SELECT EXISTS(SELECT 1 FROM events) as "exists: bool""#)
            .fetch_one(db())
            .await?;

    let html = if !exists {
        // If no items left, return OOB swap with empty state
        let content = events_search_internal(EventSearchQuery {
            search: None,
            status: None,
            page: Some(1),
        })
        .await?;
        format!(
            r#"<div id="events-content" hx-swap-oob="outerHTML">{}</div>"#,
            content.render_once()?
        )
    } else {
        // Otherwise just return empty to remove the row
        String::new()
    };

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(html))
}

#[derive(Debug, Deserialize)]
struct UpdateEventFieldForm {
    value: Option<String>,
}

#[handler]
async fn events_update_field(
    Path((event_id, field_type)): Path<(String, String)>,
    Form(form): Form<UpdateEventFieldForm>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    use crate::event::field::{EventField, EventFieldValue};

    let event_ulid = event_id
        .parse::<Ulid>()
        .map_err(|_| anyhow::anyhow!("Invalid event ID format"))?;

    // For checkboxes, an unchecked box sends no value, so treat None/empty as empty string
    let form_value = form.value.unwrap_or_default();

    // Parse the field type and create the appropriate EventField
    let field_value = match field_type.as_str() {
        "name" => EventFieldValue::Name(form_value),
        "description" => EventFieldValue::Description(form_value),
        "presenter" => EventFieldValue::Presenter(form_value),
        "url" => EventFieldValue::Url(form_value),
        "image" => EventFieldValue::Image(form_value),
        "image_credit" => EventFieldValue::ImageCredit(form_value),
        "how_to_join" => EventFieldValue::HowToJoin(form_value),
        "location_type" => EventFieldValue::LocationType(form_value),
        "location_name" => EventFieldValue::LocationName(form_value),
        "location_description" => EventFieldValue::LocationDescription(form_value),
        "address" => EventFieldValue::Address(form_value),
        "city" => EventFieldValue::City(form_value),
        "state" => EventFieldValue::State(form_value),
        "zip" => EventFieldValue::Zip(form_value),
        "country" => EventFieldValue::Country(form_value),
        "admin_notes" => EventFieldValue::AdminNotes(form_value),
        "flagged" => EventFieldValue::Flagged(form_value == "true"),
        "entirely_online" => EventFieldValue::EntirelyOnline(form_value == "true"),
        _ => return Err(anyhow::anyhow!("Unknown field type: {}", field_type)),
    };

    let event_field = EventField::new(event_ulid, field_value);
    event_field.store().await?;

    // Log the activity
    if let Some(user_info) = &user_info {
        common::service::log_activity(
            None,
            "event-edit",
            user_info.user_id,
            event_ulid,
            field_type,
        )
        .await?;
    }

    Ok(poem::Response::builder().content_type("text/html").body(""))
}
