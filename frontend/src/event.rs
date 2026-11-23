use anyhow::Error;
use std::collections::{BTreeSet, HashSet};

use common::{
    insensitivestring::InsensitiveString, timestring::TimeString, transaction::Immediate,
};

use crate::choice::{ChoiceCheckboxes, ChoiceList, ChoiceValue, OrreryValue};
use crate::event::attendance::Attendance;

pub mod admission;
pub mod all;
pub mod attendance;
pub mod facebook;
pub mod field;
pub mod location;
pub mod part_of;
pub mod registration;
pub mod repeat;
pub mod rules;
pub mod zone;

/// The most topics an event may pick, as orrery allows.
pub const TOPIC_LIMIT: usize = 3;

/// The query an event's page is opened with straight after it is submitted
/// for review, which thanks its submitter, as orrery's `?new=true` does.
pub const NEW_SUBMISSION_QUERY: &str = "new=true";

/// What the event forms and page call an event's presenter (Darlene's
/// review): the organization hosting it.
pub const PRESENTER_LABEL: &str = "Host organization";

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

/// When the last of `instances` ends, after which the event is over; `None`
/// for an event with no dates.
pub fn last_end(instances: &[EventInstance]) -> Option<jiff::Timestamp> {
    instances
        .iter()
        .filter_map(|instance| instance.end.parse::<jiff::Timestamp>().ok())
        .max()
}

/// The dates of the event `event_id`, in date order.
pub async fn read_instances<'e, E>(
    executor: E,
    event_id: &str,
) -> Result<Box<[EventInstance]>, Error>
where
    E: sqlx::SqliteExecutor<'e>,
{
    sqlx::query!(
        r#"SELECT "begin", "end" FROM event_instances WHERE event_id = ? ORDER BY "begin""#,
        event_id
    )
    .fetch_all(executor)
    .await?
    .into_iter()
    .map(|row| {
        Ok(EventInstance {
            begin: TimeString::try_from(row.begin)
                .map_err(|e| anyhow::anyhow!("Invalid begin TimeString: {e}"))?,
            end: TimeString::try_from(row.end)
                .map_err(|e| anyhow::anyhow!("Invalid end TimeString: {e}"))?,
        })
    })
    .collect()
}

/// Replace the tags of the event `event_id` with `tags`, within `tx`.
pub async fn write_tags(
    tx: &mut Immediate,
    event_id: &str,
    tags: &HashSet<InsensitiveString>,
) -> Result<(), Error> {
    sqlx::query!(r#"DELETE FROM event_tags WHERE event_id = ?"#, event_id)
        .execute(&mut *tx)
        .await?;
    for tag in tags {
        let tag = tag.as_ref();
        sqlx::query!(
            r#"INSERT INTO event_tags (event_id, tag) VALUES (?, ?)"#,
            event_id,
            tag
        )
        .execute(&mut *tx)
        .await?;
    }
    Ok(())
}

/// The tags of the event `event_id`.
pub async fn read_tags<'e, E: sqlx::SqliteExecutor<'e>>(
    executor: E,
    event_id: &str,
) -> Result<HashSet<InsensitiveString>, Error> {
    Ok(
        sqlx::query_scalar!("SELECT tag FROM event_tags WHERE event_id = ?", event_id)
            .fetch_all(executor)
            .await?
            .into_iter()
            .map(InsensitiveString::from)
            .collect(),
    )
}

/// Replace the dates of the event `event_id` with `instances`, within `tx`.
pub async fn write_instances(
    tx: &mut Immediate,
    event_id: &str,
    instances: &[EventInstance],
) -> Result<(), Error> {
    sqlx::query!(
        r#"DELETE FROM event_instances WHERE event_id = ?"#,
        event_id
    )
    .execute(&mut *tx)
    .await?;

    for instance in instances {
        sqlx::query!(
            r#"INSERT INTO event_instances ("event_id", "begin", "end") VALUES (?, ?, ?)"#,
            event_id,
            instance.begin,
            instance.end,
        )
        .execute(&mut *tx)
        .await?;
    }

    Ok(())
}

#[derive(Clone, Copy, Default, Debug, sqlx::Type, serde::Serialize, serde::Deserialize)]
pub enum EventStatus {
    #[default]
    Active,
    Cancelled,
    Expired,
}

/// The question the event forms ask of `list`, as orrery's event form asks
/// it. The ages and the program are asked in words of their own; every
/// other list by the heading it has everywhere.
pub fn choice_question(list: ChoiceList) -> &'static str {
    match list {
        ChoiceList::Audience => "What ages is this event suitable for?",
        ChoiceList::EventProgram => "Is your event part of a SciStarter program or campaign?",
        _ => list.heading(),
    }
}

/// What the event forms say under the question of `list`, if anything: how
/// to choose, with how many topics may be.
pub fn choice_help(list: ChoiceList) -> Option<&'static str> {
    match list {
        ChoiceList::Topic => Some(TOPIC_HELP),
        ChoiceList::EventFormat => Some("Select the format that best describes your event."),
        ChoiceList::Audience | ChoiceList::EventProgram => Some("Select all that apply."),
        ChoiceList::EventVenue | ChoiceList::Activity => None,
    }
}

/// The guidance over an event's topics, which says how many it may have.
const TOPIC_HELP: &str = "Choose up to three topics that best describe your event.";

/// The slugs of the ages to save for an event being added that picks
/// `picked` from the audience list, with the answer `attendance` to who can
/// participate: those it picks, or, when an event open to everyone picks
/// none, the ages an event added with none is saved with
/// ([`OrreryValue::EVENT_AGES_WHEN_NONE`]). An event meant for a specific
/// audience ("Senior citizens") is asked its ages, and leaving them blank
/// there says nothing about them, where every age would contradict the
/// audience. Only adding an event goes through here: an edit saves exactly
/// the ages chosen, none included, as orrery's does.
pub async fn ages_for_new_event(
    attendance: Attendance,
    picked: BTreeSet<String>,
) -> Result<BTreeSet<String>, Error> {
    if !picked.is_empty() || attendance == Attendance::Limited {
        return Ok(picked);
    }

    let ages = ages_when_none(&ChoiceValue::all(ChoiceList::Audience).await?);
    if ages.len() < OrreryValue::EVENT_AGES_WHEN_NONE.len() {
        tracing::warn!(
            found = ?ages,
            "Not every age an event added with none is saved with is in the audience list"
        );
    }
    Ok(ages)
}

/// The slugs, among the audience list's `values`, of the ages an event
/// added with none is saved with.
fn ages_when_none(values: &[ChoiceValue]) -> BTreeSet<String> {
    values
        .iter()
        .filter(|value| {
            OrreryValue::EVENT_AGES_WHEN_NONE
                .iter()
                .any(|age| age.is(value))
        })
        .map(|value| value.slug.clone())
        .collect()
}

/// Whether the event forms ask `list` as one column of boxes rather than a
/// grid: the ages are a short list read top to bottom, youngest first.
pub fn choice_stacked(list: ChoiceList) -> bool {
    list == ChoiceList::Audience
}

/// How an event's page heads what it picked from `list`.
pub fn choice_row_heading(list: ChoiceList) -> &'static str {
    match list {
        ChoiceList::Audience => "Recommended ages",
        _ => list.heading(),
    }
}

/// Whether the event forms take one answer from `list`, or none: the kind of
/// venue. An event saved with several shows, and is saved with, the first
/// ([`first_offered`]).
pub fn one_answer(list: ChoiceList) -> bool {
    list == ChoiceList::EventVenue
}

/// What the event forms ask of `list`: its question, and, unless the help
/// under it says how to choose, how many may be picked from it ("Topics (up
/// to 3)"), or, for a list of one answer, that it's optional.
pub fn choice_legend(list: ChoiceList) -> std::borrow::Cow<'static, str> {
    let question = choice_question(list);
    match pick_limit(list) {
        _ if choice_help(list).is_some() => question.into(),
        _ if one_answer(list) => format!("{question} (optional)").into(),
        Some(limit) => crate::choice::limited_heading(question, Some(limit)),
        None => format!("{question} (choose any)").into(),
    }
}

/// Whether an event must pick from `list` ([`rules::topics`]): its topics.
pub fn choice_required(list: ChoiceList) -> bool {
    list == ChoiceList::Topic
}

/// Checkboxes for picking from `values` of `list` as the event forms'
/// `field`, with `picked` ticked, under the list's question, marked when the
/// event must pick from it, and how many may be picked.
pub fn choice_checkboxes(
    field: &'static str,
    list: ChoiceList,
    values: Vec<ChoiceValue>,
    picked: &[String],
) -> ChoiceCheckboxes {
    ChoiceCheckboxes::new(
        field,
        values,
        picked.iter().cloned().collect(),
        pick_limit(list),
    )
    .with_legend(choice_legend(list))
    .with_help(choice_help(list))
    .required(choice_required(list))
    .stacked(choice_stacked(list))
}

/// Whether `value`, of one of the event-type lists, is one an organizer
/// chooses: all but Online, which where the event is held answers. The event
/// forms offer only these, and the event page shows only these, beside the
/// "Virtual" it says of where an online event is held.
pub fn type_is_chosen(value: &ChoiceValue) -> bool {
    !OrreryValue::EVENT_ONLINE.is(value)
}

/// The values of `list` the event forms offer, in the order they offer them,
/// which is the order the event page lists an event's picks in: all but
/// Online ([`type_is_chosen`]), the formats, programs and kinds of venue
/// alphabetically with the catch-all Other last ([`in_type_order`]), and any
/// other list in its own order, as orrery's `event_type_choices` orders them.
pub async fn offered(list: ChoiceList) -> Result<Vec<ChoiceValue>, Error> {
    let values = ChoiceValue::all(list)
        .await?
        .into_iter()
        .filter(type_is_chosen)
        .collect();
    Ok(if ChoiceList::EVENT_TYPES.contains(&list) {
        in_type_order(values)
    } else {
        values
    })
}

/// The first of `picked`, slugs of `list`, in the order the event forms offer
/// them ([`offered`]): what a form asking for one answer shows of an event
/// saved with several, and saves.
pub async fn first_offered(
    list: ChoiceList,
    picked: &std::collections::BTreeSet<String>,
) -> Result<Option<String>, Error> {
    Ok(offered(list)
        .await?
        .into_iter()
        .map(|value| value.slug)
        .find(|slug| picked.contains(slug)))
}

/// Event types (formats, programs, kinds of venue) as the event forms offer
/// them: alphabetically, with the catch-all Other last.
pub fn in_type_order(mut types: Vec<ChoiceValue>) -> Vec<ChoiceValue> {
    types.sort_by_cached_key(|value| {
        (
            OrreryValue::EVENT_OTHER.is(value),
            value.name.to_lowercase(),
        )
    });
    types
}

#[cfg(test)]
mod tests {
    use assert2::check;

    use super::*;

    #[test]
    fn online_is_no_type_an_organizer_chooses() {
        let format = |orrery_id| ChoiceValue {
            list: ChoiceList::EventFormat,
            slug: "x".to_string(),
            name: "X".to_string(),
            position: 0,
            orrery_id,
        };
        check!(!type_is_chosen(&format(Some(OrreryValue::EVENT_ONLINE.id))));
        check!(type_is_chosen(&format(Some(OrreryValue::EVENT_OTHER.id))));
        check!(type_is_chosen(&format(None)));
    }

    #[test]
    fn the_topic_guidance_says_how_many_topics_may_be_picked() {
        // The guidance spells the limit out in words, so the two change
        // together.
        check!(TOPIC_LIMIT == 3);
        check!(TOPIC_HELP.contains("up to three topics"));
    }

    fn age(slug: &str, orrery_id: Option<i64>) -> ChoiceValue {
        ChoiceValue {
            list: ChoiceList::Audience,
            slug: slug.to_string(),
            name: slug.to_string(),
            position: 0,
            orrery_id,
        }
    }

    #[test]
    fn an_event_added_with_no_ages_is_saved_with_the_school_levels_and_adults() {
        let values = [
            age("elementary", Some(111)),
            age("middle", Some(112)),
            age("high", Some(113)),
            age("families", Some(117)),
            age("adults", Some(116)),
            age("local", None),
        ];
        check!(
            ages_when_none(&values)
                == BTreeSet::from(["adults", "elementary", "high", "middle"].map(String::from))
        );
    }

    #[tokio::test]
    async fn the_ages_an_event_is_added_with_are_saved_as_picked() {
        let picked = BTreeSet::from(["families".to_string()]);
        for attendance in Attendance::ALL {
            check!(
                ages_for_new_event(attendance, picked.clone())
                    .await
                    .unwrap()
                    == picked
            );
        }
    }

    #[tokio::test]
    async fn an_event_for_a_specific_audience_added_without_ages_gets_none() {
        let ages = ages_for_new_event(Attendance::Limited, BTreeSet::new()).await;
        check!(ages.unwrap().is_empty());
    }
}
