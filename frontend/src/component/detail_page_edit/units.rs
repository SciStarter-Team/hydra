//! Parts of an event the event page changes as a whole, as orrery's event
//! form changes them, because their values depend on each other: where it is
//! held ([`super::EventLocationDisplay`]), how people sign up for it
//! ([`EventRegistrationDisplay`]), what attending costs
//! ([`EventAdmissionDisplay`]), who can participate
//! ([`EventAttendanceDisplay`]), and the larger event it is part of
//! ([`EventPartOfDisplay`]). Each is shown, opened for change and saved the
//! same way, by [`show`], [`edit`] and [`save`].

use anyhow::Error;
use common::{message::user::BasicInfo, service::log_activity, shared::db, timestring::TimeString};
use jiff::Timestamp;
use poem::{IntoResponse, Response, web::Html};
use sailfish::TemplateOnce;
use ulid::Ulid;

use crate::{
    choice::{ChoiceList, ChoiceValue},
    component::{
        detail_page::DetailPageKind, event_answers::PartOfSelected, floating_field::FormErrors,
    },
    event::{
        EventInstance,
        admission::{Admission, EventAdmission},
        attendance::{Attendance, EventAttendance},
        field::{EventField, EventFieldValue},
        part_of::{self, LargerEvent},
        registration::{EventRegistration, Opening, Registration},
        rules::{self, Refusal},
    },
    local_time::{Clocks, TimeFormat},
    page_change::record_edit,
    peering::LoadStorePeered,
};

use super::{WaysToJoin, check_edit_permission, choice_row, instance_editor, internal_error};

/// A part of an event the event page changes as a whole: its display, which
/// opens, to an editor, onto the form that changes it.
pub trait EventUnit: TemplateOnce + Sized + Send {
    /// The part's path under `/-detail/:id/`, and the name its changes are
    /// recorded under.
    const NAME: &'static str;

    /// Whether only staff change the part, which the event's other editors
    /// see as it stands: one the event form no longer asks.
    const STAFF_ONLY: bool = false;

    /// The part as its form submits it.
    type Form: Send + Sync;

    /// The part as it is saved.
    type Value: PartialEq + Send + Sync;

    /// The display of the event `id`'s part as it now stands, to someone who
    /// can edit it or not, and is staff or not.
    fn load(
        id: &str,
        can_edit: bool,
        is_staff: bool,
    ) -> impl Future<Output = Result<Self, Error>> + Send;

    /// The part as it is saved, to change.
    fn stored(id: &str) -> impl Future<Output = Result<Self::Value, Error>> + Send;

    /// The form holding the part as it is saved.
    fn form_of(id: &str) -> impl Future<Output = Result<Self::Form, Error>> + Send;

    /// Open the display onto `form`, refused for `errors` when it was.
    fn open(
        &mut self,
        form: Self::Form,
        errors: FormErrors,
    ) -> impl Future<Output = Result<(), Error>> + Send;

    /// The part `form` settles on, by the rules orrery's event form keeps,
    /// or everything that keeps it from being saved over `stored`.
    fn settle(
        id: &str,
        form: &Self::Form,
        stored: &Self::Value,
    ) -> impl Future<Output = Result<Result<Self::Value, FormErrors>, Error>> + Send;

    /// The change saving `value` is.
    fn change(value: Self::Value) -> EventFieldValue;

    /// Once a change is saved, show what else it changed, out of band.
    fn saved(
        &mut self,
        _id: &str,
        _is_staff: bool,
    ) -> impl Future<Output = Result<(), Error>> + Send {
        async { Ok(()) }
    }
}

/// Whether someone who can edit the event (`can_edit`), and is staff or
/// not, changes the part `U`.
pub fn may_edit<U: EventUnit>(can_edit: bool, is_staff: bool) -> bool {
    can_edit && (is_staff || !U::STAFF_ONLY)
}

/// Whether `user` may change the event `id`'s part `U`, and is staff; or why
/// they can't.
pub async fn unit_editor<U: EventUnit>(id: &str, user: Option<&BasicInfo>) -> poem::Result<bool> {
    let is_staff = instance_editor(id, user).await?;
    if !may_edit::<U>(true, is_staff) {
        return Err(poem::Error::from_string(
            "Forbidden",
            poem::http::StatusCode::FORBIDDEN,
        ));
    }
    Ok(is_staff)
}

fn respond(unit: impl TemplateOnce) -> poem::Result<Response> {
    Ok(Html(unit.render_once().map_err(internal_error)?).into_response())
}

/// GET `/-detail/:id/<part>`: the part as it now stands.
pub async fn show<U: EventUnit>(id: &str, user: Option<&BasicInfo>) -> poem::Result<Response> {
    let (can_edit, is_staff) = match user {
        Some(user) => check_edit_permission(id, user, DetailPageKind::Event)
            .await
            .unwrap_or((false, false)),
        None => (false, false),
    };
    respond(
        U::load(id, may_edit::<U>(can_edit, is_staff), is_staff)
            .await
            .map_err(internal_error)?,
    )
}

/// GET `/-detail/:id/<part>/edit`: the part, open to change.
pub async fn edit<U: EventUnit>(id: &str, user: Option<&BasicInfo>) -> poem::Result<Response> {
    let is_staff = unit_editor::<U>(id, user).await?;
    let mut unit = U::load(id, true, is_staff).await.map_err(internal_error)?;
    let form = U::form_of(id).await.map_err(internal_error)?;
    unit.open(form, FormErrors::new())
        .await
        .map_err(internal_error)?;
    respond(unit)
}

/// PUT `/-detail/:id/<part>`: change the part, or say why it can't be.
pub async fn save<U: EventUnit>(
    id: &str,
    user: Option<&BasicInfo>,
    form: U::Form,
) -> poem::Result<Response> {
    let is_staff = unit_editor::<U>(id, user).await?;
    let user = user.expect("an editor is signed in");
    let target = Ulid::from_string(id).map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::BAD_REQUEST)
    })?;

    let stored = U::stored(id).await.map_err(internal_error)?;
    let value = match U::settle(id, &form, &stored)
        .await
        .map_err(internal_error)?
    {
        Ok(value) => value,
        Err(errors) => {
            let mut unit = U::load(id, true, is_staff).await.map_err(internal_error)?;
            unit.open(form, errors).await.map_err(internal_error)?;
            return respond(unit);
        }
    };

    if value != stored {
        EventField::new(target, U::change(value))
            .store()
            .await
            .map_err(internal_error)?;
        let _ = log_activity(
            None,
            record_edit(DetailPageKind::Event, is_staff),
            user.user_id,
            target,
            U::NAME.to_string(),
        )
        .await;
    }

    let mut unit = U::load(id, true, is_staff).await.map_err(internal_error)?;
    unit.saved(id, is_staff).await.map_err(internal_error)?;
    respond(unit)
}

// ---------------------------------------------------------------------------
// Admission
// ---------------------------------------------------------------------------

/// The event page's Admission row: whether attending is free, paid or a
/// suggested donation, with the price or the donation, or the cost an event
/// never asked gave.
#[derive(TemplateOnce)]
#[template(path = "pages/detail_page_admission.stpl.html")]
pub struct EventAdmissionDisplay {
    target_id: String,
    can_edit: bool,
    admission: EventAdmission,
    editing: Option<AdmissionForm>,
    errors: FormErrors,
}

impl EventAdmissionDisplay {
    /// The row from the event page's own values.
    pub fn new(target_id: String, can_edit: bool, admission: EventAdmission) -> Self {
        EventAdmissionDisplay {
            target_id,
            can_edit,
            admission,
            editing: None,
            errors: FormErrors::new(),
        }
    }
}

/// Admission as its editor submits it, named as the add event form names it.
#[derive(Debug, Default, serde::Deserialize)]
#[serde(default)]
pub struct AdmissionForm {
    admission: String,
    cost: String,
}

impl EventUnit for EventAdmissionDisplay {
    const NAME: &'static str = "admission";
    type Form = AdmissionForm;
    type Value = EventAdmission;

    async fn load(id: &str, can_edit: bool, _is_staff: bool) -> Result<Self, Error> {
        Ok(EventAdmissionDisplay::new(
            id.to_string(),
            can_edit,
            EventAdmission::read(Ulid::from_string(id)?).await?,
        ))
    }

    async fn stored(id: &str) -> Result<EventAdmission, Error> {
        EventAdmission::read(Ulid::from_string(id)?).await
    }

    async fn form_of(id: &str) -> Result<AdmissionForm, Error> {
        let stored = Self::stored(id).await?;
        Ok(AdmissionForm {
            admission: Admission::stored(stored.admission).to_string(),
            cost: stored.cost,
        })
    }

    async fn open(&mut self, form: AdmissionForm, errors: FormErrors) -> Result<(), Error> {
        self.editing = Some(form);
        self.errors = errors;
        Ok(())
    }

    async fn settle(
        _id: &str,
        form: &AdmissionForm,
        stored: &EventAdmission,
    ) -> Result<Result<EventAdmission, FormErrors>, Error> {
        Ok(
            rules::admission(Admission::parse(&form.admission), &form.cost, Some(stored))
                .map_err(|refusal| refusal.into_form_error(0).into()),
        )
    }

    fn change(value: EventAdmission) -> EventFieldValue {
        EventFieldValue::Admission(value)
    }
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

/// The event page's Registration row: whether people must register, where
/// they register or get tickets, and when registration opens while that's
/// still to come, on the clocks of the event's zone, or that it's open, until
/// the event is over.
#[derive(TemplateOnce)]
#[template(path = "pages/detail_page_registration.stpl.html")]
pub struct EventRegistrationDisplay {
    target_id: String,
    can_edit: bool,
    registration: EventRegistration,
    /// The event's venue's time zone, which the opening is shown on; `None`
    /// shows it on the viewer's clocks.
    zone: Option<jiff::tz::TimeZone>,
    /// When the event's last date ends, after which it's over; `None` for an
    /// event with no dates.
    ends: Option<Timestamp>,
    editing: Option<RegistrationForm>,
    errors: FormErrors,
}

impl EventRegistrationDisplay {
    /// The row from the event page's own values, `instances` its dates.
    pub fn new(
        target_id: String,
        can_edit: bool,
        registration: EventRegistration,
        zone: Option<jiff::tz::TimeZone>,
        instances: &[EventInstance],
    ) -> Self {
        EventRegistrationDisplay {
            target_id,
            can_edit,
            registration,
            zone,
            ends: crate::event::last_end(instances),
            editing: None,
            errors: FormErrors::new(),
        }
    }

    /// What the page says of when registration opens: when, while that's to
    /// come, or that it's open, until the event is over.
    fn opening(&self) -> Option<String> {
        match self.registration.opening(Timestamp::now(), self.ends)? {
            Opening::Upcoming(opens) => Some(format!(
                "Opens {}",
                crate::local_time::zoned_time(
                    opens,
                    TimeFormat::DateTime,
                    Clocks::of(self.zone.as_ref()),
                )
            )),
            Opening::Open => Some("Registration open".to_string()),
        }
    }
}

/// Registration as its editor submits it, named as the add event form names
/// it.
#[derive(Debug, Default, serde::Deserialize)]
#[serde(default)]
pub struct RegistrationForm {
    registration: String,
    ticket_url: String,
    ticketing_date: String,
    ticketing_time: String,
    ticketing_timezone: String,
}

impl EventUnit for EventRegistrationDisplay {
    const NAME: &'static str = "registration";
    type Form = RegistrationForm;
    type Value = EventRegistration;

    async fn load(id: &str, can_edit: bool, _is_staff: bool) -> Result<Self, Error> {
        Ok(EventRegistrationDisplay::new(
            id.to_string(),
            can_edit,
            Self::stored(id).await?,
            crate::event::zone::of_event(id).await?,
            &crate::event::read_instances(db(), id).await?,
        ))
    }

    async fn stored(id: &str) -> Result<EventRegistration, Error> {
        EventRegistration::read(Ulid::from_string(id)?).await
    }

    /// The opening is given on the clocks of the event's zone, or UTC's for
    /// an event with none.
    async fn form_of(id: &str) -> Result<RegistrationForm, Error> {
        let stored = Self::stored(id).await?;
        let zone = crate::event::zone::of_event(id)
            .await?
            .unwrap_or(jiff::tz::TimeZone::UTC);
        let opens = stored
            .ticketing_begins
            .as_deref()
            .map(str::parse::<Timestamp>)
            .transpose()?
            .map(|opens| opens.to_zoned(zone.clone()));
        Ok(RegistrationForm {
            registration: Registration::stored(stored.registration).to_string(),
            ticket_url: stored.ticket_url,
            ticketing_date: opens
                .as_ref()
                .map(|opens| opens.strftime("%Y-%m-%d").to_string())
                .unwrap_or_default(),
            ticketing_time: opens
                .as_ref()
                .map(|opens| opens.strftime("%H:%M").to_string())
                .unwrap_or_default(),
            ticketing_timezone: zone.iana_name().unwrap_or("UTC").to_string(),
        })
    }

    async fn open(&mut self, form: RegistrationForm, errors: FormErrors) -> Result<(), Error> {
        self.editing = Some(form);
        self.errors = errors;
        Ok(())
    }

    /// A virtual event keeps a way to join, which its registration link may
    /// be.
    async fn settle(
        id: &str,
        form: &RegistrationForm,
        stored: &EventRegistration,
    ) -> Result<Result<EventRegistration, FormErrors>, Error> {
        let answer = Registration::parse(&form.registration);
        let ticket_url = form.ticket_url.trim();
        let ways = WaysToJoin::of_event(id).await?;
        let mut errors = FormErrors::new();
        let refused = |refusal: Refusal| refusal.into_form_error(0);
        errors.take(
            rules::ticket_link(ticket_url, answer, Some(&stored.ticket_url))
                .and_then(|()| {
                    if ticket_url == stored.ticket_url.trim() {
                        Ok(())
                    } else {
                        ways.with_ticket_url(ticket_url).check()
                    }
                })
                .map_err(refused),
        );
        let opens = errors.take(
            rules::registration_opens(
                answer,
                &form.ticketing_date,
                &form.ticketing_time,
                &form.ticketing_timezone,
            )
            .map_err(refused),
        );
        let Some(opens) = opens else {
            return Ok(Err(errors));
        };
        Ok(errors.into_result().map(|()| EventRegistration {
            registration: answer,
            ticket_url: ticket_url.to_string(),
            ticketing_begins: opens.map(TimeString::from),
        }))
    }

    fn change(value: EventRegistration) -> EventFieldValue {
        EventFieldValue::Registration(value)
    }
}

// ---------------------------------------------------------------------------
// Who can participate
// ---------------------------------------------------------------------------

/// The event page's "Who can participate" row: everyone, or the audience the
/// event is meant for, and then the ages it suits, which are shown only of
/// an event for a specific audience, as only it is asked them.
#[derive(TemplateOnce)]
#[template(path = "pages/detail_page_attendance.stpl.html")]
pub struct EventAttendanceDisplay {
    target_id: String,
    can_edit: bool,
    limited_to: String,
    /// The values of the ages the event picks, in list order.
    ages: Vec<ChoiceValue>,
    /// The form, with the ages it offers.
    editing: Option<(AttendanceForm, Vec<ChoiceValue>)>,
    errors: FormErrors,
}

impl EventAttendanceDisplay {
    /// The row from the event page's own values: the audience the event is
    /// limited to, and the values of the ages it picks.
    pub fn new(
        target_id: String,
        can_edit: bool,
        limited_to: String,
        ages: Vec<ChoiceValue>,
    ) -> Self {
        EventAttendanceDisplay {
            target_id,
            can_edit,
            limited_to,
            ages,
            editing: None,
            errors: FormErrors::new(),
        }
    }

    /// The row of the ages, shown of an event for a specific audience.
    fn ages_row(&self) -> Option<String> {
        Attendance::of(&self.limited_to)
            .asks_ages()
            .then(|| choice_row(ChoiceList::Audience, &self.ages, self.can_edit))
            .filter(|row| !row.is_empty())
    }
}

/// Who can participate as its editor submits it, named as the add event form
/// names it.
#[derive(Debug, Default, serde::Deserialize)]
#[serde(default)]
pub struct AttendanceForm {
    attendance: Attendance,
    limited_to: String,
    audience: Vec<String>,
}

impl EventUnit for EventAttendanceDisplay {
    const NAME: &'static str = "attendance";
    type Form = AttendanceForm;
    type Value = EventAttendance;

    async fn load(id: &str, can_edit: bool, _is_staff: bool) -> Result<Self, Error> {
        let stored = Self::stored(id).await?;
        Ok(EventAttendanceDisplay::new(
            id.to_string(),
            can_edit,
            stored.limited_to,
            ChoiceValue::picked_by(ChoiceList::Audience, id).await?,
        ))
    }

    async fn stored(id: &str) -> Result<EventAttendance, Error> {
        EventAttendance::read(Ulid::from_string(id)?).await
    }

    async fn form_of(id: &str) -> Result<AttendanceForm, Error> {
        let stored = Self::stored(id).await?;
        Ok(AttendanceForm {
            attendance: stored.attendance(),
            limited_to: stored.limited_to,
            audience: stored.ages.into_iter().collect(),
        })
    }

    async fn open(&mut self, form: AttendanceForm, errors: FormErrors) -> Result<(), Error> {
        self.editing = Some((form, ChoiceValue::all(ChoiceList::Audience).await?));
        self.errors = errors;
        Ok(())
    }

    /// The ages ticked are read against their list.
    async fn settle(
        _id: &str,
        form: &AttendanceForm,
        stored: &EventAttendance,
    ) -> Result<Result<EventAttendance, FormErrors>, Error> {
        let ages = ChoiceValue::known(ChoiceList::Audience, form.audience.iter().cloned()).await?;
        Ok(
            rules::attendance(form.attendance, &form.limited_to, ages, Some(stored))
                .map_err(|refusal| refusal.into_form_error(0).into()),
        )
    }

    fn change(value: EventAttendance) -> EventFieldValue {
        EventFieldValue::Attendance(value)
    }
}

// ---------------------------------------------------------------------------
// Part of a larger event
// ---------------------------------------------------------------------------

/// The event page's "Part of" row: the larger event this one is part of,
/// named and linked while it is published. The event form no longer asks it
/// (Darlene's review), so only staff change it; an edit by anyone else
/// keeps it as it was saved.
#[derive(TemplateOnce)]
#[template(path = "pages/detail_page_part_of.stpl.html")]
pub struct EventPartOfDisplay {
    target_id: String,
    can_edit: bool,
    /// The larger event, as it is saved.
    part_of: Option<Ulid>,
    /// It, while it is published.
    larger: Option<LargerEvent>,
    /// The form, with the larger event it picks.
    editing: Option<PartOfSelected>,
    errors: FormErrors,
}

impl EventPartOfDisplay {
    /// The row from the event page's own values, to someone who can edit
    /// the event or not, and is staff or not: the larger event saved, and it
    /// while it is published.
    pub fn new(
        target_id: String,
        can_edit: bool,
        is_staff: bool,
        part_of: Option<Ulid>,
        larger: Option<LargerEvent>,
    ) -> Self {
        EventPartOfDisplay {
            target_id,
            can_edit: may_edit::<Self>(can_edit, is_staff),
            part_of,
            larger,
            editing: None,
            errors: FormErrors::new(),
        }
    }
}

/// The larger event as its editor submits it, named as the add event form
/// names it.
#[derive(Debug, Default, serde::Deserialize)]
#[serde(default)]
pub struct PartOfForm {
    part_of: String,
}

impl EventUnit for EventPartOfDisplay {
    const NAME: &'static str = "part_of";
    const STAFF_ONLY: bool = true;
    type Form = PartOfForm;
    type Value = Option<Ulid>;

    async fn load(id: &str, can_edit: bool, is_staff: bool) -> Result<Self, Error> {
        let part_of = Self::stored(id).await?;
        let larger = match part_of {
            Some(part_of) => LargerEvent::published(part_of).await?,
            None => None,
        };
        Ok(EventPartOfDisplay::new(
            id.to_string(),
            can_edit,
            is_staff,
            part_of,
            larger,
        ))
    }

    async fn stored(id: &str) -> Result<Option<Ulid>, Error> {
        part_of::read(Ulid::from_string(id)?).await
    }

    async fn form_of(id: &str) -> Result<PartOfForm, Error> {
        Ok(PartOfForm {
            part_of: Self::stored(id)
                .await?
                .map(|id| id.to_string())
                .unwrap_or_default(),
        })
    }

    async fn open(&mut self, form: PartOfForm, errors: FormErrors) -> Result<(), Error> {
        self.editing =
            Some(PartOfSelected::of(&self.target_id, form.part_of.trim().parse().ok()).await?);
        self.errors = errors;
        Ok(())
    }

    async fn settle(
        id: &str,
        form: &PartOfForm,
        stored: &Option<Ulid>,
    ) -> Result<Result<Option<Ulid>, FormErrors>, Error> {
        Ok(
            part_of::settle(Ulid::from_string(id)?, &form.part_of, *stored)
                .await?
                .map_err(|refusal| refusal.into_form_error(0).into()),
        )
    }

    fn change(value: Option<Ulid>) -> EventFieldValue {
        EventFieldValue::PartOf(value)
    }
}

#[cfg(test)]
mod tests {
    use assert2::check;

    use super::*;
    use crate::component::floating_field::FormError;

    fn admission_row(admission: Option<Admission>, cost: &str, can_edit: bool) -> String {
        EventAdmissionDisplay::new(
            Ulid::nil().to_string(),
            can_edit,
            EventAdmission {
                admission,
                cost: cost.to_string(),
            },
        )
        .render_once()
        .unwrap()
    }

    #[test]
    fn the_admission_row_says_the_answer_with_its_price() {
        let row = admission_row(Some(Admission::Donation), "$5", false);
        check!(row.contains(
            r#"<span class="font-semibold">Admission:</span><span>Suggested donation: $5</span>"#
        ));
        check!(!row.contains("edit-only"));

        // An event never asked, that gave no cost, has no row but to its
        // editors.
        let row = admission_row(None, "", false);
        check!(!row.contains("Admission:"));
        let row = admission_row(None, "", true);
        check!(row.contains("Admission:"));
        check!(row.contains(r#"<div id="event-admission" class="relative edit-only">"#));
    }

    #[test]
    fn admission_opens_onto_the_add_forms_question() {
        let mut row =
            EventAdmissionDisplay::new(Ulid::nil().to_string(), true, EventAdmission::default());
        row.editing = Some(AdmissionForm {
            admission: "paid".to_string(),
            cost: String::new(),
        });
        row.errors = FormError::at("cost", "Please give the admission price.").into();
        let html = row.render_once().unwrap();
        check!(html.contains(r#"hx-put="/-detail/00000000000000000000000000/admission""#));
        check!(html.contains("Please give the admission price."));
        check!(html.contains(r#"name="admission" value="paid" required checked"#));
        check!(html.contains("Admission price"));
    }

    fn registration_row(registration: EventRegistration) -> String {
        EventRegistrationDisplay::new(Ulid::nil().to_string(), false, registration, None, &[])
            .render_once()
            .unwrap()
    }

    #[test]
    fn the_registration_row_says_the_answer_the_link_and_when_it_opens() {
        let row = registration_row(EventRegistration {
            registration: Some(Registration::Required),
            ticket_url: "https://example.org/register".to_string(),
            ticketing_begins: Some(
                TimeString::try_from("2099-10-01T14:00:00Z".to_string()).unwrap(),
            ),
        });
        check!(row.contains("<span>Registration required</span>"));
        check!(row.contains(r#"href="https://example.org/register""#));
        check!(row.contains("<span>Opens "));

        // Nobody registers for an event that needs none, though its link may
        // serve for tickets.
        let row = registration_row(EventRegistration {
            registration: Some(Registration::NotNeeded),
            ticket_url: "https://example.org/tickets".to_string(),
            ticketing_begins: None,
        });
        check!(row.contains("<span>No registration needed</span>"));
        check!(row.contains(r#"href="https://example.org/tickets""#));
        check!(!row.contains("Opens"));

        // Once it has opened, it's open until the event is over.
        let opened = EventRegistration {
            registration: Some(Registration::Required),
            ticket_url: String::new(),
            ticketing_begins: Some(
                TimeString::try_from("2020-10-01T14:00:00Z".to_string()).unwrap(),
            ),
        };
        let ending = |end: &str| {
            let at = TimeString::try_from(end.to_string()).unwrap();
            EventRegistrationDisplay::new(
                Ulid::nil().to_string(),
                false,
                opened.clone(),
                None,
                &[EventInstance {
                    begin: at.clone(),
                    end: at,
                }],
            )
            .render_once()
            .unwrap()
        };
        check!(ending("2099-11-01T14:00:00Z").contains("<span>Registration open</span>"));
        let over = ending("2020-11-01T14:00:00Z");
        check!(!over.contains("Registration open"));
        check!(!over.contains("Opens"));

        check!(!registration_row(EventRegistration::default()).contains("Registration:"));
    }

    fn age(slug: &str, name: &str) -> ChoiceValue {
        ChoiceValue {
            list: ChoiceList::Audience,
            slug: slug.to_string(),
            name: name.to_string(),
            position: 0,
            orrery_id: None,
        }
    }

    fn attendance_row(limited_to: &str, can_edit: bool) -> String {
        EventAttendanceDisplay::new(
            Ulid::nil().to_string(),
            can_edit,
            limited_to.to_string(),
            vec![age("adults", "Adults")],
        )
        .render_once()
        .unwrap()
    }

    #[test]
    fn an_event_says_who_can_participate_and_the_ages_of_a_specific_audience() {
        let row = attendance_row("K–12 teachers & aides", false);
        check!(row.contains(
            r#"<span class="font-semibold">Who can participate:</span>K–12 teachers &amp; aides</p>"#
        ));
        check!(row.contains(">Recommended ages</span>"));
        check!(row.contains(">Adults</span>"));

        // An event for everyone keeps its ages for the finder, unshown, even
        // to its editors, as its form doesn't ask them.
        for can_edit in [false, true] {
            let row = attendance_row("", can_edit);
            check!(row.contains(
                r#"<span class="font-semibold">Who can participate:</span>Everyone is welcome</p>"#
            ));
            check!(!row.contains("Recommended ages"));
            check!(!row.contains("Adults"));
        }
    }

    #[test]
    fn who_can_participate_opens_onto_the_add_forms_question() {
        let mut row =
            EventAttendanceDisplay::new(Ulid::nil().to_string(), true, String::new(), Vec::new());
        row.editing = Some((
            AttendanceForm {
                attendance: Attendance::Limited,
                limited_to: String::new(),
                audience: vec!["adults".to_string()],
            },
            vec![age("families", "Families"), age("adults", "Adults")],
        ));
        row.errors = FormError::at("limited_to", "Please say who the event is for.").into();
        let html = row.render_once().unwrap();
        check!(html.contains(r#"hx-put="/-detail/00000000000000000000000000/attendance""#));
        check!(html.contains(r#"name="attendance" value="limited" checked>"#));
        check!(html.contains(r#"<div class="radio-option-field" data-limited-only >"#));
        check!(html.contains("Who is this event for?"));
        check!(html.contains("Please say who the event is for."));
        check!(html.contains(r#"value="adults" checked"#), "{html}");
        check!(!html.contains(r#"value="families" checked"#));
    }

    fn part_of_row(
        part_of: Option<Ulid>,
        larger: Option<LargerEvent>,
        can_edit: bool,
        is_staff: bool,
    ) -> String {
        EventPartOfDisplay::new(Ulid::nil().to_string(), can_edit, is_staff, part_of, larger)
            .render_once()
            .unwrap()
    }

    #[test]
    fn the_larger_event_is_linked_while_it_is_published() {
        let id = Ulid::from_string("01JBBBBBBBBBBBBBBBBBBBBBBB").unwrap();
        let larger = LargerEvent {
            event_id: id,
            name: "City Nature Challenge".to_string(),
            path: "/events/city-nature-challenge".to_string(),
        };
        let row = part_of_row(Some(id), Some(larger.clone()), false, false);
        check!(row.contains(r#"href="/events/city-nature-challenge">City Nature Challenge</a>"#));

        // One that isn't published is named to no one, and only staff, who
        // change it, are told one was picked.
        let row = part_of_row(Some(id), None, false, false);
        check!(!row.contains("Part of"));
        let row = part_of_row(Some(id), None, true, true);
        check!(row.contains("an event that isn&rsquo;t published"));
        check!(row.contains(r#"<div id="event-part-of" class="relative edit-only">"#));

        // The event's other editors see it as it stands, and can't change it.
        let row = part_of_row(Some(id), Some(larger), true, false);
        check!(row.contains("City Nature Challenge"));
        check!(!row.contains("/part_of/edit"));
        check!(!part_of_row(Some(id), None, true, false).contains("Part of"));
        check!(may_edit::<EventPartOfDisplay>(true, true));
        check!(!may_edit::<EventPartOfDisplay>(true, false));
        check!(may_edit::<EventAdmissionDisplay>(true, false));
    }

    #[test]
    fn the_larger_event_opens_onto_a_search_of_the_others() {
        let mut row = EventPartOfDisplay::new(Ulid::nil().to_string(), true, true, None, None);
        row.editing = Some(PartOfSelected::none(&Ulid::nil().to_string()));
        let html = row.render_once().unwrap();
        check!(html.contains(r#"hx-put="/-detail/00000000000000000000000000/part_of""#));
        check!(html.contains("Is this part of a larger event?"));
        check!(html.contains(r#"hx-get="/-detail/00000000000000000000000000/part_of/search""#));
        check!(html.contains(r#"<input type="hidden" name="part_of" id="part_of" value="">"#));
    }
}
