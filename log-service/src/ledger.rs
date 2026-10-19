//! The importers' ledger of the source rows they have consumed.
//!
//! A feed that can hand an importer the same row twice -- because it pages on
//! a column orrery mutates, or because its watermark is inclusive -- has to
//! remember which rows it has already decided about, or it logs one event
//! once per delivery. The source id lives here rather
//! than on the logged row, so the log stays a log of events and nothing has to
//! change when hydra becomes the source of new ones.

use anyhow::Result;

/// The feeds the ledger records. A source's ids are only unique within one
/// of its tables, so the ledger is keyed by both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Feed {
    Joins,
    Bookmarks,
    /// Not an orrery table: observations the iNaturalist bridge has credited,
    /// by iNaturalist's own id. Its watermark is inclusive, so every sweep
    /// meets the newest observation it credited last time again.
    InatObservations,
}

impl Feed {
    fn name(self) -> &'static str {
        match self {
            Feed::Joins => "joins",
            Feed::Bookmarks => "bookmarks",
            Feed::InatObservations => "inat-observations",
        }
    }
}

/// Claim `source_id` for `feed`, reporting whether this importer had it already.
///
/// `true` means the row has not been decided about before and its events
/// should be written; `false` means this is a re-delivery of a row that was,
/// and writing it again would log the same event twice.
///
/// The claim is taken before the write rather than after, so that a crash
/// between the two costs one duplicated event rather than a silently dropped
/// one. A write that fails outright gives the claim back via [`release`].
pub async fn claim(feed: Feed, source_id: i64) -> Result<bool> {
    let feed = feed.name();
    let claimed = sqlx::query!(
        r#"INSERT OR IGNORE INTO synced_source_rows ("feed", "source_id") VALUES (?, ?)"#,
        feed,
        source_id
    )
    .execute(common::shared::db())
    .await?
    .rows_affected();

    Ok(claimed > 0)
}

/// Give back a claim whose events could not be written, so that the next pass
/// treats the row as new rather than as a re-delivery.
///
/// A failure here is only reported: the row keeps its claim, the event stays
/// unlogged, and the repair tooling is what recovers it. Propagating would
/// take down an importer over a row it was already failing to write.
pub async fn release(feed: Feed, source_id: i64) {
    let name = feed.name();
    let released = sqlx::query!(
        r#"DELETE FROM synced_source_rows WHERE "feed" = ? AND "source_id" = ?"#,
        name,
        source_id
    )
    .execute(common::shared::db())
    .await;

    if let Err(e) = released {
        eprintln!("Failed to release {name} row {source_id} after a failed write: {e}");
    }
}
