use std::sync::Arc;

use jiff::{Zoned, civil, tz::TimeZone};
use poem::{
    IntoResponse, Response, handler,
    web::{Query, Redirect},
};
use sailfish::TemplateOnce;
use serde::{Deserialize, Serialize};
use ulid::Ulid;

use common::{
    insensitivestring::InsensitiveString,
    message::location::{GeocodeReverseRequest, GeocodeReverseResponse, Point},
    message::user::BasicInfo,
    service::{TriggerEvent, log_activity, trigger_email},
    shared::db,
    timestring::TimeString,
};

use crate::{
    component::{Component, Head, StandardLayout, image_field::ImageField},
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

/// Every IANA time zone, grouped by region for the picker's `<optgroup>`s.
/// The curated `COMMON_TIMEZONES` stay at the top of the select; these groups
/// make the rest of the world reachable. Entries are `(value, label)` where
/// the label drops the region prefix and underscores ("America/Argentina/
/// Buenos_Aires" -> "Argentina/Buenos Aires" under "America"). Built once.
fn all_timezones() -> &'static [(String, Vec<(String, String)>)] {
    static ZONES: std::sync::OnceLock<Vec<(String, Vec<(String, String)>)>> =
        std::sync::OnceLock::new();
    ZONES.get_or_init(|| {
        const REGIONS: &[&str] = &[
            "Africa",
            "America",
            "Antarctica",
            "Asia",
            "Atlantic",
            "Australia",
            "Europe",
            "Indian",
            "Pacific",
        ];
        let mut groups: Vec<(String, Vec<(String, String)>)> = REGIONS
            .iter()
            .map(|region| (region.to_string(), Vec::new()))
            .collect();

        let mut names: Vec<String> = jiff::tz::db()
            .available()
            .map(|name| name.to_string())
            .collect();
        names.sort();

        for name in names {
            // Region-less names (UTC, EST5EDT, ...) and non-geographic
            // regions (Etc/*, US/*, ...) are aliases or legacy entries; the
            // canonical geographic zones all live under the regions above.
            let Some((region, rest)) = name.split_once('/') else {
                continue;
            };
            if let Some(group) = groups.iter_mut().find(|(r, _)| r == region) {
                let label = rest.replace('_', " ");
                group.1.push((name.clone(), label));
            }
        }
        groups
    })
}

/// All event types, in the order they appear in the picker. The option value
/// is the type's `Display` string, which round-trips through `FromStr`.
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

/// Deserialize an optional field, treating an empty string as `None`. The
/// location-mode select submits an empty value while its "Is it in person or
/// virtual?" placeholder is still selected.
fn empty_as_none<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::de::DeserializeOwned,
{
    let value: Option<String> = Option::deserialize(deserializer)?;
    match value.as_deref() {
        None | Some("") => Ok(None),
        Some(s) => {
            T::deserialize(serde::de::value::StrDeserializer::<serde::de::value::Error>::new(s))
                .map(Some)
                .map_err(serde::de::Error::custom)
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
    #[serde(default, deserialize_with = "empty_as_none")]
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
    /// Coordinates of the place chosen in the location picker's type-ahead
    /// search, when one was picked (empty for a hand-entered address).
    #[serde(default)]
    latitude: String,
    #[serde(default)]
    longitude: String,
    /// GeoJSON populated by the reusable map-draw component.
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
    latitude: String,
    longitude: String,
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
            location_mode: f
                .location_mode
                .map(|m| m.as_str().to_string())
                .unwrap_or_default(),
            location_name: f.location_name.clone(),
            location_description: f.location_description.clone(),
            address: f.address.clone(),
            city: f.city.clone(),
            state: f.state.clone(),
            zip: f.zip.clone(),
            country: f.country.clone(),
            latitude: f.latitude.clone(),
            longitude: f.longitude.clone(),
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
            repeat_frequency: f.repeat_frequency.unwrap_or_default().as_str().to_string(),
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

/// The location section: an "in person or virtual?" mode select, a place
/// type-ahead search backed by `/add/place-search` with the structured address
/// fields behind a details disclosure, the region editor (revealed on request),
/// a free-text location note, and the link/URL. Backed by
/// `js/location-picker.js`.
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
    latitude: String,
    longitude: String,
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
        latitude: &str,
        longitude: &str,
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
            latitude: latitude.to_string(),
            longitude: longitude.to_string(),
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

/// The time-zone control for one date/time row: the zone in effect stated in a
/// line of prose, with the full picker collapsed behind it. Zones are inferred
/// from the picked venue or the browser, so the list is there to correct an
/// inference rather than to ask a question. Rendered open when nothing is
/// selected yet, which is what a visitor without JavaScript gets.
#[derive(sailfish::TemplateOnce)]
#[template(path = "site/timezone_field.stpl.html")]
struct TimezoneField {
    field_name: String,
    selected: String,
    selected_label: String,
    required: bool,
    timezones: &'static [(&'static str, &'static str)],
    all_timezones: &'static [(String, Vec<(String, String)>)],
}

impl TimezoneField {
    /// Build the control for `field_name`, preselecting `selected`.
    fn build(field_name: impl Into<String>, selected: &str, required: bool) -> Self {
        TimezoneField {
            field_name: field_name.into(),
            selected: selected.to_string(),
            selected_label: timezone_label(selected),
            required,
            timezones: COMMON_TIMEZONES,
            all_timezones: all_timezones(),
        }
    }
}

/// Display label for an IANA zone: the curated name where there is one, the
/// grouped name otherwise, and the identifier itself as a last resort (which is
/// also what a browser-supplied zone missing from the list renders as).
fn timezone_label(tz: &str) -> String {
    if tz.is_empty() {
        return String::new();
    }

    if let Some((_, label)) = COMMON_TIMEZONES.iter().find(|(value, _)| *value == tz) {
        return (*label).to_string();
    }

    for (region, zones) in all_timezones() {
        if let Some((_, label)) = zones.iter().find(|(value, _)| value == tz) {
            return format!("{region}/{label}");
        }
    }

    tz.to_string()
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
        values.part_of_name = published_event_name(&values.part_of)
            .await
            .unwrap_or_default();
    }

    let location = LocationPicker::build(
        &values.location_mode,
        &values.location_name,
        &values.address,
        &values.city,
        &values.state,
        &values.zip,
        &values.country,
        &values.latitude,
        &values.longitude,
        &values.location_description,
        &values.url,
        &values.regions,
    );
    let image = ImageField {
        label: "Image".to_string(),
        value: values.image.clone(),
        credit: Some(values.image_credit.clone()),
    };
    let tags = TagInput {
        value: values.tags.clone(),
    };

    let content = AddEventContent {
        error,
        values,
        event_types: EventType::ALL.iter().map(|e| e.to_string()).collect(),
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
pub async fn add_tag_search(
    Query(query): Query<TagSearchQuery>,
) -> Result<Response, anyhow::Error> {
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

#[derive(Debug, Deserialize)]
struct PlaceSearchQuery {
    #[serde(default)]
    q: String,
}

/// One suggestion returned by `/add/place-search`: a display label plus the
/// structured address components the location picker copies into the form.
#[derive(Debug, Serialize)]
struct PlaceSuggestion {
    /// The suggestion's primary line (place or venue name).
    name: String,
    /// A short label for what the place is ("City", "County", "Museum") —
    /// often the only thing telling same-named suggestions apart. May be
    /// empty. Display-only: the picker shows it but never stores it.
    kind: String,
    /// The secondary line: the locality context ("Beach Haven West, NJ, USA").
    context: String,
    /// The IANA time zone at the place's coordinates (empty when unknown).
    /// Used to default the event form's time-zone selects on pick — a venue
    /// pinpoints its zone, unlike a drawn region, which may span several and
    /// deliberately gets no inference.
    timezone: String,
    /// Venue name for the form's `location_name` (empty for a plain locality).
    location_name: String,
    address: String,
    city: String,
    state: String,
    zip: String,
    country: String,
    latitude: f64,
    longitude: f64,
}

/// GET `/add/place-search?q=` — type-ahead place and address search for the
/// location picker, via the Photon geocoder (OSM data; Photon permits
/// autocomplete-style traffic, which Nominatim's usage policy does not).
/// Returns a JSON array of `PlaceSuggestion`s. Shared by both add forms.
#[handler]
#[tracing::instrument]
pub async fn add_place_search(
    Authenticated(user_info): Authenticated,
    Query(query): Query<PlaceSearchQuery>,
) -> Result<Response, anyhow::Error> {
    if user_info.is_none() {
        return Ok(Response::builder()
            .status(poem::http::StatusCode::UNAUTHORIZED)
            .finish());
    }

    let q = query.q.trim();
    if q.len() < 3 {
        return Ok(Response::builder()
            .content_type("application/json")
            .body("[]"));
    }

    // Over-fetch: OSM often carries same-named entries (a city and its
    // county, a node and its relation) that render identically and are
    // deduplicated below, so ask for more than we intend to show.
    let response: serde_json::Value = reqwest::Client::new()
        .get("https://photon.komoot.io/api")
        .query(&[("q", q), ("limit", "10"), ("lang", "en")])
        .header("User-Agent", "SciStarter/1.0 (+https://scistarter.org)")
        .send()
        .await?
        .json()
        .await?;

    let mut suggestions: Vec<PlaceSuggestion> = response
        .get("features")
        .and_then(|f| f.as_array())
        .map(|features| features.iter().filter_map(photon_suggestion).collect())
        .unwrap_or_default();

    // Drop suggestions the visitor couldn't tell apart: same name, kind, and
    // context. Photon ranks better matches first, so keep the first of each.
    let mut seen = std::collections::HashSet::new();
    suggestions.retain(|s| seen.insert((s.name.clone(), s.kind.clone(), s.context.clone())));
    suggestions.truncate(5);

    Ok(Response::builder()
        .content_type("application/json")
        .body(serde_json::to_string(&suggestions)?))
}

/// The IANA time zone at a coordinate, from tzf-rs's embedded boundary data
/// (empty when the lookup has no answer). The finder builds its spatial index
/// once, on first use.
fn timezone_at(latitude: f64, longitude: f64) -> String {
    static FINDER: std::sync::OnceLock<tzf_rs::DefaultFinder> = std::sync::OnceLock::new();
    FINDER
        .get_or_init(tzf_rs::DefaultFinder::new)
        .get_tz_name(longitude, latitude)
        .to_string()
}

/// A short human label for what kind of place a Photon feature is, from its
/// OSM tag value ("city" -> "City", "place_of_worship" -> "Place of worship").
/// Values that don't describe anything ("yes") map to an empty label.
fn photon_kind(osm_value: &str) -> String {
    match osm_value {
        "yes" | "unclassified" => String::new(),
        "administrative" => "Area".to_string(),
        "neighbourhood" => "Neighborhood".to_string(),
        other => {
            let words = other.replace('_', " ");
            let mut chars = words.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        }
    }
}

#[derive(Debug, Deserialize)]
struct ZipLookupQuery {
    lat: f64,
    lon: f64,
}

/// GET `/add/zip-lookup?lat=&lon=` — resolve coordinates to a postal code via
/// the `user.geocode.reverse` service (OpenCage, cached), which knows
/// postcodes the OSM feature Photon matched may lack. Called once per picked
/// place-search suggestion and only when the suggestion itself carried no
/// postcode. Returns `{"zip": "..."}`, with an empty string when none was
/// found.
#[handler]
#[tracing::instrument]
pub async fn add_zip_lookup(
    Authenticated(user_info): Authenticated,
    Query(query): Query<ZipLookupQuery>,
) -> Result<Response, anyhow::Error> {
    if user_info.is_none() {
        return Ok(Response::builder()
            .status(poem::http::StatusCode::UNAUTHORIZED)
            .finish());
    }

    if !(-90.0..=90.0).contains(&query.lat) || !(-180.0..=180.0).contains(&query.lon) {
        return Ok(Response::builder()
            .status(poem::http::StatusCode::BAD_REQUEST)
            .finish());
    }

    let response: GeocodeReverseResponse = common::service::request(
        "user.geocode.reverse",
        GeocodeReverseRequest {
            latitude: query.lat,
            longitude: query.lon,
        },
    )
    .await?;

    Ok(Response::builder()
        .content_type("application/json")
        .body(serde_json::to_string(
            &serde_json::json!({ "zip": response.postcode }),
        )?))
}

/// Convert one Photon GeoJSON feature into a `PlaceSuggestion`, sorting the
/// feature's name into the right address component: venues keep it as the
/// venue name, while a bare locality result (a city, state, or country) files
/// it under the component it names.
fn photon_suggestion(feature: &serde_json::Value) -> Option<PlaceSuggestion> {
    let props = feature.get("properties")?;
    let prop = |key: &str| {
        props
            .get(key)
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    };

    let coordinates = feature.get("geometry")?.get("coordinates")?.as_array()?;
    let longitude = coordinates.first()?.as_f64()?;
    let latitude = coordinates.get(1)?.as_f64()?;

    let name = prop("name");
    if name.is_empty() {
        return None;
    }

    let mut address = prop("street");
    let housenumber = prop("housenumber");
    if !housenumber.is_empty() {
        address = if address.is_empty() {
            housenumber
        } else {
            format!("{housenumber} {address}")
        };
    }

    let mut city = [
        prop("city"),
        prop("town"),
        prop("village"),
        prop("district"),
    ]
    .into_iter()
    .find(|c| !c.is_empty())
    .unwrap_or_default();
    let mut state = prop("state");
    let mut country = prop("country");
    let zip = prop("postcode");

    let mut location_name = name.clone();
    let osm_key = prop("osm_key");
    let osm_value = prop("osm_value");
    if osm_key == "place" || osm_key == "boundary" {
        // A locality, not a venue: the name belongs in an address component.
        // When the component it names is already filled (e.g. a neighborhood
        // inside a known city), keep it as the place name instead.
        location_name = String::new();
        match osm_value.as_str() {
            "state" | "region" | "province" if state.is_empty() => state = name.clone(),
            "country" if country.is_empty() => country = name.clone(),
            _ if city.is_empty() => city = name.clone(),
            _ => location_name = name.clone(),
        }
    }

    let context = [&city, &state, &country]
        .into_iter()
        .filter(|part| !part.is_empty() && **part != name)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");

    Some(PlaceSuggestion {
        name,
        kind: photon_kind(&osm_value),
        context,
        timezone: timezone_at(latitude, longitude),
        location_name,
        address,
        city,
        state,
        zip,
        country,
        latitude,
        longitude,
    })
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
            let end = parse_datetime(
                inst.end_date.trim(),
                inst.end_time.trim(),
                inst.timezone.trim(),
            )?;
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
                    "Please provide an address or city for the in-person event.".to_string()
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
        let begins = parse_datetime(
            ticketing_date,
            ticketing_time,
            form.ticketing_timezone.trim(),
        )?;
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
            event.point = picked_point(&form.latitude, &form.longitude);
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

/// The coordinates of the place picked in the location search, as a GeoJSON
/// point string, when both are present and in range. A picked place pins the
/// point column exactly; without one the save path falls back to geocoding
/// the address fields.
fn picked_point(latitude: &str, longitude: &str) -> Option<String> {
    let latitude: f64 = latitude.trim().parse().ok()?;
    let longitude: f64 = longitude.trim().parse().ok()?;
    if !(-90.0..=90.0).contains(&latitude) || !(-180.0..=180.0).contains(&longitude) {
        return None;
    }
    Some(
        Point {
            latitude,
            longitude,
        }
        .to_string(),
    )
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
    #[serde(default, deserialize_with = "empty_as_none")]
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
    /// Coordinates of the place chosen in the location picker's type-ahead
    /// search, when one was picked (empty for a hand-entered address).
    #[serde(default)]
    latitude: String,
    #[serde(default)]
    longitude: String,
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
    latitude: String,
    longitude: String,
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
            location_mode: f
                .location_mode
                .map(|m| m.as_str().to_string())
                .unwrap_or_default(),
            location_name: f.location_name.clone(),
            location_description: f.location_description.clone(),
            address: f.address.clone(),
            city: f.city.clone(),
            state: f.state.clone(),
            zip: f.zip.clone(),
            country: f.country.clone(),
            latitude: f.latitude.clone(),
            longitude: f.longitude.clone(),
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
            render_project_form(
                Some(message),
                ProjectFormValues::from(&form),
                Some(user_info),
            )
            .await
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
        &values.latitude,
        &values.longitude,
        &values.location_description,
        &values.url,
        &values.regions,
    );
    let image = ImageField {
        label: "Image".to_string(),
        value: values.image.clone(),
        credit: Some(values.image_credit.clone()),
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
            project.point = picked_point(&form.latitude, &form.longitude);
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A venue pick's inferred zone must be a real IANA zone that the form's
    /// time-zone select actually offers, or the client-side defaulting is a
    /// silent no-op.
    #[test]
    fn venue_timezone_is_selectable() {
        // Philadelphia city hall.
        let zone = timezone_at(39.9527, -75.1635);
        assert_eq!(zone, "America/New_York");

        let listed = all_timezones()
            .iter()
            .any(|(_, zones)| zones.iter().any(|(value, _)| *value == zone));
        assert!(listed, "inferred zone {zone} missing from the tz select");
    }

    /// The zone stated on the collapsed time-zone field: the curated name where
    /// there is one, the grouped name otherwise, and never a bare identifier for
    /// a zone the select actually lists.
    #[test]
    fn timezone_labels_read_as_names() {
        assert_eq!(timezone_label(""), "");
        assert_eq!(
            timezone_label("America/New_York"),
            "Eastern Time (US & Canada)"
        );
        assert_eq!(timezone_label("Africa/Nairobi"), "Africa/Nairobi");
        assert_eq!(timezone_label("Mars/Olympus"), "Mars/Olympus");
    }
}
