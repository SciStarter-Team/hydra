use anyhow::Error;
use std::collections::HashSet;

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

/// What the event forms ask of `list`: its heading, and how many may be
/// picked from it ("Topics (up to 3)", "Type of event (choose any)").
pub fn choice_legend(list: ChoiceList) -> std::borrow::Cow<'static, str> {
    match pick_limit(list) {
        Some(limit) => crate::choice::limited_heading(list.heading(), Some(limit)),
        None => format!("{} (choose any)", list.heading()).into(),
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
