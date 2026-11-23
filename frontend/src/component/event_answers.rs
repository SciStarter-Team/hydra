//! The answers orrery's event form asks about signing up for an event, what
//! attending it costs and who can participate ("Registration required?" with
//! the registration link and opening date, "Admission" with the price or the
//! suggested donation, and "Who can participate?" with the audience and its
//! ages), as the add event form asks them and the event page's editors ask
//! them again. Each set of fields is one component, so both places ask in
//! the same words; `js/event-answers.js` shows each answer's own fields under
//! it. Here too is "Is this part of a larger event?", which the form no
//! longer asks, and staff change on the event page.

use sailfish::TemplateOnce;

use crate::choice::ChoiceCheckboxes;
use crate::component::floating_field::FormErrors;
use crate::event::{
    admission::Admission, attendance::Attendance, part_of::LargerEvent, registration::Registration,
};
use ulid::Ulid;

/// "Who can participate?": everyone, or a specific audience, which is asked
/// under that answer, with the ages it suits.
#[derive(TemplateOnce)]
#[template(path = "site/attendance_fields.stpl.html")]
pub struct AttendanceFields<'a> {
    pub attendance: Attendance,
    pub limited_to: &'a str,
    /// The ages, ticked as picked.
    pub ages: ChoiceCheckboxes,
    /// Whether the answers carry their own legend, where nothing heads them.
    pub legend_shown: bool,
    pub errors: &'a FormErrors,
}

/// "Admission": free, paid or a suggested donation, the price asked under
/// Paid and the donation under Suggested donation. The two are the one
/// `cost`; the one not asked is disabled, so as not to submit.
#[derive(TemplateOnce)]
#[template(path = "site/admission_fields.stpl.html")]
pub struct AdmissionFields<'a> {
    pub admission: Option<Admission>,
    pub cost: &'a str,
    /// Whether the answers carry their own legend, where nothing heads them.
    pub legend_shown: bool,
    pub errors: &'a FormErrors,
}

/// Where people register or get tickets, "Registration required?", and,
/// unless nobody registers, when registration opens, on the clocks of a time
/// zone named as of that date.
#[derive(TemplateOnce)]
#[template(path = "site/registration_fields.stpl.html")]
pub struct RegistrationFields<'a> {
    pub registration: Option<Registration>,
    pub ticket_url: &'a str,
    pub ticketing_date: &'a str,
    pub ticketing_time: &'a str,
    pub ticketing_timezone: &'a str,
    pub errors: &'a FormErrors,
}

impl RegistrationFields<'_> {
    /// Whether one of the errors is about the field `name`.
    fn about(&self, name: &str) -> bool {
        self.errors.is_about(name)
    }
}

/// "Is this part of a larger event?": a search of the published events
/// other than the one being changed, and the one picked.
#[derive(TemplateOnce)]
#[template(path = "site/part_of_field.stpl.html")]
pub struct PartOfField {
    pub selected: PartOfSelected,
    /// The id of the message telling the pick's problem, when it has one,
    /// which marks the search.
    pub problem: Option<String>,
}

/// The larger event picked for the event `event_id`, with a button to clear
/// it; nothing when none is.
#[derive(TemplateOnce)]
#[template(path = "site/part_of_selected.stpl.html")]
pub struct PartOfSelected {
    /// The event being changed, whose picker this is.
    pub event_id: String,
    /// The id of the event picked; empty for none.
    pub id: String,
    /// Its name, while it is published.
    pub name: Option<String>,
    /// Whether the clear button takes focus, as it does once a pick is made.
    pub autofocus: bool,
}

impl PartOfSelected {
    /// No event picked for the event `event_id`.
    pub fn none(event_id: &str) -> Self {
        PartOfSelected {
            event_id: event_id.to_string(),
            id: String::new(),
            name: None,
            autofocus: false,
        }
    }

    /// The event `picked` picked for the event `event_id`, named if it is
    /// published.
    pub async fn of(event_id: &str, picked: Option<Ulid>) -> Result<Self, anyhow::Error> {
        let Some(picked) = picked else {
            return Ok(PartOfSelected::none(event_id));
        };
        Ok(PartOfSelected {
            event_id: event_id.to_string(),
            id: picked.to_string(),
            name: LargerEvent::published(picked)
                .await?
                .map(|event| event.name),
            autofocus: false,
        })
    }

    /// The published event `picked`, as it was just picked for the event
    /// `event_id`, or none when it isn't one.
    pub async fn picked(event_id: &str, picked: &str) -> Result<Self, anyhow::Error> {
        let larger = match picked.trim().parse::<Ulid>() {
            Ok(picked) => LargerEvent::published(picked).await?,
            Err(_) => None,
        };
        Ok(match larger {
            Some(larger) => PartOfSelected {
                event_id: event_id.to_string(),
                id: larger.event_id.to_string(),
                name: Some(larger.name),
                autofocus: true,
            },
            None => PartOfSelected::none(event_id),
        })
    }
}

/// The published events whose names hold what was typed in the search, to
/// pick from, with how many there are said to assistive technology out of
/// band.
#[derive(TemplateOnce)]
#[template(path = "site/part_of_results.stpl.html")]
pub struct PartOfResults {
    /// The event being changed.
    pub event_id: String,
    pub events: Vec<LargerEvent>,
}

/// The larger event picked, replacing the one picked before, with the
/// hidden field holding it and the search's results cleared, out of band.
#[derive(TemplateOnce)]
#[template(path = "site/part_of_picked.stpl.html")]
pub struct PartOfPicked {
    pub selected: PartOfSelected,
}
