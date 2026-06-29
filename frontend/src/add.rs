use std::sync::Arc;

use jiff::{Zoned, civil, tz::TimeZone};
use poem::{
    IntoResponse, Response, handler,
    web::{Query, Redirect},
};
use sailfish::TemplateOnce;
use serde::Deserialize;
use ulid::Ulid;

use common::{
    insensitivestring::InsensitiveString,
    message::user::BasicInfo,
    service::{TriggerEvent, log_activity, trigger_email},
    shared::db,
    timestring::TimeString,
};

use crate::{
    component::{Component, Head, StandardLayout},
    contact::{Contact, ContactRole},
    entity::{Entity, EntityKind, EntityTarget},
    event::{EventInstance, EventType, all::EventAll},
    form::QsForm,
    middleware::authorization::Authenticated,
    peering::LoadStorePeered,
};

// ---------------------------------------------------------------------------
// Chooser landing page
// ---------------------------------------------------------------------------

#[derive(sailfish::TemplateOnce)]
#[template(path = "site/add_chooser.stpl.html")]
struct AddChooserPage {
    head: <Head as Component>::Prepared,
    layout: <StandardLayout<AddChooserContent> as Component>::Prepared,
}

#[derive(sailfish::TemplateOnce)]
#[template(path = "site/add_chooser_content.stpl.html")]
struct AddChooserContent {
    logged_in: bool,
}

/// Landing page for the contribute flow: lets a visitor choose whether
/// to add a project or an event. The submission forms themselves live
/// at `/add-project` and `/add-event` and require authentication.
#[handler]
#[tracing::instrument]
pub async fn add_chooser_page(
    Authenticated(user_info): Authenticated,
) -> Result<Response, anyhow::Error> {
    let layout = StandardLayout::new(
        "/add",
        AddChooserContent {
            logged_in: user_info.is_some(),
        },
    )
    .prepare(user_info.clone())
    .await?;

    AddChooserPage {
        head: Head::default().prepare(None).await?,
        layout,
    }
    .into_response(None)
    .await
}

// ---------------------------------------------------------------------------
// Add Event: option vocabularies
// ---------------------------------------------------------------------------

/// Time zones offered in the start-time picker. The page's script adds the
/// visitor's actual browser time zone if it isn't already listed and
/// preselects it, so this curated list only needs to cover common cases.
const COMMON_TIMEZONES: &[(&str, &str)] = &[
    ("America/New_York", "Eastern Time (US & Canada)"),
    ("America/Chicago", "Central Time (US & Canada)"),
    ("America/Denver", "Mountain Time (US & Canada)"),
    ("America/Phoenix", "Mountain Time - Arizona (no DST)"),
    ("America/Los_Angeles", "Pacific Time (US & Canada)"),
    ("America/Anchorage", "Alaska Time"),
    ("Pacific/Honolulu", "Hawaii Time"),
    ("America/Toronto", "Eastern Time - Toronto"),
    ("America/Mexico_City", "Central Time - Mexico City"),
    ("UTC", "UTC"),
    ("Europe/London", "London (GMT/BST)"),
    ("Europe/Paris", "Central European Time"),
    ("Europe/Berlin", "Central European Time - Berlin"),
    ("Africa/Johannesburg", "South Africa Standard Time"),
    ("Asia/Kolkata", "India Standard Time"),
    ("Asia/Shanghai", "China Standard Time"),
    ("Asia/Tokyo", "Japan Standard Time"),
    ("Australia/Sydney", "Australian Eastern Time"),
];

/// All event types, in the order they appear in the picker. The option value
/// is the type's `Display` string, which round-trips through `FromStr`.
const EVENT_TYPES: &[EventType] = &[
    EventType::BioBlitz,
    EventType::CitizenForum,
    EventType::CityNatureChallenge,
    EventType::Conference,
    EventType::Hackfest,
    EventType::InternationalMonarchMonitoringBlitz,
    EventType::KidFriendly,
    EventType::Library,
    EventType::Megathon,
    EventType::MuseumEvent,
    EventType::NatureMonitoring,
    EventType::Online,
    EventType::ProfessionalDevelopment,
    EventType::ScienceFestival,
    EventType::SocialMediaEvent,
    EventType::TalkOrScienceCafe,
    EventType::Training,
    EventType::Webinar,
    EventType::Ambassador,
    EventType::ActsOfScienceConnected,
    EventType::Other,
];

/// Where the event takes place. Every mode also permits an event URL, so a
/// venue or region event can carry a livestream/info link (covering the
/// hybrid case) without a dedicated mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum LocationMode {
    InPerson,
    Online,
    Region,
}

impl LocationMode {
    fn as_str(self) -> &'static str {
        match self {
            LocationMode::InPerson => "in_person",
            LocationMode::Online => "online",
            LocationMode::Region => "region",
        }
    }
}

/// How often a recurring event repeats. Expanded server-side into one
/// `EventInstance` per occurrence.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum RepeatFrequency {
    Daily,
    Weekly,
    Biweekly,
    Monthly,
}

impl RepeatFrequency {
    fn as_str(self) -> &'static str {
        match self {
            RepeatFrequency::Daily => "daily",
            RepeatFrequency::Weekly => "weekly",
            RepeatFrequency::Biweekly => "biweekly",
            RepeatFrequency::Monthly => "monthly",
        }
    }

    fn span(self) -> jiff::Span {
        use jiff::ToSpan;
        match self {
            RepeatFrequency::Daily => 1.day(),
            RepeatFrequency::Weekly => 1.week(),
            RepeatFrequency::Biweekly => 2.weeks(),
            RepeatFrequency::Monthly => 1.month(),
        }
    }
}

/// A recurrence is never expanded beyond this many occurrences per base
/// date/time, as a guard against runaway input.
const MAX_OCCURRENCES: usize = 366;

// ---------------------------------------------------------------------------
// Add Event: submitted form
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct AddEventForm {
    #[serde(default)]
    name: String,
    #[serde(default)]
    description: String,
    location_mode: Option<LocationMode>,
    #[serde(default)]
    location_name: String,
    #[serde(default)]
    location_description: String,
    #[serde(default)]
    address: String,
    #[serde(default)]
    city: String,
    #[serde(default)]
    state: String,
    #[serde(default)]
    zip: String,
    #[serde(default)]
    country: String,
    /// GeoJSON populated by the (forthcoming) reusable map-draw component.
    #[serde(default)]
    regions: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    event_type: String,
    #[serde(default)]
    presenter: String,
    #[serde(default)]
    image: String,
    #[serde(default)]
    image_credit: String,
    #[serde(default)]
    how_to_join: String,
    #[serde(default)]
    tags: String,
    /// ULID of an existing event this one is part of (chosen via the picker).
    #[serde(default)]
    part_of: String,
    #[serde(default)]
    contact_name: String,
    #[serde(default)]
    contact_affiliation: String,
    #[serde(default)]
    contact_email: String,
    #[serde(default)]
    contact_phone: String,
    #[serde(default)]
    contact_address: String,
    #[serde(default)]
    ticketing_date: String,
    #[serde(default)]
    ticketing_time: String,
    #[serde(default)]
    ticketing_timezone: String,
    /// Checkbox: present (any value) when the visitor enabled repetition.
    #[serde(default)]
    repeat_enabled: Option<String>,
    repeat_frequency: Option<RepeatFrequency>,
    #[serde(default)]
    repeat_until: String,
    #[serde(default)]
    instances: Vec<InstanceInput>,
}

#[derive(Debug, Deserialize)]
struct InstanceInput {
    #[serde(default)]
    begin_date: String,
    #[serde(default)]
    begin_time: String,
    #[serde(default)]
    timezone: String,
    /// Checkbox: present when the visitor added an end date/time.
    #[serde(default)]
    has_end: Option<String>,
    #[serde(default)]
    end_date: String,
    #[serde(default)]
    end_time: String,
}

/// A start (and optional end) resolved to concrete zoned instants, kept in the
/// original time zone so recurrence steps preserve local clock time across DST.
struct ZonedInstance {
    begin: Zoned,
    end: Option<Zoned>,
}

// ---------------------------------------------------------------------------
// Add Event: form-value carriers (for first render and re-render on error)
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
struct InstanceValues {
    begin_date: String,
    begin_time: String,
    timezone: String,
    has_end: bool,
    end_date: String,
    end_time: String,
}

#[derive(Debug, Default)]
struct EventFormValues {
    name: String,
    description: String,
    location_mode: String,
    location_name: String,
    location_description: String,
    address: String,
    city: String,
    state: String,
    zip: String,
    country: String,
    regions: String,
    url: String,
    event_type: String,
    presenter: String,
    image: String,
    image_credit: String,
    how_to_join: String,
    tags: String,
    part_of: String,
    part_of_name: String,
    contact_name: String,
    contact_affiliation: String,
    contact_email: String,
    contact_phone: String,
    contact_address: String,
    ticketing_date: String,
    ticketing_time: String,
    ticketing_timezone: String,
    repeat_enabled: bool,
    repeat_frequency: String,
    repeat_until: String,
    instances: Vec<InstanceValues>,
}

impl From<&AddEventForm> for EventFormValues {
    fn from(f: &AddEventForm) -> Self {
        let mut instances: Vec<InstanceValues> = f
            .instances
            .iter()
            .map(|i| InstanceValues {
                begin_date: i.begin_date.clone(),
                begin_time: i.begin_time.clone(),
                timezone: i.timezone.clone(),
                has_end: i.has_end.is_some(),
                end_date: i.end_date.clone(),
                end_time: i.end_time.clone(),
            })
            .collect();

        if instances.is_empty() {
            instances.push(InstanceValues::default());
        }

        EventFormValues {
            name: f.name.clone(),
            description: f.description.clone(),
            location_mode: f.location_mode.map(|m| m.as_str().to_string()).unwrap_or_default(),
            location_name: f.location_name.clone(),
            location_description: f.location_description.clone(),
            address: f.address.clone(),
            city: f.city.clone(),
            state: f.state.clone(),
            zip: f.zip.clone(),
            country: f.country.clone(),
            regions: f.regions.clone(),
            url: f.url.clone(),
            event_type: if f.event_type.is_empty() {
                EventType::Other.to_string()
            } else {
                f.event_type.clone()
            },
            presenter: f.presenter.clone(),
            image: f.image.clone(),
            image_credit: f.image_credit.clone(),
            how_to_join: f.how_to_join.clone(),
            tags: f.tags.clone(),
            part_of: f.part_of.trim().to_string(),
            part_of_name: String::new(),
            contact_name: f.contact_name.clone(),
            contact_affiliation: f.contact_affiliation.clone(),
            contact_email: f.contact_email.clone(),
            contact_phone: f.contact_phone.clone(),
            contact_address: f.contact_address.clone(),
            ticketing_date: f.ticketing_date.clone(),
            ticketing_time: f.ticketing_time.clone(),
            ticketing_timezone: f.ticketing_timezone.clone(),
            repeat_enabled: f.repeat_enabled.is_some(),
            repeat_frequency: f
                .repeat_frequency
                .map(|r| r.as_str().to_string())
                .unwrap_or_else(|| RepeatFrequency::Weekly.as_str().to_string()),
            repeat_until: f.repeat_until.clone(),
            instances,
        }
    }
}

#[derive(sailfish::TemplateOnce)]
#[template(path = "site/add_event.stpl.html")]
struct AddEventPage {
    head: <Head as Component>::Prepared,
    layout: <StandardLayout<AddEventContent> as Component>::Prepared,
}

#[derive(sailfish::TemplateOnce)]
#[template(path = "site/add_event_content.stpl.html")]
struct AddEventContent {
    error: Option<String>,
    values: EventFormValues,
    timezones: &'static [(&'static str, &'static str)],
    event_types: Vec<String>,
    region_editor: RegionEditor,
}

/// Reusable map-based area editor. Renders a MapLibre map plus the hidden
/// field it keeps in sync; the `js/region-editor.js` module (loaded by
/// `Head::with_region_editor`) wires up click-to-draw behavior. Drop this into
/// any form that needs to capture a `regions` geometry (e.g. the project form).
#[derive(sailfish::TemplateOnce)]
#[template(path = "site/region_editor.stpl.html")]
struct RegionEditor {
    /// Form field name the geometry is submitted under.
    field_name: String,
    /// DOM id of the hidden input (the map editor's `data-target`).
    field_id: String,
    /// Existing geometry as a GeoJSON string (empty for a fresh editor).
    initial: String,
}

// ---------------------------------------------------------------------------
// Add Event: handlers
// ---------------------------------------------------------------------------

/// GET `/add-event` — render the (empty) submission form. Submitting requires
/// authentication, so unauthenticated visitors are sent to log in first.
#[handler]
#[tracing::instrument]
pub async fn add_event_page(
    Authenticated(user_info): Authenticated,
) -> Result<Response, anyhow::Error> {
    if user_info.is_none() {
        return Ok(Redirect::see_other("/login?next=/add-event").into_response());
    }

    let mut values = EventFormValues::default();
    values.event_type = EventType::Other.to_string();
    values.repeat_frequency = RepeatFrequency::Weekly.as_str().to_string();
    values.instances.push(InstanceValues::default());

    render_event_form(None, values, user_info).await
}

/// POST `/add-event` — validate, create the event (unpublished), and redirect
/// to the confirmation page. On validation failure, re-render the form with
/// the entered values preserved.
#[handler]
#[tracing::instrument(skip(form))]
pub async fn add_event_submit(
    Authenticated(user_info): Authenticated,
    QsForm(form): QsForm<AddEventForm>,
) -> Result<Response, anyhow::Error> {
    let Some(user_info) = user_info else {
        return Ok(Redirect::see_other("/login?next=/add-event").into_response());
    };

    match validate_and_build(&form, user_info.user_id) {
        Ok(event) => {
            let name = event.name.clone();
            let event_id = event.event_id;

            // Persist (and distribute to peers) via LoadStorePeered.
            event.store().await?;

            // Create the page entity, unpublished — it becomes publicly
            // resolvable at /events/<slug> only after staff publish it.
            let entity =
                Entity::new_by_name(&name, EntityKind::Event, EntityTarget::Ulid(event_id)).await?;
            Entity::new_with_publish(
                entity.slug(),
                *entity.kind(),
                entity.target().clone(),
                false,
            )
            .store()
            .await?;

            log_activity(
                None,
                "event-create",
                user_info.user_id,
                event_id,
                format!("name={name}"),
            )
            .await?;

            // Notify staff that a submission is awaiting review. The submitter
            // is acknowledged by the confirmation page, so no email to them.
            trigger_email(
                TriggerEvent::StaffNotification,
                user_info.user_id,
                Some(&[
                    ("kind".to_string(), "event".to_string()),
                    ("event_name".to_string(), name.clone()),
                    ("event_id".to_string(), event_id.to_string()),
                ]),
            )
            .await
            .ok();

            Ok(Redirect::see_other(format!(
                "/add-event/submitted?name={}",
                urlencoding::encode(&name)
            ))
            .into_response())
        }
        Err(message) => {
            render_event_form(Some(message), EventFormValues::from(&form), Some(user_info)).await
        }
    }
}

async fn render_event_form(
    error: Option<String>,
    mut values: EventFormValues,
    user_info: Option<Arc<BasicInfo>>,
) -> Result<Response, anyhow::Error> {
    // Resolve the chosen parent event's name so the picker shows it on re-render.
    if !values.part_of.is_empty() && values.part_of_name.is_empty() {
        values.part_of_name = published_event_name(&values.part_of).await.unwrap_or_default();
    }

    let region_editor = RegionEditor {
        field_name: "regions".to_string(),
        field_id: "regions-geojson".to_string(),
        initial: values.regions.clone(),
    };

    let content = AddEventContent {
        error,
        values,
        timezones: COMMON_TIMEZONES,
        event_types: EVENT_TYPES.iter().map(|e| e.to_string()).collect(),
        region_editor,
    };

    let layout = StandardLayout::new("/add-event", content)
        .prepare(user_info)
        .await?;

    AddEventPage {
        head: Head::default().with_region_editor().prepare(None).await?,
        layout,
    }
    .into_response(None)
    .await
}

#[derive(Debug, Deserialize)]
struct SubmittedQuery {
    #[serde(default)]
    name: String,
}

#[derive(sailfish::TemplateOnce)]
#[template(path = "site/add_submitted.stpl.html")]
struct AddSubmittedPage {
    head: <Head as Component>::Prepared,
    layout: <StandardLayout<AddSubmittedContent> as Component>::Prepared,
}

#[derive(sailfish::TemplateOnce)]
#[template(path = "site/add_submitted_content.stpl.html")]
struct AddSubmittedContent {
    name: String,
}

/// GET `/add-event/submitted` — "thanks, pending review" confirmation page.
#[handler]
#[tracing::instrument]
pub async fn add_event_submitted_page(
    Authenticated(user_info): Authenticated,
    Query(query): Query<SubmittedQuery>,
) -> Result<Response, anyhow::Error> {
    let layout = StandardLayout::new("/add-event", AddSubmittedContent { name: query.name })
        .prepare(user_info)
        .await?;

    AddSubmittedPage {
        head: Head::default().prepare(None).await?,
        layout,
    }
    .into_response(None)
    .await
}

// ---------------------------------------------------------------------------
// Add Event: "part of a larger event" picker (HTMX)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct ParentSearchQuery {
    #[serde(default)]
    q: String,
}

#[derive(sailfish::TemplateOnce)]
#[template(path = "site/add_event_parent_results.stpl.html")]
struct ParentResults {
    events: Vec<(String, String)>,
}

#[derive(sailfish::TemplateOnce)]
#[template(path = "site/add_event_parent_selected.stpl.html")]
struct ParentSelected {
    id: String,
    name: String,
}

/// GET `/add-event/parent-search?q=` — list published events matching the query
/// as pickable options (only published events can serve as an umbrella).
#[handler]
#[tracing::instrument]
pub async fn add_event_parent_search(
    Query(query): Query<ParentSearchQuery>,
) -> Result<Response, anyhow::Error> {
    let q = query.q.trim();
    if q.is_empty() {
        return Ok(html_fragment(String::new()));
    }

    let pattern = format!("%{q}%");
    let events = sqlx::query!(
        r#"SELECT e.event_id AS "event_id!", e.name AS "name!"
           FROM events e
           JOIN entities ent ON ent.target = e.event_id AND ent.kind = 'Event'
           WHERE ent.publish = 1 AND e.name LIKE ?
           ORDER BY e.name
           LIMIT 10"#,
        pattern
    )
    .fetch_all(db())
    .await?
    .into_iter()
    .map(|r| (r.event_id, r.name))
    .collect();

    Ok(html_fragment(ParentResults { events }.render_once()?))
}

/// GET `/add-event/parent-select?id=` — record the chosen parent: returns the
/// "selected" display plus out-of-band updates for the hidden field and the
/// (now-cleared) results list.
#[handler]
#[tracing::instrument]
pub async fn add_event_parent_select(
    Query(query): Query<ParentSelectQuery>,
) -> Result<Response, anyhow::Error> {
    let name = published_event_name(&query.id).await.unwrap_or_default();
    let (id, name) = if name.is_empty() {
        (String::new(), String::new())
    } else {
        (query.id, name)
    };

    Ok(html_fragment(ParentSelected { id, name }.render_once()?))
}

#[derive(Debug, Deserialize)]
struct ParentSelectQuery {
    #[serde(default)]
    id: String,
}

/// GET `/add-event/parent-clear` — clear the chosen parent.
#[handler]
#[tracing::instrument]
pub async fn add_event_parent_clear() -> Result<Response, anyhow::Error> {
    Ok(html_fragment(
        ParentSelected {
            id: String::new(),
            name: String::new(),
        }
        .render_once()?,
    ))
}

fn html_fragment(body: String) -> Response {
    Response::builder().content_type("text/html").body(body)
}

/// Look up an event's name, but only if it is published.
async fn published_event_name(id: &str) -> Option<String> {
    let id = id.parse::<Ulid>().ok()?.to_string();
    sqlx::query_scalar!(
        r#"SELECT e.name
           FROM events e
           JOIN entities ent ON ent.target = e.event_id AND ent.kind = 'Event'
           WHERE ent.publish = 1 AND e.event_id = ?"#,
        id
    )
    .fetch_optional(db())
    .await
    .ok()
    .flatten()
}

// ---------------------------------------------------------------------------
// Add Event: validation + assembly (the event create core)
// ---------------------------------------------------------------------------

/// Validate the submitted form and assemble an `EventAll` ready to store.
/// Returns a user-facing message string on validation failure.
fn validate_and_build(form: &AddEventForm, user_id: Ulid) -> Result<EventAll, String> {
    let name = form.name.trim();
    if name.is_empty() {
        return Err("Please enter a name for your event.".to_string());
    }

    let description = form.description.trim();
    if description.is_empty() {
        return Err("Please add a description of your event.".to_string());
    }

    let mode = form
        .location_mode
        .ok_or_else(|| "Please choose where the event takes place.".to_string())?;

    // Resolve the date/time rows that carry a start.
    let mut base: Vec<ZonedInstance> = Vec::new();
    for inst in &form.instances {
        let begin_date = inst.begin_date.trim();
        let begin_time = inst.begin_time.trim();

        if begin_date.is_empty() && begin_time.is_empty() {
            continue; // an untouched row
        }
        if begin_date.is_empty() || begin_time.is_empty() {
            return Err("Each date/time needs both a date and a start time.".to_string());
        }
        if inst.timezone.trim().is_empty() {
            return Err("Please choose a time zone for each date/time.".to_string());
        }

        let begin = parse_datetime(begin_date, begin_time, inst.timezone.trim())?;

        let end = if inst.has_end.is_some()
            && !inst.end_date.trim().is_empty()
            && !inst.end_time.trim().is_empty()
        {
            let end =
                parse_datetime(inst.end_date.trim(), inst.end_time.trim(), inst.timezone.trim())?;
            if end < begin {
                return Err("An event's end time can't be before its start time.".to_string());
            }
            Some(end)
        } else {
            None
        };

        base.push(ZonedInstance { begin, end });
    }

    if base.is_empty() {
        return Err("Please add at least one date and start time.".to_string());
    }

    let instances = expand_recurrence(base, form)?;

    let url = form.url.trim().to_string();

    // Location-specific requirements.
    match mode {
        LocationMode::Online => {
            if url.is_empty() {
                return Err(
                    "Please provide a link where people can join the online event.".to_string(),
                );
            }
        }
        LocationMode::InPerson => {
            if form.address.trim().is_empty() && form.city.trim().is_empty() {
                return Err(
                    "Please provide an address or city for the in-person event.".to_string(),
                );
            }
        }
        LocationMode::Region => {
            if form.location_description.trim().is_empty() {
                return Err("Please describe the area the event covers.".to_string());
            }
        }
    }

    let mut event = EventAll::default();
    event.event_id = Ulid::new();
    event.created = TimeString::now();
    event.page_creator_id = user_id;
    event.origin = "scistarter".to_string();
    event.name = name.to_string();
    event.description = description.to_string();
    event.url = url;
    event.presenter = form.presenter.trim().to_string();
    event.image = form.image.trim().to_string();
    event.image_credit = form.image_credit.trim().to_string();
    event.how_to_join = form.how_to_join.trim().to_string();
    event.event_type = form.event_type.parse().unwrap_or_default();
    event.part_of = form.part_of.trim().parse::<Ulid>().ok();
    // A location note is meaningful in every mode.
    event.location_description = form.location_description.trim().to_string();

    // Optional ticketing/registration open date.
    let ticketing_date = form.ticketing_date.trim();
    if !ticketing_date.is_empty() {
        if form.ticketing_timezone.trim().is_empty() {
            return Err("Please choose a time zone for the ticketing date.".to_string());
        }
        let ticketing_time = form.ticketing_time.trim();
        let ticketing_time = if ticketing_time.is_empty() {
            "00:00"
        } else {
            ticketing_time
        };
        let begins = parse_datetime(ticketing_date, ticketing_time, form.ticketing_timezone.trim())?;
        event.ticketing_begins = Some(TimeString::from(begins.timestamp()));
    }
    event.tags = form
        .tags
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(InsensitiveString::from)
        .collect();

    // A single organizer contact, if one was provided.
    let contact_email = form.contact_email.trim();
    if !contact_email.is_empty() || !form.contact_name.trim().is_empty() {
        event.contacts = Box::new([Contact {
            contact_id: Ulid::new(),
            role: ContactRole::ContactPerson,
            name: empty_to_none(&form.contact_name),
            affiliation: empty_to_none(&form.contact_affiliation),
            email: contact_email.to_string(),
            phone: empty_to_none(&form.contact_phone),
            address: empty_to_none(&form.contact_address),
        }]);
    }

    match mode {
        LocationMode::Online => {
            event.entirely_online = true;
            event.location_type = "online".to_string();
        }
        LocationMode::InPerson => {
            event.entirely_online = false;
            event.location_type = "in_person".to_string();
            event.location_name = form.location_name.trim().to_string();
            event.address = form.address.trim().to_string();
            event.city = form.city.trim().to_string();
            event.state = form.state.trim().to_string();
            event.zip = form.zip.trim().to_string();
            event.country = form.country.trim().to_string();
        }
        LocationMode::Region => {
            event.entirely_online = false;
            event.location_type = "region".to_string();
            event.city = form.city.trim().to_string();
            event.state = form.state.trim().to_string();
            event.country = form.country.trim().to_string();
            let regions = form.regions.trim();
            if !regions.is_empty() {
                event.regions = Some(regions.to_string());
            }
        }
    }

    event.instances = instances.into_boxed_slice();

    Ok(event)
}

fn empty_to_none(s: &str) -> Option<String> {
    let s = s.trim();
    if s.is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}

/// Expand each base date/time into one or more `EventInstance`s according to
/// the optional recurrence settings.
fn expand_recurrence(
    base: Vec<ZonedInstance>,
    form: &AddEventForm,
) -> Result<Vec<EventInstance>, String> {
    let recurring = form.repeat_enabled.is_some();
    let frequency = if recurring { form.repeat_frequency } else { None };
    let until = if recurring && !form.repeat_until.trim().is_empty() {
        Some(parse_date(form.repeat_until.trim())?)
    } else {
        None
    };

    let mut out: Vec<EventInstance> = Vec::new();

    for inst in base {
        match (frequency, until) {
            (Some(frequency), Some(until)) => {
                let mut begin = inst.begin.clone();
                let mut end = inst.end.clone();
                let mut count = 0usize;

                while begin.date() <= until {
                    out.push(to_event_instance(&begin, end.as_ref()));
                    count += 1;
                    if count >= MAX_OCCURRENCES {
                        tracing::warn!(
                            "Add Event recurrence hit the {MAX_OCCURRENCES}-occurrence cap; truncating."
                        );
                        break;
                    }

                    let span = frequency.span();
                    begin = begin
                        .checked_add(span)
                        .map_err(|_| "Couldn't compute the repeat schedule.".to_string())?;
                    end = match end {
                        Some(e) => Some(
                            e.checked_add(span)
                                .map_err(|_| "Couldn't compute the repeat schedule.".to_string())?,
                        ),
                        None => None,
                    };
                }
            }
            _ => out.push(to_event_instance(&inst.begin, inst.end.as_ref())),
        }
    }

    Ok(out)
}

/// An instance with no explicit end is stored as a zero-duration occurrence
/// (end == begin), matching how point-in-time events are represented.
fn to_event_instance(begin: &Zoned, end: Option<&Zoned>) -> EventInstance {
    EventInstance {
        begin: TimeString::from(begin.timestamp()),
        end: TimeString::from(end.unwrap_or(begin).timestamp()),
    }
}

/// Combine a `YYYY-MM-DD` date, an `HH:MM` (or `HH:MM:SS`) time, and an IANA
/// time-zone name into a zoned instant.
fn parse_datetime(date: &str, time: &str, timezone: &str) -> Result<Zoned, String> {
    let tz = TimeZone::get(timezone)
        .map_err(|_| format!("Unrecognized time zone \u{201c}{timezone}\u{201d}."))?;

    let time = if time.len() == 5 {
        format!("{time}:00")
    } else {
        time.to_string()
    };

    let datetime: civil::DateTime = format!("{date}T{time}")
        .parse()
        .map_err(|_| "Please enter a valid date and time.".to_string())?;

    datetime
        .to_zoned(tz)
        .map_err(|e| format!("That date and time isn't valid in the chosen time zone: {e}"))
}

fn parse_date(date: &str) -> Result<civil::Date, String> {
    civil::Date::strptime("%Y-%m-%d", date).map_err(|_| "Please enter a valid date.".to_string())
}
