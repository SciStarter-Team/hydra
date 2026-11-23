//! Event management handlers for the admin interface.
//!
//! This module handles event listing, search, editing, deletion,
//! and field updates.

use super::types::lookup_user_email;
use crate::component::{AdminLayout, Component, admin_layout::AdminLayoutPrepared};
use crate::local_time::{Clocks, TimeFormat, zoned_time};
use crate::middleware::authorization::Authenticated;
use crate::peering::LoadStorePeered;
use common::message::log::Housekeeping;
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
        .at("/:id/raw", get(events_edit_raw))
        .at("/:id/field/:field", put(events_update_field))
}

#[derive(Debug, Clone)]
pub struct AdminEvent {
    pub id: String,
    pub title: String,
    pub instances: u32, // Number of scheduled instances
    pub is_ongoing: bool,
    pub next_instance: Option<TimeString>,
    /// The venue's time zone, which `next_instance` is told in.
    pub zone: Option<jiff::tz::TimeZone>,
    pub location: String,

    /// The names of the event's types, in list order.
    pub event_types: String,
}

#[derive(Debug, Clone, Default)]
pub struct AdminEventFull {
    pub id: String,
    pub name: String,
    pub created_date: String,
    pub page_creator_id: String,
    pub page_creator_email: Option<String>,
    pub description: String,
    pub presenter: String,
    pub url: String,
    pub limited_to: String,
    pub image: String,
    pub image_credit: String,
    pub how_to_join: String,
    pub admin_notes: String,
    pub flagged: bool,
    pub location_type: String,
    pub entirely_online: bool,
    pub also_online: bool,
    pub location_name: String,
    pub location_description: String,
    pub address: String,
    pub city: String,
    pub state: String,
    pub zip: String,
    pub country: String,
    pub origin: String,
    pub facebook_event: crate::event::facebook::FacebookEvent,
    /// The organizer contacts, in the card the event page shows its editors.
    pub contacts: Vec<crate::component::detail_page::ContactDisplay>,
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
            limited_to: event.limited_to,
            image: event.image,
            image_credit: event.image_credit,
            how_to_join: event.how_to_join,
            admin_notes: event.admin_notes,
            flagged: event.flagged,
            location_type: event.location_type,
            entirely_online: event.entirely_online,
            also_online: event.also_online,
            location_name: event.location_name,
            location_description: event.location_description,
            address: event.address,
            city: event.city,
            state: event.state,
            zip: event.zip,
            country: event.country,
            origin: event.origin,
            facebook_event: event.facebook_event,
            contacts: crate::component::detail_page_edit::contacts_to_display(&event.contacts),
        })
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
        event_types: Option<String>,
        entirely_online: bool,
        next_instance_begin: Option<String>,
        instance_count: i64,
        is_currently_ongoing: i32,
    }

    let mut query = QueryBuilder::new("");

    if let Some(search_term) = search.filter(|s| !s.trim().is_empty()) {
        query.push(
            "SELECT e.event_id, e.name, e.created, e.location_name,
                    (SELECT group_concat(n.name, ', ' ORDER BY n.rank, n.position, n.name) FROM event_type_names n WHERE n.target = e.event_id) as event_types,
                    e.entirely_online,
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
            "SELECT e.event_id, e.name, e.created, e.location_name,
                    (SELECT group_concat(n.name, ', ' ORDER BY n.rank, n.position, n.name) FROM event_type_names n WHERE n.target = e.event_id) as event_types,
                    e.entirely_online,
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

    query.push(" GROUP BY e.event_id, e.name, e.created, e.location_name, e.entirely_online");
    query.push(" ORDER BY e.created DESC LIMIT ");
    query.push_bind(per_page);
    query.push(" OFFSET ");
    query.push_bind(offset);

    let event_rows = query.build_query_as::<EventRow>().fetch_all(db()).await?;
    // Each event's times are on its venue's clocks, as on its page.
    let mut zones =
        crate::event::zone::of_events(event_rows.iter().map(|row| row.event_id.as_str())).await?;

    let admin_events = event_rows
        .into_iter()
        .map(|row| {
            let zone = zones.remove(&row.event_id);
            let location = if row.entirely_online {
                "Online".to_string()
            } else {
                row.location_name
            };

            let is_ongoing = row.is_currently_ongoing != 0;
            let next_instance = row
                .next_instance_begin
                .and_then(|s| TimeString::try_from(s).ok());

            AdminEvent {
                id: row.event_id,
                title: row.name,
                instances: row.instance_count as u32,
                is_ongoing,
                next_instance,
                zone,
                location,
                event_types: row.event_types.unwrap_or_default(),
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

/// Which of the event's two admin editors a page is.
enum EditEventPage {
    /// The event page's own editors, with what staff alone change.
    Editors(EditEventContent),
    /// Each of the event's columns, set directly.
    Raw(EditEventRawContent),
}

impl EditEventPage {
    /// The editor `raw` or not of the event `event_id`, as it now stands.
    async fn load(event_id: Ulid, raw: bool) -> anyhow::Result<Self> {
        let event_all = crate::event::all::EventAll::load(event_id).await?;
        let event = AdminEventFull::from_event_all(event_all).await?;
        Ok(if raw {
            EditEventPage::Raw(EditEventRawContent { event })
        } else {
            let editors = crate::component::detail_page_edit::EventEditors::load(event_id).await?;
            EditEventPage::Editors(EditEventContent { event, editors })
        })
    }

    async fn into_response(
        self,
        user_info: Option<Arc<BasicInfo>>,
    ) -> anyhow::Result<poem::Response> {
        match self {
            EditEventPage::Editors(content) => {
                let title = content.event.name.clone();
                Ok(AdminLayout::new(&format!("Edit Event: {title}"), content)
                    .with_breadcrumb("Events", Some("/-admin/events"))
                    .with_breadcrumb_short(&title, "Event", None)
                    .with_sidebar_active("events")
                    .with_quill()
                    .with_event_editors()
                    .into_response(user_info)
                    .await?)
            }
            EditEventPage::Raw(content) => {
                let title = content.event.name.clone();
                let id = content.event.id.clone();
                Ok(AdminLayout::new(&format!("Raw Fields: {title}"), content)
                    .with_breadcrumb("Events", Some("/-admin/events"))
                    .with_breadcrumb_short(&title, "Event", Some(&format!("/-admin/events/{id}")))
                    .with_breadcrumb("Raw fields", None)
                    .with_sidebar_active("events")
                    .into_response(user_info)
                    .await?)
            }
        }
    }
}

#[derive(TemplateOnce)]
#[template(path = "admin/edit_event_content.stpl.html")]
struct EditEventContent {
    event: AdminEventFull,
    editors: crate::component::detail_page_edit::EventEditors,
}

#[derive(TemplateOnce)]
#[template(path = "admin/edit_event_raw_content.stpl.html")]
struct EditEventRawContent {
    event: AdminEventFull,
}

fn parse_event_id(event_id: &str) -> anyhow::Result<Ulid> {
    event_id
        .parse::<Ulid>()
        .map_err(|_| anyhow::anyhow!("Invalid event ID format"))
}

/// GET `/-admin/events/:id`: the event page's editors of the event.
#[handler]
async fn events_edit(
    Path(event_id): Path<String>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    EditEventPage::load(parse_event_id(&event_id)?, false)
        .await?
        .into_response(user_info)
        .await
}

/// GET `/-admin/events/:id/raw`: the event's columns, to set directly.
#[handler]
async fn events_edit_raw(
    Path(event_id): Path<String>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    EditEventPage::load(parse_event_id(&event_id)?, true)
        .await?
        .into_response(user_info)
        .await
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
        "limited_to" => EventFieldValue::limited_to(&form_value),
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
        "also_online" => EventFieldValue::AlsoOnline(form_value == "true"),
        _ => return Err(anyhow::anyhow!("Unknown field type: {}", field_type)),
    };

    let event_field = EventField::new(event_ulid, field_value);
    event_field.store().await?;

    // Log the activity
    if let Some(user_info) = &user_info {
        common::service::log_activity(
            None,
            Housekeeping::Event,
            user_info.user_id,
            event_ulid,
            field_type,
        )
        .await?;
    }

    Ok(poem::Response::builder().content_type("text/html").body(""))
}

#[cfg(test)]
mod tests {
    use assert2::check;

    use super::*;
    use crate::component::detail_page::DetailPageKind;
    use crate::component::detail_page_edit::{
        EventAdmissionDisplay, EventAttendanceDisplay, EventEditors, EventLocationDisplay,
        EventPartOfDisplay, EventRegistrationDisplay, FieldDisplay, InstancesDisplay,
        ManagersDisplay,
    };

    const ID: &str = "00000000000000000000000000";

    fn event() -> AdminEventFull {
        AdminEventFull {
            id: ID.to_string(),
            name: "Bioblitz".to_string(),
            ..AdminEventFull::default()
        }
    }

    fn field(field_type: &str) -> FieldDisplay {
        FieldDisplay::build(
            ID.to_string(),
            field_type,
            DetailPageKind::Event,
            String::new(),
            true,
            String::new(),
            true,
        )
    }

    /// The editors of an event that has nothing set, open to staff.
    fn editors() -> EventEditors {
        EventEditors {
            fields: EventEditors::FIELDS
                .iter()
                .map(|&name| (name, field(name)))
                .collect(),
            location: EventLocationDisplay::new(
                ID.to_string(),
                true,
                field,
                "",
                false,
                false,
                false,
                None,
            ),
            instances: InstancesDisplay::new(ID, Box::new([]), true, None),
            registration: EventRegistrationDisplay::new(
                ID.to_string(),
                true,
                Default::default(),
                None,
                &[],
            ),
            admission: EventAdmissionDisplay::new(ID.to_string(), true, Default::default()),
            attendance: EventAttendanceDisplay::new(
                ID.to_string(),
                true,
                String::new(),
                Vec::new(),
            ),
            part_of: EventPartOfDisplay::new(ID.to_string(), true, true, None, None),
            managers: ManagersDisplay {
                target_id: ID.to_string(),
                managers: Vec::new(),
                can_edit: true,
                error: None,
            },
        }
    }

    fn editor_page() -> String {
        EditEventContent {
            event: event(),
            editors: editors(),
        }
        .render_once()
        .unwrap()
    }

    #[test]
    fn staff_edit_everything_the_event_page_does_with_its_own_editors() {
        let html = editor_page();
        for name in EventEditors::FIELDS {
            check!(html.contains(&format!(r#"id="field-{name}""#)), "{name}");
        }
        for part in [
            "location",
            "instances",
            "registration",
            "admission",
            "attendance",
            "part_of",
        ] {
            check!(html.contains(&format!("/-detail/{ID}/{part}")), "{part}");
        }
        check!(html.contains(&format!("/-detail/{ID}/managers")));
    }

    #[test]
    fn the_editors_page_sets_no_column_directly_but_what_staff_alone_change() {
        let html = editor_page();
        let raw_fields = html
            .match_indices(&format!("/-admin/events/{ID}/field/"))
            .map(|(at, prefix)| {
                let rest = &html[at + prefix.len()..];
                &rest[..rest.find('"').unwrap()]
            })
            .collect::<Vec<_>>();
        check!(raw_fields == ["admin_notes", "flagged"]);
        check!(html.contains(&format!(r#"href="/-admin/events/{ID}/raw""#)));
    }

    #[test]
    fn the_raw_page_sets_the_columns_directly() {
        let html = EditEventRawContent { event: event() }
            .render_once()
            .unwrap();
        for column in [
            "name",
            "description",
            "how_to_join",
            "location_type",
            "also_online",
            "address",
            "image",
            "image_credit",
        ] {
            check!(
                html.contains(&format!(r#"hx-put="/-admin/events/{ID}/field/{column}""#)),
                "{column}"
            );
        }
        check!(html.contains(&format!(r#"href="/-admin/events/{ID}""#)));
        check!(!html.contains("/-detail/"));
    }
}
