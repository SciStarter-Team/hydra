use common::message::log::PageChange;
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use jiff::Zoned;
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
    message::user::{BasicInfo, UserEmailInfo, UserEmailInfoRequest},
    service::{log_activity, request},
    shared::db,
    timestring::TimeString,
};

use crate::{
    choice::{ChoiceCheckboxes, ChoiceList, ChoiceValue, OrreryValue, limited_heading},
    component::{
        Component, Head, StandardLayout,
        date_row::DateRowValues,
        floating_field::{FormError, FormErrors},
        image_field::{ImageField, ImageLayout, ImageUploadResult, ImageUrlInput},
        location_picker::LocationPicker,
    },
    contact::{Contact, ContactRole},
    edit_access::Editor,
    entity::{Entity, EntityKind, EntityTarget},
    event::{
        admission::Admission,
        all::EventAll,
        attendance::Attendance,
        choice_checkboxes,
        location::{EventLocation, LocationInput, answered_venue, picked_point, settle_online},
        registration::Registration,
        repeat::{RepeatFrequency, Repetition},
        rules::{self, DateRow, Refusal, RowReading, ZonedInstance},
    },
    exchange::SubmissionExchange,
    form::QsForm,
    middleware::authorization::Authenticated,
    peering::LoadStorePeered,
    project::{
        all::ProjectAll,
        location::{
            ANYWHERE, AT_AN_ADDRESS, IN_A_RANGE, LocationMode, PlaceGiven, PlaceOf, missing_place,
        },
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
    /// The event's webpage.
    #[serde(default)]
    url: String,
    /// What was left typed in the location picker's place search; read only
    /// to say, when no address was given, that it went unused.
    #[serde(default)]
    place_query: String,
    /// The slugs of the formats, programs or campaigns and kinds of venue
    /// ticked.
    #[serde(default)]
    formats: Vec<String>,
    #[serde(default)]
    programs: Vec<String>,
    /// The kind of venue chosen, empty for none.
    #[serde(default)]
    venue: String,
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
    /// When registration opens, unless nobody registers.
    #[serde(default)]
    ticketing_date: String,
    #[serde(default)]
    ticketing_time: String,
    #[serde(default)]
    ticketing_timezone: String,
    /// Where people register or get tickets.
    #[serde(default)]
    ticket_url: String,
    /// The answer to "Registration required?".
    #[serde(default, deserialize_with = "empty_as_none")]
    registration: Option<Registration>,
    /// The answer to "Admission".
    #[serde(default, deserialize_with = "empty_as_none")]
    admission: Option<Admission>,
    /// The price, or the suggested donation.
    #[serde(default)]
    cost: String,
    #[serde(default)]
    instances: Vec<InstanceInput>,
    #[serde(default)]
    attendance: Attendance,
    /// The audience the event is meant for, read only when it is for a
    /// specific one.
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
    /// How the date/time repeats, and through when.
    #[serde(default)]
    repeat_frequency: RepeatFrequency,
    #[serde(default)]
    repeat_until: String,
}

impl InstanceInput {
    fn repetition(&self) -> Repetition {
        Repetition {
            frequency: self.repeat_frequency,
            until: self.repeat_until.clone(),
        }
    }
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
    place_query: String,
    formats: Vec<String>,
    programs: Vec<String>,
    venue: String,
    topics: Vec<String>,
    audience: Vec<String>,
    presenter: String,
    image: String,
    image_credit: String,
    how_to_join: String,
    tags: String,
    ticketing_date: String,
    ticketing_time: String,
    ticketing_timezone: String,
    ticket_url: String,
    registration: Option<Registration>,
    admission: Option<Admission>,
    cost: String,
    instances: Vec<InstanceValues>,
    /// Whether the event is meant for a specific audience, and which.
    attendance: Attendance,
    limited_to: String,
}

/// One of the add form's date/time rows, as submitted or as shown: the date,
/// and how it repeats.
#[derive(Debug, Clone, Default)]
struct InstanceValues {
    row: DateRowValues,
    repetition: Repetition,
}

impl From<&AddEventForm> for EventFormValues {
    fn from(f: &AddEventForm) -> Self {
        let mut instances: Vec<InstanceValues> = f
            .instances
            .iter()
            .map(|i| InstanceValues {
                row: DateRowValues {
                    begin_date: i.begin_date.clone(),
                    begin_time: i.begin_time.clone(),
                    timezone: i.timezone.clone(),
                    has_end: i.has_end.is_some(),
                    end_date: i.end_date.clone(),
                    end_time: i.end_time.clone(),
                },
                repetition: i.repetition(),
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
            place_query: f.place_query.clone(),
            formats: f.formats.clone(),
            programs: f.programs.clone(),
            venue: f.venue.clone(),
            topics: f.topics.clone(),
            audience: f.audience.clone(),
            presenter: f.presenter.clone(),
            image: f.image.clone(),
            image_credit: f.image_credit.clone(),
            how_to_join: f.how_to_join.clone(),
            tags: f.tags.clone(),
            ticketing_date: f.ticketing_date.clone(),
            ticketing_time: f.ticketing_time.clone(),
            ticketing_timezone: f.ticketing_timezone.clone(),
            ticket_url: f.ticket_url.clone(),
            registration: f.registration,
            admission: f.admission,
            cost: f.cost.clone(),
            instances,
            attendance: f.attendance,
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
    errors: FormErrors,
    values: EventFormValues,
    /// The exchange the form's `exchange` field names, if it names one.
    exchange: Option<SubmissionExchange>,
    /// Whether that exchange publishes what this visitor adds as it arrives.
    publishes: bool,
    /// What the event picks from the lists that aren't asked with its
    /// location or who can participate, under "Help people discover your
    /// event": its topics, formats, and programs or campaigns.
    discover: Vec<ChoiceCheckboxes>,
    tags: TagInput,
    /// The ages the event suits, asked of an event for a specific audience.
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
    Contact,
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

    checkboxes: Vec<ChoiceCheckboxes>,
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
            checkboxes,
        }
    }
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
                answered_venue(&form.venue).into_iter().collect()
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
    /// What the tags are for, under the field.
    help: &'static str,
}

impl TagInput {
    /// What a project's tags are for.
    const PROJECT_HELP: &'static str =
        "Press Enter or type a comma after each tag, or pick a suggestion.";
    /// What an event's tags are for, as orrery's event form says it.
    const EVENT_HELP: &'static str = "Add keywords that help people find your event, such as birds, stargazing, or water quality. Press Enter or type a comma after each one.";
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
    values.instances.push(InstanceValues::default());

    render_event_form(FormErrors::new(), values, user_info).await
}

/// POST `/add/event` — validate, create the event, and redirect to the
/// event's page, which thanks the person for an event waiting for review
/// (`?new=true`). On validation failure, re-render the form with the entered
/// values preserved.
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
            event.contacts = Box::new([account_contact(user_info.user_id).await?]);

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
                Attendance::of(&event.limited_to),
                ChoiceValue::known(ChoiceList::Audience, std::mem::take(&mut event.audience))
                    .await?,
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

            // Its page is the submitter's to see, published or not, as its
            // creator; one waiting for review thanks them for it.
            let path = entity.canonical_path();
            Ok(if published {
                Redirect::see_other(path)
            } else {
                Redirect::see_other(format!("{path}?{}", crate::event::NEW_SUBMISSION_QUERY))
            }
            .into_response())
        }
        Err(errors) => {
            render_event_form(errors, EventFormValues::from(&form), Some(user_info)).await
        }
    }
}

/// The contact of an event added by the account `user_id`: the account
/// itself, by its full name, or its email address when it gave no name, as
/// orrery's `entity_save` makes the adding account an added event's contact.
async fn account_contact(user_id: Ulid) -> Result<Contact, anyhow::Error> {
    let info: UserEmailInfo =
        request("user.info.email_info", UserEmailInfoRequest { user_id }).await?;
    Ok(contact_of_account(&info))
}

/// The contact person `info`'s account is.
fn contact_of_account(info: &UserEmailInfo) -> Contact {
    let full_name = format!("{} {}", info.first_name.trim(), info.last_name.trim());
    let full_name = full_name.trim();
    Contact {
        contact_id: Ulid::new(),
        role: ContactRole::ContactPerson,
        name: Some(if full_name.is_empty() {
            info.primary_email.clone()
        } else {
            full_name.to_string()
        }),
        affiliation: None,
        email: info.primary_email.clone(),
        phone: None,
        address: None,
        url: None,
    }
}

async fn render_event_form(
    errors: FormErrors,
    mut values: EventFormValues,
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
        "",
        &values.regions,
    )
    .with_place_query(&values.place_query)
    .for_events()
    .with_venues(
        crate::event::offered(ChoiceList::EventVenue).await?,
        &values.venue,
    )
    .errors(&errors);
    let image = ImageField {
        label: "Image link (instead of uploading)".to_string(),
        value: values.image.clone(),
        credit: Some(values.image_credit.clone()),
        layout: ImageLayout::Cover,
        heading: Some("Event cover image"),
        required: true,
        problem: errors.described_by(ImageUrlInput::NAME),
    };
    // Whether the event is online, and the kind of venue it's held at, are
    // asked with its location, and the ages it suits with who can
    // participate; the rest of what it picks is under "Help people discover
    // your event".
    let audience = choice_checkboxes(
        "audience",
        ChoiceList::Audience,
        ChoiceValue::all(ChoiceList::Audience).await?,
        &values.audience,
    );
    let discover = vec![
        choice_checkboxes(
            "topics",
            ChoiceList::Topic,
            ChoiceValue::all(ChoiceList::Topic).await?,
            &values.topics,
        )
        .errors(&errors),
        choice_checkboxes(
            "formats",
            ChoiceList::EventFormat,
            crate::event::offered(ChoiceList::EventFormat).await?,
            &values.formats,
        ),
        choice_checkboxes(
            "programs",
            ChoiceList::EventProgram,
            crate::event::offered(ChoiceList::EventProgram).await?,
            &values.programs,
        ),
    ];
    let tags = TagInput {
        value: values.tags.clone(),
        help: TagInput::EVENT_HELP,
    };

    let exchange = SubmissionExchange::find(&values.exchange).await?;
    if exchange.is_none() {
        values.exchange.clear();
    }
    let publishes = published_on_arrival(exchange.as_ref(), user_info.as_deref()).await?;

    let content = AddEventContent {
        errors,
        values,
        exchange,
        publishes,
        discover,
        tags,
        audience,
        location,
        image,
    };

    let layout = StandardLayout::new("/add/event", content)
        .prepare(user_info)
        .await?;

    AddEventPage {
        head: Head::default().with_event_editors().prepare(None).await?,
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

/// GET `/add/project/submitted` — "thanks, pending review" confirmation page.
#[handler]
#[tracing::instrument]
pub async fn add_project_submitted_page(
    Authenticated(user_info): Authenticated,
    Query(query): Query<SubmittedQuery>,
) -> Result<Response, anyhow::Error> {
    render_submitted(user_info, "/add/project", query.name, "project").await
}

fn html_fragment(body: String) -> Response {
    Response::builder().content_type("text/html").body(body)
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

/// Validate the add event form and build the event it adds, without its
/// contact, which is the account adding it; or say everything there is to
/// fix.
///
/// Every check the form makes is here (the form is `novalidate`), in the
/// order orrery's event form makes the same checks in its `check`: the name,
/// cover image and description; the dates and where it's held; who it's
/// for; how people join it; the links; admission; its topics; and when
/// registration opens.
/// As orrery's does, it finds every problem rather than stopping at the
/// first, and passes over the checks that a value refused would leave
/// meaningless: where an event is held, with no answer to how it's held;
/// a date's repetition, with the date refused.
fn validate_and_build(form: &AddEventForm, author: Editor) -> Result<EventAll, FormErrors> {
    let user_id = author.user_id;
    let mut errors = FormErrors::new();

    errors.take(rules::name(&form.name, None).map_err(refused));
    let name = form.name.trim();

    errors.take(rules::image(&form.image, None).map_err(refused));
    let image = form.image.trim();

    errors.take(rules::description(&form.description).map_err(refused));
    let description = form.description.trim();

    // Resolve the date/time rows that carry a start, each with the row it's
    // in, noting whether any was filled in at all, passing or not.
    let now = Zoned::now();
    let mut base: Vec<(usize, ZonedInstance)> = Vec::new();
    let mut dated = false;
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
            Ok(RowReading::Dated(instance)) => {
                dated = true;
                base.push((i, instance));
            }
            Ok(RowReading::Kept) => dated = true,
            Ok(RowReading::Blank) => {} // an untouched row
            Err(refusal) => {
                dated = true;
                errors.push(refused_in_row(refusal, i));
            }
        }
    }
    errors.take(rules::some_date(dated).map_err(|refusal| refused_in_row(refusal, 0)));

    // Each date/time repeats on its own.
    let mut instances = Vec::new();
    for (row, instance) in &base {
        let occurrences = form.instances[*row]
            .repetition()
            .occurrences(instance)
            .map_err(|refusal| refused_in_row(refusal, *row));
        instances.extend(errors.take(occurrences).into_iter().flatten());
    }

    let mode = errors.take(rules::mode(form.location_mode).map_err(refused));
    if let Some(mode) = mode {
        errors.take(
            rules::place(
                mode,
                PlaceGiven {
                    location_name: &form.location_name,
                    address: &form.address,
                    city: &form.city,
                    place_query: &form.place_query,
                },
            )
            .map_err(refused),
        );
        errors.take(rules::location_note(mode, &form.location_description).map_err(refused));
    }

    // Who can participate, the section after "When and where". The ages
    // ticked are read against their list, and an event ticking none given
    // its ages, once it's built.
    let attendance = errors.take(
        rules::attendance(
            form.attendance,
            &form.limited_to,
            form.audience.iter().cloned().collect(),
            None,
        )
        .map_err(refused),
    );

    if let Some(mode) = mode {
        errors.take(
            rules::way_to_join(mode, &form.how_to_join, &form.url, &form.ticket_url)
                .map_err(refused),
        );
    }

    errors.take(rules::link(&form.url, None).map_err(refused));
    errors.take(rules::ticket_link(&form.ticket_url, form.registration, None).map_err(refused));

    let admission =
        errors.take(rules::admission(form.admission, &form.cost, None).map_err(refused));

    errors.take(rules::topics(crate::choice::distinct_picks(&form.topics), None).map_err(refused));

    let ticketing_begins = errors.take(
        rules::registration_opens(
            form.registration,
            &form.ticketing_date,
            &form.ticketing_time,
            &form.ticketing_timezone,
        )
        .map_err(refused),
    );

    // A check that settles a value told what was wrong with it.
    let (Some(mode), Some(admission), Some(ticketing_begins), Some(attendance)) =
        (mode, admission, ticketing_begins, attendance)
    else {
        return Err(errors);
    };
    errors.into_result()?;
    let ticketing_begins = ticketing_begins.map(TimeString::from);

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
    event.url = form.url.trim().to_string();
    event.how_to_join = form.how_to_join.trim().to_string();
    event.ticket_url = form.ticket_url.trim().to_string();
    event.registration = form.registration;
    event.ticketing_begins = ticketing_begins;
    event.admission = admission.admission;
    event.cost = admission.cost;
    event.limited_to = attendance.limited_to;
    event.audience = attendance.ages;
    event.tags = crate::tag::as_set_by(crate::tag::split(&form.tags), [], author.is_staff);

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
            regions: &form.regions,
            venues: answered_venue(&form.venue).as_slice(),
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
    refusal.into_form_error(row)
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
    errors: FormErrors,
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
    render_project_form(FormErrors::new(), values, user_info).await
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
            render_project_form(
                error.into(),
                ProjectFormValues::from(&form),
                Some(user_info),
            )
            .await
        }
    }
}

async fn render_project_form(
    errors: FormErrors,
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
    .errors(&errors);
    let image = ImageField {
        label: "Image link".to_string(),
        value: values.image.clone(),
        credit: Some(values.image_credit.clone()),
        layout: ImageLayout::Cover,
        heading: None,
        required: false,
        problem: errors.described_by(ImageUrlInput::NAME),
    };
    let tags = TagInput {
        value: values.tags.clone(),
        help: TagInput::PROJECT_HELP,
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
        errors,
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
            let place = PlaceGiven {
                location_name: &form.location_name,
                address: &form.address,
                city: &form.city,
                place_query: &form.place_query,
            };
            if let Some(message) = missing_place(place, PlaceOf::Project) {
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
        let body = "name=Bioblitz&location_mode=online\
            &instances[0][begin_date]=2026-11-01&instances[0][begin_time]=10:00\
            &instances[0][timezone]=America/Chicago&instances[0][repeat_frequency]=weekly\
            &instances[1][begin_date]=2026-11-02&instances[1][begin_time]=11:00\
            &instances[1][timezone]=America/Chicago";
        let form: AddEventForm = serde_qs::Config::new(10, false)
            .deserialize_bytes(body.as_bytes())
            .unwrap();

        check!(form.name == "Bioblitz");
        check!(form.location_mode == Some(LocationMode::Online));
        check!(form.instances[0].repeat_frequency == RepeatFrequency::Weekly);
        check!(form.instances[1].repeat_frequency == RepeatFrequency::Once);
        check!(form.instances.len() == 2);
        check!(form.instances[1].begin_time == "11:00");
    }

    fn account(first_name: &str, last_name: &str) -> UserEmailInfo {
        UserEmailInfo {
            user_id: Ulid::new(),
            handle: "ada".to_string(),
            first_name: first_name.to_string(),
            last_name: last_name.to_string(),
            primary_email: "ada@example.org".to_string(),
            verified: true,
            badge_allowed: false,
            accept_emails: true,
        }
    }

    #[test]
    fn the_contact_is_the_account_adding_the_event() {
        let contact = contact_of_account(&account(" Ada ", "Lovelace "));
        check!(contact.role == ContactRole::ContactPerson);
        check!(contact.name.as_deref() == Some("Ada Lovelace"));
        check!(contact.email == "ada@example.org");
        check!(contact.url == None);

        check!(contact_of_account(&account("Ada", "")).name.as_deref() == Some("Ada"));
        // An account with no name is known by its email address.
        check!(contact_of_account(&account(" ", "")).name.as_deref() == Some("ada@example.org"));
    }

    #[test]
    fn the_form_asks_for_no_contact() {
        let html = event_form(fresh_event(), None);
        check!(!html.contains("contact_"));
        check!(submit_event(&[]).unwrap().contacts.is_empty());
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
                == "Please choose a place from the list, or choose to use what you typed as the venue name."
        );

        // Used as the venue name, the words still want the venue's address.
        typed.push(("location_name", "Shedd Aquarium loading dock"));
        let refused = submit_event(&typed).unwrap_err();
        check!(refused.is_about("address"));
        check!(refused.message == "Please add the venue\u{2019}s city or street address.");

        typed.push(("city", "Chicago"));
        let event = submit_event(&typed).unwrap();
        check!(event.location_name == "Shedd Aquarium loading dock");
        check!(event.city == "Chicago");
    }

    #[test]
    fn an_in_person_event_is_held_at_its_address() {
        let in_person = [("location_mode", "in_person"), ("url", "")];

        let placeless = submit_event(&in_person).unwrap_err();
        check!(placeless.is_about("address"));

        let event = submit_event(&[&in_person[..], &[("city", "Chicago")]].concat()).unwrap();
        check!(!event.entirely_online);
        check!(!event.also_online);
        check!(event.city == "Chicago");
        check!(event.url.is_empty());
    }

    #[test]
    fn a_virtual_event_says_how_people_join() {
        let nothing = [("url", ""), ("ticket_url", ""), ("how_to_join", " ")];
        let refused = submit_event(&nothing).unwrap_err();
        check!(refused.is_about("how_to_join"));
        check!(refused.is_about("url"));
        check!(refused.is_about("ticket_url"));
        check!(
            refused.message
                == "Please explain how people join the virtual event, or add its webpage or registration link."
        );

        for (field, value) in [
            ("how_to_join", "Dial 555-0100"),
            ("url", "https://example.org/bioblitz"),
            ("ticket_url", "https://example.org/register"),
        ] {
            let mut changes = nothing.to_vec();
            changes.retain(|(name, _)| *name != field);
            changes.push((field, value));
            check!(submit_event(&changes).is_ok(), "{field}");
        }

        // An event held somewhere is joined there.
        check!(submit_event(&[&nothing[..], &[("location_mode", "anywhere")]].concat()).is_ok());
    }

    #[test]
    fn a_new_event_needs_a_cover_image() {
        let refused = submit_event(&[(ImageUrlInput::NAME, " ")]).unwrap_err();
        check!(refused.is_about(ImageUrlInput::NAME));
        check!(refused.message == "Please add a cover image for your event.");

        let html = event_form(fresh_event(), Some(refused));
        check!(html.contains(r#"<div class="image-cover image-cover-invalid">"#));
    }

    #[test]
    fn admission_is_free_paid_or_a_suggested_donation() {
        let refused = submit_event(&[("admission", "")]).unwrap_err();
        check!(refused.is_about("admission"));
        check!(
            refused.message
                == "Please choose whether admission is free, paid, or a suggested donation."
        );

        // A free event has no price, whatever was left typed.
        let free = submit_event(&[("cost", "$10")]).unwrap();
        check!(free.admission == Some(Admission::Free));
        check!(free.cost.is_empty());

        let refused = submit_event(&[("admission", "paid"), ("cost", " ")]).unwrap_err();
        check!(refused.is_about("cost"));
        check!(refused.message == "Please give the admission price.");
        let paid = submit_event(&[("admission", "paid"), ("cost", " $10 per person ")]).unwrap();
        check!(paid.admission == Some(Admission::Paid));
        check!(paid.cost == "$10 per person");

        // The donation is up to the organizer to suggest.
        let donation = submit_event(&[("admission", "donation")]).unwrap();
        check!(donation.admission == Some(Admission::Donation));
        check!(donation.cost.is_empty());
    }

    #[test]
    fn registration_asks_its_link_and_when_it_opens() {
        let refused =
            submit_event(&[("registration", "required"), ("ticket_url", " ")]).unwrap_err();
        check!(refused.is_about("ticket_url"));
        check!(refused.message == "Please add the link where people register.");

        let opening = [
            ("ticket_url", "https://example.org/register"),
            ("ticketing_date", "2099-10-01"),
            ("ticketing_time", "09:00"),
            ("ticketing_timezone", "America/Chicago"),
        ];
        let required =
            submit_event(&[&opening[..], &[("registration", "required")]].concat()).unwrap();
        check!(required.registration == Some(Registration::Required));
        check!(required.ticket_url == "https://example.org/register");
        let opens = required
            .ticketing_begins
            .as_deref()
            .map(|opens| opens.parse::<jiff::Timestamp>().unwrap());
        check!(opens == Some("2099-10-01T14:00:00Z".parse().unwrap()));

        // Nobody registers for an event that needs none, so it never opens;
        // its link may still serve, for tickets.
        let none = submit_event(&[&opening[..], &[("registration", "none")]].concat()).unwrap();
        check!(none.registration == Some(Registration::NotNeeded));
        check!(none.ticket_url == "https://example.org/register");
        check!(none.ticketing_begins == None);

        let refused = submit_event(&[("ticketing_date", "2099-10-01")]).unwrap_err();
        check!(refused.is_about("ticketing_timezone"));
    }

    #[test]
    fn hybrid_is_no_longer_a_mode() {
        let body = "location_mode=hybrid";
        let form: Result<AddEventForm, _> =
            serde_qs::Config::new(10, false).deserialize_bytes(body.as_bytes());
        check!(form.is_err());
    }

    #[test]
    fn everyone_is_welcome_unless_the_event_is_for_a_specific_audience() {
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
    fn a_specific_audience_is_named_briefly() {
        let refused = submit_event(&[("attendance", "limited"), ("limited_to", " ")]).unwrap_err();
        check!(refused.is_about("limited_to"));
        check!(
            refused.message
                == "Please say who the event is for, such as \u{201c}Educators\u{201d} or \u{201c}Girl Scouts.\u{201d}"
        );

        let at_limit = "é".repeat(rules::LIMITED_TO_LIMIT);
        let event = submit_event(&[("attendance", "limited"), ("limited_to", &at_limit)]).unwrap();
        check!(event.limited_to == at_limit);

        let over = format!("{at_limit}!");
        let refused =
            submit_event(&[("attendance", "limited"), ("limited_to", &over)]).unwrap_err();
        check!(refused.is_about("limited_to"));
        check!(refused.message == "Please say who the event is for in 140 characters or fewer.");
    }

    /// What a fresh add-event form shows: one date, not repeating.
    fn fresh_event() -> EventFormValues {
        EventFormValues {
            instances: vec![InstanceValues::default()],
            ..Default::default()
        }
    }

    /// The add-event form, showing `values` and refused over `error`.
    fn event_form(values: EventFormValues, error: Option<FormError>) -> String {
        let errors = FormErrors::from(error);
        let image_problem = errors.described_by(ImageUrlInput::NAME);
        AddEventContent {
            errors,
            values,
            exchange: None,
            publishes: false,
            discover: vec![choice_checkboxes(
                "topics",
                ChoiceList::Topic,
                vec![choice(ChoiceList::Topic, "birds", "Birds", None)],
                &[],
            )],
            tags: TagInput {
                value: String::new(),
                help: TagInput::EVENT_HELP,
            },
            audience: choice_checkboxes(
                "audience",
                ChoiceList::Audience,
                vec![choice(ChoiceList::Audience, "families", "Families", None)],
                &[],
            ),
            location: picker("online").for_events(),
            image: ImageField {
                label: "Image link (instead of uploading)".to_string(),
                value: String::new(),
                credit: Some(String::new()),
                layout: ImageLayout::Cover,
                heading: Some("Event cover image"),
                required: true,
                problem: image_problem,
            },
        }
        .render_once()
        .unwrap()
    }

    #[test]
    fn the_sections_come_in_orrerys_order() {
        let html = event_form(fresh_event(), None);
        let at = |text: &str| html.find(text).unwrap();
        let order = [
            ">About your event</h2>",
            "Event name",
            "Event cover image",
            "Event description",
            "Host organization",
            ">When and where</h2>",
            "Start date",
            "How will people attend?",
            ">Who can participate?<span",
            "What ages is this event suitable for?",
            ">How to participate</h2>",
            ">Event links &amp; registration</h2>",
            "Event webpage",
            "Registration or ticket URL",
            "Registration required?",
            "+ Set registration opening date",
            ">Admission<span",
            ">Help people discover your event</h2>",
            "<legend>Topics<span",
            "Press Enter or type a comma",
            "Once you submit your event, SciStarter staff will review it",
            "Submit event for review",
        ];
        for pair in order.windows(2) {
            check!(at(pair[0]) < at(pair[1]), "{} before {}", pair[0], pair[1]);
        }
        check!(!html.contains("Is this part of a larger event?"));
        check!(!html.contains(r#"name="part_of""#));
    }

    #[test]
    fn the_ages_are_asked_only_of_a_specific_audience() {
        let html = event_form(EventFormValues::default(), None);
        check!(html.contains(r#"name="attendance" value="open" checked>"#));
        check!(html.contains("<span>Everyone is welcome</span>"));
        check!(html.contains("<span>This event is intended for a specific audience</span>"));

        let specific = html
            .find(r#"<div class="attendance-ages" data-limited-only hidden>"#)
            .unwrap();
        let ages = html
            .find("<legend>What ages is this event suitable for?</legend>")
            .unwrap();
        let section_end = specific + html[specific..].find("</section>").unwrap();
        check!(specific < ages && ages < section_end);
        check!(html[ages..].contains(r#"<div class="choice-checkboxes stacked-options">"#));
    }

    #[test]
    fn the_audience_is_asked_under_its_answer() {
        let html = event_form(EventFormValues::default(), None);
        let specific = html
            .find("<span>This event is intended for a specific audience</span>")
            .unwrap();
        let field = html
            .find(r#"<div class="radio-option-field" data-limited-only hidden>"#)
            .unwrap();
        let answers_end = specific + html[specific..].find("</fieldset>").unwrap();
        check!(specific < field && field < answers_end);
        check!(html.contains("Who is this event for?"));
        check!(html.contains(
            r#"placeholder="e.g., Educators, library cardholders, Girl Scouts, registered volunteers""#
        ));

        let refused = submit_event(&[("attendance", "limited"), ("limited_to", "")]).unwrap_err();
        let values = EventFormValues {
            attendance: Attendance::Limited,
            ..EventFormValues::default()
        };
        let html = event_form(values, Some(refused));
        check!(html.contains(r#"name="attendance" value="limited" checked>"#));
        check!(html.contains(r#"<div class="radio-option-field" data-limited-only >"#));
        let field = &html[html.find(r#"id="limited-to""#).unwrap()..];
        let field = &field[..field.find('>').unwrap()];
        check!(field.contains(r#"aria-invalid="true""#), "{field}");
    }

    #[test]
    fn each_question_is_a_list_of_radio_buttons() {
        let html = event_form(EventFormValues::default(), None);
        check!(!html.contains("toggle-option"));
        // How people attend, where an in-person event takes place,
        // registration, admission, and who can participate.
        check!(html.matches(r#"<div class="radio-options">"#).count() == 5);
        check!(html.contains(r#"<label class="radio-option">"#));
    }

    #[test]
    fn each_answer_asks_its_own_fields() {
        let html = event_form(
            EventFormValues {
                admission: Some(Admission::Donation),
                registration: Some(Registration::NotNeeded),
                ..fresh_event()
            },
            None,
        );
        let opening_tag = |marker: &str| {
            let at = html.find(marker).unwrap();
            html[at..at + html[at..].find('>').unwrap()].to_string()
        };
        check!(opening_tag(r#"data-cost-for="paid""#).ends_with("hidden disabled"));
        check!(!opening_tag(r#"data-cost-for="donation""#).contains("disabled"));
        check!(opening_tag("data-ticketing").ends_with(" hidden"));
        check!(html.contains("Suggested donation amount (optional)"));

        // The registration link is marked required, and required, only
        // while registration is.
        let link_required = |registration| {
            let html = event_form(
                EventFormValues {
                    registration,
                    ..fresh_event()
                },
                None,
            );
            let at = html.find(r#"name="ticket_url""#).unwrap();
            html[at..at + html[at..].find('>').unwrap()].contains(" required")
        };
        check!(link_required(Some(Registration::Required)));
        check!(!link_required(Some(Registration::Optional)));
        check!(!link_required(None));
    }

    #[test]
    fn the_virtual_guidance_shows_for_a_virtual_event() {
        let guidance = |mode: &str| {
            let html = event_form(
                EventFormValues {
                    location_mode: mode.to_string(),
                    ..fresh_event()
                },
                None,
            );
            let at = html.find("data-online-only").unwrap();
            html[at..at + html[at..].find('>').unwrap()].contains("hidden")
        };
        check!(!guidance("online"));
        check!(guidance("in_person"));
    }

    #[test]
    fn repetition_is_offered_under_every_date() {
        let mut values = fresh_event();
        values.instances.push(InstanceValues {
            repetition: Repetition {
                frequency: RepeatFrequency::Weekly,
                until: "2099-11-15".to_string(),
            },
            ..Default::default()
        });
        let html = event_form(values, None);
        check!(html.matches("data-repeat-toggle").count() == 2);
        check!(html.matches("Add an end time").count() == 2);
        // Put away until it's asked for, unless the date repeats already.
        let first = html.find(r#"name="instances[1][begin_date]""#).unwrap();
        check!(html[..first].contains(r#"data-repeat-toggle >"#));
        check!(html[..first].contains(r#"data-repeat hidden"#));
        check!(html[first..].contains(r#"data-repeat-toggle hidden"#));
        check!(!html[first..].contains(r#"data-repeat hidden"#));
        check!(html[first..].contains(r#"<option value="weekly" selected>Weekly"#));
        check!(html[first..].contains(r#"value="2099-11-15""#));
        check!(html[first..].contains(r#"id="instances[1]-repeat-help""#));
    }

    #[test]
    fn the_zone_is_the_right_half_of_the_start_time() {
        let html = event_form(fresh_event(), None);
        let pair = html.find(r#"<div class="time-zone">"#).unwrap();
        let time = html.find("Start time").unwrap();
        let zone = html.find(r#"name="instances[0][timezone]""#).unwrap();
        check!(pair < time && time < zone);
        check!(!html.contains("tz-chip"));
    }

    #[test]
    fn a_refused_topic_choice_is_marked() {
        let refused = FormError::at("topics", "Please pick no more than 3 topics.");
        let topics = |errors: FormErrors| {
            choice_checkboxes(
                "topics",
                ChoiceList::Topic,
                vec![choice(ChoiceList::Topic, "birds", "Birds", None)],
                &[],
            )
            .errors(&errors)
            .render_once()
            .unwrap()
        };
        let html = topics(refused.into());
        check!(html.contains(r#"name="topics[]" value="birds" aria-invalid="true">"#));
        check!(html.contains(r#"aria-describedby="topics-problem topics-help""#));
        check!(!topics(FormErrors::new()).contains("aria-invalid"));
    }

    /// Validate an add-event submission: an online event that passes every
    /// check, with the fields in `changes` set as given (an empty value
    /// leaves one unanswered). Refused, it is refused for the one problem
    /// the changes make.
    fn submit_event(changes: &[(&str, &str)]) -> Result<EventAll, FormError> {
        submit_event_with_problems(changes).map_err(|errors| {
            let told = errors
                .iter()
                .map(|e| e.message.as_str())
                .collect::<Vec<_>>();
            assert!(told.len() == 1, "{told:?}");
            errors.first().unwrap().clone()
        })
    }

    /// [`submit_event`], refused for every problem the changes make.
    fn submit_event_with_problems(changes: &[(&str, &str)]) -> Result<EventAll, FormErrors> {
        validate_and_build(&event_form_submitted(changes), test_author())
    }

    /// As orrery's event form does, every problem is told, in the order of
    /// the page, and a check left meaningless by one is passed over: a date
    /// refused is a date given, and repeats nothing.
    #[test]
    fn every_problem_is_told_in_the_pages_order() {
        let refused = submit_event_with_problems(&[
            ("admission", ""),
            ("name", " "),
            ("instances[0][begin_time]", ""),
            ("instances[0][repeat_frequency]", "weekly"),
            ("description", ""),
            ("location_mode", ""),
        ])
        .unwrap_err();
        let fields = [
            "name",
            "description",
            "instances[0][begin_time]",
            "location_mode",
            "admission",
        ];
        check!(refused.len() == fields.len());
        for (error, field) in refused.iter().zip(fields) {
            check!(error.is_about(field), "{}", error.message);
        }
    }

    /// An add-event submission, otherwise complete, with the fields in
    /// `changes` set as given.
    fn event_form_submitted(changes: &[(&str, &str)]) -> AddEventForm {
        let mut fields = vec![
            ("name", "Bioblitz".to_string()),
            (
                ImageUrlInput::NAME,
                "https://example.org/bioblitz.jpg".to_string(),
            ),
            ("description", "Count bugs".to_string()),
            ("location_mode", "online".to_string()),
            ("url", "https://example.org/bioblitz".to_string()),
            ("admission", "free".to_string()),
            ("topics[]", "birds".to_string()),
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
        serde_qs::Config::new(10, false)
            .deserialize_bytes(body.as_bytes())
            .unwrap()
    }

    #[test]
    fn only_a_specific_audience_has_ages() {
        let ages = |changes: &[(&str, &str)]| {
            submit_event(changes)
                .unwrap()
                .audience
                .into_iter()
                .collect::<Vec<_>>()
        };
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
                == "Please check the registration or ticket link; it should be a whole web address, such as https://example.org/register."
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
        // The name, then the dates, then the audience, then the webpage's
        // shape, then admission, then the topics, then when registration
        // opens.
        let long_name = "n".repeat(rules::NAME_LIMIT + 1);
        let changes = [
            ("attendance", "limited"),
            ("ticketing_date", "2099-10-01"),
            ("topics[]", ""),
            ("admission", ""),
            ("url", "example.org"),
            ("instances[0][repeat_frequency]", "weekly"),
            ("name", long_name.as_str()),
        ];
        let fields = [
            "name",
            "instances[0][repeat_until]",
            "limited_to",
            "url",
            "admission",
            "topics",
            "ticketing_timezone",
        ];
        let refused = submit_event_with_problems(&changes).unwrap_err();
        check!(refused.len() == fields.len());
        for (error, field) in refused.iter().zip(fields) {
            check!(error.is_about(field), "{}", error.message);
        }
    }

    #[test]
    fn a_new_event_picks_at_least_one_topic_and_at_most_three() {
        let refused = submit_event(&[("topics[]", "")]).unwrap_err();
        check!(refused.is_about("topics"));
        check!(refused.message == "Please choose at least one topic.");

        check!(submit_event(&[]).is_ok());

        let mut form = event_form_submitted(&[]);
        form.topics = ["a", "b", "c", "d"].map(String::from).to_vec();
        let refused = validate_and_build(&form, test_author()).unwrap_err();
        check!(refused.is_about("topics"));
        check!(refused.first().unwrap().message == "Please pick no more than 3 topics.");
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
    fn a_repeating_date_needs_a_date_to_repeat_until() {
        let refused = submit_event(&[("instances[0][repeat_frequency]", "weekly")]).unwrap_err();
        check!(refused.is_about("instances[0][repeat_until]"));
        check!(refused.message == "Please choose the date this date/time repeats until.");

        let event = submit_event(&[
            ("instances[0][repeat_frequency]", "weekly"),
            ("instances[0][repeat_until]", "2099-11-15"),
        ])
        .unwrap();
        check!(event.instances.len() == 3);

        // Never repeating, a stray date is ignored.
        let event = submit_event(&[("instances[0][repeat_until]", "2099-11-15")]).unwrap();
        check!(event.instances.len() == 1);
    }

    #[test]
    fn each_date_repeats_on_its_own() {
        // A Monday-to-Tuesday span every 2 weeks, and a Thursday that
        // doesn't repeat (Roland's case).
        let event = submit_event(&[
            ("instances[0][begin_date]", "2099-11-02"),
            ("instances[0][has_end]", "on"),
            ("instances[0][end_date]", "2099-11-03"),
            ("instances[0][end_time]", "16:00"),
            ("instances[0][repeat_frequency]", "biweekly"),
            ("instances[0][repeat_until]", "2099-11-30"),
            ("instances[1][begin_date]", "2099-11-05"),
            ("instances[1][begin_time]", "10:00"),
            ("instances[1][timezone]", "America/Chicago"),
            ("instances[1][repeat_until]", "2099-11-30"),
        ])
        .unwrap();
        let begins = event
            .instances
            .iter()
            .map(|instance| instance.begin.to_string())
            .collect::<Vec<_>>();
        check!(
            begins
                == [
                    "2099-11-02T16:00:00Z",
                    "2099-11-16T16:00:00Z",
                    "2099-11-30T16:00:00Z",
                    "2099-11-05T16:00:00Z",
                ]
        );
        check!(&*event.instances[2].end == "2099-12-01T22:00:00Z");

        // The refusal marks the row that repeats without a date to stop.
        let refused = submit_event(&[
            ("instances[1][begin_date]", "2099-11-05"),
            ("instances[1][begin_time]", "10:00"),
            ("instances[1][timezone]", "America/Chicago"),
            ("instances[1][repeat_frequency]", "weekly"),
        ])
        .unwrap_err();
        check!(refused.is_about("instances[1][repeat_until]"));
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
            list => panic!("{list:?} isn't a list of several answers"),
        };
        let body = format!(
            "location_mode={mode}&formats[]=bioblitz&venue=library\
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
    fn event_types_are_alphabetical_with_other_last() {
        let formats = crate::event::in_type_order(vec![
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

        // Programs too, which their list keeps in another order.
        let programs = crate::event::in_type_order(
            [
                "Acts of Science Connected",
                "NASA Night Sky Network",
                "Ambassador",
                "City Nature Challenge",
                "Do NASA Science Live",
                "International Monarch Monitoring Blitz",
                "Citizen Science Month",
                "SciStarter Library Network",
            ]
            .into_iter()
            .map(|name| choice(ChoiceList::EventProgram, name, name, None))
            .collect(),
        );
        let names = programs
            .iter()
            .map(|value| value.name.as_str())
            .collect::<Vec<_>>();
        check!(
            names
                == [
                    "Acts of Science Connected",
                    "Ambassador",
                    "Citizen Science Month",
                    "City Nature Challenge",
                    "Do NASA Science Live",
                    "International Monarch Monitoring Blitz",
                    "NASA Night Sky Network",
                    "SciStarter Library Network",
                ]
        );
    }

    #[test]
    fn the_event_choices_say_how_many_may_be_picked() {
        let audience = choice_checkboxes(
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

        let topics = choice_checkboxes(
            "topics",
            ChoiceList::Topic,
            vec![choice(ChoiceList::Topic, "birds", "Birds", None)],
            &["birds".to_string()],
        )
        .render_once()
        .unwrap();
        check!(topics.contains(
            r#"<legend>Topics<span class="pill-required" aria-hidden="true"> *</span></legend>"#
        ));
        check!(topics.contains("Choose up to three topics that best describe your event."));
        check!(topics.contains(r#"name="topics[]" value="birds" checked"#));
    }

    #[test]
    fn the_kind_of_venue_is_asked_under_the_location_note() {
        let venues = vec![
            choice(ChoiceList::EventVenue, "library", "Library", Some(406)),
            choice(
                ChoiceList::EventVenue,
                "museum-event",
                "Museum event",
                Some(190),
            ),
        ];
        let html = picker("in_person")
            .for_events()
            .with_venues(venues.clone(), "library")
            .render_once()
            .unwrap();

        // The kind of venue follows the note, as the event page lists them.
        let at = |text: &str| html.find(text).unwrap();
        let kind = at("<legend>Kind of venue (optional)</legend>");
        check!(at("Venue &amp; address details") < at(r#"data-mode="region""#));
        check!(at(r#"data-mode="region""#) < at("Location note"));
        check!(at("Location note") < kind);
        check!(!html.contains("data-mode-link"));
        // One answer, or none of them.
        check!(html.contains(r#"type="radio" name="venue" value="library" checked"#));
        check!(html.contains(r#"type="radio" name="venue" value="museum-event">"#));
        check!(html.contains(r#"type="radio" name="venue" value="">"#));
        check!(kind < at("None of these"));

        // Nothing is picked until one is; "None of these" takes a pick back.
        let html = picker("in_person")
            .for_events()
            .with_venues(venues, "")
            .render_once()
            .unwrap();
        check!(!html.contains(r#"name="venue" value="library" checked"#));
        check!(html.contains(r#"type="radio" name="venue" value="">"#));

        let html = picker("in_person")
            .with_venues(Vec::new(), "")
            .render_once()
            .unwrap();
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

        let online_event = picker("online").for_events().render_once().unwrap();
        check!(note(&online_event).contains(" data-no-online-note"));
        check!(note(&online_event).ends_with(" hidden"));

        let in_person_event = picker("in_person").for_events().render_once().unwrap();
        check!(note(&in_person_event).contains(" data-no-online-note"));
        check!(!note(&in_person_event).ends_with(" hidden"));

        // A project joined online may still say where.
        let online_project = picker("online").render_once().unwrap();
        check!(!note(&online_project).contains("data-no-online-note"));
        check!(!note(&online_project).ends_with(" hidden"));
    }

    #[test]
    fn an_event_is_asked_no_link_with_its_location() {
        let html = picker("online").for_events().render_once().unwrap();
        check!(!html.contains(r#"name="url""#));
        check!(!html.contains("virtual_join"));

        let html = picker("online").render_once().unwrap();
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
        let html = picker("").for_events().render_once().unwrap();
        check!(html.contains("How will people attend?"));
        for (value, label) in [("in_person", "In person"), ("online", "Virtual")] {
            let radio = toggle_radio(&html, value).unwrap();
            check!(radio.contains(" required"));
            check!(!radio.contains(" checked"));
            check!(html.contains(&format!("<span>{label}</span>")));
        }
        check!(html.matches(r#"name="location_mode""#).count() == 2);
        check!(!html.contains("Hybrid"));

        let html = picker("").render_once().unwrap();
        check!(html.matches(r#"name="location_mode""#).count() == 2);
        check!(!html.contains("Anywhere in the world"));
        check!(!html.contains("Where does it take place?"));
    }

    #[test]
    fn an_in_person_event_is_asked_where_it_takes_place() {
        let place = |html: &str, value: &str| {
            let start = html
                .find(&format!(
                    r#"<input type="radio" name="location_place" value="{value}""#
                ))
                .unwrap();
            html[start..start + html[start..].find('>').unwrap()].to_string()
        };

        // Asked once In person is chosen, under how people attend and before
        // the place search, each answer with its note under its words.
        let html = picker("").for_events().render_once().unwrap();
        check!(html.contains(
            r#"<fieldset class="choice-group" data-only-mode="in_person region anywhere" hidden>"#
        ));
        let at = |text: &str| html.find(text).unwrap();
        check!(at("How will people attend?") < at("Where does it take place?"));
        check!(at("Where does it take place?") < at("Add a place or address"));
        check!(html.contains(
            r#"<span>At one place<span class="radio-option-note">A venue, park, or other address people come to</span></span>"#
        ));
        check!(html.contains("Across an area"));
        check!(html.contains("Anywhere in the world"));
        check!(!html.contains("People take part in person wherever they are"));

        // The answer is the mode the In person answer submits.
        for mode in ["in_person", "region", "anywhere"] {
            let html = picker(mode).for_events().render_once().unwrap();
            check!(html.contains(
                r#"<fieldset class="choice-group" data-only-mode="in_person region anywhere" >"#
            ));
            for (value, _, _) in crate::component::location_picker::PLACE_ANSWERS {
                let radio = place(&html, value);
                check!(radio.contains(" data-mode-choice"));
                check!(
                    radio.contains(" checked") == (value == mode),
                    "{mode}: {radio}"
                );
            }
        }
    }

    #[test]
    fn the_toggle_shows_in_person_for_a_region_or_anywhere() {
        for mode in ["region", "anywhere"] {
            let html = picker(mode).for_events().render_once().unwrap();
            let radio = toggle_radio(&html, mode).unwrap();
            check!(radio.contains(" data-in-person"), "{mode}");
            check!(radio.contains(" checked"), "{mode}");
            check!(toggle_radio(&html, "in_person").is_none(), "{mode}");
        }
    }

    #[test]
    fn a_refused_mode_marks_the_toggle() {
        let error = FormError::at(
            "location_mode",
            "Please choose where the event takes place.",
        );
        let html = picker("")
            .for_events()
            .errors(&error.into())
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
    fn a_projects_place_search_words_are_used_as_its_address() {
        let in_person = "&location_mode=in_person&url=https://example.org/bugs";
        let refused = submit_project(&format!(
            "{in_person}&place_query=Field+station&location_name=Field+station"
        ))
        .unwrap_err();
        check!(refused.is_about("address"));
        check!(
            refused.message
                == "Please choose a place from the list, or choose to use what you typed as the address."
        );

        // A project's section is as it was: its way to a region is with the
        // address, its way back with the map.
        let html = picker("in_person").render_once().unwrap();
        check!(html.contains(r#"data-typed-into="address" data-typed-as="address""#));
        let at = |text: &str| html.find(text).unwrap();
        check!(at(r#"data-mode-link="region""#) < at(r#"data-mode="region""#));
        check!(at(r#"data-mode="region""#) < at("Back to a single place"));
        check!(at("Back to a single place") < at("Location note"));
        check!(html.contains(r#"placeholder="https://""#));

        let html = picker("in_person").for_events().render_once().unwrap();
        check!(html.contains(r#"data-typed-into="location_name" data-typed-as="venue name""#));
    }

    #[test]
    fn a_project_is_never_anywhere() {
        let refused =
            submit_project("&location_mode=anywhere&city=Chicago&url=https://example.org/bugs")
                .unwrap_err();
        check!(refused.is_about("location_mode"));
    }
}
