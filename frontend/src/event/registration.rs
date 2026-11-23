//! How people sign up for an event, as orrery's event form asks it: whether
//! they must register ("Registration required?"), where they register or get
//! tickets, and when registration opens. An event saved before the question
//! was asked has no answer, and keeps its link and opening date alone.
//!
//! The three are one value to change: an event nobody registers for has no
//! opening date, and one that requires registration says where. So the event
//! page edits them together.

use anyhow::Error;
use common::{shared::db, timestring::TimeString};
use jiff::Timestamp;
use ulid::Ulid;

/// The answer to "Registration required?".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Registration {
    #[serde(rename = "required")]
    Required,
    /// Nobody registers.
    #[serde(rename = "none")]
    NotNeeded,
    #[serde(rename = "optional")]
    Optional,
}

impl Registration {
    /// The answers, in the order the forms offer them.
    pub const ALL: [Registration; 3] = [
        Registration::Required,
        Registration::NotNeeded,
        Registration::Optional,
    ];

    /// The answer as it is stored and submitted.
    pub fn as_str(self) -> &'static str {
        match self {
            Registration::Required => "required",
            Registration::NotNeeded => "none",
            Registration::Optional => "optional",
        }
    }

    /// The answer `value` names, or `None` for no answer or one that isn't
    /// known.
    pub fn parse(value: &str) -> Option<Registration> {
        Registration::ALL
            .into_iter()
            .find(|answer| answer.as_str() == value.trim())
    }

    /// The answer as the forms offer it, to "Registration required?".
    pub fn label(self) -> &'static str {
        match self {
            Registration::Required => "Yes",
            Registration::NotNeeded => "No",
            Registration::Optional => "Optional",
        }
    }

    /// What the event page says of the answer.
    pub fn status(self) -> &'static str {
        match self {
            Registration::Required => "Registration required",
            Registration::NotNeeded => "No registration needed",
            Registration::Optional => "Registration optional",
        }
    }

    /// Whether an event answering `answer` is asked when registration opens:
    /// unless nobody registers.
    pub fn asks_opening(answer: Option<Registration>) -> bool {
        answer != Some(Registration::NotNeeded)
    }

    /// `answer` as its column stores it: empty for none.
    pub fn stored(answer: Option<Registration>) -> &'static str {
        answer.map_or("", Registration::as_str)
    }
}

/// How people sign up for an event.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EventRegistration {
    /// `None` for an event that was never asked.
    pub registration: Option<Registration>,
    /// Where people register or get tickets, whatever the answer: a ticket
    /// link may serve an event nobody registers for.
    pub ticket_url: String,
    /// When registration opens; never set for an event nobody registers for.
    pub ticketing_begins: Option<TimeString>,
}

/// What the event page says of when registration opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Opening<'a> {
    /// It opens at this time, still to come.
    Upcoming(&'a TimeString),
    /// It has opened, and the event isn't over.
    Open,
}

impl EventRegistration {
    /// What to say at `now` of when registration opens, for an event people
    /// register for that says when, whose last date ends at `ends`: when it
    /// opens, while that's to come; that it's open, once it has opened,
    /// until the event is over; and nothing after that.
    pub fn opening(&self, now: Timestamp, ends: Option<Timestamp>) -> Option<Opening<'_>> {
        let opens = self
            .ticketing_begins
            .as_ref()
            .filter(|_| Registration::asks_opening(self.registration))?;
        if opens.parse::<Timestamp>().ok()? > now {
            Some(Opening::Upcoming(opens))
        } else if ends.is_some_and(|ends| ends < now) {
            None
        } else {
            Some(Opening::Open)
        }
    }

    /// Whether the event page has anything to say of it.
    pub fn is_shown(&self) -> bool {
        self.registration.is_some() || !self.ticket_url.trim().is_empty()
    }

    /// The registration of the event `event_id`, as it is saved.
    pub async fn read(event_id: Ulid) -> Result<Self, Error> {
        let id = event_id.to_string();
        let row = sqlx::query!(
            "SELECT registration, ticket_url, ticketing_begins FROM events WHERE event_id = ?",
            id
        )
        .fetch_one(db())
        .await?;
        Ok(EventRegistration {
            registration: Registration::parse(&row.registration),
            ticket_url: row.ticket_url,
            ticketing_begins: row
                .ticketing_begins
                .map(TimeString::try_from)
                .transpose()
                .map_err(|e| anyhow::anyhow!("Invalid ticketing_begins TimeString: {e}"))?,
        })
    }

    /// Save this as the registration of the event `event_id`.
    pub async fn save(&self, event_id: Ulid) -> Result<(), Error> {
        let id = event_id.to_string();
        let registration = Registration::stored(self.registration);
        let ticketing_begins = self.ticketing_begins.as_deref();
        sqlx::query!(
            "UPDATE events SET registration = ?, ticket_url = ?, ticketing_begins = ? WHERE event_id = ?",
            registration,
            self.ticket_url,
            ticketing_begins,
            id
        )
        .execute(db())
        .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use assert2::check;

    use super::*;

    fn registration(answer: Option<Registration>, opens: &str) -> EventRegistration {
        EventRegistration {
            registration: answer,
            ticket_url: String::new(),
            ticketing_begins: Some(TimeString::try_from(opens.to_string()).unwrap()),
        }
    }

    fn now() -> Timestamp {
        "2027-03-10T12:00:00Z".parse().unwrap()
    }

    #[test]
    fn an_opening_is_told_unless_nobody_registers() {
        let ahead = "2027-04-01T15:00:00Z";
        let upcoming = |answer| {
            matches!(
                registration(answer, ahead).opening(now(), None),
                Some(Opening::Upcoming(_))
            )
        };
        check!(upcoming(Some(Registration::Required)));
        check!(upcoming(None));
        check!(
            registration(Some(Registration::NotNeeded), ahead)
                .opening(now(), None)
                .is_none()
        );
        check!(EventRegistration::default().opening(now(), None).is_none());
    }

    #[test]
    fn registration_is_open_once_it_opens_until_the_event_is_over() {
        let opened = registration(Some(Registration::Optional), "2027-03-01T15:00:00Z");
        let ends = |at: &str| Some(at.parse::<Timestamp>().unwrap());
        check!(opened.opening(now(), ends("2027-03-20T15:00:00Z")) == Some(Opening::Open));
        check!(opened.opening(now(), None) == Some(Opening::Open));
        check!(
            opened
                .opening(now(), ends("2027-03-05T15:00:00Z"))
                .is_none()
        );
    }

    #[test]
    fn answers_read_as_they_are_stored() {
        for answer in Registration::ALL {
            check!(Registration::parse(Registration::stored(Some(answer))) == Some(answer));
        }
        check!(Registration::parse("") == None);
    }
}
