//! Repeating one of a new event's dates: each date/time row of the add event
//! form repeats on its own, at its own frequency, through its own "repeat
//! until" date, as orrery's event form repeats them. Repetition is expanded
//! into the dates it comes to when the event is added, and isn't kept.

use jiff::{Span, ToSpan, civil};
use serde::Deserialize;

use super::{
    EventInstance,
    rules::{Checked, Field, Refusal, RowPart, ZonedInstance, stored_instance},
};

/// How often a date/time repeats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepeatFrequency {
    /// It happens once.
    #[default]
    Once,
    Daily,
    Weekly,
    Biweekly,
    Monthly,
}

impl RepeatFrequency {
    /// Every frequency, in the order the form offers them.
    pub const ALL: [RepeatFrequency; 5] = [
        RepeatFrequency::Once,
        RepeatFrequency::Daily,
        RepeatFrequency::Weekly,
        RepeatFrequency::Biweekly,
        RepeatFrequency::Monthly,
    ];

    /// The value the form submits for it.
    pub fn as_str(self) -> &'static str {
        match self {
            RepeatFrequency::Once => "once",
            RepeatFrequency::Daily => "daily",
            RepeatFrequency::Weekly => "weekly",
            RepeatFrequency::Biweekly => "biweekly",
            RepeatFrequency::Monthly => "monthly",
        }
    }

    /// How the form names it.
    pub fn label(self) -> &'static str {
        match self {
            RepeatFrequency::Once => "Never",
            RepeatFrequency::Daily => "Daily",
            RepeatFrequency::Weekly => "Weekly",
            RepeatFrequency::Biweekly => "Every 2 weeks",
            RepeatFrequency::Monthly => "Monthly",
        }
    }

    /// The step from one occurrence to the next, or `None` for a date that
    /// happens once.
    fn step(self) -> Option<Span> {
        match self {
            RepeatFrequency::Once => None,
            RepeatFrequency::Daily => Some(1.day()),
            RepeatFrequency::Weekly => Some(1.week()),
            RepeatFrequency::Biweekly => Some(2.weeks()),
            RepeatFrequency::Monthly => Some(1.month()),
        }
    }
}

/// A date is never repeated into more than this many occurrences, as a guard
/// against runaway input.
const MAX_OCCURRENCES: usize = 366;

/// How one date/time row repeats, as submitted or as shown.
#[derive(Debug, Clone, Default)]
pub struct Repetition {
    pub frequency: RepeatFrequency,
    /// The last date an occurrence may start on, on the row's clocks;
    /// ignored while the row doesn't repeat.
    pub until: String,
}

impl Repetition {
    pub fn repeats(&self) -> bool {
        self.frequency != RepeatFrequency::Once
    }

    /// The dates `base` comes to: itself alone, or repeated at the frequency
    /// through the "repeat until" date, on its own clocks, so a weekly 10:00
    /// stays 10:00 across a daylight-saving change.
    pub fn occurrences(&self, base: &ZonedInstance) -> Checked<Vec<EventInstance>> {
        let Some(step) = self.frequency.step() else {
            return Ok(vec![base.stored()]);
        };
        let until = self.until()?;

        let mut out = Vec::new();
        let mut begin = base.begin.clone();
        let mut end = base.end.clone();
        while begin.date() <= until {
            out.push(stored_instance(&begin, end.as_ref()));
            if out.len() >= MAX_OCCURRENCES {
                tracing::warn!(
                    "Add Event recurrence hit the {MAX_OCCURRENCES}-occurrence cap; truncating."
                );
                break;
            }
            // Only a date thousands of years on is past stepping to.
            let past = || {
                Refusal::new(
                    UNTIL,
                    "Please choose an earlier \u{201c}repeat until\u{201d} date.",
                )
            };
            begin = begin.checked_add(step).map_err(|_| past())?;
            end = end
                .map(|end| end.checked_add(step))
                .transpose()
                .map_err(|_| past())?;
        }
        Ok(out)
    }

    /// The "repeat until" date a repeating row must be given.
    fn until(&self) -> Checked<civil::Date> {
        let until = self.until.trim();
        if until.is_empty() {
            return Err(Refusal::new(
                UNTIL,
                "Please choose the date this date/time repeats until.",
            ));
        }
        civil::Date::strptime("%Y-%m-%d", until)
            .map_err(|_| Refusal::new(UNTIL, "Please enter a valid date."))
    }
}

/// The control a refused repetition marks.
const UNTIL: Field = Field::Date(RowPart::RepeatUntil);
