//! orrery's participation feed, and how far this service has read it.
//!
//! The feed, `/staff/sync/affiliate_events`, is every write orrery makes to
//! its participation table, in order: each insert and update numbers the
//! event with the next value of one sequence, its `revision`, and each delete
//! leaves a tombstone numbered from the same sequence. An event is written
//! more than once -- later contributions fold into its `repetitions`, repairs
//! rewrite it -- so the feed delivers an event again after every write, and
//! [`crate::participation_ledger`] is what turns each delivery into the change
//! it makes to the log.
//!
//! Not on `when`: that is when the contribution was made, which imports set
//! from the past and a mistyped year can set in the future, so a reader
//! following it passes over every row that arrives dated behind the latest
//! it has seen. Not on the id either, which numbers an event once.
//!
//! A revision is taken when the write is made, not when its transaction
//! commits, so revisions become visible slightly out of order: a transaction
//! holding a lower one can commit after a higher one has been read. Reading
//! strictly past the highest revision seen would skip such a write for good.
//! So the reader keeps two marks, in a [`FeedCursor`]: `settled`, below which
//! nothing new can appear, and `frontier`, how far the last pass read and
//! when it finished. Every pass reads everything after `settled`, and a
//! frontier becomes the settled mark once a pass that began at least
//! [`SETTLE`] after it was recorded has read past it -- any transaction that
//! held a lower revision then has committed since, and that pass has read
//! what it wrote. Re-reading the window delivers writes the importer has
//! seen before, which the ledger recognizes as changing nothing.
//!
//! [`SETTLE`] is an assumption about orrery: that nothing writing to its
//! participation table keeps a transaction open that long.

use anyhow::{Result, anyhow};
use jiff::{SignedDuration, Timestamp};
use serde::{Deserialize, Serialize};

use crate::database::{Database, get_sync_entry, set_sync_bookmark};

const URL: &str = "https://scistarter.org/staff/sync/affiliate_events";

/// The most rows orrery returns for one request. A shorter page is the end
/// of the feed.
pub const PAGE_SIZE: usize = 500;

/// How long a transaction in orrery is assumed to stay open at most.
pub const SETTLE: SignedDuration = SignedDuration::from_hours(1);

const CURSOR_KEY: &str = "participation-sync-cursor";

/// One write to an event: the event as the write left it, or `None` once
/// the event no longer counts anywhere -- deleted, or its project deleted.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedItem {
    pub revision: i64,
    /// The event's id in orrery, which every revision of it shares.
    pub id: i32,
    pub event: Option<ParticipationEventData>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParticipationEventData {
    pub origin: i16,
    pub when: String, // ISO 8601 timestamp string
    pub duration: Option<f64>,
    pub where_point: Option<String>,
    pub magnitude: Option<i32>,
    pub authenticated: Option<bool>,
    pub extra: Option<String>,
    pub repetitions: i32,
    pub fidelity: Option<f64>,
    pub entity_ulid: String,
    pub profile_user_ulid: Option<String>,
    pub task_name: Option<String>,
}

/// Up to [`PAGE_SIZE`] writes with revisions above `after_revision`, in
/// revision order.
pub async fn fetch_page(client: &reqwest::Client, after_revision: i64) -> Result<Vec<FeedItem>> {
    let body = client
        .get(URL)
        .bearer_auth(env!("SYNC_TOKEN"))
        .query(&[("after_revision", after_revision)])
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;

    serde_json::from_str(&body).map_err(|e| {
        anyhow!("{e} in the participation feed after revision {after_revision}: {body}")
    })
}

/// How far the feed has been read; see the module docs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeedCursor {
    /// Every write at or below this revision has been read, and no write at
    /// or below it can still appear.
    pub settled: i64,
    /// How far a pass has read beyond `settled`, waiting to settle.
    pub frontier: Option<Frontier>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Frontier {
    pub revision: i64,
    /// When the pass that read through `revision` finished.
    pub read_at: Timestamp,
}

impl FeedCursor {
    /// A cursor whose next pass reads every write after `revision`.
    pub fn starting_after(revision: i64) -> FeedCursor {
        FeedCursor {
            settled: revision,
            frontier: None,
        }
    }

    /// The stored cursor, or `None` when there is none. There is no default:
    /// reading reconciles each event with the ledger, and only the repair
    /// that sets the cursor knows that the ledger records what the log holds.
    pub async fn load(database: &Database) -> Result<Option<FeedCursor>> {
        get_sync_entry(database, CURSOR_KEY)
            .await?
            .map(|stored| {
                serde_json::from_str(&stored)
                    .map_err(|e| anyhow!("unreadable participation cursor {stored:?}: {e}"))
            })
            .transpose()
    }

    pub async fn store(&self, database: &Database) -> Result<()> {
        set_sync_bookmark(database, CURSOR_KEY, &serde_json::to_string(self)?).await?;
        Ok(())
    }

    /// The cursor after a pass that began at `started`, read every write
    /// after `self.settled` through `read_through` without failing, and
    /// finished at `finished`.
    ///
    /// A pass that fails part-way must leave the cursor as it was: the writes
    /// past the failure are unread, and the next pass reads them from
    /// `settled` again.
    pub fn after_pass(
        self,
        started: Timestamp,
        read_through: i64,
        finished: Timestamp,
    ) -> FeedCursor {
        let promoted = self
            .frontier
            .filter(|frontier| started.duration_since(frontier.read_at) >= SETTLE);

        match (promoted, self.frontier) {
            (Some(promoted), _) => FeedCursor {
                settled: promoted.revision,
                frontier: Frontier::beyond(promoted.revision, read_through, finished),
            },
            // Still settling, and replacing it would restart the wait.
            (None, Some(pending)) => FeedCursor {
                settled: self.settled,
                frontier: Some(pending),
            },
            (None, None) => FeedCursor {
                settled: self.settled,
                frontier: Frontier::beyond(self.settled, read_through, finished),
            },
        }
    }
}

impl Frontier {
    fn beyond(settled: i64, read_through: i64, finished: Timestamp) -> Option<Frontier> {
        (read_through > settled).then_some(Frontier {
            revision: read_through,
            read_at: finished,
        })
    }
}

#[cfg(test)]
mod tests {
    use assert2::check;

    use super::*;

    fn at(when: &str) -> Timestamp {
        when.parse().unwrap()
    }

    #[test]
    fn a_first_pass_leaves_what_it_read_to_settle() {
        let cursor = FeedCursor::starting_after(100).after_pass(
            at("2026-10-19T12:00:00Z"),
            250,
            at("2026-10-19T12:01:00Z"),
        );

        check!(
            cursor
                == FeedCursor {
                    settled: 100,
                    frontier: Some(Frontier {
                        revision: 250,
                        read_at: at("2026-10-19T12:01:00Z"),
                    }),
                }
        );
    }

    #[test]
    fn a_frontier_settles_once_a_pass_begins_after_the_window() {
        let cursor = FeedCursor {
            settled: 100,
            frontier: Some(Frontier {
                revision: 250,
                read_at: at("2026-10-19T12:01:00Z"),
            }),
        };

        let after = cursor.after_pass(at("2026-10-19T13:01:00Z"), 300, at("2026-10-19T13:02:00Z"));

        check!(
            after
                == FeedCursor {
                    settled: 250,
                    frontier: Some(Frontier {
                        revision: 300,
                        read_at: at("2026-10-19T13:02:00Z"),
                    }),
                }
        );
    }

    #[test]
    fn a_frontier_does_not_settle_inside_the_window() {
        // Begun a minute short of the window: a transaction that held a
        // revision below 250 when it was recorded may not have committed yet, so the
        // next pass has to read from 100 again. The pending frontier stays
        // as it was, or the wait would start over every pass.
        let cursor = FeedCursor {
            settled: 100,
            frontier: Some(Frontier {
                revision: 250,
                read_at: at("2026-10-19T12:01:00Z"),
            }),
        };

        check!(
            cursor.after_pass(at("2026-10-19T13:00:00Z"), 300, at("2026-10-19T13:01:00Z"))
                == cursor
        );
    }

    #[test]
    fn a_pass_that_reads_nothing_new_records_no_frontier() {
        let cursor = FeedCursor::starting_after(100);

        check!(
            cursor.after_pass(at("2026-10-19T12:00:00Z"), 100, at("2026-10-19T12:00:01Z"))
                == cursor
        );
    }

    #[test]
    fn settling_with_nothing_new_leaves_no_frontier() {
        let cursor = FeedCursor {
            settled: 100,
            frontier: Some(Frontier {
                revision: 250,
                read_at: at("2026-10-19T12:01:00Z"),
            }),
        };

        check!(
            cursor.after_pass(at("2026-10-19T14:00:00Z"), 250, at("2026-10-19T14:00:01Z"))
                == FeedCursor::starting_after(250)
        );
    }
}
