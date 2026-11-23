//! The larger event an event is part of ("Is this part of a larger
//! event?"): a festival, conference or other event listed on SciStarter,
//! picked by searching the published events. The event form no longer asks
//! it (Darlene's review); staff change it on the event page, and the page
//! names and links it only while it is published.

use anyhow::Error;
use common::shared::db;
use ulid::Ulid;

use crate::event::rules::{Checked, Field, Refusal};

/// The most events a search offers.
const SEARCH_LIMIT: i64 = 10;

/// How far up a chain of larger events [`settle`] looks for the event
/// itself, beyond which a chain is taken to hold no loop.
const CHAIN_LIMIT: usize = 32;

/// A published event another may be part of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LargerEvent {
    pub event_id: Ulid,
    pub name: String,
    /// The path of its page.
    pub path: String,
}

impl LargerEvent {
    /// The event `event_id`, while it is published.
    pub async fn published(event_id: Ulid) -> Result<Option<Self>, Error> {
        let id = event_id.to_string();
        Ok(sqlx::query!(
            r#"SELECT e.name AS "name!", ent.slug AS "slug!"
               FROM events e
               JOIN entities ent ON ent.target = e.event_id AND ent.kind = 'Event'
               WHERE ent.publish = 1 AND e.event_id = ?"#,
            id
        )
        .fetch_optional(db())
        .await?
        .map(|row| LargerEvent {
            event_id,
            name: row.name,
            path: format!("/events/{}", row.slug),
        }))
    }

    /// The published events whose names hold `query`, by name, apart from
    /// `excluding`, the event being changed, which can't be part of itself.
    pub async fn search(query: &str, excluding: Option<Ulid>) -> Result<Vec<Self>, Error> {
        let pattern = format!("%{}%", query.trim());
        let excluding = excluding.map(|id| id.to_string());
        sqlx::query!(
            r#"SELECT e.event_id AS "event_id!", e.name AS "name!", ent.slug AS "slug!"
               FROM events e
               JOIN entities ent ON ent.target = e.event_id AND ent.kind = 'Event'
               WHERE ent.publish = 1 AND e.name LIKE ?1
                 AND (?2 IS NULL OR e.event_id != ?2)
               ORDER BY e.name
               LIMIT ?3"#,
            pattern,
            excluding,
            SEARCH_LIMIT
        )
        .fetch_all(db())
        .await?
        .into_iter()
        .map(|row| {
            Ok(LargerEvent {
                event_id: row.event_id.parse()?,
                name: row.name,
                path: format!("/events/{}", row.slug),
            })
        })
        .collect()
    }
}

/// The larger event the event `event_id` is part of, as it is saved: none
/// for an event that isn't.
pub async fn read(event_id: Ulid) -> Result<Option<Ulid>, Error> {
    let id = event_id.to_string();
    let part_of = sqlx::query_scalar!("SELECT part_of FROM events WHERE event_id = ?", id)
        .fetch_optional(db())
        .await?
        .flatten();
    Ok(part_of.map(|id| id.parse()).transpose()?)
}

/// The larger event `submitted` names, as it is saved, or why it can't be:
/// none, or a published event the event `event_id` isn't itself, nor part
/// of the chain of. The larger event it was saved with (`stored`) may be
/// kept, published or not.
pub async fn settle(
    event_id: Ulid,
    submitted: &str,
    stored: Option<Ulid>,
) -> Result<Checked<Option<Ulid>>, Error> {
    let submitted = submitted.trim();
    if submitted.is_empty() {
        return Ok(Ok(None));
    }
    let Ok(part_of) = submitted.parse::<Ulid>() else {
        return Ok(Err(not_listed()));
    };
    if stored == Some(part_of) {
        return Ok(Ok(Some(part_of)));
    }
    if LargerEvent::published(part_of).await?.is_none() {
        return Ok(Err(not_listed()));
    }
    if leads_back(part_of, event_id).await? {
        return Ok(Err(Refusal::new(
            Field::PartOf,
            "An event can\u{2019}t be part of itself, or of an event that is part of it.",
        )));
    }
    Ok(Ok(Some(part_of)))
}

fn not_listed() -> Refusal {
    Refusal::new(
        Field::PartOf,
        "Please search for the larger event and pick it from the list.",
    )
}

/// Whether the chain of larger events from `from` comes to `event_id`.
async fn leads_back(from: Ulid, event_id: Ulid) -> Result<bool, Error> {
    let mut at = Some(from);
    for _ in 0..CHAIN_LIMIT {
        match at {
            Some(id) if id == event_id => return Ok(true),
            Some(id) => at = read(id).await?,
            None => return Ok(false),
        }
    }
    Ok(false)
}
