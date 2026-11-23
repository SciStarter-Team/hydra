//! orrery's repair of its iNaturalist history, carried over to the Wall of
//! Fame acts hydra mirrors.
//!
//! orrery's iNaturalist sweep once credited the observation at its watermark
//! again on every pass, and each credit made in April minted another Wall of
//! Fame act carrying that observation's time. Its `repair_inat_history`
//! deletes the surplus and records each account's run, and
//! `/staff/sync/inat_history_repairs` serves the runs that deleted any.
//!
//! The acts feed hydra mirrors acts from carries no deletions, and hydra's
//! copies keep no orrery id, so a copy can only be found by what it says:
//! the account, the moment, the project, the contributions and the source.
//! Copies alike in all of those are interchangeable, and the surplus ones
//! are among them.
//!
//! How many to delete is not how many orrery deleted. The acts feed pages on
//! `when`, and a re-credit carries a time from the past, so hydra holds only
//! the copies that happened to cross its watermark -- fewer than orrery did,
//! sometimes none. So each run names, for each kind of act it deleted, how
//! many orrery holds now, and hydra deletes its copies down to that number.
//! Applying a run again finds nothing more to delete, which is what lets
//! the feed deliver it twice.

use anyhow::{Context, Result};
use common::{shared::db, transaction::Immediate};
use jiff::Timestamp;
use serde::Deserialize;
use std::str::FromStr;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

use log_service::database::Database;

use super::{OrreryFeedRow, synchronize_orrery_feed};

/// One account's repair, as `/staff/sync/inat_history_repairs` sends it.
#[derive(Debug, Deserialize)]
pub struct InatHistoryRepairData {
    pub id: i32,
    pub repaired_at: String,
    pub acts: Vec<RepairedActs>,
}

/// The acts of one kind that a repair deleted surplus copies of.
#[derive(Debug, Deserialize)]
pub struct RepairedActs {
    pub user_ulid: Option<String>,
    /// The moment the acts carry, which is the observation's.
    pub when: String,
    pub entity_ulid: Option<String>,
    pub project: Option<String>,
    pub contributions: i64,
    pub source: i64,
    /// How many acts of this kind orrery holds now.
    pub kept: usize,
}

/// Watermark: `repaired_at`. orrery serves a run only once it is an hour
/// old, by which time every run holding a lower id has committed, so a run
/// cannot arrive behind the watermark.
impl OrreryFeedRow for InatHistoryRepairData {
    const FEED: &'static str = "inat_history_repairs";
    const NAME: &'static str = "iNaturalist history repair";
    const BOOKMARK: &'static str = "inat-history-repair-sync-bookmark";
    const LAST_ID: &'static str = "inat-history-repair-sync-last-id";
    const START: &'static str = "1900-01-01T00:00:00Z";
    const PAGE: usize = 100;

    fn cursor(&self) -> (String, i32) {
        (self.repaired_at.clone(), self.id)
    }

    async fn store(&self) -> Result<()> {
        let mut tx = Immediate::begin_with_retry(db(), None, None).await?;
        let mut deleted = 0;

        for acts in &self.acts {
            let when = Timestamp::from_str(&acts.when)
                .with_context(|| format!("repair {} names the moment {:?}", self.id, acts.when))?;

            let copies: Vec<MirroredAct> = sqlx::query_as!(
                MirroredAct,
                r#"SELECT act_id AS "act_id!", created_at FROM acts_of_science
                   WHERE user_id IS ? AND entity_slug IS ? AND project IS ?
                     AND contributions = ? AND source = ?
                   ORDER BY act_id"#,
                acts.user_ulid,
                acts.entity_ulid,
                acts.project,
                acts.contributions,
                acts.source,
            )
            .fetch_all(&mut tx)
            .await?;

            for act_id in surplus(&copies, when, acts.kept) {
                sqlx::query!(r#"DELETE FROM acts_of_science WHERE act_id = ?"#, act_id)
                    .execute(&mut tx)
                    .await?;

                deleted += 1;
            }
        }

        tx.commit().await?;

        if deleted > 0 {
            println!(
                "iNaturalist history repair {}: deleted {deleted} surplus acts",
                self.id
            );
        }

        Ok(())
    }
}

/// A copy of an act, as found by what it says.
struct MirroredAct {
    act_id: String,
    created_at: String,
}

/// The copies to delete so that `kept` remain of those made at `when`.
///
/// `created_at` holds the time as the acts feed spelled it, and orrery names
/// the same moment its own way, so the two are compared as instants. A copy
/// whose time cannot be read is not one of them.
fn surplus(copies: &[MirroredAct], when: Timestamp, kept: usize) -> impl Iterator<Item = &str> {
    copies
        .iter()
        .filter(move |copy| Timestamp::from_str(&copy.created_at).is_ok_and(|at| at == when))
        .skip(kept)
        .map(|copy| copy.act_id.as_str())
}

pub async fn inat_history_repair_synchronize(
    database: Arc<Database>,
    cancellation_token: CancellationToken,
) {
    synchronize_orrery_feed::<InatHistoryRepairData>(database, cancellation_token).await
}

#[cfg(test)]
mod tests {
    use assert2::check;

    use super::*;

    fn copy(act_id: &str, created_at: &str) -> MirroredAct {
        MirroredAct {
            act_id: act_id.to_string(),
            created_at: created_at.to_string(),
        }
    }

    fn at(when: &str) -> Timestamp {
        when.parse().unwrap()
    }

    #[test]
    fn copies_are_deleted_down_to_what_orrery_kept() {
        let copies = [
            copy("A", "2025-04-17T13:37:33Z"),
            copy("B", "2025-04-17T13:37:33Z"),
            copy("C", "2025-04-17T13:37:33Z"),
        ];

        let doomed: Vec<_> = surplus(&copies, at("2025-04-17T13:37:33+00:00"), 1).collect();

        check!(doomed == ["B", "C"]);
    }

    #[test]
    fn fewer_copies_than_orrery_kept_lose_none() {
        let copies = [copy("A", "2026-04-13T17:26:31Z")];

        check!(surplus(&copies, at("2026-04-13T17:26:31+00:00"), 1).count() == 0);
        check!(surplus(&[], at("2026-04-13T17:26:31+00:00"), 1).count() == 0);
    }

    #[test]
    fn only_copies_of_the_same_moment_count() {
        let copies = [
            copy("A", "2025-04-17T13:37:33Z"),
            copy("B", "2025-04-17T13:37:34Z"),
            copy("C", "2025-04-17T09:37:33-04:00"),
            copy("D", "not a time"),
        ];

        let doomed: Vec<_> = surplus(&copies, at("2025-04-17T13:37:33+00:00"), 1).collect();

        check!(doomed == ["C"]);
    }

    #[test]
    fn fractional_seconds_are_part_of_the_moment() {
        let copies = [
            copy("A", "2025-04-17T13:37:33.250Z"),
            copy("B", "2025-04-17T13:37:33Z"),
        ];

        let doomed: Vec<_> = surplus(&copies, at("2025-04-17T13:37:33.250000+00:00"), 0).collect();

        check!(doomed == ["A"]);
    }

    #[test]
    fn a_run_applied_again_deletes_nothing_more() {
        let when = at("2025-04-17T13:37:33+00:00");
        let copies = [
            copy("A", "2025-04-17T13:37:33Z"),
            copy("B", "2025-04-17T13:37:33Z"),
        ];

        let doomed: Vec<_> = surplus(&copies, when, 1).collect();
        let remaining: Vec<_> = copies
            .iter()
            .filter(|act| !doomed.contains(&act.act_id.as_str()))
            .map(|act| copy(&act.act_id, &act.created_at))
            .collect();

        check!(doomed == ["B"]);
        check!(surplus(&remaining, when, 1).count() == 0);
    }
}
