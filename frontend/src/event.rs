use anyhow::Error;
use std::collections::{BTreeSet, HashSet};

use common::{
    insensitivestring::InsensitiveString, timestring::TimeString, transaction::Immediate,
};

use crate::choice::{ChoiceList, ChoiceValue, OrreryValue};

pub mod all;
pub mod facebook;
pub mod field;
pub mod location;
pub mod rules;
pub mod zone;

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

/// The question the event forms ask of `list`. The audience is asked in
/// words of its own; every other list by the heading it has everywhere.
pub fn choice_question(list: ChoiceList) -> &'static str {
    match list {
        ChoiceList::Audience => "What ages is this event suitable for?",
        _ => list.heading(),
    }
}

/// What the event forms say under the question of `list`, if anything.
pub fn choice_help(list: ChoiceList) -> Option<&'static str> {
    match list {
        ChoiceList::Audience => Some("Select all that apply."),
        _ => None,
    }
}

/// The slugs of the ages to save for an event being added that picks
/// `picked` from the audience list: those it picks, or, when it picks none,
/// the ages an event added with none is saved with
/// ([`OrreryValue::EVENT_AGES_WHEN_NONE`]). Only adding an event goes
/// through here: an edit saves exactly the ages chosen, none included, as
/// orrery's does.
pub async fn ages_for_new_event(picked: BTreeSet<String>) -> Result<BTreeSet<String>, Error> {
    if !picked.is_empty() {
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

/// What the event forms ask of `list`: its question, and how many may be
/// picked from it ("Topics (up to 3)", "Type of event (choose any)"). A list
/// with help under its question says there that any may be picked.
pub fn choice_legend(list: ChoiceList) -> std::borrow::Cow<'static, str> {
    let question = choice_question(list);
    match pick_limit(list) {
        Some(limit) => crate::choice::limited_heading(question, Some(limit)),
        None if choice_help(list).is_some() => question.into(),
        None => format!("{question} (choose any)").into(),
    }
}

/// The values of `list`, one of the event-type lists, that the event forms
/// offer: all but Online, which their location question answers.
pub async fn offered_types(list: ChoiceList) -> Result<Vec<ChoiceValue>, Error> {
    Ok(ChoiceValue::all(list)
        .await?
        .into_iter()
        .filter(|value| !OrreryValue::EVENT_ONLINE.is(value))
        .collect())
}

/// Event formats as the event forms offer them: alphabetically, with
/// the catch-all Other last.
pub fn in_format_order(mut formats: Vec<ChoiceValue>) -> Vec<ChoiceValue> {
    formats.sort_by_cached_key(|value| {
        (
            OrreryValue::EVENT_OTHER.is(value),
            value.name.to_lowercase(),
        )
    });
    formats
}

#[cfg(test)]
mod tests {
    use assert2::check;

    use super::*;

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
        check!(ages_for_new_event(picked.clone()).await.unwrap() == picked);
    }
}
