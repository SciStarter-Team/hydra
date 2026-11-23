//! Who can participate in an event, as orrery's event form asks it: everyone
//! is welcome, or the event is meant for a specific audience, which it names
//! ("Who is this event for?") along with the ages it suits.
//!
//! The answer, the audience and the ages are one value to change: the ages
//! are asked, and shown, only of an event for a specific audience. An event
//! for everyone keeps the ages it was saved with, unshown, because the
//! finder's age filter still reads them.

use std::collections::BTreeSet;

use anyhow::Error;
use common::{shared::db, transaction::Immediate};
use ulid::Ulid;

use crate::choice::{ChoiceList, ChoiceValue};

/// The answer to "Who can participate?".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Attendance {
    /// Everyone is welcome.
    #[default]
    Open,
    /// The event is meant for a specific audience.
    Limited,
}

impl Attendance {
    /// The answers, in the order the forms offer them.
    pub const ALL: [Attendance; 2] = [Attendance::Open, Attendance::Limited];

    /// The answer as it is submitted.
    pub fn as_str(self) -> &'static str {
        match self {
            Attendance::Open => "open",
            Attendance::Limited => "limited",
        }
    }

    /// The answer an event limited to `limited_to` gives: everyone is
    /// welcome unless it names an audience.
    pub fn of(limited_to: &str) -> Attendance {
        if limited_to.trim().is_empty() {
            Attendance::Open
        } else {
            Attendance::Limited
        }
    }

    /// Whether an event giving this answer is asked the ages it suits, and
    /// its page shows them: only an event for a specific audience.
    pub fn asks_ages(self) -> bool {
        self == Attendance::Limited
    }

    /// The answer as the forms offer it.
    pub fn label(self) -> &'static str {
        match self {
            Attendance::Open => "Everyone is welcome",
            Attendance::Limited => "This event is intended for a specific audience",
        }
    }
}

/// Who can participate in an event.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EventAttendance {
    /// The audience the event is meant for; empty when everyone is welcome.
    pub limited_to: String,
    /// The slugs of the ages the event suits, from the audience list.
    pub ages: BTreeSet<String>,
}

impl EventAttendance {
    /// The answer the event gives.
    pub fn attendance(&self) -> Attendance {
        Attendance::of(&self.limited_to)
    }

    /// Who can participate in the event `event_id`, as it is saved.
    pub async fn read(event_id: Ulid) -> Result<Self, Error> {
        let id = event_id.to_string();
        let limited_to =
            sqlx::query_scalar!("SELECT limited_to FROM events WHERE event_id = ?", id)
                .fetch_one(db())
                .await?;
        let ages = ChoiceValue::of_target(ChoiceList::Audience, &id).await?;
        Ok(EventAttendance { limited_to, ages })
    }

    /// Save this as who can participate in the event `event_id`.
    pub async fn save(&self, event_id: Ulid) -> Result<(), Error> {
        let id = event_id.to_string();
        let mut tx = Immediate::begin_with_retry(db(), None, None).await?;
        sqlx::query!(
            "UPDATE events SET limited_to = ? WHERE event_id = ?",
            self.limited_to,
            id
        )
        .execute(&mut tx)
        .await?;
        crate::choice::assign(&mut tx, &id, ChoiceList::Audience, &self.ages).await?;
        tx.commit().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use assert2::check;

    use super::*;

    #[test]
    fn only_an_event_for_a_specific_audience_is_asked_its_ages() {
        check!(Attendance::of("Library cardholders").asks_ages());
        check!(!Attendance::of("").asks_ages());
        check!(Attendance::of("  ") == Attendance::Open);
    }

    #[test]
    fn an_event_for_everyone_names_no_audience_and_keeps_its_saved_ages() {
        use crate::event::rules;

        let ages = |slugs: &[&str]| slugs.iter().map(|slug| slug.to_string()).collect();
        let stored = EventAttendance {
            limited_to: "Girl Scouts".to_string(),
            ages: ages(&["teens"]),
        };

        let open = rules::attendance(
            Attendance::Open,
            "Girl Scouts",
            ages(&["adults"]),
            Some(&stored),
        )
        .unwrap();
        check!(open.limited_to.is_empty());
        check!(open.ages == ages(&["teens"]));

        let limited = rules::attendance(
            Attendance::Limited,
            "  Library cardholders ",
            ages(&["adults"]),
            Some(&stored),
        )
        .unwrap();
        check!(limited.limited_to == "Library cardholders");
        check!(limited.ages == ages(&["adults"]));

        let refused = rules::attendance(Attendance::Limited, " ", ages(&[]), None).unwrap_err();
        check!(refused.field == rules::Field::LimitedTo);

        // A new event for everyone has no ages of its own to keep.
        check!(
            rules::attendance(Attendance::Open, "", ages(&["adults"]), None)
                .unwrap()
                .ages
                .is_empty()
        );
    }
}
