use common::message::log::PageChange;
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use jiff::{Zoned, civil, tz::TimeZone};
use poem::{
    IntoResponse, Response, handler,
    session::Session,
    web::{Html, Query, Redirect},
};
use sailfish::TemplateOnce;
use serde::{Deserialize, Serialize};
use ulid::Ulid;

use common::{
    insensitivestring::InsensitiveString,
    message::location::{GeocodeReverseRequest, GeocodeReverseResponse, Point},
    message::user::BasicInfo,
    service::log_activity,
    shared::db,
    timestring::TimeString,
};

use crate::{
    choice::{ChoiceCheckboxes, ChoiceList, ChoiceValue, OrreryValue, limited_heading},
    component::{
        Component, Head, StandardLayout,
        floating_field::FormError,
        image_field::{ImageField, ImageLayout, ImageUploadResult, ImageUrlInput},
    },
    contact::{Contact, ContactRole},
    entity::{Entity, EntityKind, EntityTarget},
    event::{EventInstance, all::EventAll},
    exchange::SubmissionExchange,
    form::QsForm,
    middleware::authorization::Authenticated,
    peering::LoadStorePeered,
    project::{
        all::ProjectAll,
        location::{ANYWHERE, AT_AN_ADDRESS, IN_A_RANGE},
    },
    submission::{Arrival, Submission, SubmissionKind, announce},
    upload::storage::untracked_file_id,
    web_address::is_web_address,
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
/// at `/add/project` and `/add/event` and require authentication.
#[handler]
#[tracing::instrument]
pub async fn add_chooser_page(
    Authenticated(user_info): Authenticated,
    session: &Session,
    Query(query): Query<AddQuery>,
) -> Result<Response, anyhow::Error> {
    // The chooser only passes the exchange on to the form picked from it.
    session_exchange(session, &query).await?;

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

/// Where the event or project takes place. Every mode also permits a link,
/// so a venue or a region can carry a livestream or information link.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum LocationMode {
    InPerson,
    Online,
    /// In person, wherever each participant is, as in a worldwide bioblitz:
    /// no address and no region. An event's mode alone, offered only by the
    /// event form's picker.
    Anywhere,
    Region,
}

impl LocationMode {
    fn as_str(self) -> &'static str {
        match self {
            LocationMode::InPerson => "in_person",
            LocationMode::Online => "online",
            LocationMode::Anywhere => "anywhere",
            LocationMode::Region => "region",
        }
    }
}

/// Deserialize an optional field, treating an empty string as `None`, as a
/// form submits a field left unanswered.
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

/// Who can attend an event: anyone, or only the group it names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Attendance {
    #[default]
    Open,
    Limited,
}

/// The most characters (code points) the group an event is limited to may
/// have, as orrery's `SciStarter.EVENT_LIMITED_TO_LIMIT`.
pub const LIMITED_TO_LIMIT: usize = 140;

/// A recurrence is never expanded beyond this many occurrences per base
/// date/time, as a guard against runaway input.
const MAX_OCCURRENCES: usize = 366;

/// The most characters (code points) a new event's name may have, as
/// orrery's `SciStarter.EVENT_NAME_LIMIT`. Older events may be longer, so
/// only the add form holds names to it.
pub const EVENT_NAME_LIMIT: usize = 100;

// ---------------------------------------------------------------------------
// Add Event: submitted form
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct AddEventForm {
    /// The `?exchange=` the form was opened with, carried in a hidden field.
    #[serde(default)]
    exchange: String,
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
    /// The slugs of the formats, programs or campaigns and kinds of venue
    /// ticked.
    #[serde(default)]
    formats: Vec<String>,
    #[serde(default)]
    programs: Vec<String>,
    #[serde(default)]
    venues: Vec<String>,
    /// The slugs of the topics ticked.
    #[serde(default)]
    topics: Vec<String>,
    /// The slugs of the audiences ticked.
    #[serde(default)]
    audience: Vec<String>,
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
    #[serde(default)]
    ticket_url: String,
    #[serde(default)]
    cost: String,
    repeat_frequency: Option<RepeatFrequency>,
    #[serde(default)]
    repeat_until: String,
    #[serde(default)]
    instances: Vec<InstanceInput>,
    #[serde(default)]
    attendance: Attendance,
    /// The group the event is limited to, read only when it is limited.
    #[serde(default)]
    limited_to: String,
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
    exchange: String,
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
    formats: Vec<String>,
    programs: Vec<String>,
    venues: Vec<String>,
    topics: Vec<String>,
    audience: Vec<String>,
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
    ticket_url: String,
    cost: String,
    repeat_frequency: String,
    repeat_until: String,
    instances: Vec<InstanceValues>,
    /// Whether the event is limited to a group, and the group.
    limited: bool,
    limited_to: String,
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
            exchange: f.exchange.clone(),
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
            formats: f.formats.clone(),
            programs: f.programs.clone(),
            venues: f.venues.clone(),
            topics: f.topics.clone(),
            audience: f.audience.clone(),
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
            ticket_url: f.ticket_url.clone(),
            cost: f.cost.clone(),
            repeat_frequency: f.repeat_frequency.unwrap_or_default().as_str().to_string(),
            repeat_until: f.repeat_until.clone(),
            instances,
            limited: f.attendance == Attendance::Limited,
            limited_to: f.limited_to.clone(),
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
    error: Option<FormError>,
    values: EventFormValues,
    /// The exchange the form's `exchange` field names, if it names one.
    exchange: Option<SubmissionExchange>,
    /// Whether that exchange publishes what this visitor adds as it arrives.
    publishes: bool,
    /// The collapsed section of what the event picks from the lists that
    /// aren't asked with its location.
    categories: ChoiceGroup,
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
// Shared add-form components (used by both /add/event and /add/project)
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
    /// Whether the mode only an event has is offered: in person, anywhere
    /// in the world.
    offers_event_modes: bool,
    /// The kinds of venue an event held at an address may pick, asked with
    /// the address.
    venues: Option<ChoiceCheckboxes>,
    /// The form's error, when it was refused; marks the control it is about.
    error: Option<FormError>,
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
            offers_event_modes: false,
            venues: None,
            error: None,
        }
    }

    /// Offer the modes only an event has besides the others.
    fn with_event_modes(mut self) -> Self {
        self.offers_event_modes = true;
        self
    }

    /// Ask which kinds of `venues` an event held at an address is held at,
    /// when there are any.
    fn with_venues(mut self, venues: ChoiceCheckboxes) -> Self {
        self.venues = (!venues.is_empty()).then(|| venues.with_legend("Kind of venue (optional)"));
        self
    }

    /// Mark the control `error` is about, when it is one of this section's.
    fn errors(mut self, error: Option<&FormError>) -> Self {
        self.error = error.cloned();
        self
    }

    /// Whether the form's error is about the control named `field`.
    fn is_invalid(&self, field: &str) -> bool {
        self.error.as_ref().is_some_and(|e| e.is_about(field))
    }
}

/// The time-zone control for one date/time row, a field in the row beside
/// the date and time. Zones are inferred from the picked venue or the
/// browser, so the list is there to correct an inference rather than to ask a
/// question. With JavaScript the field shows the zone's short name ("PDT",
/// "GMT+3") over the full list, which opens on click; without it, the list
/// itself shows.
#[derive(sailfish::TemplateOnce)]
#[template(path = "site/timezone_field.stpl.html")]
struct TimezoneField {
    field_name: String,
    selected: String,
    required: bool,
    invalid: bool,
    timezones: &'static [(&'static str, &'static str)],
    all_timezones: &'static [(String, Vec<(String, String)>)],
}

impl TimezoneField {
    /// Build the control for `field_name`, preselecting `selected`.
    fn build(field_name: impl Into<String>, selected: &str, required: bool) -> Self {
        TimezoneField {
            field_name: field_name.into(),
            selected: selected.to_string(),
            required,
            invalid: false,
            timezones: COMMON_TIMEZONES,
            all_timezones: all_timezones(),
        }
    }

    /// Mark the field invalid when `error` is about it.
    fn errors(mut self, error: Option<&FormError>) -> Self {
        self.invalid = error.is_some_and(|e| e.is_about(&self.field_name));
        self
    }
}

/// The header of one of an add form's optional sections: an icon and a title
/// on the `<summary>` of a collapsed `details.add-section` (the chevron is
/// drawn by the stylesheet).
#[derive(sailfish::TemplateOnce)]
#[template(path = "site/add_section_summary.stpl.html")]
struct SectionSummary {
    icon: SectionIcon,
    title: Cow<'static, str>,
}

impl SectionSummary {
    fn new(icon: SectionIcon, title: impl Into<Cow<'static, str>>) -> Self {
        SectionSummary {
            icon,
            title: title.into(),
        }
    }
}

#[derive(Clone, Copy)]
enum SectionIcon {
    Details,
    Umbrella,
    Contact,
    Ticket,
    Topics,
    Science,
    Audience,
    Activities,
}

/// One of an add form's choice fields, as one of its optional sections: a
/// checkbox for each value of a list, grouped under the question each
/// answers when the list's values answer several.
#[derive(sailfish::TemplateOnce)]
#[template(path = "site/add_choice_group.stpl.html")]
struct ChoiceGroup {
    icon: SectionIcon,

    /// The section's title, which says how many may be picked when there is
    /// one list with a limit.
    title: Cow<'static, str>,

    /// The form fields, each submitted as `field[]`.
    fields: Vec<String>,
    checkboxes: Vec<ChoiceCheckboxes>,
    invalid: bool,
}

impl ChoiceGroup {
    /// A section picking from `lists`, each list's boxes submitted as its
    /// field with its picks ticked, and limited as `limit` says. One list
    /// is the whole section; each of several is a fieldset under its
    /// heading.
    async fn load(
        icon: SectionIcon,
        title: &'static str,
        lists: &[(ChoiceList, &'static str, &[String])],
        limit: fn(ChoiceList) -> Option<usize>,
    ) -> Result<Self, anyhow::Error> {
        let mut checkboxes = Vec::with_capacity(lists.len());
        for (list, field, picked) in lists.iter().copied() {
            let boxes =
                ChoiceCheckboxes::load(field, list, picked.iter().cloned().collect(), limit(list))
                    .await?;
            checkboxes.push(if lists.len() > 1 {
                boxes.with_legend(limited_heading(list.heading(), limit(list)))
            } else {
                boxes
            });
        }

        let title = match lists {
            [(list, _, _)] => limited_heading(title, limit(*list)),
            _ => title.into(),
        };
        Ok(ChoiceGroup::new(icon, title, checkboxes))
    }

    /// A section of `checkboxes`, each set under its own legend or alone.
    fn new(
        icon: SectionIcon,
        title: impl Into<Cow<'static, str>>,
        checkboxes: Vec<ChoiceCheckboxes>,
    ) -> Self {
        ChoiceGroup {
            icon,
            title: title.into(),
            fields: checkboxes
                .iter()
                .map(|boxes| boxes.field().to_string())
                .collect(),
            checkboxes,
            invalid: false,
        }
    }

    /// Mark the section invalid when `error` is about one of its fields.
    fn errors(mut self, error: Option<&FormError>) -> Self {
        self.invalid = error.is_some_and(|e| self.fields.iter().any(|field| e.is_about(field)));
        self
    }
}

/// Checkboxes for picking from `values` of `list` as the add-event form's
/// `field`, under `title` and how many may be picked.
fn event_choices(
    field: &'static str,
    title: &'static str,
    list: ChoiceList,
    values: Vec<ChoiceValue>,
    picked: &[String],
) -> ChoiceCheckboxes {
    let limit = crate::event::pick_limit(list);
    let legend = match limit {
        Some(_) => limited_heading(title, limit),
        None => format!("{title} (choose any)").into(),
    };
    ChoiceCheckboxes::new(field, values, picked.iter().cloned().collect(), limit)
        .with_legend(legend)
}

/// The values of `list`, one of the event-type lists, that the add-event
/// form offers: all but Online, which its location toggle answers.
async fn offered_event_types(list: ChoiceList) -> Result<Vec<ChoiceValue>, anyhow::Error> {
    Ok(ChoiceValue::all(list)
        .await?
        .into_iter()
        .filter(|value| !OrreryValue::EVENT_ONLINE.is(value))
        .collect())
}

/// Event formats as the add-event form offers them: alphabetically, with
/// the catch-all Other last.
fn in_format_order(mut formats: Vec<ChoiceValue>) -> Vec<ChoiceValue> {
    formats.sort_by_cached_key(|value| {
        (
            OrreryValue::EVENT_OTHER.is(value),
            value.name.to_lowercase(),
        )
    });
    formats
}

/// What an add-event submission picks from each event-type list: the
/// formats and programs ticked, the kinds of venue ticked when the event is
/// held at an address, and `online`, the value saying an event is held
/// online, exactly when it is, whichever list that value is in.
fn event_type_picks(
    form: &AddEventForm,
    online: Option<&ChoiceValue>,
) -> BTreeMap<ChoiceList, BTreeSet<String>> {
    let at_address = form.location_mode == Some(LocationMode::InPerson);
    let mut picks = BTreeMap::from([
        (
            ChoiceList::EventFormat,
            form.formats.iter().cloned().collect(),
        ),
        (
            ChoiceList::EventProgram,
            form.programs.iter().cloned().collect(),
        ),
        (
            ChoiceList::EventVenue,
            if at_address {
                form.venues.iter().cloned().collect()
            } else {
                BTreeSet::new()
            },
        ),
    ]);

    if let Some(online) = online {
        let list = picks.entry(online.list).or_default();
        if form.location_mode == Some(LocationMode::Online) {
            list.insert(online.slug.clone());
        } else {
            list.remove(&online.slug);
        }
    }

    picks
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

/// The query an add page can be opened with.
#[derive(Debug, Default, Deserialize)]
pub struct AddQuery {
    /// The `path_id` of the exchange the submission is made through, which
    /// is how an exchange's "Add a Project" and "Add an Event" links open
    /// these pages. Present but empty, it stops submitting through one.
    exchange: Option<String>,
}

impl AddQuery {
    /// Where to send someone who must log in first, so they come back to
    /// the same form for the same exchange.
    fn login_redirect(&self, path: &str) -> Redirect {
        let next = match self.exchange.as_deref() {
            Some(exchange) => format!("{path}?exchange={}", urlencoding::encode(exchange)),
            None => path.to_string(),
        };
        Redirect::see_other(format!("/login?next={}", urlencoding::encode(&next)))
    }
}

/// The session key holding the `path_id` of the exchange this visitor's
/// submissions go through.
const SESSION_EXCHANGE: &str = "submission_exchange";

/// How long the exchange is remembered once no add page has used it. A
/// session lasts months, and someone who once followed an exchange's link
/// does not mean everything they add afterwards to go to it.
const SESSION_EXCHANGE_IDLE: jiff::SignedDuration = jiff::SignedDuration::from_hours(4);

/// The exchange remembered in the session, and when an add page last used
/// it.
#[derive(Debug, Serialize, Deserialize)]
struct RememberedExchange {
    path_id: String,
    used: jiff::Timestamp,
}

impl RememberedExchange {
    /// Whether it has gone unused long enough at `now` to be forgotten.
    fn lapsed(&self, now: jiff::Timestamp) -> bool {
        now.duration_since(self.used) > SESSION_EXCHANGE_IDLE
    }
}

/// The exchange this visitor's submissions go through.
///
/// An add page opened with `?exchange=` remembers it in the session, so
/// that a project or event started later from the site's own "add" links
/// still goes to that exchange; an empty `?exchange=` clears it. Each add
/// page that uses it keeps it for another [`SESSION_EXCHANGE_IDLE`], and
/// after that long unused it is forgotten, as is one that names no
/// exchange.
async fn session_exchange(
    session: &Session,
    query: &AddQuery,
) -> Result<Option<SubmissionExchange>, anyhow::Error> {
    let now = jiff::Timestamp::now();

    let path_id = match query.exchange.as_deref().map(str::trim) {
        Some("") => None,
        Some(path_id) => Some(path_id.to_string()),
        None => session
            .get::<RememberedExchange>(SESSION_EXCHANGE)
            .filter(|remembered| !remembered.lapsed(now))
            .map(|remembered| remembered.path_id),
    };

    let exchange = match &path_id {
        Some(path_id) => SubmissionExchange::find(path_id).await?,
        None => None,
    };

    match (path_id, &exchange) {
        (Some(path_id), Some(_)) => {
            session.set(SESSION_EXCHANGE, RememberedExchange { path_id, used: now })
        }
        _ => session.remove(SESSION_EXCHANGE),
    }

    Ok(exchange)
}

/// Whether what `user` submits through `exchange` is published as it
/// arrives. Nothing is without an exchange, or before anyone has signed in.
async fn published_on_arrival(
    exchange: Option<&SubmissionExchange>,
    user: Option<&BasicInfo>,
) -> Result<bool, anyhow::Error> {
    match (exchange, user) {
        (Some(exchange), Some(user)) => exchange.publishes_for(user).await,
        _ => Ok(false),
    }
}

/// The notice under an add form's heading naming the exchange the
/// submission goes through, and the form's field carrying it. Answering
/// the notice's link replaces it, and the field out of band, with an empty
/// one.
#[derive(sailfish::TemplateOnce)]
#[template(path = "site/add_exchange_notice.stpl.html")]
pub struct ExchangeNotice<'a> {
    pub exchange: Option<&'a SubmissionExchange>,
    /// Whether the exchange publishes what this visitor adds as it arrives,
    /// as [`SubmissionExchange::publishes_for`] decides.
    pub publishes: bool,
    /// What is being added: "project" or "event".
    pub noun: &'a str,
    /// The form's own path, which the link falls back to without htmx.
    pub path: &'a str,
    /// Whether this is the reply to the link, which also clears the field.
    pub oob: bool,
}

/// POST `/add/exchange/clear` — stop submitting through an exchange, in
/// answer to the link in [`ExchangeNotice`].
#[handler]
pub async fn add_exchange_clear(session: &Session) -> Result<Response, anyhow::Error> {
    session.remove(SESSION_EXCHANGE);

    let html = ExchangeNotice {
        exchange: None,
        publishes: false,
        noun: "",
        path: "",
        oob: true,
    }
    .render_once()?;

    Ok(Html(html).into_response())
}

/// GET `/add/event` — render the (empty) submission form. Submitting requires
/// authentication, so unauthenticated visitors are sent to log in first.
#[handler]
#[tracing::instrument]
pub async fn add_event_page(
    Authenticated(user_info): Authenticated,
    session: &Session,
    Query(query): Query<AddQuery>,
) -> Result<Response, anyhow::Error> {
    let exchange = session_exchange(session, &query).await?;

    if user_info.is_none() {
        return Ok(query.login_redirect("/add/event").into_response());
    }

    let mut values = EventFormValues::default();
    values.exchange = exchange.map(|e| e.path_id.to_string()).unwrap_or_default();
    values.repeat_frequency = RepeatFrequency::Once.as_str().to_string();
    values.instances.push(InstanceValues::default());

    render_event_form(None, values, user_info).await
}

/// POST `/add/event` — validate, create the event, and redirect to the
/// confirmation page, or to the event itself when it is published as it
/// arrives. On validation failure, re-render the form with the entered values
/// preserved.
#[handler]
#[tracing::instrument(skip(form))]
pub async fn add_event_submit(
    Authenticated(user_info): Authenticated,
    QsForm(form): QsForm<AddEventForm>,
) -> Result<Response, anyhow::Error> {
    let Some(user_info) = user_info else {
        return Ok(Redirect::see_other("/login?next=/add/event").into_response());
    };

    match validate_and_build(&form, user_info.user_id) {
        Ok(mut event) => {
            // Submitted through an exchange, it waits in that exchange's
            // approval queue (`reviewed` starts false), unless the exchange
            // publishes it as it arrives.
            let exchange = SubmissionExchange::find(&form.exchange).await?;
            let published = published_on_arrival(exchange.as_ref(), Some(&user_info)).await?;
            event.exchange = exchange.as_ref().map(|exchange| exchange.exchange_id.clone());
            event.reviewed = published;

            let online = OrreryValue::EVENT_ONLINE.find().await?;
            if online.is_none() {
                tracing::warn!(
                    "No event type has orrery's id for Online, {}; the event is saved without it",
                    OrreryValue::EVENT_ONLINE.id
                );
            }
            let mut types = event_type_picks(&form, online.as_ref());
            let mut picked = |list| types.remove(&list).unwrap_or_default();
            event.formats =
                ChoiceValue::known(ChoiceList::EventFormat, picked(ChoiceList::EventFormat))
                    .await?;
            event.programs =
                ChoiceValue::known(ChoiceList::EventProgram, picked(ChoiceList::EventProgram))
                    .await?;
            event.venues =
                ChoiceValue::known(ChoiceList::EventVenue, picked(ChoiceList::EventVenue)).await?;
            event.topics =
                ChoiceValue::known(ChoiceList::Topic, form.topics.iter().cloned()).await?;
            event.audience =
                ChoiceValue::known(ChoiceList::Audience, form.audience.iter().cloned()).await?;

            let name = event.name.clone();
            let event_id = event.event_id;

            // Persist (and distribute to peers) via LoadStorePeered.
            event.store().await?;

            // Create the page entity. Unless it is published as it arrives,
            // it becomes publicly resolvable at /events/<slug> only once its
            // reviewer publishes it.
            let entity =
                Entity::new_by_name(&name, EntityKind::Event, EntityTarget::Ulid(event_id)).await?;
            let entity = Entity::new_with_publish(
                entity.slug(),
                *entity.kind(),
                entity.target().clone(),
                published,
            );
            entity.store().await?;

            log_activity(
                None,
                PageChange::EventCreate,
                user_info.user_id,
                event_id,
                format!("name={name}"),
            )
            .await?;

            let arrival = Arrival {
                kind: SubmissionKind::Event,
                id: event_id,
                creator: user_info.user_id,
                exchange: exchange.as_ref(),
                published,
                submission: &Submission::Submitted,
            };

            // The event exists whether or not the mail about it could be
            // queued.
            if let Err(err) = announce(arrival).await {
                tracing::error!(%event_id, ?err, "Could not tell everyone of a new event");
            }

            if published {
                return Ok(Redirect::see_other(entity.canonical_path()).into_response());
            }

            Ok(Redirect::see_other(format!(
                "/add/event/submitted?name={}",
                urlencoding::encode(&name)
            ))
            .into_response())
        }
        Err(error) => {
            render_event_form(Some(error), EventFormValues::from(&form), Some(user_info)).await
        }
    }
}

async fn render_event_form(
    error: Option<FormError>,
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
    )
    .with_event_modes()
    .with_venues(ChoiceCheckboxes::new(
        "venues",
        offered_event_types(ChoiceList::EventVenue).await?,
        values.venues.iter().cloned().collect(),
        crate::event::pick_limit(ChoiceList::EventVenue),
    ))
    .errors(error.as_ref());
    let image = ImageField {
        label: "Image link".to_string(),
        value: values.image.clone(),
        credit: Some(values.image_credit.clone()),
        layout: ImageLayout::Cover,
        invalid: error
            .as_ref()
            .is_some_and(|error| error.is_about(ImageUrlInput::NAME)),
    };
    let tags = TagInput {
        value: values.tags.clone(),
    };

    // Whether the event is online, and the kind of venue it's held at, are
    // asked with its location; the rest of what it picks is here.
    let categories = ChoiceGroup::new(
        SectionIcon::Details,
        "Audience, topics and event type",
        vec![
            event_choices(
                "audience",
                "Audience",
                ChoiceList::Audience,
                ChoiceValue::all(ChoiceList::Audience).await?,
                &values.audience,
            ),
            event_choices(
                "topics",
                "Topics",
                ChoiceList::Topic,
                ChoiceValue::all(ChoiceList::Topic).await?,
                &values.topics,
            ),
            event_choices(
                "formats",
                "Type of event",
                ChoiceList::EventFormat,
                in_format_order(offered_event_types(ChoiceList::EventFormat).await?),
                &values.formats,
            ),
            event_choices(
                "programs",
                "Part of a program or campaign?",
                ChoiceList::EventProgram,
                offered_event_types(ChoiceList::EventProgram).await?,
                &values.programs,
            ),
        ],
    )
    .errors(error.as_ref());

    let exchange = SubmissionExchange::find(&values.exchange).await?;
    if exchange.is_none() {
        values.exchange.clear();
    }
    let publishes = published_on_arrival(exchange.as_ref(), user_info.as_deref()).await?;

    let content = AddEventContent {
        error,
        values,
        exchange,
        publishes,
        categories,
        location,
        image,
        tags,
    };

    let layout = StandardLayout::new("/add/event", content)
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

/// GET `/add/event/submitted` — "thanks, pending review" confirmation page.
#[handler]
#[tracing::instrument]
pub async fn add_event_submitted_page(
    Authenticated(user_info): Authenticated,
    Query(query): Query<SubmittedQuery>,
) -> Result<Response, anyhow::Error> {
    render_submitted(user_info, "/add/event", query.name, "event").await
}

/// GET `/add/project/submitted` — "thanks, pending review" confirmation page.
#[handler]
#[tracing::instrument]
pub async fn add_project_submitted_page(
    Authenticated(user_info): Authenticated,
    Query(query): Query<SubmittedQuery>,
) -> Result<Response, anyhow::Error> {
    render_submitted(user_info, "/add/project", query.name, "project").await
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

/// GET `/add/event/parent-search?q=` — list published events matching the query
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

/// GET `/add/event/parent-select?id=` — record the chosen parent: returns the
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

/// GET `/add/event/parent-clear` — clear the chosen parent.
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
    /// Whether there was a query to answer; an emptied input clears the
    /// status line rather than reporting no matches.
    searched: bool,
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
        let none = TagSuggestions {
            tags: Vec::new(),
            searched: false,
        };
        return Ok(html_fragment(none.render_once()?));
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

    Ok(html_fragment(
        TagSuggestions {
            tags,
            searched: true,
        }
        .render_once()?,
    ))
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
    let user = user_info.ok_or_else(|| anyhow::anyhow!("Authentication required"))?;

    // The upload control sends its field's layout alongside the file (the
    // enclosing form's other values come along too, and are ignored), so the
    // answer can render the URL input the way that layout does.
    let mut layout = ImageLayout::default();
    let mut url = None;

    while let Some(field) = multipart.next_field().await? {
        match field.name() {
            Some("image_layout") => {
                layout = field.text().await?.parse()?;
            }
            Some("file") if url.is_none() => {
                url = Some(crate::upload::store_untracked(field, Some(user.user_id)).await?);
            }
            _ => {}
        }
    }

    let url = url.ok_or_else(|| anyhow::anyhow!("No file provided"))?;
    Ok(html_fragment(
        ImageUploadResult { url, layout }.render_once()?,
    ))
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

    let mut address = prop("street");
    let housenumber = prop("housenumber");
    if !housenumber.is_empty() {
        address = if address.is_empty() {
            housenumber
        } else {
            format!("{housenumber} {address}")
        };
    }

    // Most buildings in OSM have no name, so a search for an ordinary street
    // address matches only unnamed features. Such a feature is offered by its
    // address; one with neither a name nor an address has nothing to show.
    let named = !prop("name").is_empty();
    let name = if named { prop("name") } else { address.clone() };
    if name.is_empty() {
        return None;
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

    let osm_key = prop("osm_key");
    let osm_value = prop("osm_value");
    // An address is not a venue, so it leaves the venue name empty.
    let mut location_name = if named { name.clone() } else { String::new() };
    if named && (osm_key == "place" || osm_key == "boundary") {
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

    let kind = if named {
        photon_kind(&osm_value)
    } else {
        "Address".to_string()
    };

    Some(PlaceSuggestion {
        name,
        kind,
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
///
/// Every check the form makes is here (the form is `novalidate`), in the
/// order orrery's add form makes the same checks in its `build`: the name,
/// description, mode and dates; what the mode requires; the shape of the
/// links; the ticketing date; and the contact.
fn validate_and_build(form: &AddEventForm, user_id: Ulid) -> Result<EventAll, FormError> {
    let name = form.name.trim();
    if name.is_empty() {
        return Err(FormError::at("name", "Please enter a name for your event."));
    }
    // Counted in characters, as the field counts them.
    if name.chars().count() > EVENT_NAME_LIMIT {
        return Err(FormError::at(
            "name",
            format!("Please shorten your event's name to {EVENT_NAME_LIMIT} characters or fewer."),
        ));
    }

    let topics = form.topics.iter().collect::<BTreeSet<_>>().len();
    if let Some(refusal) = crate::event::refuse_picks(ChoiceList::Topic, topics) {
        return Err(FormError::at("topics", refusal));
    }

    let description = form.description.trim();
    if description.is_empty() {
        return Err(FormError::at(
            "description",
            "Please add a description of your event.",
        ));
    }

    let mode = form.location_mode.ok_or_else(|| {
        FormError::at(
            "location_mode",
            "Please choose where the event takes place.",
        )
    })?;

    // Resolve the date/time rows that carry a start.
    let mut base: Vec<ZonedInstance> = Vec::new();
    for (i, inst) in form.instances.iter().enumerate() {
        let field = |part: &str| format!("instances[{i}][{part}]");
        let begin_date = inst.begin_date.trim();
        let begin_time = inst.begin_time.trim();

        if begin_date.is_empty() && begin_time.is_empty() {
            continue; // an untouched row
        }
        if begin_date.is_empty() || begin_time.is_empty() {
            let missing = if begin_date.is_empty() {
                "begin_date"
            } else {
                "begin_time"
            };
            return Err(FormError::at(
                field(missing),
                "Each date/time needs both a date and a start time.",
            ));
        }
        if inst.timezone.trim().is_empty() {
            return Err(FormError::at(
                field("timezone"),
                "Please choose a time zone for each date/time.",
            ));
        }

        let begin = parse_datetime(begin_date, begin_time, inst.timezone.trim())
            .map_err(|message| FormError::at(field("begin_date"), message))?;
        if starts_before_today(&begin, &Zoned::now()) {
            return Err(FormError::at(
                field("begin_date"),
                "An event can't start on a date that has already passed.",
            ));
        }

        let end = if inst.has_end.is_some()
            && !inst.end_date.trim().is_empty()
            && !inst.end_time.trim().is_empty()
        {
            let end = parse_datetime(
                inst.end_date.trim(),
                inst.end_time.trim(),
                inst.timezone.trim(),
            )
            .map_err(|message| FormError::at(field("end_date"), message))?;
            if end < begin {
                return Err(FormError::at(
                    field("end_date"),
                    "An event's end time can't be before its start time.",
                ));
            }
            Some(end)
        } else {
            None
        };

        base.push(ZonedInstance { begin, end });
    }

    if base.is_empty() {
        return Err(FormError::at(
            "instances[0][begin_date]",
            "Please add at least one date and start time.",
        ));
    }

    let instances = expand_recurrence(base, form)?;

    let url = form.url.trim().to_string();

    // Location-specific requirements.
    match mode {
        LocationMode::Online => {
            if url.is_empty() {
                return Err(FormError::at(
                    "url",
                    "Please provide a link where people can join the online event.",
                ));
            }
        }
        LocationMode::InPerson => {
            if form.address.trim().is_empty() && form.city.trim().is_empty() {
                return Err(FormError::at(
                    "address",
                    "Please provide an address or city for the in-person event.",
                ));
            }
        }
        LocationMode::Region => {
            if form.location_description.trim().is_empty() {
                return Err(FormError::at(
                    "location_description",
                    "Please describe the area the event covers.",
                ));
            }
        }
        LocationMode::Anywhere => {}
    }

    let limited_to = match form.attendance {
        Attendance::Open => "",
        Attendance::Limited => {
            let group = form.limited_to.trim();
            if group.is_empty() {
                return Err(FormError::at(
                    "limited_to",
                    "Please say who is eligible to attend, such as \u{201c}Library cardholders only.\u{201d}",
                ));
            }
            // Counted in characters, as the field counts them.
            if group.chars().count() > LIMITED_TO_LIMIT {
                return Err(FormError::at(
                    "limited_to",
                    format!(
                        "Please say who is eligible to attend in {LIMITED_TO_LIMIT} characters or fewer."
                    ),
                ));
            }
            group
        }
    };

    if !url.is_empty() && !is_web_address(&url) {
        return Err(FormError::at(
            "url",
            "Please check the event's link; it should be a whole web address, such as https://example.org/event.",
        ));
    }

    // An uploaded image's address is the site's own; only a typed link needs
    // checking.
    let image = form.image.trim();
    if !image.is_empty() && untracked_file_id(image).is_none() && !is_web_address(image) {
        return Err(FormError::at(
            ImageUrlInput::NAME,
            "Please check the image link; it should be a whole web address, such as https://example.org/photo.jpg.",
        ));
    }

    let ticket_url = form.ticket_url.trim();
    if !ticket_url.is_empty() && !is_web_address(ticket_url) {
        return Err(FormError::at(
            "ticket_url",
            "Please check the ticket / registration link; it should be a whole web address, such as https://example.org/tickets.",
        ));
    }

    // Optional ticketing/registration open date.
    let ticketing_date = form.ticketing_date.trim();
    let ticketing_begins = if ticketing_date.is_empty() {
        None
    } else {
        if form.ticketing_timezone.trim().is_empty() {
            return Err(FormError::at(
                "ticketing_timezone",
                "Please choose a time zone for the ticketing date.",
            ));
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
        )
        .map_err(|message| FormError::at("ticketing_date", message))?;
        Some(TimeString::from(begins.timestamp()))
    };

    // A single organizer contact, if one was provided.
    let contact_email = form.contact_email.trim();
    if !contact_email.is_empty() && !crate::contact::plausible_email(contact_email) {
        return Err(FormError::at(
            "contact_email",
            "Please check the contact person's email; it should look like name@example.org.",
        ));
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
    event.image = image.to_string();
    event.image_credit = form.image_credit.trim().to_string();
    event.how_to_join = form.how_to_join.trim().to_string();
    event.part_of = form.part_of.trim().parse::<Ulid>().ok();
    // A location note is meaningful in every mode.
    event.location_description = form.location_description.trim().to_string();
    event.ticket_url = ticket_url.to_string();
    event.cost = form.cost.trim().to_string();
    event.limited_to = limited_to.to_string();
    event.ticketing_begins = ticketing_begins;
    event.tags = form
        .tags
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(InsensitiveString::from)
        .collect();

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

    // The `location_type`s orrery's form gives each mode, as orrery's sync
    // spells them here: an online event, and one in person wherever each
    // participant is, are both anywhere.
    match mode {
        LocationMode::Online => {
            event.entirely_online = true;
            event.location_type = ANYWHERE.to_string();
        }
        LocationMode::Anywhere => {
            event.entirely_online = false;
            event.location_type = ANYWHERE.to_string();
        }
        LocationMode::InPerson => {
            event.entirely_online = false;
            event.location_type = AT_AN_ADDRESS.to_string();
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
            event.location_type = IN_A_RANGE.to_string();
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
) -> Result<Vec<EventInstance>, FormError> {
    let frequency = form.repeat_frequency.unwrap_or_default();
    // A repeating event repeats through a date it must be given.
    let until = if frequency == RepeatFrequency::Once {
        None
    } else {
        let until = form.repeat_until.trim();
        if until.is_empty() {
            return Err(FormError::at(
                "repeat_until",
                "Please choose the date the event repeats until.",
            ));
        }
        Some(parse_date(until).map_err(|message| FormError::at("repeat_until", message))?)
    };

    let mut out: Vec<EventInstance> = Vec::new();

    for inst in base {
        match until {
            Some(until) => {
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
                        .map_err(|_| FormError::general("Couldn't compute the repeat schedule."))?;
                    end = match end {
                        Some(e) => Some(e.checked_add(span).map_err(|_| {
                            FormError::general("Couldn't compute the repeat schedule.")
                        })?),
                        None => None,
                    };
                }
            }
            None => out.push(to_event_instance(&inst.begin, inst.end.as_ref())),
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

/// Whether `begin` falls on a day before `now`'s, both read in `begin`'s own
/// zone: "today" is the event's today, wherever the visitor is. Earlier the
/// same day is allowed, so an event can be listed while it's under way.
fn starts_before_today(begin: &Zoned, now: &Zoned) -> bool {
    begin.date() < now.with_time_zone(begin.time_zone().clone()).date()
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
    /// The `?exchange=` the form was opened with, carried in a hidden field.
    #[serde(default)]
    exchange: String,
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
    exchange: String,
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
            exchange: f.exchange.clone(),
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
    error: Option<FormError>,
    values: ProjectFormValues,
    /// The exchange the form's `exchange` field names, if it names one.
    exchange: Option<SubmissionExchange>,
    /// Whether that exchange publishes what this visitor adds as it arrives.
    publishes: bool,
    topics: ChoiceGroup,
    audience: ChoiceGroup,
    activities: ChoiceGroup,
    location: LocationPicker,
    image: ImageField,
    tags: TagInput,
}

/// GET `/add/project` — render the (empty) submission form (login required).
#[handler]
#[tracing::instrument]
pub async fn add_project_page(
    Authenticated(user_info): Authenticated,
    session: &Session,
    Query(query): Query<AddQuery>,
) -> Result<Response, anyhow::Error> {
    let exchange = session_exchange(session, &query).await?;

    if user_info.is_none() {
        return Ok(query.login_redirect("/add/project").into_response());
    }

    let values = ProjectFormValues {
        exchange: exchange.map(|e| e.path_id.to_string()).unwrap_or_default(),
        ..Default::default()
    };
    render_project_form(None, values, user_info).await
}

/// POST `/add/project` — validate, create the project (a draft), and redirect
/// to the confirmation page, or to the project itself when it is published as
/// it arrives.
#[handler]
#[tracing::instrument(skip(form))]
pub async fn add_project_submit(
    Authenticated(user_info): Authenticated,
    QsForm(form): QsForm<AddProjectForm>,
) -> Result<Response, anyhow::Error> {
    let Some(user_info) = user_info else {
        return Ok(Redirect::see_other("/login?next=/add/project").into_response());
    };

    match validate_and_build_project(&form, user_info.user_id) {
        Ok(mut project) => {
            // Submitted through an exchange, it waits in that exchange's
            // approval queue (`reviewed` starts false), unless the exchange
            // publishes it as it arrives.
            let exchange = SubmissionExchange::find(&form.exchange).await?;
            let published = published_on_arrival(exchange.as_ref(), Some(&user_info)).await?;

            // Everything the project's own person wrote stands against
            // imports, as orrery gives a project made through its form full
            // authority.
            project.update_source = Some(crate::project::field_source::FieldSource::Form);

            project.topics =
                ChoiceValue::known(ChoiceList::Topic, form.topics.iter().cloned()).await?;
            project.audience =
                ChoiceValue::known(ChoiceList::Audience, form.audience.iter().cloned()).await?;
            project.activities =
                ChoiceValue::known(ChoiceList::Activity, form.activities.iter().cloned()).await?;

            let name = project.name.clone();
            let slug = crate::project::all::persist_new_project(
                project,
                published,
                user_info.user_id,
                Submission::Submitted,
                exchange.as_ref(),
            )
            .await?;

            if published {
                return Ok(Redirect::see_other(EntityKind::Project.path(&slug)).into_response());
            }

            Ok(Redirect::see_other(format!(
                "/add/project/submitted?name={}",
                urlencoding::encode(&name)
            ))
            .into_response())
        }
        Err(error) => {
            render_project_form(Some(error), ProjectFormValues::from(&form), Some(user_info)).await
        }
    }
}

async fn render_project_form(
    error: Option<FormError>,
    mut values: ProjectFormValues,
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
    )
    .errors(error.as_ref());
    let image = ImageField {
        label: "Image link".to_string(),
        value: values.image.clone(),
        credit: Some(values.image_credit.clone()),
        layout: ImageLayout::Cover,
        invalid: error
            .as_ref()
            .is_some_and(|error| error.is_about(ImageUrlInput::NAME)),
    };
    let tags = TagInput {
        value: values.tags.clone(),
    };

    let topics = ChoiceGroup::load(
        SectionIcon::Topics,
        "Topics",
        &[(ChoiceList::Topic, "topics", &values.topics)],
        |_| None,
    )
    .await?;
    let audience = ChoiceGroup::load(
        SectionIcon::Audience,
        "Audience",
        &[(ChoiceList::Audience, "audience", &values.audience)],
        |_| None,
    )
    .await?;
    let activities = ChoiceGroup::load(
        SectionIcon::Activities,
        "Activities",
        &[(ChoiceList::Activity, "activities", &values.activities)],
        |_| None,
    )
    .await?;

    let exchange = SubmissionExchange::find(&values.exchange).await?;
    if exchange.is_none() {
        values.exchange.clear();
    }
    let publishes = published_on_arrival(exchange.as_ref(), user_info.as_deref()).await?;

    let content = AddProjectContent {
        error,
        values,
        exchange,
        publishes,
        topics,
        audience,
        activities,
        location,
        image,
        tags,
    };

    let layout = StandardLayout::new("/add/project", content)
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
fn validate_and_build_project(
    form: &AddProjectForm,
    user_id: Ulid,
) -> Result<ProjectAll, FormError> {
    let name = form.name.trim();
    if name.is_empty() {
        return Err(FormError::at(
            "name",
            "Please enter a name for your project.",
        ));
    }

    let description = form.description.trim();
    if description.is_empty() {
        return Err(FormError::at(
            "description",
            "Please add a description of your project.",
        ));
    }

    let mode = form.location_mode.ok_or_else(|| {
        FormError::at(
            "location_mode",
            "Please choose where the project takes place.",
        )
    })?;

    let url = form.url.trim().to_string();
    match mode {
        LocationMode::Online => {
            if url.is_empty() {
                return Err(FormError::at(
                    "url",
                    "Please provide a link for the online project.",
                ));
            }
        }
        // Never offered for a project.
        LocationMode::Anywhere => {
            return Err(FormError::at(
                "location_mode",
                "Please choose where the project takes place.",
            ));
        }
        LocationMode::InPerson => {
            if form.address.trim().is_empty() && form.city.trim().is_empty() {
                return Err(FormError::at(
                    "address",
                    "Please provide an address or city for the in-person project.",
                ));
            }
        }
        LocationMode::Region => {
            if form.location_description.trim().is_empty() {
                return Err(FormError::at(
                    "location_description",
                    "Please describe the area the project covers.",
                ));
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

    // The topics, audience and activities are read against their lists by
    // the handler, which can reach the database.
    project.fields_of_science = filter_to_vocabulary(
        &form.fields_of_science,
        crate::project::field::FIELDS_OF_SCIENCE_VOCABULARY,
    );

    let contact_email = form.contact_email.trim();
    if !contact_email.is_empty() && !crate::contact::plausible_email(contact_email) {
        return Err(FormError::at(
            "contact_email",
            "Please check the contact person's email; it should look like name@example.org.",
        ));
    }
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
        LocationMode::Anywhere => {
            unreachable!("a project's event-only mode is refused above")
        }
        LocationMode::Online => {
            project.entirely_online = true;
            project.location_type = ANYWHERE.to_string();
        }
        LocationMode::InPerson => {
            project.entirely_online = false;
            project.in_person = true;
            project.location_type = AT_AN_ADDRESS.to_string();
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
            project.location_type = IN_A_RANGE.to_string();
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
    use assert2::check;

    use super::*;

    #[test]
    fn an_unnamed_building_is_offered_by_its_address() {
        let house = serde_json::json!({
            "geometry": { "coordinates": [-122.31, 37.91] },
            "properties": {
                "osm_key": "place",
                "osm_value": "house",
                "housenumber": "1234",
                "street": "Elm Street",
                "city": "El Cerrito",
                "state": "California",
                "country": "United States",
            },
        });

        let suggestion = photon_suggestion(&house).unwrap();
        check!(suggestion.name == "1234 Elm Street");
        check!(suggestion.kind == "Address");
        check!(suggestion.location_name == "");
        check!(suggestion.address == "1234 Elm Street");
        check!(suggestion.city == "El Cerrito");
        check!(suggestion.context == "El Cerrito, California, United States");

        let nothing = serde_json::json!({
            "geometry": { "coordinates": [-122.31, 37.91] },
            "properties": { "osm_key": "building", "osm_value": "yes" },
        });
        check!(photon_suggestion(&nothing).is_none());
    }

    fn exchange() -> SubmissionExchange {
        SubmissionExchange {
            exchange_id: Ulid::new().to_string(),
            path_id: 42,
            name: "SciStarter Australia".to_string(),
            notice: String::new(),
            restricted_add: false,
        }
    }

    #[test]
    fn the_exchange_notice_names_the_reviewer_and_offers_scistarter_instead() {
        let exchange = exchange();
        let html = ExchangeNotice {
            exchange: Some(&exchange),
            publishes: false,
            noun: "event",
            path: "/add/event",
            oob: false,
        }
        .render_once()
        .unwrap();

        check!(html.contains(
            "You&rsquo;re adding this event through <strong>SciStarter Australia</strong>, \
             who will be responsible for reviewing it."
        ));
        check!(html.contains(r#"href="/add/event?exchange=" hx-post="/add/exchange/clear""#));
        check!(!html.contains("hx-swap-oob"));
    }

    #[test]
    fn an_exchanges_own_notice_replaces_the_standard_sentence() {
        let exchange = SubmissionExchange {
            notice: "The Australian volunteer will review <your> submission.".to_string(),
            ..exchange()
        };
        let html = ExchangeNotice {
            exchange: Some(&exchange),
            publishes: false,
            noun: "project",
            path: "/add/project",
            oob: false,
        }
        .render_once()
        .unwrap();

        check!(html.contains("The Australian volunteer will review &lt;your&gt; submission."));
        check!(!html.contains("responsible for reviewing"));
        check!(html.contains(r#"href="/add/project?exchange=" hx-post="/add/exchange/clear""#));
    }

    #[test]
    fn an_exchange_that_publishes_what_its_people_add_says_so() {
        let exchange = SubmissionExchange {
            notice: "The Australian volunteer will review your submission.".to_string(),
            restricted_add: true,
            ..exchange()
        };
        let html = ExchangeNotice {
            exchange: Some(&exchange),
            publishes: true,
            noun: "event",
            path: "/add/event",
            oob: false,
        }
        .render_once()
        .unwrap();

        check!(html.contains(
            "You&rsquo;re adding this event to <strong>SciStarter Australia</strong>, \
             which will publish it as soon as you submit it."
        ));
        check!(!html.contains("review"));
    }

    #[test]
    fn without_an_exchange_the_notice_is_empty() {
        let html = ExchangeNotice {
            exchange: None,
            publishes: false,
            noun: "project",
            path: "/add/project",
            oob: false,
        }
        .render_once()
        .unwrap();

        check!(html.contains(r#"<div id="submission-exchange">"#));
        check!(!html.contains("adding this"));
        check!(!html.contains("<input"));
    }

    #[test]
    fn choosing_scistarter_also_empties_the_forms_exchange_field() {
        let html = ExchangeNotice {
            exchange: None,
            publishes: false,
            noun: "",
            path: "",
            oob: true,
        }
        .render_once()
        .unwrap();

        check!(html.contains(
            r#"<input type="hidden" id="submission-exchange-field" name="exchange" value="" hx-swap-oob="true">"#
        ));
        check!(!html.contains("adding this"));
    }

    #[test]
    fn a_remembered_exchange_lapses_once_left_unused() {
        let used: jiff::Timestamp = "2026-11-11T09:00:00Z".parse().unwrap();
        let remembered = RememberedExchange {
            path_id: "42".to_string(),
            used,
        };

        check!(!remembered.lapsed("2026-11-11T13:00:00Z".parse().unwrap()));
        check!(remembered.lapsed("2026-11-11T13:00:01Z".parse().unwrap()));
    }

    #[test]
    fn logging_in_returns_to_the_same_exchange() {
        let query = AddQuery {
            exchange: Some("42".to_string()),
        };

        let response = query.login_redirect("/add/project").into_response();

        check!(
            response
                .headers()
                .get(poem::http::header::LOCATION)
                .unwrap()
                == "/login?next=%2Fadd%2Fproject%3Fexchange%3D42"
        );
    }

    /// serde_qs reads the add-event form's indexed date/time rows and its
    /// enum-valued fields.
    #[test]
    fn add_event_form_reads_indexed_rows_and_enums() {
        let body = "name=Bioblitz&location_mode=online&repeat_frequency=weekly\
            &instances[0][begin_date]=2026-11-01&instances[0][begin_time]=10:00\
            &instances[0][timezone]=America/Chicago\
            &instances[1][begin_date]=2026-11-02&instances[1][begin_time]=11:00\
            &instances[1][timezone]=America/Chicago";
        let form: AddEventForm = serde_qs::Config::new(10, false)
            .deserialize_bytes(body.as_bytes())
            .unwrap();

        check!(form.name == "Bioblitz");
        check!(form.location_mode == Some(LocationMode::Online));
        check!(form.repeat_frequency == Some(RepeatFrequency::Weekly));
        check!(form.instances.len() == 2);
        check!(form.instances[1].begin_time == "11:00");
    }

    #[test]
    fn an_implausible_contact_email_is_refused() {
        let submit = |email: &str| {
            let body = format!(
                "name=Bioblitz&description=Count+bugs&location_mode=online\
                 &url=https://example.org/bioblitz&repeat_frequency=once\
                 &instances[0][begin_date]=2099-11-01&instances[0][begin_time]=10:00\
                 &instances[0][timezone]=America/Chicago\
                 &contact_email={email}"
            );
            let form: AddEventForm = serde_qs::Config::new(10, false)
                .deserialize_bytes(body.as_bytes())
                .unwrap();
            validate_and_build(&form, Ulid::new())
        };

        let refused = submit("test@test").unwrap_err();
        check!(refused.is_about("contact_email"));

        let event = submit("name@example.org").unwrap();
        check!(event.contacts[0].email == "name@example.org");
    }

    #[test]
    fn an_in_person_event_is_held_at_its_address() {
        let in_person = [("location_mode", "in_person"), ("url", "")];

        let placeless = submit_event(&in_person).unwrap_err();
        check!(placeless.is_about("address"));

        let event = submit_event(
            &[
                &in_person[..],
                &[
                    ("city", "Chicago"),
                    ("ticket_url", " https://example.org/tickets "),
                    ("cost", " Free "),
                ],
            ]
            .concat(),
        )
        .unwrap();
        check!(!event.entirely_online);
        check!(!event.also_online);
        check!(event.city == "Chicago");
        check!(event.url.is_empty());
        check!(event.ticket_url == "https://example.org/tickets");
        check!(event.cost == "Free");
    }

    #[test]
    fn hybrid_is_no_longer_a_mode() {
        let body = "location_mode=hybrid";
        let form: Result<AddEventForm, _> =
            serde_qs::Config::new(10, false).deserialize_bytes(body.as_bytes());
        check!(form.is_err());
    }

    #[test]
    fn an_event_is_open_to_everyone_unless_limited() {
        let event = submit_event(&[("limited_to", "Library cardholders")]).unwrap();
        check!(event.limited_to.is_empty());

        let event = submit_event(&[
            ("attendance", "limited"),
            ("limited_to", "  Library cardholders "),
        ])
        .unwrap();
        check!(event.limited_to == "Library cardholders");
    }

    #[test]
    fn a_limited_event_names_its_group_briefly() {
        let refused = submit_event(&[("attendance", "limited"), ("limited_to", " ")]).unwrap_err();
        check!(refused.is_about("limited_to"));
        check!(
            refused.message
                == "Please say who is eligible to attend, such as \u{201c}Library cardholders only.\u{201d}"
        );

        let at_limit = "é".repeat(LIMITED_TO_LIMIT);
        let event = submit_event(&[("attendance", "limited"), ("limited_to", &at_limit)]).unwrap();
        check!(event.limited_to == at_limit);

        let over = format!("{at_limit}!");
        let refused =
            submit_event(&[("attendance", "limited"), ("limited_to", &over)]).unwrap_err();
        check!(refused.is_about("limited_to"));
        check!(
            refused.message == "Please say who is eligible to attend in 140 characters or fewer."
        );
    }

    /// The add-event form, showing `values` and refused over `error`.
    fn event_form(values: EventFormValues, error: Option<FormError>) -> String {
        AddEventContent {
            error,
            values,
            exchange: None,
            publishes: false,
            categories: ChoiceGroup::new(SectionIcon::Details, "Categories", Vec::new()),
            location: picker("online").with_event_modes(),
            image: ImageField {
                label: "Image link".to_string(),
                value: String::new(),
                credit: None,
                layout: ImageLayout::Cover,
                invalid: false,
            },
            tags: TagInput {
                value: String::new(),
            },
        }
        .render_once()
        .unwrap()
    }

    #[test]
    fn the_attendance_hint_describes_the_question_and_the_field() {
        let html = event_form(EventFormValues::default(), None);
        check!(
            html.contains(r#"<fieldset class="choice-group" aria-describedby="attendance-help""#)
        );
        check!(html.contains(r#"name="attendance" value="open" checked>"#));
        check!(html.contains("<span>Open to everyone</span>"));
        check!(html.contains("<span>Limited to a specific group</span>"));
        check!(html.contains(
            r#"<p id="attendance-help" class="field-help">If your event is limited to a specific group, tell people who can attend. For example, &ldquo;Library cardholders only,&rdquo; &ldquo;Open to K&ndash;12 teachers,&rdquo; or &ldquo;For members of our book club.&rdquo;</p>"#
        ));
        check!(html.contains("<div data-limited-only hidden>"));
        check!(html.contains("Who is eligible to attend?"));
        check!(!html.contains("limited-to-help"));

        let refused = submit_event(&[("attendance", "limited"), ("limited_to", "")]).unwrap_err();
        let values = EventFormValues {
            limited: true,
            ..EventFormValues::default()
        };
        let html = event_form(values, Some(refused));
        check!(html.contains(r#"name="attendance" value="limited" checked>"#));
        check!(html.contains("<div data-limited-only >"));
        let field = &html[html.find(r#"id="limited-to""#).unwrap()..];
        let field = &field[..field.find('>').unwrap()];
        check!(field.contains("attendance-help"), "{field}");
        check!(field.contains(r#"aria-invalid="true""#), "{field}");
    }

    #[test]
    fn the_group_is_checked_after_the_place_and_before_the_links() {
        let limited_unnamed = [("attendance", "limited"), ("limited_to", "")];

        let refused = submit_event(
            &[
                &limited_unnamed[..],
                &[("location_mode", "region"), ("url", "example.org")],
            ]
            .concat(),
        )
        .unwrap_err();
        check!(refused.is_about("location_description"));

        let refused =
            submit_event(&[&limited_unnamed[..], &[("url", "example.org")]].concat()).unwrap_err();
        check!(refused.is_about("limited_to"));
    }

    /// Validate an add-event submission: an online event that passes every
    /// check, with the fields in `changes` set as given (an empty value
    /// leaves one unanswered).
    fn submit_event(changes: &[(&str, &str)]) -> Result<EventAll, FormError> {
        let mut fields = vec![
            ("name", "Bioblitz".to_string()),
            ("description", "Count bugs".to_string()),
            ("location_mode", "online".to_string()),
            ("url", "https://example.org/bioblitz".to_string()),
            ("repeat_frequency", "once".to_string()),
            ("instances[0][begin_date]", "2099-11-01".to_string()),
            ("instances[0][begin_time]", "10:00".to_string()),
            ("instances[0][timezone]", "America/Chicago".to_string()),
        ];
        for (name, value) in changes {
            match fields.iter_mut().find(|(field, _)| field == name) {
                Some((_, old)) => *old = value.to_string(),
                None => fields.push((name, value.to_string())),
            }
        }

        let body = fields
            .iter()
            .map(|(name, value)| format!("{name}={}", urlencoding::encode(value)))
            .collect::<Vec<_>>()
            .join("&");
        let form: AddEventForm = serde_qs::Config::new(10, false)
            .deserialize_bytes(body.as_bytes())
            .unwrap();
        validate_and_build(&form, Ulid::new())
    }

    #[test]
    fn every_link_must_be_a_whole_web_address() {
        for (field, value) in [
            ("url", "https:example.org/bioblitz"),
            ("url", "https://example"),
            (ImageUrlInput::NAME, "example.org/photo.jpg"),
            (ImageUrlInput::NAME, "/-uploaded/photo.jpg"),
            ("ticket_url", "tickets.example.org"),
        ] {
            let refused = submit_event(&[(field, value)]).unwrap_err();
            check!(refused.is_about(field), "{value}");
        }

        let refused = submit_event(&[("ticket_url", "https:tickets")]).unwrap_err();
        check!(
            refused.message
                == "Please check the ticket / registration link; it should be a whole web address, such as https://example.org/tickets."
        );

        let uploaded = format!("{}/{}", crate::upload::storage::UNTRACKED_PATH, Ulid::new());
        let event = submit_event(&[
            (ImageUrlInput::NAME, &uploaded),
            ("ticket_url", " https://example.org/tickets "),
        ])
        .unwrap();
        check!(event.image == uploaded);
        check!(event.ticket_url == "https://example.org/tickets");
    }

    #[test]
    fn checks_come_in_orrerys_order() {
        // The name, then the dates, then the link's shape, then the
        // ticketing date, then the contact.
        let long_name = "n".repeat(EVENT_NAME_LIMIT + 1);
        let mut changes = vec![
            ("contact_email", "test@test"),
            ("ticketing_date", "2099-10-01"),
            ("url", "example.org"),
            ("repeat_frequency", "weekly"),
            ("name", long_name.as_str()),
        ];
        for field in [
            "name",
            "repeat_until",
            "url",
            "ticketing_timezone",
            "contact_email",
        ] {
            let refused = submit_event(&changes).unwrap_err();
            check!(refused.is_about(field));
            changes.pop();
        }
    }

    #[test]
    fn a_new_events_name_is_held_to_the_limit_in_characters() {
        let at_limit = "é".repeat(EVENT_NAME_LIMIT);
        let event = submit_event(&[("name", &at_limit)]).unwrap();
        check!(event.name == at_limit);

        let over = format!("{at_limit}!");
        let refused = submit_event(&[("name", &over)]).unwrap_err();
        check!(refused.is_about("name"));
        check!(refused.message == "Please shorten your event's name to 100 characters or fewer.");
    }

    #[test]
    fn a_repeating_event_needs_a_date_to_repeat_until() {
        let refused = submit_event(&[("repeat_frequency", "weekly")]).unwrap_err();
        check!(refused.is_about("repeat_until"));
        check!(refused.message == "Please choose the date the event repeats until.");

        let event = submit_event(&[
            ("repeat_frequency", "weekly"),
            ("repeat_until", "2099-11-15"),
        ])
        .unwrap();
        check!(event.instances.len() == 3);

        // Never repeating, a stray date is ignored.
        let event = submit_event(&[("repeat_until", "2099-11-15")]).unwrap();
        check!(event.instances.len() == 1);
    }

    #[test]
    fn the_add_form_asks_nothing_about_facebook() {
        use crate::event::facebook::FacebookEvent;

        let event = submit_event(&[]).unwrap();
        check!(event.facebook_event == FacebookEvent::Unanswered);
    }

    #[test]
    fn an_event_held_anywhere_has_no_place() {
        let event = submit_event(&[
            ("location_mode", "anywhere"),
            ("url", ""),
            ("city", "Chicago"),
            ("latitude", "41.88"),
            ("longitude", "-87.63"),
            ("regions", "x"),
        ])
        .unwrap();
        check!(event.location_type == ANYWHERE);
        check!(!event.entirely_online);
        check!(!event.also_online);
        check!(event.url.is_empty());
        check!(event.city.is_empty());
        check!(event.point.is_none());
        check!(event.regions.is_none());
    }

    #[test]
    fn each_mode_has_orrerys_location_type() {
        let at_address = |mode| vec![("location_mode", mode), ("city", "Chicago")];
        for (changes, location_type) in [
            (vec![], ANYWHERE),
            (at_address("in_person"), AT_AN_ADDRESS),
            (
                vec![
                    ("location_mode", "region"),
                    ("location_description", "The Midwest"),
                ],
                IN_A_RANGE,
            ),
        ] {
            let event = submit_event(&changes).unwrap();
            check!(event.location_type == location_type, "{changes:?}");
        }
    }

    /// The toggle's radio for a mode, as `picker` renders it.
    fn toggle_radio(html: &str, value: &str) -> Option<String> {
        let start = html.find(&format!(
            r#"<input type="radio" name="location_mode" value="{value}""#
        ))?;
        let end = start + html[start..].find('>')?;
        Some(html[start..=end].to_string())
    }

    fn picker(mode: &str) -> LocationPicker {
        LocationPicker::build(mode, "", "", "", "", "", "", "", "", "", "", "")
    }

    fn choice(list: ChoiceList, slug: &str, name: &str, orrery_id: Option<i64>) -> ChoiceValue {
        ChoiceValue {
            list,
            slug: slug.to_string(),
            name: name.to_string(),
            position: 0,
            orrery_id,
        }
    }

    /// The event types saved for an event in `mode` with BioBlitz, Library,
    /// City Nature Challenge and Online ticked, with Online in `online`'s
    /// list.
    fn types_ticked_in(mode: &str, online: &ChoiceValue) -> BTreeMap<ChoiceList, BTreeSet<String>> {
        let online_field = match online.list {
            ChoiceList::EventFormat => "formats",
            ChoiceList::EventProgram => "programs",
            ChoiceList::EventVenue => "venues",
            list => panic!("{list:?} isn't an event-type list"),
        };
        let body = format!(
            "location_mode={mode}&formats[]=bioblitz&venues[]=library\
             &programs[]=city-nature-challenge&{online_field}[]={}",
            online.slug
        );
        let form: AddEventForm = serde_qs::Config::new(10, false)
            .deserialize_bytes(body.as_bytes())
            .unwrap();
        event_type_picks(&form, Some(online))
    }

    fn saved(picks: &BTreeMap<ChoiceList, BTreeSet<String>>, list: ChoiceList) -> Vec<&str> {
        picks
            .get(&list)
            .into_iter()
            .flatten()
            .map(String::as_str)
            .collect()
    }

    #[test]
    fn online_comes_from_the_toggle_and_a_venue_only_with_an_address() {
        let online = choice(ChoiceList::EventFormat, "online", "Online", Some(405));
        check!(OrreryValue::EVENT_ONLINE.is(&online));

        let picks = types_ticked_in("in_person", &online);
        check!(saved(&picks, ChoiceList::EventFormat) == ["bioblitz"]);
        check!(saved(&picks, ChoiceList::EventVenue) == ["library"]);
        check!(saved(&picks, ChoiceList::EventProgram) == ["city-nature-challenge"]);

        let picks = types_ticked_in("online", &online);
        check!(saved(&picks, ChoiceList::EventFormat) == ["bioblitz", "online"]);
        check!(saved(&picks, ChoiceList::EventVenue).is_empty());
        check!(saved(&picks, ChoiceList::EventProgram) == ["city-nature-challenge"]);

        for mode in ["anywhere", "region"] {
            let picks = types_ticked_in(mode, &online);
            check!(
                saved(&picks, ChoiceList::EventFormat) == ["bioblitz"],
                "{mode}"
            );
            check!(saved(&picks, ChoiceList::EventVenue).is_empty(), "{mode}");
            check!(
                saved(&picks, ChoiceList::EventProgram) == ["city-nature-challenge"],
                "{mode}"
            );
        }
    }

    #[test]
    fn online_is_known_by_orrerys_id_in_whichever_list_it_is() {
        let renamed = choice(ChoiceList::EventFormat, "online", "Virtual", Some(405));
        check!(OrreryValue::EVENT_ONLINE.is(&renamed));
        check!(!OrreryValue::EVENT_ONLINE.is(&choice(
            ChoiceList::EventFormat,
            "online",
            "Online",
            None
        )));
        check!(!OrreryValue::EVENT_ONLINE.is(&choice(
            ChoiceList::Topic,
            "online",
            "Online",
            Some(405)
        )));

        let moved = choice(ChoiceList::EventProgram, "online", "Online", Some(405));
        let picks = types_ticked_in("online", &moved);
        check!(saved(&picks, ChoiceList::EventProgram) == ["city-nature-challenge", "online"]);
        check!(saved(&picks, ChoiceList::EventFormat) == ["bioblitz"]);
    }

    #[test]
    fn formats_are_alphabetical_with_other_last() {
        let formats = in_format_order(vec![
            choice(ChoiceList::EventFormat, "other", "Other", Some(208)),
            choice(ChoiceList::EventFormat, "webinar", "Webinar", Some(205)),
            choice(ChoiceList::EventFormat, "hackfest", "hackfest", Some(188)),
            choice(ChoiceList::EventFormat, "bioblitz", "BioBlitz", Some(187)),
            choice(ChoiceList::EventFormat, "outreach", "Outreach", None),
        ]);
        let names = formats
            .iter()
            .map(|value| value.name.as_str())
            .collect::<Vec<_>>();
        check!(names == ["BioBlitz", "hackfest", "Outreach", "Webinar", "Other"]);
    }

    #[test]
    fn the_event_choices_say_how_many_may_be_picked() {
        let audience = event_choices(
            "audience",
            "Audience",
            ChoiceList::Audience,
            vec![choice(ChoiceList::Audience, "families", "Families", None)],
            &[],
        )
        .render_once()
        .unwrap();
        check!(audience.contains("<legend>Audience (choose any)</legend>"));

        let topics = event_choices(
            "topics",
            "Topics",
            ChoiceList::Topic,
            vec![choice(ChoiceList::Topic, "birds", "Birds", None)],
            &["birds".to_string()],
        )
        .render_once()
        .unwrap();
        check!(topics.contains("<legend>Topics (up to 3)</legend>"));
        check!(topics.contains(r#"name="topics[]" value="birds" checked"#));
    }

    #[test]
    fn the_kind_of_venue_is_asked_with_the_address() {
        let venues = ChoiceCheckboxes::new(
            "venues",
            vec![
                choice(ChoiceList::EventVenue, "library", "Library", Some(406)),
                choice(
                    ChoiceList::EventVenue,
                    "museum-event",
                    "Museum event",
                    Some(190),
                ),
            ],
            BTreeSet::from(["library".to_string()]),
            None,
        );
        let html = picker("in_person")
            .with_event_modes()
            .with_venues(venues)
            .render_once()
            .unwrap();

        let at = |text: &str| html.find(text).unwrap();
        let kind = at("<legend>Kind of venue (optional)</legend>");
        check!(at("Venue &amp; address details") < kind);
        check!(kind < at(r#"data-mode-link="region""#));
        check!(kind < at(r#"data-mode="region""#));
        check!(html.contains(r#"name="venues[]" value="library" checked"#));
        check!(html.contains(r#"name="venues[]" value="museum-event">"#));

        let none = ChoiceCheckboxes::new("venues", Vec::new(), BTreeSet::new(), None);
        let html = picker("in_person").with_venues(none).render_once().unwrap();
        check!(!html.contains("Kind of venue"));
    }

    #[test]
    fn the_toggle_is_in_person_or_online() {
        let html = picker("").with_event_modes().render_once().unwrap();
        check!(html.contains("Is it in person or online?"));
        for (value, label) in [("in_person", "In person"), ("online", "Online")] {
            let radio = toggle_radio(&html, value).unwrap();
            check!(radio.contains(" required"));
            check!(!radio.contains(" checked"));
            check!(html.contains(&format!("<span>{label}</span>")));
        }
        check!(html.matches(r#"name="location_mode""#).count() == 2);
        check!(!html.contains("Hybrid"));
        check!(html.contains(
            ">Can people take part anywhere in the world? Choose that instead.</button>"
        ));

        let html = picker("").render_once().unwrap();
        check!(html.matches(r#"name="location_mode""#).count() == 2);
        check!(!html.contains("anywhere in the world"));
    }

    #[test]
    fn the_toggle_shows_in_person_for_a_region_or_anywhere() {
        for mode in ["region", "anywhere"] {
            let html = picker(mode).with_event_modes().render_once().unwrap();
            let radio = toggle_radio(&html, mode).unwrap();
            check!(radio.contains(" data-in-person"), "{mode}");
            check!(radio.contains(" checked"), "{mode}");
            check!(toggle_radio(&html, "in_person").is_none(), "{mode}");
        }

        let html = picker("anywhere").with_event_modes().render_once().unwrap();
        check!(
            html.contains(r#"<p class="field-help" data-only-mode="anywhere" >People take part"#)
        );
        check!(html.contains(
            r#"data-mode-link="in_person" data-only-mode="anywhere" >Back to a single place</button>"#
        ));
    }

    #[test]
    fn a_refused_mode_marks_the_toggle() {
        let error = FormError::at(
            "location_mode",
            "Please choose where the event takes place.",
        );
        let html = picker("")
            .with_event_modes()
            .errors(Some(&error))
            .render_once()
            .unwrap();
        for value in ["in_person", "online"] {
            check!(
                toggle_radio(&html, value)
                    .unwrap()
                    .contains(r#"aria-invalid="true""#)
            );
        }
    }

    #[test]
    fn the_address_disclosure_reads_as_a_link_until_a_place_is_picked() {
        let html = picker("in_person").render_once().unwrap();
        check!(html.contains(r#"<span class="place-summary" data-place-summary hidden></span>"#));
        check!(html.contains(
            r#"<span class="place-summary-action" data-place-summary-action>Venue &amp; address details</span>"#
        ));
    }

    fn submit_project(extra: &str) -> Result<ProjectAll, FormError> {
        let body = format!("name=Bugs&description=Count+bugs{extra}");
        let form: AddProjectForm = serde_qs::Config::new(10, false)
            .deserialize_bytes(body.as_bytes())
            .unwrap();
        validate_and_build_project(&form, Ulid::new())
    }

    #[test]
    fn an_implausible_project_contact_email_is_refused() {
        let online = "&location_mode=online&url=https://example.org/bugs";

        let refused = submit_project(&format!("{online}&contact_email=test@test")).unwrap_err();
        check!(refused.is_about("contact_email"));

        let project = submit_project(&format!("{online}&contact_email=name@example.org")).unwrap();
        check!(project.contacts[0].email == "name@example.org");

        let uncontacted = submit_project(online).unwrap();
        check!(uncontacted.contacts.is_empty());
    }

    #[test]
    fn a_project_is_never_anywhere() {
        let refused =
            submit_project("&location_mode=anywhere&city=Chicago&url=https://example.org/bugs")
                .unwrap_err();
        check!(refused.is_about("location_mode"));
    }

    #[test]
    fn past_start_dates_are_judged_in_the_events_zone() {
        // 02:00 UTC on 2026-09-21 is still the evening of the 20th in Chicago.
        let now: Zoned = "2026-09-21T02:00:00+00:00[UTC]".parse().unwrap();

        let chicago_evening = parse_datetime("2026-09-20", "21:00", "America/Chicago").unwrap();
        assert!(!starts_before_today(&chicago_evening, &now));

        let chicago_yesterday = parse_datetime("2026-09-19", "21:00", "America/Chicago").unwrap();
        assert!(starts_before_today(&chicago_yesterday, &now));

        let tokyo_same_instant = parse_datetime("2026-09-20", "21:00", "Asia/Tokyo").unwrap();
        assert!(starts_before_today(&tokyo_same_instant, &now));
    }

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
}
