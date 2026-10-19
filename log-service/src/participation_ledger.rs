//! What the participation importer has logged for each of orrery's events.
//!
//! orrery delivers an event again after every write to it (see
//! [`crate::participation_feed`]), and each delivery is the event as it now
//! stands, not the change: an event that has gained three repetitions arrives
//! with its new total. So the importer keeps, per event, the row it logged
//! for each repetition and how many copies of it are in the log, and turns
//! every delivery into the difference between the two.
//!
//! What the log should hold for an event is one row per repetition, unless
//! the event is gone or a bridge here is the importer of record for it, in
//! which case it should hold nothing. Getting there from what it holds is
//! [`reconcile`]: more repetitions of the same row are logged as more copies
//! of it, fewer are taken back, and anything else -- a rewritten time, a
//! moved project, a deleted account -- takes back every copy and logs the
//! event as it now stands. Taking back is asynchronous; see
//! [`crate::database::retraction`].
//!
//! Re-reading a delivery changes nothing, since the log already holds what it
//! asks for, so a feed that delivers a write twice is harmless.
//!
//! Whether a bridge covers an event can change without orrery writing to it
//! -- an account links a platform, a project joins a bridge -- so an event
//! the log holds rows for is also taken back when coverage that has since
//! changed now covers it: [`cover`].
//!
//! Nobody participates in the future, so an event orrery dates after the
//! moment it is read is logged as of that moment, as everything reported to
//! hydra is (see [`common::contribution_time`]). Its later deliveries keep
//! the time it was logged at, until orrery corrects the event's own.

use common::transaction::Immediate;
use futures::{Stream, TryStreamExt};
use jiff::Timestamp;
use std::str::FromStr;

use crate::database::retraction;
use crate::database::tables::ParticipationRow;
use crate::participation_feed::ParticipationEventData;

/// What the log holds for one event.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Entry {
    /// The row logged for each repetition, exactly as it was written.
    pub row: Option<ParticipationRow>,
    /// How many copies of `row` the log holds.
    pub logged: u32,
    /// Whether a bridge here has been the importer of record for the event.
    pub covered: bool,
}

/// What the log has to gain and lose for one delivery, and what it then
/// holds.
#[derive(Debug, Clone, PartialEq)]
pub struct Reconciliation {
    pub next: Entry,
    /// Copies of a row to log.
    pub append: Option<(ParticipationRow, u32)>,
    /// Copies of a row to take back, and why.
    pub retract: Option<(ParticipationRow, u32, Reason)>,
}

impl Reconciliation {
    /// Whether the delivery changes anything at all.
    pub fn is_change(&self, entry: &Entry) -> bool {
        self.append.is_some() || self.retract.is_some() || self.next != *entry
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// orrery deleted the event, or the project it was for.
    Deleted,
    /// A bridge here is the importer of record for the event.
    Covered,
    /// orrery rewrote the event as something the logged row no longer is.
    Rewritten,
    /// orrery holds fewer repetitions than were logged.
    FewerRepetitions,
}

impl Reason {
    pub fn describe(self) -> &'static str {
        match self {
            Reason::Deleted => "deleted in orrery",
            Reason::Covered => "covered by a bridge",
            Reason::Rewritten => "rewritten in orrery",
            Reason::FewerRepetitions => "fewer repetitions in orrery",
        }
    }
}

/// The row logged for each repetition of `event`.
pub fn row_of(event: &ParticipationEventData) -> ParticipationRow {
    ParticipationRow {
        origin: event.origin,
        when: event.when.clone(),
        duration: event.duration.unwrap_or(1.0),
        where_point: event.where_point.clone(),
        magnitude: event.magnitude.unwrap_or(1),
        authenticated: event.authenticated.unwrap_or(false),
        extra: event.extra.clone().unwrap_or_default(),
        fidelity: event.fidelity.unwrap_or(0.5),
        entity_id: event.entity_ulid.clone(),
        user_id: event.profile_user_ulid.clone(),
        task_name: event.task_name.clone(),
    }
}

/// Everything about a logged row except its origin, compared as what it
/// means rather than how it is spelled.
///
/// `when` is compared as the instant it names: the log holds orrery's times
/// spelled however the feed spelled them when they were logged, and the same
/// instant can come back spelled differently. One that does not parse is
/// compared as written.
///
/// The origin is left out because it is the one column orrery has renumbered
/// wholesale, which is a reason to rewrite the rows rather than a sign that
/// they belong to some other event; see `participation-repair`. Whether two
/// rows are the same row is [`same_row`], which compares it too.
#[derive(Debug, PartialEq, Eq, Hash)]
pub struct Shape<'a> {
    when: When<'a>,
    duration: u64,
    where_point: Option<&'a str>,
    magnitude: i32,
    authenticated: bool,
    extra: &'a str,
    fidelity: u64,
    entity_id: &'a str,
    user_id: Option<&'a str>,
    task_name: Option<&'a str>,
}

#[derive(Debug, PartialEq, Eq, Hash)]
enum When<'a> {
    Instant(Timestamp),
    Unreadable(&'a str),
}

impl<'a> Shape<'a> {
    pub fn of(row: &'a ParticipationRow) -> Shape<'a> {
        Shape {
            when: Timestamp::from_str(&row.when)
                .map(When::Instant)
                .unwrap_or(When::Unreadable(&row.when)),
            duration: row.duration.to_bits(),
            where_point: row.where_point.as_deref(),
            magnitude: row.magnitude,
            authenticated: row.authenticated,
            extra: &row.extra,
            fidelity: row.fidelity.to_bits(),
            entity_id: &row.entity_id,
            user_id: row.user_id.as_deref(),
            task_name: row.task_name.as_deref(),
        }
    }
}

/// Whether two logged rows record the same thing.
pub fn same_row(a: &ParticipationRow, b: &ParticipationRow) -> bool {
    a.origin == b.origin && Shape::of(a) == Shape::of(b)
}

/// What the log has to gain and lose for one delivery of orrery's event
/// `source_id`, read at `now`.
///
/// `event` is the event as orrery now holds it, `None` once orrery has
/// deleted it; `covered_now` is whether a bridge here is the importer of
/// record for it as things stand.
pub fn reconcile(
    entry: &Entry,
    source_id: i32,
    event: Option<&ParticipationEventData>,
    covered_now: bool,
    now: Timestamp,
) -> Reconciliation {
    let covered = entry.covered || (event.is_some() && covered_now);

    let target = match event {
        Some(event) if !covered => Some((
            row_at_latest(entry, source_id, event, now),
            event.repetitions.max(0) as u32,
        )),
        _ => None,
    };

    match (&entry.row, target) {
        (Some(logged_row), Some((row, repetitions))) if same_row(logged_row, &row) => {
            // Later copies repeat the logged row rather than the delivered
            // one, so every copy of the event stays identical and a
            // retraction finds all of them.
            let next = Entry {
                row: Some(logged_row.clone()),
                logged: repetitions,
                covered,
            };

            let (append, retract) = if repetitions >= entry.logged {
                let more = repetitions - entry.logged;
                ((more > 0).then(|| (logged_row.clone(), more)), None)
            } else {
                let fewer = entry.logged - repetitions;
                (
                    None,
                    Some((logged_row.clone(), fewer, Reason::FewerRepetitions)),
                )
            };

            Reconciliation {
                next,
                append,
                retract,
            }
        }
        (_, target) => {
            let reason = match (event, &target) {
                (None, _) => Reason::Deleted,
                (Some(_), None) => Reason::Covered,
                (Some(_), Some(_)) => Reason::Rewritten,
            };

            replace(entry, target, reason, covered)
        }
    }
}

/// What the log has to lose for an event that a bridge here has come to
/// cover since it was last delivered: everything logged for it.
pub fn cover(entry: &Entry) -> Reconciliation {
    replace(entry, None, Reason::Covered, true)
}

/// Take back every copy logged for `entry`, for `reason`, and log `target`'s
/// row once per repetition in their place.
fn replace(
    entry: &Entry,
    target: Option<(ParticipationRow, u32)>,
    reason: Reason,
    covered: bool,
) -> Reconciliation {
    let retract = entry
        .row
        .as_ref()
        .filter(|_| entry.logged > 0)
        .map(|row| (row.clone(), entry.logged, reason));

    let append = target
        .as_ref()
        .filter(|(_, repetitions)| *repetitions > 0)
        .map(|(row, repetitions)| (row.clone(), *repetitions));

    let (row, logged) = match target {
        Some((row, repetitions)) => (Some(row), repetitions),
        None => (None, 0),
    };

    Reconciliation {
        next: Entry {
            row,
            logged,
            covered,
        },
        append,
        retract,
    }
}

/// The row to log for `event`, dated no later than `now`.
///
/// An event dated in the future that was logged already, as of when it was
/// first read, keeps that time: it is the earliest anything was known of it,
/// and taking it back to log it again as of a later moment would change
/// nothing but churn the log.
fn row_at_latest(
    entry: &Entry,
    source_id: i32,
    event: &ParticipationEventData,
    now: Timestamp,
) -> ParticipationRow {
    let mut row = row_of(event);

    let Ok(reported) = Timestamp::from_str(&row.when) else {
        return row;
    };

    if reported <= now {
        return row;
    }

    let logged_when = entry
        .row
        .as_ref()
        .filter(|logged| {
            let mut dated = row.clone();
            dated.when = logged.when.clone();
            same_row(logged, &dated)
        })
        .filter(|logged| Timestamp::from_str(&logged.when).is_ok_and(|at| at <= now))
        .map(|logged| logged.when.clone());

    row.when = logged_when.unwrap_or_else(|| {
        common::contribution_time::not_after_arrival(
            reported,
            now,
            &format!("orrery event {source_id}"),
        )
        .to_string()
    });

    row
}

/// What the log holds for orrery's event `source_id`; an event the ledger
/// has no record of holds nothing.
pub async fn load(source_id: i32) -> anyhow::Result<Entry> {
    let stored = sqlx::query!(
        r#"SELECT "row", "logged", "covered" FROM participation_sources WHERE "source_id" = ?"#,
        source_id
    )
    .fetch_optional(common::shared::db())
    .await?;

    let Some(stored) = stored else {
        return Ok(Entry::default());
    };

    Ok(Entry {
        row: stored
            .row
            .as_deref()
            .map(serde_json::from_str)
            .transpose()?,
        logged: u32::try_from(stored.logged)?,
        covered: stored.covered != 0,
    })
}

/// Every event the log holds rows for that no bridge has covered, with the
/// row it holds, for checking against coverage as it now stands.
pub fn uncovered() -> impl Stream<Item = anyhow::Result<(i32, ParticipationRow)>> {
    sqlx::query!(
        r#"SELECT "source_id" AS "source_id!: i32", "row" AS "row!"
           FROM participation_sources
           WHERE "logged" > 0 AND NOT "covered" AND "row" IS NOT NULL"#
    )
    .fetch(common::shared::db())
    .map_err(anyhow::Error::from)
    .and_then(|stored| async move { Ok((stored.source_id, serde_json::from_str(&stored.row)?)) })
}

/// What brought about a change to an event's entry.
#[derive(Debug, Clone, Copy)]
pub enum Cause {
    /// A delivery of the event, at this revision.
    Delivery(i64),
    /// A change to what the bridges here cover.
    Coverage,
}

/// Begin recording that the log holds `reconciliation.next` for
/// `source_id`, and queueing what it takes back.
///
/// The recording holds the ledger's write lock until it is committed, which
/// is to be done once the appended rows are in the log, never before: a
/// ledger that records rows the log never received holds the event short for
/// good. Dropped uncommitted, it records nothing, so rows that failed to be
/// written are logged again from the start on the next pass. Only a failure
/// of the commit itself, after the rows are written, logs them twice.
pub async fn record(
    source_id: i32,
    reconciliation: &Reconciliation,
    cause: Cause,
) -> anyhow::Result<Recording> {
    let next = &reconciliation.next;
    let row = next.row.as_ref().map(serde_json::to_string).transpose()?;
    let logged = i64::from(next.logged);
    let covered = i64::from(next.covered);

    let mut tx = Immediate::begin_with_retry(common::shared::db(), None, None).await?;

    sqlx::query!(
        r#"INSERT INTO participation_sources ("source_id", "row", "logged", "covered")
           VALUES (?, ?, ?, ?)
           ON CONFLICT ("source_id") DO UPDATE SET
               "row" = excluded."row",
               "logged" = excluded."logged",
               "covered" = excluded."covered""#,
        source_id,
        row,
        logged,
        covered,
    )
    .execute(&mut tx)
    .await?;

    if let Some((row, count, reason)) = &reconciliation.retract {
        let reason = match cause {
            Cause::Delivery(revision) => format!(
                "orrery event {source_id} at revision {revision}: {}",
                reason.describe()
            ),
            Cause::Coverage => format!(
                "orrery event {source_id}, as coverage changed: {}",
                reason.describe()
            ),
        };

        retraction::queue(&mut tx, row, *count, &reason).await?;
    }

    Ok(Recording { tx })
}

/// A change to the ledger, waiting for the log to catch up with it; see
/// [`record`].
#[must_use = "a recording is rolled back unless it is committed"]
pub struct Recording {
    tx: Immediate,
}

impl Recording {
    pub async fn commit(self) -> anyhow::Result<()> {
        self.tx.commit().await?;
        Ok(())
    }
}

/// Seed the ledger with what the log already holds for orrery's events, for
/// the repair that brings the ledger in.
pub async fn seed(entries: &[(i32, Entry)]) -> anyhow::Result<()> {
    let mut tx = Immediate::begin_with_retry(common::shared::db(), None, None).await?;

    for (source_id, entry) in entries {
        let row = entry.row.as_ref().map(serde_json::to_string).transpose()?;
        let logged = i64::from(entry.logged);
        let covered = i64::from(entry.covered);

        sqlx::query!(
            r#"INSERT INTO participation_sources ("source_id", "row", "logged", "covered")
               VALUES (?, ?, ?, ?)"#,
            source_id,
            row,
            logged,
            covered,
        )
        .execute(&mut tx)
        .await?;
    }

    tx.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use assert2::check;

    use super::*;

    fn event(when: &str, repetitions: i32) -> ParticipationEventData {
        ParticipationEventData {
            origin: 3,
            when: when.to_string(),
            duration: Some(1.0),
            where_point: None,
            magnitude: Some(1),
            authenticated: Some(true),
            extra: Some(String::new()),
            repetitions,
            fidelity: Some(0.5),
            entity_ulid: "01JKXMGGMD577ZA8Q57S2GY1V8".to_string(),
            profile_user_ulid: Some("01JKXMFYFZ0000000000LINKED".to_string()),
            task_name: Some("Data collection".to_string()),
        }
    }

    const WHEN: &str = "2026-03-01T12:00:00.250Z";

    fn now() -> Timestamp {
        "2026-10-19T12:00:00Z".parse().unwrap()
    }

    fn logged(when: &str, copies: u32) -> Entry {
        Entry {
            row: Some(row_of(&event(when, 1))),
            logged: copies,
            covered: false,
        }
    }

    #[test]
    fn a_new_event_is_logged_once_per_repetition() {
        let r = reconcile(&Entry::default(), 1, Some(&event(WHEN, 3)), false, now());

        check!(r.append == Some((row_of(&event(WHEN, 3)), 3)));
        check!(r.retract == None);
        check!(r.next == logged(WHEN, 3));
    }

    #[test]
    fn a_redelivery_changes_nothing() {
        let entry = logged(WHEN, 3);
        let r = reconcile(&entry, 1, Some(&event(WHEN, 3)), false, now());

        check!(!r.is_change(&entry));
    }

    #[test]
    fn a_fold_logs_only_the_repetitions_it_added() {
        let r = reconcile(&logged(WHEN, 34), 1, Some(&event(WHEN, 310)), false, now());

        check!(r.append == Some((row_of(&event(WHEN, 1)), 276)));
        check!(r.retract == None);
        check!(r.next.logged == 310);
    }

    #[test]
    fn later_copies_repeat_the_row_as_it_was_logged() {
        // The same instant, spelled as the old feed spelled it.
        let mut entry = logged(WHEN, 1);
        entry.row.as_mut().unwrap().when = "2026-03-01T12:00:00.250000Z".to_string();

        let r = reconcile(&entry, 1, Some(&event(WHEN, 2)), false, now());

        check!(r.append.unwrap().0.when == "2026-03-01T12:00:00.250000Z");
        check!(r.next.row == entry.row);
    }

    #[test]
    fn fewer_repetitions_take_back_the_difference() {
        let r = reconcile(&logged(WHEN, 5), 1, Some(&event(WHEN, 2)), false, now());

        check!(r.append == None);
        check!(r.retract == Some((row_of(&event(WHEN, 1)), 3, Reason::FewerRepetitions)));
        check!(r.next.logged == 2);
    }

    #[test]
    fn a_rewritten_event_is_taken_back_and_logged_as_it_now_stands() {
        let corrected = "2025-03-01T12:00:00.250Z";
        let r = reconcile(
            &logged(WHEN, 4),
            1,
            Some(&event(corrected, 4)),
            false,
            now(),
        );

        check!(r.retract == Some((row_of(&event(WHEN, 1)), 4, Reason::Rewritten)));
        check!(r.append == Some((row_of(&event(corrected, 1)), 4)));
        check!(r.next == logged(corrected, 4));
    }

    #[test]
    fn a_renumbered_origin_is_a_rewrite() {
        let mut renumbered = event(WHEN, 2);
        renumbered.origin = 5;

        let r = reconcile(&logged(WHEN, 2), 1, Some(&renumbered), false, now());

        check!(r.retract.map(|(_, count, reason)| (count, reason)) == Some((2, Reason::Rewritten)));
        check!(r.append == Some((row_of(&renumbered), 2)));
    }

    #[test]
    fn a_deleted_event_is_taken_back() {
        let r = reconcile(&logged(WHEN, 4), 1, None, false, now());

        check!(r.retract == Some((row_of(&event(WHEN, 1)), 4, Reason::Deleted)));
        check!(r.append == None);
        check!(r.next == Entry::default());
    }

    #[test]
    fn a_covered_event_is_taken_back_and_stays_covered() {
        let r = reconcile(&logged(WHEN, 4), 1, Some(&event(WHEN, 4)), true, now());

        check!(r.retract == Some((row_of(&event(WHEN, 1)), 4, Reason::Covered)));
        check!(r.next.covered);

        // Unlinking the platform later leaves the bridge's rows where they
        // are, so the event is still not imported.
        let later = reconcile(&r.next, 1, Some(&event(WHEN, 6)), false, now());
        check!(later.append == None);
        check!(!later.is_change(&r.next));
    }

    #[test]
    fn an_event_covered_since_it_was_logged_is_taken_back() {
        let r = cover(&logged(WHEN, 4));

        check!(r.retract == Some((row_of(&event(WHEN, 1)), 4, Reason::Covered)));
        check!(r.append == None);
        check!(r.next == reconcile(&logged(WHEN, 4), 1, Some(&event(WHEN, 4)), true, now()).next);
    }

    #[test]
    fn a_covered_event_that_was_never_logged_is_only_marked() {
        let r = reconcile(&Entry::default(), 1, Some(&event(WHEN, 4)), true, now());

        check!(r.append == None);
        check!(r.retract == None);
        check!(r.next.covered);
    }

    #[test]
    fn a_deletion_of_an_event_never_logged_changes_nothing() {
        let r = reconcile(&Entry::default(), 1, None, false, now());

        check!(!r.is_change(&Entry::default()));
    }

    #[test]
    fn an_event_dated_in_the_future_is_logged_as_of_when_it_was_read() {
        let future = "2027-04-02T12:00:00Z";
        let r = reconcile(&Entry::default(), 1, Some(&event(future, 1)), false, now());

        check!(r.append.unwrap().0.when == now().to_string());

        // Read again later, it is the same event logged at the same time.
        let later: Timestamp = "2026-10-19T13:00:00Z".parse().unwrap();
        let again = reconcile(&r.next, 1, Some(&event(future, 1)), false, later);
        check!(!again.is_change(&r.next));
    }

    #[test]
    fn a_future_time_already_in_the_log_is_brought_back_to_now() {
        let future = "2027-04-02T12:00:00Z";
        let r = reconcile(&logged(future, 1), 1, Some(&event(future, 1)), false, now());

        check!(r.retract == Some((row_of(&event(future, 1)), 1, Reason::Rewritten)));
        check!(r.append.unwrap().0.when == now().to_string());
    }

    #[test]
    fn a_corrected_future_time_replaces_the_one_logged() {
        let future = "2027-04-02T12:00:00Z";
        let first = reconcile(&Entry::default(), 1, Some(&event(future, 1)), false, now());
        let corrected = reconcile(&first.next, 1, Some(&event(WHEN, 1)), false, now());

        check!(
            corrected.retract.map(|(_, count, reason)| (count, reason))
                == Some((1, Reason::Rewritten))
        );
        check!(corrected.append == Some((row_of(&event(WHEN, 1)), 1)));
    }

    #[test]
    fn shapes_compare_times_as_instants() {
        let a = row_of(&event("2026-03-01T12:00:00Z", 1));
        let b = row_of(&event("2026-03-01T07:00:00-05:00", 1));
        let c = row_of(&event("2026-03-01T12:00:01Z", 1));

        check!(same_row(&a, &b));
        check!(!same_row(&a, &c));
    }

    #[test]
    fn an_unreadable_time_is_compared_as_written() {
        let a = row_of(&event("sometime", 1));
        let b = row_of(&event("sometime", 1));
        let c = row_of(&event("another time", 1));

        check!(same_row(&a, &b));
        check!(!same_row(&a, &c));
    }
}
