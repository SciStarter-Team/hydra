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
    project::all::ProjectAll,
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
enum RepeatFrequency {
    /// The event happens once (no recurrence). This is the default.
    #[default]
    Once,
    Daily,
    Weekly,
    Biweekly,
    Monthly,
}

impl RepeatFrequency {
    fn as_str(self) -> &'static str {
        match self {
            RepeatFrequency::Once => "once",
            RepeatFrequency::Daily => "daily",
            RepeatFrequency::Weekly => "weekly",
            RepeatFrequency::Biweekly => "biweekly",
            RepeatFrequency::Monthly => "monthly",
        }
    }

    /// The recurrence step. Only meaningful for the repeating variants;
    /// `Once` never reaches `expand_recurrence`'s stepping branch.
    fn span(self) -> jiff::Span {
        use jiff::ToSpan;
        match self {
            RepeatFrequency::Daily => 1.day(),
            RepeatFrequency::Weekly => 1.week(),
            RepeatFrequency::Biweekly => 2.weeks(),
            RepeatFrequency::Monthly => 1.month(),
            RepeatFrequency::Once => 1.day(),
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
            repeat_frequency: f
                .repeat_frequency
                .unwrap_or_default()
                .as_str()
                .to_string(),
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
    location: LocationPicker,
    image: ImageField,
    tags: TagInput,
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
// Shared add-form components (used by both /add-event and /add-project)
// ---------------------------------------------------------------------------

/// The location section: in-person / online / region radios, the per-mode
/// fields, the region editor, a free-text location note, and the link/URL.
/// Backed by `js/location-picker.js`.
#[derive(sailfish::TemplateOnce)]
#[template(path = "site/location_picker.stpl.html")]
struct LocationPicker {
    mode: String,
    location_name: String,
    address: String,
    city: String,
    state: String,
    zip: String,
    country: String,
    location_description: String,
    url: String,
    region_editor: RegionEditor,
}

impl LocationPicker {
    /// Build from the common location fields shared by both forms.
    fn build(
        mode: &str,
        location_name: &str,
        address: &str,
        city: &str,
        state: &str,
        zip: &str,
        country: &str,
        location_description: &str,
        url: &str,
        regions_geojson: &str,
    ) -> Self {
        LocationPicker {
            mode: mode.to_string(),
            location_name: location_name.to_string(),
            address: address.to_string(),
            city: city.to_string(),
            state: state.to_string(),
            zip: zip.to_string(),
            country: country.to_string(),
            location_description: location_description.to_string(),
            url: url.to_string(),
            region_editor: RegionEditor {
                field_name: "regions".to_string(),
                field_id: "regions-geojson".to_string(),
                initial: regions_geojson.to_string(),
            },
        }
    }
}

/// The image URL field with an upload control + credit, posting to
/// `/add/upload-image`.
#[derive(sailfish::TemplateOnce)]
#[template(path = "site/image_field.stpl.html")]
struct ImageField {
    value: String,
    credit: String,
}

/// The tag-pill widget with HTMX autocomplete, backed by `js/tags-input.js`.
#[derive(sailfish::TemplateOnce)]
#[template(path = "site/tag_input.stpl.html")]
struct TagInput {
    value: String,
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
    values.repeat_frequency = RepeatFrequency::Once.as_str().to_string();
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

    let location = LocationPicker::build(
        &values.location_mode,
        &values.location_name,
        &values.address,
        &values.city,
        &values.state,
        &values.zip,
        &values.country,
        &values.location_description,
        &values.url,
        &values.regions,
    );
    let image = ImageField {
        value: values.image.clone(),
        credit: values.image_credit.clone(),
    };
    let tags = TagInput {
        value: values.tags.clone(),
    };

    let content = AddEventContent {
        error,
        values,
        timezones: COMMON_TIMEZONES,
        event_types: EVENT_TYPES.iter().map(|e| e.to_string()).collect(),
        location,
        image,
        tags,
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
    /// The submitted thing's noun, e.g. "event" or "project".
    kind: String,
}

async fn render_submitted(
    user_info: Option<Arc<BasicInfo>>,
    path: &'static str,
    name: String,
    kind: &str,
) -> Result<Response, anyhow::Error> {
    let layout = StandardLayout::new(
        path,
        AddSubmittedContent {
            name,
            kind: kind.to_string(),
        },
    )
    .prepare(user_info)
    .await?;

    AddSubmittedPage {
        head: Head::default().prepare(None).await?,
        layout,
    }
    .into_response(None)
    .await
}

/// GET `/add-event/submitted` — "thanks, pending review" confirmation page.
#[handler]
#[tracing::instrument]
pub async fn add_event_submitted_page(
    Authenticated(user_info): Authenticated,
    Query(query): Query<SubmittedQuery>,
) -> Result<Response, anyhow::Error> {
    render_submitted(user_info, "/add-event", query.name, "event").await
}

/// GET `/add-project/submitted` — "thanks, pending review" confirmation page.
#[handler]
#[tracing::instrument]
pub async fn add_project_submitted_page(
    Authenticated(user_info): Authenticated,
    Query(query): Query<SubmittedQuery>,
) -> Result<Response, anyhow::Error> {
    render_submitted(user_info, "/add-project", query.name, "project").await
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
// Add Event: tag autocomplete (HTMX) + image upload
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct TagSearchQuery {
    #[serde(default)]
    q: String,
}

#[derive(sailfish::TemplateOnce)]
#[template(path = "site/add_tag_suggestions.stpl.html")]
struct TagSuggestions {
    tags: Vec<String>,
}

/// GET `/add/tag-search?q=` — suggest existing tags (across both projects and
/// events) to reduce duplication. Shared by both add forms.
#[handler]
#[tracing::instrument]
pub async fn add_tag_search(Query(query): Query<TagSearchQuery>) -> Result<Response, anyhow::Error> {
    let q = query.q.trim();
    if q.is_empty() {
        return Ok(html_fragment(String::new()));
    }

    let pattern = format!("%{q}%");
    let tags = sqlx::query_scalar!(
        r#"SELECT DISTINCT tag AS "tag!" FROM (
               SELECT tag FROM event_tags
               UNION
               SELECT tag FROM project_tags
           ) WHERE tag LIKE ? ORDER BY tag LIMIT 10"#,
        pattern
    )
    .fetch_all(db())
    .await?;

    if tags.is_empty() {
        return Ok(html_fragment(String::new()));
    }

    Ok(html_fragment(TagSuggestions { tags }.render_once()?))
}

#[derive(sailfish::TemplateOnce)]
#[template(path = "site/add_image_uploaded.stpl.html")]
struct ImageUploadResult {
    url: String,
}

/// POST `/add/upload-image` — store an uploaded image (untracked) and return
/// out-of-band updates setting the shared image URL field + preview. Shared by
/// both add forms.
#[handler]
#[tracing::instrument(skip(multipart))]
pub async fn add_upload_image(
    Authenticated(user_info): Authenticated,
    mut multipart: poem::web::Multipart,
) -> Result<Response, anyhow::Error> {
    use tokio::io::AsyncReadExt;

    let user = user_info.ok_or_else(|| anyhow::anyhow!("Authentication required"))?;

    while let Some(field) = multipart.next_field().await? {
        if field.name() != Some("file") {
            continue;
        }

        let filename = field.file_name().map(|s| s.to_string());
        let content_type = field.content_type().map(|s| s.to_string());
        let file_id = Ulid::new();

        let mut data = Vec::new();
        field.into_async_read().read_to_end(&mut data).await?;

        let metadata = crate::upload::storage::FileMetadata {
            file_id,
            original_filename: filename,
            content_type,
            size: data.len() as u64,
            uploaded_by: user.user_id,
            access: crate::upload::storage::FileAccess::Untracked,
            variant: None,
            force_download: false,
        };
        crate::upload::STORAGE.upload_file(metadata, data).await?;

        let url = format!("/-uploaded/{file_id}");
        return Ok(html_fragment(ImageUploadResult { url }.render_once()?));
    }

    Err(anyhow::anyhow!("No file provided"))
}

#[derive(Debug, Deserialize)]
struct RegionLookupQuery {
    #[serde(default)]
    q: String,
}

/// GET `/add/region-lookup?q=` — resolve a place name (city / state / country)
/// to a simplified boundary polygon via OpenStreetMap Nominatim, for the region
/// editor's "look up a place" mode. Returns the GeoJSON geometry, or `{}` when
/// there's no match. Shared by both add forms (region editor).
#[handler]
#[tracing::instrument]
pub async fn add_region_lookup(
    Authenticated(user_info): Authenticated,
    Query(query): Query<RegionLookupQuery>,
) -> Result<Response, anyhow::Error> {
    if user_info.is_none() {
        return Ok(Response::builder()
            .status(poem::http::StatusCode::UNAUTHORIZED)
            .finish());
    }

    let q = query.q.trim();
    if q.is_empty() {
        return Ok(Response::builder()
            .content_type("application/json")
            .body("{}"));
    }

    // Nominatim returns the administrative boundary polygon when
    // `polygon_geojson=1`; `polygon_threshold` applies Douglas-Peucker
    // simplification (in degrees) so the result is a manageable shape.
    let results: Vec<serde_json::Value> = reqwest::Client::new()
        .get("https://nominatim.openstreetmap.org/search")
        .query(&[
            ("q", q),
            ("format", "jsonv2"),
            ("polygon_geojson", "1"),
            ("polygon_threshold", "0.01"),
            ("limit", "1"),
        ])
        .header("User-Agent", "SciStarter/1.0 (+https://scistarter.org)")
        .header("Accept-Language", "en")
        .send()
        .await?
        .json()
        .await?;

    let geometry = results
        .into_iter()
        .next()
        .and_then(|r| r.get("geojson").cloned())
        .filter(|g| g.get("coordinates").is_some());

    let body = match geometry {
        Some(g) => serde_json::to_string(&g)?,
        None => "{}".to_string(),
    };

    Ok(Response::builder()
        .content_type("application/json")
        .body(body))
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
            // A region is defined by the drawn polygon + the location note; it
            // doesn't carry a single city/state/country.
            event.entirely_online = false;
            event.location_type = "region".to_string();
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

/// Keep only submitted values that are part of the controlled vocabulary, so a
/// forged request can't store arbitrary categorization values.
fn filter_to_vocabulary(values: &[String], vocabulary: &[&str]) -> Box<[String]> {
    values
        .iter()
        .filter(|v| vocabulary.contains(&v.as_str()))
        .cloned()
        .collect()
}

/// Expand each base date/time into one or more `EventInstance`s according to
/// the optional recurrence settings.
fn expand_recurrence(
    base: Vec<ZonedInstance>,
    form: &AddEventForm,
) -> Result<Vec<EventInstance>, String> {
    let frequency = form.repeat_frequency.unwrap_or_default();
    let recurring = frequency != RepeatFrequency::Once;
    let until = if recurring && !form.repeat_until.trim().is_empty() {
        Some(parse_date(form.repeat_until.trim())?)
    } else {
        None
    };

    let mut out: Vec<EventInstance> = Vec::new();

    for inst in base {
        match until {
            Some(until) if recurring => {
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

// ---------------------------------------------------------------------------
// Add Project
//
// Mirrors Add Event, adapted to project data: no dates/recurrence/ticketing,
// adds scientific goal + participant task. Reuses the shared location picker,
// image field, tag widget, and region editor, and the shared create core
// (`project::all::persist_new_project`, also used by the admin add handler).
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct AddProjectForm {
    #[serde(default)]
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    presenter: String,
    #[serde(default)]
    goal: String,
    #[serde(default)]
    task: String,
    #[serde(default)]
    how_to_join: String,
    #[serde(default)]
    image: String,
    #[serde(default)]
    image_credit: String,
    #[serde(default)]
    tags: String,
    #[serde(default)]
    topics: Vec<String>,
    #[serde(default)]
    fields_of_science: Vec<String>,
    #[serde(default)]
    audience: Vec<String>,
    #[serde(default)]
    activities: Vec<String>,
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
    #[serde(default)]
    regions: String,
    #[serde(default)]
    url: String,
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
}

#[derive(Debug, Default)]
struct ProjectFormValues {
    name: String,
    description: String,
    presenter: String,
    goal: String,
    task: String,
    how_to_join: String,
    image: String,
    image_credit: String,
    tags: String,
    topics: Vec<String>,
    fields_of_science: Vec<String>,
    audience: Vec<String>,
    activities: Vec<String>,
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
    contact_name: String,
    contact_affiliation: String,
    contact_email: String,
    contact_phone: String,
    contact_address: String,
}

impl From<&AddProjectForm> for ProjectFormValues {
    fn from(f: &AddProjectForm) -> Self {
        ProjectFormValues {
            name: f.name.clone(),
            description: f.description.clone(),
            presenter: f.presenter.clone(),
            goal: f.goal.clone(),
            task: f.task.clone(),
            how_to_join: f.how_to_join.clone(),
            image: f.image.clone(),
            image_credit: f.image_credit.clone(),
            tags: f.tags.clone(),
            topics: f.topics.clone(),
            fields_of_science: f.fields_of_science.clone(),
            audience: f.audience.clone(),
            activities: f.activities.clone(),
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
            contact_name: f.contact_name.clone(),
            contact_affiliation: f.contact_affiliation.clone(),
            contact_email: f.contact_email.clone(),
            contact_phone: f.contact_phone.clone(),
            contact_address: f.contact_address.clone(),
        }
    }
}

#[derive(sailfish::TemplateOnce)]
#[template(path = "site/add_project.stpl.html")]
struct AddProjectPage {
    head: <Head as Component>::Prepared,
    layout: <StandardLayout<AddProjectContent> as Component>::Prepared,
}

#[derive(sailfish::TemplateOnce)]
#[template(path = "site/add_project_content.stpl.html")]
struct AddProjectContent {
    error: Option<String>,
    values: ProjectFormValues,
    location: LocationPicker,
    image: ImageField,
    tags: TagInput,
}

/// GET `/add-project` — render the (empty) submission form (login required).
#[handler]
#[tracing::instrument]
pub async fn add_project_page(
    Authenticated(user_info): Authenticated,
) -> Result<Response, anyhow::Error> {
    if user_info.is_none() {
        return Ok(Redirect::see_other("/login?next=/add-project").into_response());
    }
    render_project_form(None, ProjectFormValues::default(), user_info).await
}

/// POST `/add-project` — validate, create the project (draft + unpublished),
/// and redirect to the confirmation page.
#[handler]
#[tracing::instrument(skip(form))]
pub async fn add_project_submit(
    Authenticated(user_info): Authenticated,
    QsForm(form): QsForm<AddProjectForm>,
) -> Result<Response, anyhow::Error> {
    let Some(user_info) = user_info else {
        return Ok(Redirect::see_other("/login?next=/add-project").into_response());
    };

    match validate_and_build_project(&form, user_info.user_id) {
        Ok(project) => {
            let name = project.name.clone();
            // Public submissions stay unpublished pending review.
            crate::project::all::persist_new_project(project, false, user_info.user_id).await?;
            Ok(Redirect::see_other(format!(
                "/add-project/submitted?name={}",
                urlencoding::encode(&name)
            ))
            .into_response())
        }
        Err(message) => {
            render_project_form(Some(message), ProjectFormValues::from(&form), Some(user_info)).await
        }
    }
}

async fn render_project_form(
    error: Option<String>,
    values: ProjectFormValues,
    user_info: Option<Arc<BasicInfo>>,
) -> Result<Response, anyhow::Error> {
    let location = LocationPicker::build(
        &values.location_mode,
        &values.location_name,
        &values.address,
        &values.city,
        &values.state,
        &values.zip,
        &values.country,
        &values.location_description,
        &values.url,
        &values.regions,
    );
    let image = ImageField {
        value: values.image.clone(),
        credit: values.image_credit.clone(),
    };
    let tags = TagInput {
        value: values.tags.clone(),
    };

    let content = AddProjectContent {
        error,
        values,
        location,
        image,
        tags,
    };

    let layout = StandardLayout::new("/add-project", content)
        .prepare(user_info)
        .await?;

    AddProjectPage {
        head: Head::default().with_region_editor().prepare(None).await?,
        layout,
    }
    .into_response(None)
    .await
}

/// Validate the submitted project form and assemble a `ProjectAll`. Returns a
/// user-facing message string on validation failure.
fn validate_and_build_project(form: &AddProjectForm, user_id: Ulid) -> Result<ProjectAll, String> {
    let name = form.name.trim();
    if name.is_empty() {
        return Err("Please enter a name for your project.".to_string());
    }

    let description = form.description.trim();
    if description.is_empty() {
        return Err("Please add a description of your project.".to_string());
    }

    let mode = form
        .location_mode
        .ok_or_else(|| "Please choose where the project takes place.".to_string())?;

    let url = form.url.trim().to_string();
    match mode {
        LocationMode::Online => {
            if url.is_empty() {
                return Err("Please provide a link for the online project.".to_string());
            }
        }
        LocationMode::InPerson => {
            if form.address.trim().is_empty() && form.city.trim().is_empty() {
                return Err(
                    "Please provide an address or city for the in-person project.".to_string(),
                );
            }
        }
        LocationMode::Region => {
            if form.location_description.trim().is_empty() {
                return Err("Please describe the area the project covers.".to_string());
            }
        }
    }

    let mut project = ProjectAll::default();
    project.project_id = Ulid::new();
    project.created = TimeString::now();
    project.page_creator_id = user_id;
    project.origin = "scistarter".to_string();
    project.authoritative = true;
    project.status = crate::project::ProjectStatus::Active;
    project.draft = true;
    project.name = name.to_string();
    project.description = description.to_string();
    project.presenter = form.presenter.trim().to_string();
    project.goal = form.goal.trim().to_string();
    project.task = form.task.trim().to_string();
    project.how_to_join = form.how_to_join.trim().to_string();
    project.url = url;
    project.image = form.image.trim().to_string();
    project.image_credit = form.image_credit.trim().to_string();
    project.location_description = form.location_description.trim().to_string();
    project.tags = form
        .tags
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(InsensitiveString::from)
        .collect();

    use crate::project::field::{
        ACTIVITIES_VOCABULARY, AUDIENCE_VOCABULARY, FIELDS_OF_SCIENCE_VOCABULARY, TOPICS_VOCABULARY,
    };
    project.topics = filter_to_vocabulary(&form.topics, TOPICS_VOCABULARY);
    project.fields_of_science =
        filter_to_vocabulary(&form.fields_of_science, FIELDS_OF_SCIENCE_VOCABULARY);
    project.audience = filter_to_vocabulary(&form.audience, AUDIENCE_VOCABULARY);
    project.activities = filter_to_vocabulary(&form.activities, ACTIVITIES_VOCABULARY);

    let contact_email = form.contact_email.trim();
    if !contact_email.is_empty() || !form.contact_name.trim().is_empty() {
        project.contacts = Box::new([Contact {
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
            project.entirely_online = true;
            project.location_type = "Global, anywhere on the planet".to_string();
        }
        LocationMode::InPerson => {
            project.entirely_online = false;
            project.in_person = true;
            project.location_type = "At a specific address".to_string();
            project.location_name = form.location_name.trim().to_string();
            project.address = form.address.trim().to_string();
            project.city = form.city.trim().to_string();
            project.state = form.state.trim().to_string();
            project.zip = form.zip.trim().to_string();
            project.country = form.country.trim().to_string();
        }
        LocationMode::Region => {
            // A region is defined by the drawn polygon + the location note; it
            // doesn't carry a single city/state/country.
            project.entirely_online = false;
            project.location_type = "In a geographic location or range".to_string();
            let regions = form.regions.trim();
            if !regions.is_empty() {
                project.regions = Some(regions.to_string());
            }
        }
    }

    Ok(project)
}
