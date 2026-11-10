use common::timestring::TimeString;

use crate::choice::ChoiceList;

pub mod all;
pub mod facebook;
pub mod field;

/// The most topics an event may pick, as orrery allows.
pub const TOPIC_LIMIT: usize = 3;

/// The most values an event may pick from `list`, or `None` for a list it
/// may pick any number from. Only its topics are limited.
pub fn pick_limit(list: ChoiceList) -> Option<usize> {
    match list {
        ChoiceList::Topic => Some(TOPIC_LIMIT),
        ChoiceList::EventFormat
        | ChoiceList::EventProgram
        | ChoiceList::EventVenue
        | ChoiceList::Activity
        | ChoiceList::Audience => None,
    }
}

/// Why an event may not pick the `picked` distinct values of `list`, or
/// `None` when it may.
pub fn refuse_picks(list: ChoiceList, picked: usize) -> Option<String> {
    let limit = pick_limit(list)?;
    (picked > limit).then(|| {
        format!(
            "Please pick no more than {limit} {}.",
            list.label().to_lowercase()
        )
    })
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EventInstance {
    pub begin: TimeString,
    pub end: TimeString,
}

#[derive(Clone, Copy, Default, Debug, sqlx::Type, serde::Serialize, serde::Deserialize)]
pub enum EventStatus {
    #[default]
    Active,
    Cancelled,
    Expired,
}
