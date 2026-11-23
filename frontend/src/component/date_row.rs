//! One date/time row: a start date and time, the time zone whose clocks
//! they're on as the right part of the start time's box, an optional end,
//! offered as a small link under the start, and, in the add event form, how
//! the row repeats, offered likewise. The add event form repeats it for each
//! of a new event's dates, and the event page's date editor shows one to
//! change a date or add another; `js/date-rows.js` drives both.

use jiff::{Timestamp, tz::TimeZone};
use sailfish::TemplateOnce;

use crate::{
    component::floating_field::FormErrors,
    event::{EventInstance, repeat::Repetition},
};

/// What a row holds, as submitted or as shown.
#[derive(Debug, Clone, Default)]
pub struct DateRowValues {
    pub begin_date: String,
    pub begin_time: String,
    pub timezone: String,
    pub has_end: bool,
    pub end_date: String,
    pub end_time: String,
}

impl DateRowValues {
    /// A saved date, shown on `tz`'s clocks. One that ends as it begins has
    /// no end.
    pub fn of_instance(instance: &EventInstance, tz: &TimeZone) -> Self {
        let wall = |time: &str| {
            time.parse::<Timestamp>().ok().map(|ts| {
                let zoned = ts.to_zoned(tz.clone());
                (
                    zoned.strftime("%Y-%m-%d").to_string(),
                    zoned.strftime("%H:%M").to_string(),
                )
            })
        };
        let (begin_date, begin_time) = wall(&instance.begin).unwrap_or_default();
        let has_end = instance.end != instance.begin;
        let (end_date, end_time) = if has_end {
            wall(&instance.end).unwrap_or_default()
        } else {
            Default::default()
        };
        DateRowValues {
            begin_date,
            begin_time,
            timezone: tz.iana_name().unwrap_or_default().to_string(),
            has_end,
            end_date,
            end_time,
        }
    }
}

#[derive(TemplateOnce)]
#[template(path = "site/date_row.stpl.html")]
pub struct DateRowFields {
    /// What the row's controls are named within: `instances[0]` names them
    /// `instances[0][begin_date]` and so on; empty names them `begin_date`.
    prefix: String,
    values: DateRowValues,
    errors: FormErrors,
    /// The saved date the row shows, which the script shows again on the
    /// clocks of whichever zone is chosen while the row is left as it was.
    stored: Option<EventInstance>,
    /// Whether the row is shown on UTC's clocks only because the server
    /// doesn't know the viewer's: the script moves it to theirs.
    follow_viewer: bool,
    /// How the row repeats, when it offers "Make this a recurring event"
    /// among the links under its start.
    repeat: Option<Repetition>,
}

impl DateRowFields {
    pub fn new(prefix: impl Into<String>, values: DateRowValues) -> Self {
        DateRowFields {
            prefix: prefix.into(),
            values,
            errors: FormErrors::new(),
            stored: None,
            follow_viewer: false,
            repeat: None,
        }
    }

    /// Offer "Make this a recurring event" under the row's start, the row
    /// repeating as `repetition` says.
    pub fn with_repeat(mut self, repetition: Repetition) -> Self {
        self.repeat = Some(repetition);
        self
    }

    /// Mark the controls `errors` are about, those that are this row's.
    pub fn errors(mut self, errors: &FormErrors) -> Self {
        self.errors = errors.clone();
        self
    }

    /// Show the saved date `instance` on `tz`'s clocks, or, with no zone to
    /// show it on, on the viewer's, which only the browser knows.
    pub fn stored(instance: &EventInstance, tz: Option<&TimeZone>) -> Self {
        let follow_viewer = tz.is_none();
        let tz = tz.cloned().unwrap_or(TimeZone::UTC);
        let mut row = DateRowFields::new("", DateRowValues::of_instance(instance, &tz));
        row.stored = Some(instance.clone());
        row.follow_viewer = follow_viewer;
        row
    }

    /// The name of the row's control for `part`.
    fn name(&self, part: &str) -> String {
        if self.prefix.is_empty() {
            part.to_string()
        } else {
            format!("{}[{part}]", self.prefix)
        }
    }

    /// The id of the row's element `part`, unique among the form's rows.
    fn id(&self, part: &str) -> String {
        if self.prefix.is_empty() {
            part.to_string()
        } else {
            format!("{}-{part}", self.prefix)
        }
    }
}
