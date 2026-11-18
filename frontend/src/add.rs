use common::message::log::PageChange;
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use jiff::{Zoned, civil};
use poem::{
    IntoResponse, Response, handler,
    session::Session,
    web::{Html, Query, Redirect},
};
use sailfish::TemplateOnce;
use serde::{Deserialize, Serialize};
use ulid::Ulid;

use common::{
    message::location::{GeocodeReverseRequest, GeocodeReverseResponse},
    message::user::BasicInfo,
    service::log_activity,
    shared::db,
    timestring::TimeString,
};

use crate::{
    choice::{ChoiceCheckboxes, ChoiceList, ChoiceValue, OrreryValue, limited_heading},
    component::{
        Component, Head, StandardLayout,
        date_row::DateRowValues,
        floating_field::FormError,
        image_field::{ImageField, ImageLayout, ImageUploadResult, ImageUrlInput},
        location_picker::LocationPicker,
    },
    contact::{Contact, ContactRole},
    edit_access::Editor,
    entity::{Entity, EntityKind, EntityTarget},
    event::{
        EventInstance,
        all::EventAll,
        location::{EventLocation, LocationInput, VirtualJoin, picked_point, settle_online},
        rules::{self, DateRow, Field, Refusal, RowReading, ZonedInstance, stored_instance},
    },
    exchange::SubmissionExchange,
    form::QsForm,
    middleware::authorization::Authenticated,
    peering::LoadStorePeered,
    project::{
        all::ProjectAll,
        location::{ANYWHERE, AT_AN_ADDRESS, IN_A_RANGE, LocationMode, missing_place},
    },
    submission::{Arrival, Submission, SubmissionKind, announce},
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

/// Where the event or project takes place. Every mode also permits a link,
/// so a venue or a region can carry a livestream or information link.
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

/// A recurrence is never expanded beyond this many occurrences per base
/// date/time, as a guard against runaway input.
const MAX_OCCURRENCES: usize = 366;

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
    /// How people join a virtual event: `link`, at its link, or `other`, by
    /// its instructions.
    #[serde(default)]
    virtual_join: String,
    /// What was left typed in the location picker's place search; read only
    /// to say, when no address was given, that it wasn't used as one.
    #[serde(default)]
    place_query: String,
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
    /// A contact form or page staff can reach the organizer through, beside
    /// or instead of `contact_email`.
    #[serde(default)]
    contact_url: String,
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

impl AddEventForm {
    /// Whether the event is restricted to a group.
    fn limited(&self) -> bool {
        self.attendance == Attendance::Limited
    }

    /// The slugs of the ages ticked for the event, which are asked only of
    /// an event restricted to a group: an event open to everyone ticks
    /// none, whatever was left ticked. An event ticking none is saved with
    /// the ages [`crate::event::ages_for_new_event`] gives it.
    fn audience(&self) -> impl Iterator<Item = String> + '_ {
        self.limited()
            .then_some(&self.audience)
            .into_iter()
            .flatten()
            .cloned()
    }
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

// ---------------------------------------------------------------------------
// Add Event: form-value carriers (for first render and re-render on error)
// ---------------------------------------------------------------------------

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
    virtual_join: String,
    place_query: String,
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
    contact_url: String,
    contact_phone: String,
    contact_address: String,
    ticketing_date: String,
    ticketing_time: String,
    ticketing_timezone: String,
    ticket_url: String,
    cost: String,
    repeat_frequency: String,
    repeat_until: String,
    instances: Vec<DateRowValues>,
    /// Whether the event is limited to a group, and the group.
    limited: bool,
    limited_to: String,
}

impl From<&AddEventForm> for EventFormValues {
    fn from(f: &AddEventForm) -> Self {
        let mut instances: Vec<DateRowValues> = f
            .instances
            .iter()
            .map(|i| DateRowValues {
                begin_date: i.begin_date.clone(),
                begin_time: i.begin_time.clone(),
                timezone: i.timezone.clone(),
                has_end: i.has_end.is_some(),
                end_date: i.end_date.clone(),
                end_time: i.end_time.clone(),
            })
            .collect();

        if instances.is_empty() {
            instances.push(DateRowValues::default());
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
            virtual_join: f.virtual_join.clone(),
            place_query: f.place_query.clone(),
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
            contact_url: f.contact_url.clone(),
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
    /// aren't asked with its location or who can attend.
    categories: ChoiceGroup,
    /// The ages the event suits, asked of an event restricted to a group.
    audience: ChoiceCheckboxes,
    location: LocationPicker,
    image: ImageField,
}

// ---------------------------------------------------------------------------
// Shared add-form components (used by both /add/event and /add/project)
// ---------------------------------------------------------------------------

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
    Presenter,
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
    /// Tags, asked after the lists, where the section is about how people
    /// find what is added.
    tags: Option<TagInput>,
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
            tags: None,
            invalid: false,
        }
    }

    /// Ask for tags after the lists.
    fn with_tags(mut self, tags: TagInput) -> Self {
        self.tags = Some(tags);
        self
    }

    /// Mark the section invalid when `error` is about one of its fields.
    fn errors(mut self, error: Option<&FormError>) -> Self {
        self.invalid = error.is_some_and(|e| self.fields.iter().any(|field| e.is_about(field)));
        self
    }
}

/// Checkboxes for picking from `values` of `list` as the add-event form's
/// `field`, under the list's heading and how many may be picked.
fn event_choices(
    field: &'static str,
    list: ChoiceList,
    values: Vec<ChoiceValue>,
    picked: &[String],
) -> ChoiceCheckboxes {
    let limit = crate::event::pick_limit(list);
    ChoiceCheckboxes::new(field, values, picked.iter().cloned().collect(), limit)
        .with_legend(crate::event::choice_legend(list))
        .with_help(crate::event::choice_help(list))
        .stacked(crate::event::choice_stacked(list))
}

/// What an add-event submission picks from each event-type list: the
/// formats and programs ticked, the kinds of venue ticked when the event is
/// held at an address, and `online`, the value saying an event is held
/// online, exactly when it is, whichever list that value is in.
fn event_type_picks(
    form: &AddEventForm,
    online: Option<&ChoiceValue>,
) -> BTreeMap<ChoiceList, BTreeSet<String>> {
    let at_address = form.location_mode.is_some_and(LocationMode::at_address);
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
        settle_online(
            picks.entry(online.list).or_default(),
            online,
            form.location_mode == Some(LocationMode::Online),
        );
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
    values.instances.push(DateRowValues::default());

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

    match validate_and_build(&form, Editor::from(&*user_info)) {
        Ok(mut event) => {
            // Submitted through an exchange, it waits in that exchange's
            // approval queue (`reviewed` starts false), unless the exchange
            // publishes it as it arrives.
            let exchange = SubmissionExchange::find(&form.exchange).await?;
            let published = published_on_arrival(exchange.as_ref(), Some(&user_info)).await?;
            event.exchange = exchange
                .as_ref()
                .map(|exchange| exchange.exchange_id.clone());
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
            event.audience = crate::event::ages_for_new_event(
                ChoiceValue::known(ChoiceList::Audience, form.audience()).await?,
            )
            .await?;

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
    .with_place_query(&values.place_query)
    .for_events(&values.virtual_join, &values.how_to_join)
    .with_venues(ChoiceCheckboxes::new(
        "venues",
        crate::event::offered_types(ChoiceList::EventVenue).await?,
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
    // Whether the event is online, and the kind of venue it's held at, are
    // asked with its location, and the ages it suits with who can attend;
    // the rest of what it picks is here.
    let audience = event_choices(
        "audience",
        ChoiceList::Audience,
        ChoiceValue::all(ChoiceList::Audience).await?,
        &values.audience,
    );
    let categories = ChoiceGroup::new(
        SectionIcon::Activities,
        "Topics and event type",
        vec![
            event_choices(
                "topics",
                ChoiceList::Topic,
                ChoiceValue::all(ChoiceList::Topic).await?,
                &values.topics,
            ),
            event_choices(
                "formats",
                ChoiceList::EventFormat,
                crate::event::in_format_order(
                    crate::event::offered_types(ChoiceList::EventFormat).await?,
                ),
                &values.formats,
            ),
            event_choices(
                "programs",
                ChoiceList::EventProgram,
                crate::event::offered_types(ChoiceList::EventProgram).await?,
                &values.programs,
            ),
        ],
    )
    .with_tags(TagInput {
        value: values.tags.clone(),
    })
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
        audience,
        location,
        image,
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
           ) WHERE tag LIKE ? ORDER BY tag"#,
        pattern
    )
    .fetch_all(db())
    .await?
    .into_iter()
    // Staff's own tags are never offered, as orrery never offers them.
    .filter(|tag| !crate::tag::is_hidden(tag))
    .take(10)
    .collect();

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
    // deduplicated, so ask for more than we intend to show.
    let suggestions = photon("api", &[("q", q), ("limit", "10"), ("lang", "en")]).await?;

    Ok(Response::builder()
        .content_type("application/json")
        .body(serde_json::to_string(&suggestions)?))
}

/// GET `/add/places-near?lat=&lon=` — the places nearest the visitor, for
/// the location picker's "use my location" button: at most five, within a
/// kilometer, nearest first, as `/add/place-search` lists the places it
/// finds.
#[handler]
#[tracing::instrument]
pub async fn add_places_near(
    Authenticated(user_info): Authenticated,
    Query(point): Query<PointQuery>,
) -> Result<Response, anyhow::Error> {
    if user_info.is_none() {
        return Ok(Response::builder()
            .status(poem::http::StatusCode::UNAUTHORIZED)
            .finish());
    }

    if !point.on_earth() {
        return Ok(Response::builder()
            .status(poem::http::StatusCode::BAD_REQUEST)
            .finish());
    }

    let (lat, lon) = (point.lat.to_string(), point.lon.to_string());
    let suggestions = photon(
        "reverse",
        &[
            ("lat", lat.as_str()),
            ("lon", lon.as_str()),
            ("limit", "10"),
            ("radius", "1"),
            ("lang", "en"),
        ],
    )
    .await?;

    Ok(Response::builder()
        .content_type("application/json")
        .body(serde_json::to_string(&suggestions)?))
}

/// The suggestions a Photon endpoint (`api`, the search, or `reverse`)
/// answers `params` with: both answer in the same GeoJSON.
async fn photon(
    endpoint: &str,
    params: &[(&str, &str)],
) -> Result<Vec<PlaceSuggestion>, anyhow::Error> {
    let response: serde_json::Value = reqwest::Client::new()
        .get(format!("https://photon.komoot.io/{endpoint}"))
        .query(params)
        .header("User-Agent", "SciStarter/1.0 (+https://scistarter.org)")
        .send()
        .await?
        .json()
        .await?;

    Ok(distinct_suggestions(
        response
            .get("features")
            .and_then(|f| f.as_array())
            .map(|features| features.iter().filter_map(photon_suggestion).collect())
            .unwrap_or_default(),
    ))
}

/// The first five of `suggestions` a visitor could tell apart: those that
/// differ in name, kind or context. Photon ranks better matches, and nearer
/// places, first, so the first of each is kept.
fn distinct_suggestions(mut suggestions: Vec<PlaceSuggestion>) -> Vec<PlaceSuggestion> {
    let mut seen = std::collections::HashSet::new();
    suggestions.retain(|s| seen.insert((s.name.clone(), s.kind.clone(), s.context.clone())));
    suggestions.truncate(5);
    suggestions
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

/// A point, as a browser's geolocation or a picked place gives it.
#[derive(Debug, Deserialize)]
struct PointQuery {
    lat: f64,
    lon: f64,
}

impl PointQuery {
    fn on_earth(&self) -> bool {
        (-90.0..=90.0).contains(&self.lat) && (-180.0..=180.0).contains(&self.lon)
    }
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
    Query(query): Query<PointQuery>,
) -> Result<Response, anyhow::Error> {
    if user_info.is_none() {
        return Ok(Response::builder()
            .status(poem::http::StatusCode::UNAUTHORIZED)
            .finish());
    }

    if !query.on_earth() {
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
        timezone: crate::event::zone::name_at(latitude, longitude),
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
fn validate_and_build(form: &AddEventForm, author: Editor) -> Result<EventAll, FormError> {
    let user_id = author.user_id;

    rules::image_link(&form.image, None).map_err(refused)?;
    let image = form.image.trim();

    rules::name(&form.name, None).map_err(refused)?;
    let name = form.name.trim();

    // Resolve the date/time rows that carry a start.
    let now = Zoned::now();
    let mut base: Vec<ZonedInstance> = Vec::new();
    for (i, inst) in form.instances.iter().enumerate() {
        let row = DateRow {
            begin_date: &inst.begin_date,
            begin_time: &inst.begin_time,
            end: inst
                .has_end
                .is_some()
                .then_some((inst.end_date.as_str(), inst.end_time.as_str())),
            timezone: &inst.timezone,
        };
        match rules::date_row(row, None, &now) {
            Ok(RowReading::Dated(instance)) => base.push(instance),
            Ok(RowReading::Blank | RowReading::Kept) => {} // an untouched row
            Err(refusal) => return Err(refused_in_row(refusal, i)),
        }
    }
    rules::some_date(!base.is_empty()).map_err(|refusal| refused_in_row(refusal, 0))?;

    let instances = expand_recurrence(base, form)?;

    // Where it is held, and how people join it: the link is saved only
    // when the form asks for it.
    let mode = rules::mode(form.location_mode).map_err(refused)?;
    let choice = VirtualJoin::parse(&form.virtual_join);
    let url = rules::join(mode, choice, &form.url, &form.how_to_join)
        .map_err(refused)?
        .trim()
        .to_string();
    rules::place(mode, &form.address, &form.city, &form.place_query).map_err(refused)?;
    rules::location_note(mode, &form.location_description).map_err(refused)?;
    rules::link(&url, None).map_err(refused)?;

    rules::description(&form.description).map_err(refused)?;
    let description = form.description.trim();

    let topics = form.topics.iter().collect::<BTreeSet<_>>().len();
    if let Some(refusal) = crate::event::refuse_picks(ChoiceList::Topic, topics) {
        return Err(FormError::at("topics", refusal));
    }

    rules::ticket_link(&form.ticket_url, None).map_err(refused)?;
    let ticket_url = form.ticket_url.trim();

    // Optional ticketing/registration open date.
    let ticketing_begins = rules::ticketing(
        &form.ticketing_date,
        &form.ticketing_time,
        &form.ticketing_timezone,
    )
    .map_err(refused)?
    .map(TimeString::from);

    let limited = form.limited();
    rules::limited_to(&form.limited_to, limited).map_err(refused)?;
    let limited_to = if limited { form.limited_to.trim() } else { "" };

    // The organizer contact: someone staff can ask about the event, and
    // some way to reach them.
    rules::contact(
        &form.contact_name,
        &form.contact_email,
        &form.contact_url,
        None,
    )
    .map_err(refused)?;
    let contact_email = form.contact_email.trim();
    let contact_url = form.contact_url.trim();

    let mut event = EventAll::default();
    event.event_id = Ulid::new();
    event.created = TimeString::now();
    event.page_creator_id = user_id;
    event.origin = "scistarter".to_string();
    event.name = name.to_string();
    event.description = description.to_string();
    event.presenter = form.presenter.trim().to_string();
    event.image = image.to_string();
    event.image_credit = form.image_credit.trim().to_string();
    event.part_of = form.part_of.trim().parse::<Ulid>().ok();
    event.ticket_url = ticket_url.to_string();
    event.cost = form.cost.trim().to_string();
    event.limited_to = limited_to.to_string();
    event.ticketing_begins = ticketing_begins;
    event.tags = crate::tag::as_set_by(crate::tag::split(&form.tags), [], author.is_staff);

    event.contacts = Box::new([Contact {
        contact_id: Ulid::new(),
        role: ContactRole::ContactPerson,
        name: empty_to_none(&form.contact_name),
        affiliation: empty_to_none(&form.contact_affiliation),
        email: contact_email.to_string(),
        phone: empty_to_none(&form.contact_phone),
        address: empty_to_none(&form.contact_address),
        url: empty_to_none(contact_url),
    }]);

    EventLocation::settle(
        LocationInput {
            mode,
            location_name: &form.location_name,
            address: &form.address,
            city: &form.city,
            state: &form.state,
            zip: &form.zip,
            country: &form.country,
            latitude: &form.latitude,
            longitude: &form.longitude,
            location_description: &form.location_description,
            url: &url,
            how_to_join: &form.how_to_join,
            regions: &form.regions,
            venues: &form.venues,
        },
        None,
    )
    .apply_to(&mut event);

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

/// The add event form's control a refusal is about.
fn refused(refusal: Refusal) -> FormError {
    refused_in_row(refusal, 0)
}

/// The add event form's controls a refusal is about, a date's in date/time
/// row `row`.
fn refused_in_row(refusal: Refusal, row: usize) -> FormError {
    let name = |field: Field| match field {
        Field::Name => "name".to_string(),
        Field::Description => "description".to_string(),
        Field::ImageLink => ImageUrlInput::NAME.to_string(),
        Field::Url => "url".to_string(),
        Field::VirtualJoin => "virtual_join".to_string(),
        Field::HowToJoin => "how_to_join".to_string(),
        Field::TicketUrl => "ticket_url".to_string(),
        Field::LimitedTo => "limited_to".to_string(),
        Field::LocationMode => "location_mode".to_string(),
        Field::Address => "address".to_string(),
        Field::LocationDescription => "location_description".to_string(),
        Field::TicketingDate => "ticketing_date".to_string(),
        Field::TicketingTimezone => "ticketing_timezone".to_string(),
        Field::ContactName => "contact_name".to_string(),
        Field::ContactEmail => "contact_email".to_string(),
        Field::ContactUrl => "contact_url".to_string(),
        Field::Date(part) => format!("instances[{row}][{}]", part.name()),
    };
    refusal.also.iter().fold(
        FormError::at(name(refusal.field), refusal.message),
        |error, field| error.also(name(*field)),
    )
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
                    out.push(stored_instance(&begin, end.as_ref()));
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
            None => out.push(inst.stored()),
        }
    }

    Ok(out)
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
    /// What was left typed in the location picker's place search.
    #[serde(default)]
    place_query: String,
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
    place_query: String,
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
            place_query: f.place_query.clone(),
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

    match validate_and_build_project(&form, Editor::from(&*user_info)) {
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
    .with_place_query(&values.place_query)
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
    author: Editor,
) -> Result<ProjectAll, FormError> {
    let user_id = author.user_id;
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
            if let Some(message) =
                missing_place(&form.address, &form.city, &form.place_query, "project")
            {
                return Err(FormError::at("address", message));
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
    project.tags = crate::tag::as_set_by(crate::tag::split(&form.tags), [], author.is_staff);

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
            url: None,
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

    /// Someone, not staff, adding a page.
    fn test_author() -> Editor {
        Editor {
            user_id: Ulid::new(),
            is_staff: false,
        }
    }

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
        let submit = |email: &str| submit_event(&[("contact_email", email)]);

        let refused = submit("test@test").unwrap_err();
        check!(refused.is_about("contact_email"));

        let event = submit("name@example.org").unwrap();
        check!(event.contacts[0].email == "name@example.org");
    }

    #[test]
    fn staff_need_the_contact_persons_name() {
        for name in ["", "   "] {
            let refused = submit_event(&[("contact_name", name)]).unwrap_err();
            check!(refused.is_about("contact_name"));
            check!(refused.message == "Please add the contact person's name.");
        }

        let event = submit_event(&[("contact_name", " Ada Organizer ")]).unwrap();
        check!(event.contacts[0].name.as_deref() == Some("Ada Organizer"));
    }

    #[test]
    fn staff_need_an_email_or_a_contact_form_link() {
        let refused = submit_event(&[("contact_email", "")]).unwrap_err();
        check!(refused.is_about("contact_email"));
        check!(
            refused.message
                == "Please add the contact person's email, or a link to a contact form."
        );
        // Either would do, so both are marked; the message is told once.
        check!(refused.is_about("contact_url"));
        let html = event_form(fresh_event(), Some(refused));
        for name in ["contact_email", "contact_url"] {
            let at = html.find(&format!(r#"name="{name}""#)).unwrap();
            check!(html[at..at + html[at..].find('>').unwrap()].contains(r#"aria-invalid="true""#));
        }
        check!(html.matches("email, or a link to a contact form.").count() == 1);

        // An implausible email is the email's problem alone.
        let refused = submit_event(&[("contact_email", "test@test")]).unwrap_err();
        check!(!refused.is_about("contact_url"));

        let event = submit_event(&[
            ("contact_email", ""),
            ("contact_url", " https://example.org/contact "),
        ])
        .unwrap();
        check!(event.contacts.len() == 1);
        check!(event.contacts[0].role == ContactRole::ContactPerson);
        check!(event.contacts[0].email == "");
        check!(event.contacts[0].url.as_deref() == Some("https://example.org/contact"));

        let both = submit_event(&[("contact_url", "https://example.org/contact")]).unwrap();
        check!(both.contacts[0].email == "organizer@example.org");
        check!(both.contacts[0].url.as_deref() == Some("https://example.org/contact"));

        let emailed = submit_event(&[]).unwrap();
        check!(emailed.contacts[0].url == None);
    }

    #[test]
    fn a_contact_form_link_is_a_whole_web_address() {
        for link in [
            "example.org/contact",
            "ftp://example.org/contact",
            "https://localhost/",
        ] {
            let refused = submit_event(&[("contact_url", link)]).unwrap_err();
            check!(refused.is_about("contact_url"));
            check!(
                refused.message
                    == "Please check the contact form link; it should be a whole web address, such as https://example.org/contact."
            );
        }
    }

    #[test]
    fn an_online_event_has_no_location_note() {
        let note = ("location_description", "Meet at the north entrance");

        let online = submit_event(&[note]).unwrap();
        check!(online.location_description == "");

        let in_person = submit_event(&[
            ("location_mode", "in_person"),
            ("address", "1 Main St"),
            note,
        ])
        .unwrap();
        check!(in_person.location_description == "Meet at the north entrance");

        let anywhere = submit_event(&[("location_mode", "anywhere"), note]).unwrap();
        check!(anywhere.location_description == "Meet at the north entrance");
    }

    #[test]
    fn words_left_in_the_place_search_are_not_an_address() {
        let in_person = [("location_mode", "in_person"), ("url", "")];

        let refused = submit_event(&in_person).unwrap_err();
        check!(refused.is_about("address"));
        check!(refused.message == "Please provide an address or city for the in-person event.");

        let mut typed = in_person.to_vec();
        typed.push(("place_query", "Shedd Aquarium loading dock"));
        let refused = submit_event(&typed).unwrap_err();
        check!(refused.is_about("address"));
        check!(
            refused.message
                == "Please choose a place from the list, or choose to use what you typed as the address."
        );

        // Used as the address, the words are the street address.
        typed.push(("address", "Shedd Aquarium loading dock"));
        let event = submit_event(&typed).unwrap();
        check!(event.address == "Shedd Aquarium loading dock");
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
                == "Please say which group can attend, such as \u{201c}Library cardholders.\u{201d}"
        );

        let at_limit = "é".repeat(rules::LIMITED_TO_LIMIT);
        let event = submit_event(&[("attendance", "limited"), ("limited_to", &at_limit)]).unwrap();
        check!(event.limited_to == at_limit);

        let over = format!("{at_limit}!");
        let refused =
            submit_event(&[("attendance", "limited"), ("limited_to", &over)]).unwrap_err();
        check!(refused.is_about("limited_to"));
        check!(refused.message == "Please say which group can attend in 140 characters or fewer.");
    }

    /// What a fresh add-event form shows: one date, not repeating.
    fn fresh_event() -> EventFormValues {
        EventFormValues {
            repeat_frequency: RepeatFrequency::Once.as_str().to_string(),
            instances: vec![DateRowValues::default()],
            ..Default::default()
        }
    }

    /// The add-event form, showing `values` and refused over `error`.
    fn event_form(values: EventFormValues, error: Option<FormError>) -> String {
        AddEventContent {
            error,
            values,
            exchange: None,
            publishes: false,
            categories: ChoiceGroup::new(SectionIcon::Activities, "Categories", Vec::new())
                .with_tags(TagInput {
                    value: String::new(),
                }),
            audience: event_choices(
                "audience",
                ChoiceList::Audience,
                vec![choice(ChoiceList::Audience, "families", "Families", None)],
                &[],
            ),
            location: picker("online").for_events("", ""),
            image: ImageField {
                label: "Image link".to_string(),
                value: String::new(),
                credit: None,
                layout: ImageLayout::Cover,
                invalid: false,
            },
        }
        .render_once()
        .unwrap()
    }

    #[test]
    fn the_audience_is_asked_with_who_can_attend() {
        let html = event_form(EventFormValues::default(), None);
        let at = |text: &str| html.find(text).unwrap();
        check!(
            at("<legend>Who can attend?<")
                < at("<legend>What ages is this event suitable for?</legend>")
        );
    }

    #[test]
    fn the_sections_come_in_facebooks_order() {
        let html = event_form(fresh_event(), None);
        let at = |text: &str| html.find(text).unwrap();
        let order = [
            "Event name",
            "Start date",
            "Is it in person or virtual?",
            "What are the details?",
            // The instructions below the details, rather than as the way to
            // join a virtual event.
            "data-instructions-below",
            "Categories",
            "Organizer / presenter",
            "Tickets / registration",
            "Who can attend",
            "Part of a larger event",
            "Organizer contact",
        ];
        for pair in order.windows(2) {
            check!(at(pair[0]) < at(pair[1]), "{} before {}", pair[0], pair[1]);
        }
    }

    #[test]
    fn who_can_attend_asks_ages_only_once_restricted() {
        let html = event_form(EventFormValues::default(), None);
        check!(html.contains(r#"<details class="add-section" data-attendance>"#));
        check!(!html.contains("add-section-summary"));

        let restricted = html
            .find(r#"<div class="attendance-ages" data-limited-only hidden>"#)
            .unwrap();
        let ages = html
            .find("<legend>What ages is this event suitable for?</legend>")
            .unwrap();
        let section_end = restricted + html[restricted..].find("</details>").unwrap();
        check!(restricted < ages && ages < section_end);
        // After the ages, the restricted part and the section's body close.
        let after = &html[ages..section_end];
        check!(after.matches("</div>").count() == after.matches("<div").count() + 2);
        check!(html[ages..].contains(r#"<div class="choice-checkboxes stacked-options">"#));
    }

    #[test]
    fn each_toggle_is_a_list_of_radio_buttons() {
        let html = event_form(EventFormValues::default(), None);
        check!(!html.contains("toggle-option"));
        check!(html.matches(r#"<div class="radio-options">"#).count() == 2);
        check!(html.contains(r#"<label class="radio-option">"#));
    }

    #[test]
    fn repetition_is_offered_under_the_first_date_alone() {
        let mut values = fresh_event();
        values.instances.push(DateRowValues::default());
        let html = event_form(values, None);
        check!(html.matches("data-repeat-toggle ").count() == 1);
        check!(html.matches("Add end time").count() == 2);
        // Put away until it's asked for, unless the event repeats already.
        check!(html.contains(r#"data-repeat hidden"#));
        let html = event_form(
            EventFormValues {
                repeat_frequency: "weekly".to_string(),
                ..fresh_event()
            },
            None,
        );
        check!(html.contains(r#"data-repeat-toggle hidden"#));
        check!(!html.contains(r#"data-repeat hidden"#));
    }

    #[test]
    fn the_instructions_are_asked_in_one_place_at_a_time() {
        let instructions = |mode: &str, join: &str| {
            let html = event_form(
                EventFormValues {
                    location_mode: mode.to_string(),
                    virtual_join: join.to_string(),
                    ..Default::default()
                },
                None,
            );
            let at = html.find("data-instructions-below").unwrap();
            html[at..at + html[at..].find('>').unwrap()].ends_with("hidden disabled")
        };
        check!(instructions("online", "other"));
        check!(!instructions("online", "link"));
        check!(!instructions("in_person", "other"));
    }

    #[test]
    fn the_contact_is_reached_by_email_or_contact_form() {
        let html = event_form(EventFormValues::default(), None);
        check!(html.contains(r#"name="contact_email""#));
        check!(html.contains(r#"name="contact_url""#));
        check!(html.contains("Contact form link"));
        check!(html.contains("Please give an email address, a link to a contact form, or both."));
    }

    #[test]
    fn each_way_to_reach_the_contact_is_required_until_the_other_is_given() {
        let required = |email: &str, url: &str| {
            let html = event_form(
                EventFormValues {
                    contact_email: email.to_string(),
                    contact_url: url.to_string(),
                    ..fresh_event()
                },
                None,
            );
            let input = |name: &str| {
                let at = html.find(&format!(r#"name="{name}""#)).unwrap();
                html[at..at + html[at..].find('>').unwrap()].contains(" required")
            };
            (input("contact_email"), input("contact_url"))
        };
        check!(required("", "") == (true, true));
        check!(required("ada@example.org", "") == (true, false));
        check!(required("", "https://example.org/contact") == (false, true));
        // Each is marked either way, so the page can require it again.
        let html = event_form(fresh_event(), None);
        let at = html.find("data-one-required").unwrap();
        check!(html[at..].find("Contact form link").is_some());
        check!(html[at..].matches("pill-required").count() >= 2);
    }

    #[test]
    fn the_attendance_help_describes_the_group_field() {
        let html = event_form(EventFormValues::default(), None);
        check!(html.contains(
            r#"<fieldset class="choice-group">
              <legend>Who can attend?<span class="pill-required" aria-hidden="true"> *</span></legend>"#
        ));
        check!(html.contains(r#"name="attendance" value="open" checked>"#));
        check!(html.contains("<span>Open to everyone</span>"));
        check!(html.contains("<span>Restricted to a specific group</span>"));
        // The group is asked under the answer that asks it, inside the list
        // of answers.
        let help = r#"<p id="attendance-help" class="field-help">List any requirements for attending this event.</p>"#;
        let restricted = html
            .find("<span>Restricted to a specific group</span>")
            .unwrap();
        let field = html
            .find(r#"<div class="radio-option-field" data-limited-only hidden>"#)
            .unwrap();
        let help_at = html.find(help).unwrap();
        let answers_end = restricted + html[restricted..].find("</fieldset>").unwrap();
        check!(restricted < field && field < help_at && help_at < answers_end);
        check!(html.contains("Which group can attend?"));
        check!(html.contains(r#"placeholder="e.g., Library cardholders, Girl Scouts""#));
        check!(!html.contains("limited-to-help"));

        let refused = submit_event(&[("attendance", "limited"), ("limited_to", "")]).unwrap_err();
        let values = EventFormValues {
            limited: true,
            ..EventFormValues::default()
        };
        let html = event_form(values, Some(refused));
        check!(html.contains(r#"name="attendance" value="limited" checked>"#));
        check!(html.contains(r#"<div class="radio-option-field" data-limited-only >"#));
        let field = &html[html.find(r#"id="limited-to""#).unwrap()..];
        let field = &field[..field.find('>').unwrap()];
        check!(field.contains("attendance-help"), "{field}");
        check!(field.contains(r#"aria-invalid="true""#), "{field}");
    }

    #[test]
    fn the_group_is_checked_after_the_tickets_and_before_the_contact() {
        let limited_unnamed = [("attendance", "limited"), ("limited_to", "")];

        let refused = submit_event(
            &[
                &limited_unnamed[..],
                &[("ticket_url", "tickets.example.org")],
            ]
            .concat(),
        )
        .unwrap_err();
        check!(refused.is_about("ticket_url"));

        let refused =
            submit_event(&[&limited_unnamed[..], &[("contact_name", "")]].concat()).unwrap_err();
        check!(refused.is_about("limited_to"));
    }

    /// Validate an add-event submission: an online event that passes every
    /// check, with the fields in `changes` set as given (an empty value
    /// leaves one unanswered).
    fn submit_event(changes: &[(&str, &str)]) -> Result<EventAll, FormError> {
        validate_and_build(&event_form_submitted(changes), test_author())
    }

    /// An add-event submission, otherwise complete, with the fields in
    /// `changes` set as given.
    fn event_form_submitted(changes: &[(&str, &str)]) -> AddEventForm {
        let mut fields = vec![
            ("name", "Bioblitz".to_string()),
            ("description", "Count bugs".to_string()),
            ("location_mode", "online".to_string()),
            ("virtual_join", "link".to_string()),
            ("url", "https://example.org/bioblitz".to_string()),
            ("repeat_frequency", "once".to_string()),
            ("instances[0][begin_date]", "2099-11-01".to_string()),
            ("instances[0][begin_time]", "10:00".to_string()),
            ("instances[0][timezone]", "America/Chicago".to_string()),
            ("contact_name", "Ada Organizer".to_string()),
            ("contact_email", "organizer@example.org".to_string()),
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
        serde_qs::Config::new(10, false)
            .deserialize_bytes(body.as_bytes())
            .unwrap()
    }

    #[test]
    fn only_a_restricted_event_has_ages() {
        let ages =
            |changes: &[(&str, &str)]| event_form_submitted(changes).audience().collect::<Vec<_>>();
        let families = ("audience[]", "families");

        check!(ages(&[families]).is_empty());
        check!(ages(&[("attendance", "open"), families]).is_empty());
        check!(
            ages(&[
                ("attendance", "limited"),
                ("limited_to", "Girl Scouts"),
                families
            ]) == ["families"]
        );
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
        let long_name = "n".repeat(rules::NAME_LIMIT + 1);
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
        let at_limit = "é".repeat(rules::NAME_LIMIT);
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
        let formats = crate::event::in_format_order(vec![
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
            ChoiceList::Audience,
            vec![choice(ChoiceList::Audience, "families", "Families", None)],
            &[],
        )
        .render_once()
        .unwrap();
        check!(audience.contains(
            r#"<fieldset class="choice-group choice-category" aria-describedby="audience-help">
  <legend>What ages is this event suitable for?</legend>"#
        ));
        check!(
            audience
                .contains(r#"<p id="audience-help" class="field-help">Select all that apply.</p>"#)
        );

        let topics = event_choices(
            "topics",
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
            .for_events("", "")
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
    fn an_online_event_is_asked_no_location_note() {
        let note = |html: &str| {
            let at = html
                .find(r#"<details class="quiet-disclosure location-note""#)
                .unwrap();
            html[at..at + html[at..].find('>').unwrap()].to_string()
        };

        let online_event = picker("online").for_events("", "").render_once().unwrap();
        check!(note(&online_event).contains(" data-no-online-note"));
        check!(note(&online_event).ends_with(" hidden"));

        let in_person_event = picker("in_person")
            .for_events("", "")
            .render_once()
            .unwrap();
        check!(note(&in_person_event).contains(" data-no-online-note"));
        check!(!note(&in_person_event).ends_with(" hidden"));

        // A project joined online may still say where.
        let online_project = picker("online").render_once().unwrap();
        check!(!note(&online_project).contains("data-no-online-note"));
        check!(!note(&online_project).ends_with(" hidden"));
    }

    /// The attributes of the `<fieldset>` holding the field of join choice
    /// `choice`.
    fn join_field(html: &str, choice: &str) -> String {
        let at = html.find(&format!(r#"data-join="{choice}""#)).unwrap();
        html[at..at + html[at..].find('>').unwrap()].to_string()
    }

    #[test]
    fn a_virtual_event_is_asked_how_people_join() {
        let html = picker("online").for_events("", "").render_once().unwrap();
        check!(html.matches(r#"name="virtual_join""#).count() == 2);
        check!(html.contains("External link"));
        check!(html.contains("Include clear instructions on how to participate."));
        // Neither field is open, or submitted, until its choice is made, and
        // the link shared with the other modes isn't asked of a virtual event.
        check!(join_field(&html, "link").ends_with("hidden disabled"));
        check!(join_field(&html, "other").ends_with("hidden disabled"));
        check!(html.contains(r#"data-mode="in_person region anywhere" hidden disabled"#));

        let html = picker("online")
            .for_events("other", "Dial 555-0100")
            .render_once()
            .unwrap();
        check!(!join_field(&html, "other").ends_with("hidden disabled"));
        check!(join_field(&html, "link").ends_with("hidden disabled"));
        check!(html.contains("Dial 555-0100</textarea>"));
    }

    #[test]
    fn a_project_is_not_asked_how_people_join() {
        let html = picker("online").render_once().unwrap();
        check!(!html.contains(r#"name="virtual_join""#));
        check!(html.matches(r#"name="url""#).count() == 1);
    }

    #[test]
    fn words_left_in_the_place_search_stay_there() {
        let html = picker("in_person")
            .with_place_query("Shedd \"Aquarium\"")
            .render_once()
            .unwrap();
        check!(html.contains(r#"name="place_query" value="Shedd &quot;Aquarium&quot;""#));
    }

    #[test]
    fn the_toggle_is_in_person_or_virtual() {
        let html = picker("").for_events("", "").render_once().unwrap();
        check!(html.contains("Is it in person or virtual?"));
        for (value, label) in [("in_person", "In person"), ("online", "Virtual")] {
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
            let html = picker(mode).for_events("", "").render_once().unwrap();
            let radio = toggle_radio(&html, mode).unwrap();
            check!(radio.contains(" data-in-person"), "{mode}");
            check!(radio.contains(" checked"), "{mode}");
            check!(toggle_radio(&html, "in_person").is_none(), "{mode}");
        }

        let html = picker("anywhere").for_events("", "").render_once().unwrap();
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
            .for_events("", "")
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
        validate_and_build_project(&form, test_author())
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
}
