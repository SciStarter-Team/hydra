//! Taking participation that orrery has taken back out of the log.
//!
//! The log is written append-only, so a retraction is queued, in
//! `participation_retractions`, and carried out by the compactor, the one
//! task that rewrites the log's files. Each cycle it looks through the
//! participation Parquet for copies of the rows waiting to go and replaces
//! every file that holds some with a copy that does not.
//!
//! A row still in the WAL is not looked for there: once its day is over the
//! compactor turns it into Parquet, and the next cycle finds it in the new
//! file. So a retraction has been carried out by the day after it was queued,
//! and one still waiting after that is reported -- it names rows the log
//! does not hold, which only a ledger out of step with the log can do.
//!
//! The copies of one row are interchangeable: they are the same event, or
//! events identical to the microsecond, and either way the log has to hold
//! some number of them, which is all a retraction changes.
//!
//! Removing rows from a file and recording how many are left to remove are
//! writes to two places. A replacement is written in full first; its rename
//! is recorded in `log_file_moves`, in the same transaction as the counts it
//! settles; and the rename is made afterwards. [`finish_moves`] completes any
//! rename an interrupted cycle recorded but did not make, and it runs before
//! anything else touches the files, so a file's replacement and the counts
//! it settled always take effect together.

use anyhow::{Context, Result};
use common::transaction::Immediate;
use datafusion::arrow::array::{Array, RecordBatch, StringArray};
use datafusion::arrow::datatypes::Schema;
use jiff::Timestamp;
use sqlx::SqlitePool;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::database::rewrite::{
    self, DEFAULT_ROW_GROUP_SIZE, ParquetReplacement, Replacement, batch_from_rows, rows_from_batch,
};
use crate::database::table::TableRow;
use crate::database::tables::ParticipationRow;

/// How long a retraction can wait before it is reported as stuck. A day for
/// its rows to reach Parquet, and one more for a cycle to find them.
const STUCK_AFTER: jiff::SignedDuration = jiff::SignedDuration::from_hours(48);

/// How long the queue has to have been left alone before it is carried out.
///
/// Retractions come in bursts -- a repair in orrery rewrites many events at
/// once, and the importer queues one retraction per event as it reads them --
/// and every pass through the queue may rewrite the same large files. Waiting
/// for the burst to end rewrites each of them once.
const QUIET: jiff::SignedDuration = jiff::SignedDuration::from_mins(15);

/// How long a retraction waits for the queue to go quiet before it is
/// carried out anyway, so a steady trickle cannot hold it back for good.
const PATIENCE: jiff::SignedDuration = jiff::SignedDuration::from_hours(6);

/// Queue `count` copies of `row` to be taken out of the log.
pub async fn queue(
    tx: &mut Immediate,
    row: &ParticipationRow,
    count: u32,
    reason: &str,
) -> Result<()> {
    let row = serde_json::to_string(row)?;
    let count = i64::from(count);
    let queued_at = Timestamp::now().to_string();

    sqlx::query!(
        r#"INSERT INTO participation_retractions ("row", "remaining", "queued_at", "reason")
           VALUES (?, ?, ?, ?)"#,
        row,
        count,
        queued_at,
        reason,
    )
    .execute(&mut *tx)
    .await?;

    Ok(())
}

/// Complete every rename a cycle recorded and did not get to make.
///
/// Must run before anything else touches the log's files: until it has, a
/// replacement waiting to be renamed looks like an abandoned one.
pub async fn finish_moves(pool: &SqlitePool) -> Result<()> {
    let moves = sqlx::query!(r#"SELECT "move_id", "from", "to" FROM log_file_moves"#)
        .fetch_all(pool)
        .await?;

    for recorded in moves {
        let from = PathBuf::from(&recorded.from);

        // Gone already means the rename was made and only its record was
        // left behind.
        if from.exists() {
            rewrite::move_into_place(&from, Path::new(&recorded.to))
                .with_context(|| format!("finishing the move of {}", recorded.from))?;
        }

        sqlx::query!(
            r#"DELETE FROM log_file_moves WHERE "move_id" = ?"#,
            recorded.move_id
        )
        .execute(pool)
        .await?;
    }

    Ok(())
}

/// One queued retraction, as far as it has got.
#[derive(Debug, Clone)]
struct Pending {
    retraction_id: i64,
    remaining: u32,
    /// Files last modified before this, in Unix milliseconds, have been
    /// looked through for it already.
    scanned_at: Option<i64>,
    queued_at: String,
    reason: String,
}

/// The retractions waiting, by the exact row each one takes back.
#[derive(Debug, Default)]
struct Wanted {
    by_row: HashMap<String, Vec<Pending>>,
    /// Every (project, account) a wanted row is for: a file holding none of
    /// them is passed over without decoding its rows.
    pairs: Pairs,
}

/// (project, account) pairs, looked up without allocating: the files are
/// read through them a row at a time.
#[derive(Debug, Default)]
struct Pairs(HashMap<String, Accounts>);

#[derive(Debug, Default)]
struct Accounts {
    anonymous: bool,
    named: HashSet<String>,
}

impl Pairs {
    fn insert(&mut self, entity: &str, user: Option<&str>) {
        let accounts = self.0.entry(entity.to_string()).or_default();

        match user {
            Some(user) => {
                accounts.named.insert(user.to_string());
            }
            None => accounts.anonymous = true,
        }
    }

    fn contains(&self, entity: &str, user: Option<&str>) -> bool {
        self.0.get(entity).is_some_and(|accounts| match user {
            Some(user) => accounts.named.contains(user),
            None => accounts.anonymous,
        })
    }
}

impl Wanted {
    async fn load(pool: &SqlitePool) -> Result<Wanted> {
        let stored = sqlx::query!(
            r#"SELECT "retraction_id", "row", "remaining", "scanned_at", "queued_at", "reason"
               FROM participation_retractions
               WHERE "remaining" > 0
               ORDER BY "retraction_id""#
        )
        .fetch_all(pool)
        .await?;

        let mut wanted = Wanted::default();

        for stored in stored {
            let row: ParticipationRow = serde_json::from_str(&stored.row).with_context(|| {
                format!("unreadable row in retraction {}", stored.retraction_id)
            })?;

            wanted.pairs.insert(&row.entity_id, row.user_id.as_deref());

            wanted
                .by_row
                .entry(serde_json::to_string(&row)?)
                .or_default()
                .push(Pending {
                    retraction_id: stored.retraction_id,
                    remaining: u32::try_from(stored.remaining)?,
                    scanned_at: stored.scanned_at,
                    queued_at: stored.queued_at,
                    reason: stored.reason,
                });
        }

        Ok(wanted)
    }

    fn is_empty(&self) -> bool {
        self.by_row.is_empty()
    }

    /// Whether to carry the queue out now; see [`QUIET`].
    fn is_due(&self, now: Timestamp) -> bool {
        let ages: Vec<_> = self
            .all()
            .filter_map(|pending| pending.queued_at.parse::<Timestamp>().ok())
            .map(|queued| now.duration_since(queued))
            .collect();

        let youngest = ages.iter().min().copied().unwrap_or_default();
        let oldest = ages.iter().max().copied().unwrap_or_default();

        youngest >= QUIET || oldest >= PATIENCE
    }

    fn all(&self) -> impl Iterator<Item = &Pending> {
        self.by_row.values().flatten()
    }

    /// Whether a file last modified at `modified` still has to be looked
    /// through for some retraction.
    fn wants_file(&self, modified: i64) -> bool {
        self.all().any(|pending| {
            pending.remaining > 0 && pending.scanned_at.is_none_or(|at| modified >= at)
        })
    }

    /// Take one copy of `row` if any retraction still wants it, reporting
    /// which retraction it went to.
    fn take(&mut self, row: &ParticipationRow) -> Result<Option<i64>> {
        let Some(pending) = self.by_row.get_mut(&serde_json::to_string(row)?) else {
            return Ok(None);
        };

        Ok(pending
            .iter_mut()
            .find(|pending| pending.remaining > 0)
            .map(|pending| {
                pending.remaining -= 1;
                pending.retraction_id
            }))
    }
}

/// Carry out what can be carried out of the queued retractions.
pub async fn apply_pending(pool: &SqlitePool, base: &Path) -> Result<()> {
    let mut wanted = Wanted::load(pool).await?;

    let started = Timestamp::now();

    if wanted.is_empty() || !wanted.is_due(started) {
        return Ok(());
    }

    let started_ms = started.as_millisecond();
    let schema = ParticipationRow::schema();

    for path in rewrite::parquet_files(base, ParticipationRow::TABLE_NAME)? {
        // A day whose WAL file is still waiting beside its Parquet is about
        // to be converted again from the WAL, which would put back anything
        // taken out of the Parquet now.
        if wal_counterpart(base, &path).exists() {
            continue;
        }

        if !wanted.wants_file(modified_ms(&path)?) {
            continue;
        }

        let (returned, outcome) = tokio::task::spawn_blocking({
            let schema = schema.clone();
            move || {
                let outcome = remove_wanted(&path, &schema, &mut wanted);
                (wanted, outcome)
            }
        })
        .await?;

        wanted = returned;

        if let Some((replacement, taken)) = outcome? {
            commit(pool, replacement, &taken).await?;
        }
    }

    let ids: Vec<i64> = wanted.all().map(|pending| pending.retraction_id).collect();

    for retraction_id in ids {
        sqlx::query!(
            r#"UPDATE participation_retractions SET "scanned_at" = ? WHERE "retraction_id" = ?"#,
            started_ms,
            retraction_id
        )
        .execute(pool)
        .await?;
    }

    for pending in wanted.all().filter(|pending| pending.remaining > 0) {
        let stuck = pending
            .queued_at
            .parse::<Timestamp>()
            .is_ok_and(|queued| started.duration_since(queued) >= STUCK_AFTER);

        if stuck {
            eprintln!(
                "Retraction {} ({}) still has {} rows to remove that the log does not hold",
                pending.retraction_id, pending.reason, pending.remaining
            );
        }
    }

    Ok(())
}

/// A replacement for `path` without the rows `wanted` takes, and how many
/// each retraction took; `None` when it takes none.
fn remove_wanted(
    path: &Path,
    schema: &Schema,
    wanted: &mut Wanted,
) -> Result<Option<(Replacement, HashMap<i64, u32>)>> {
    if !holds_wanted_pair(path, wanted)? {
        return Ok(None);
    }

    let mut writer = ParquetReplacement::create(path, schema, DEFAULT_ROW_GROUP_SIZE)?;
    let mut taken: HashMap<i64, u32> = HashMap::new();

    for batch in rewrite::read_parquet(path, None)? {
        let batch = batch?;

        if !batch_holds_wanted_pair(&batch, wanted)? {
            writer.write(&batch)?;
            continue;
        }

        let mut kept = Vec::with_capacity(batch.num_rows());

        for row in rows_from_batch::<ParticipationRow>(&batch)? {
            match wanted.take(&row)? {
                Some(retraction_id) => *taken.entry(retraction_id).or_default() += 1,
                None => kept.push(row),
            }
        }

        if !kept.is_empty() {
            writer.write(&batch_from_rows(&kept, schema)?)?;
        }
    }

    let replacement = writer.finish()?;

    if taken.is_empty() {
        replacement.abandon()?;
        return Ok(None);
    }

    Ok(Some((replacement, taken)))
}

/// Record what a replacement settles and put it in place, in that order.
async fn commit(
    pool: &SqlitePool,
    replacement: Replacement,
    taken: &HashMap<i64, u32>,
) -> Result<()> {
    let from = replacement.temporary().to_string_lossy().into_owned();
    let to = replacement.target().to_string_lossy().into_owned();

    let mut tx = Immediate::begin_with_retry(pool, None, None).await?;

    for (retraction_id, count) in taken {
        let count = i64::from(*count);

        sqlx::query!(
            r#"UPDATE participation_retractions SET "remaining" = "remaining" - ?
               WHERE "retraction_id" = ?"#,
            count,
            retraction_id
        )
        .execute(&mut tx)
        .await?;
    }

    let move_id = sqlx::query!(
        r#"INSERT INTO log_file_moves ("from", "to") VALUES (?, ?)"#,
        from,
        to
    )
    .execute(&mut tx)
    .await?
    .last_insert_rowid();

    tx.commit().await?;

    replacement.commit()?;

    sqlx::query!(r#"DELETE FROM log_file_moves WHERE "move_id" = ?"#, move_id)
        .execute(pool)
        .await?;

    let removed: u32 = taken.values().sum();
    println!("Retracted {removed} participation rows from {to}");

    Ok(())
}

/// Whether any row of the file is for a (project, account) a retraction
/// names, read from those two columns alone.
fn holds_wanted_pair(path: &Path, wanted: &Wanted) -> Result<bool> {
    for batch in rewrite::read_parquet(path, Some(&["entity_id", "user_id"]))? {
        if batch_holds_wanted_pair(&batch?, wanted)? {
            return Ok(true);
        }
    }

    Ok(false)
}

fn batch_holds_wanted_pair(batch: &RecordBatch, wanted: &Wanted) -> Result<bool> {
    let entities = string_column(batch, "entity_id")?;
    let users = string_column(batch, "user_id")?;

    Ok((0..batch.num_rows()).any(|i| {
        let user = (!users.is_null(i)).then(|| users.value(i));
        wanted.pairs.contains(entities.value(i), user)
    }))
}

fn string_column<'a>(batch: &'a RecordBatch, name: &str) -> Result<&'a StringArray> {
    batch
        .column_by_name(name)
        .with_context(|| format!("column {name} is missing"))?
        .as_any()
        .downcast_ref::<StringArray>()
        .with_context(|| format!("column {name} is not a string column"))
}

/// The WAL file a Parquet file of the log was converted from.
fn wal_counterpart(base: &Path, parquet: &Path) -> PathBuf {
    let name = parquet
        .file_name()
        .map(|name| name.to_string_lossy().replace(".parquet", ".jsonl"))
        .unwrap_or_default();

    base.join("wal")
        .join(ParticipationRow::TABLE_NAME)
        .join(name)
}

fn modified_ms(path: &Path) -> Result<i64> {
    let modified = std::fs::metadata(path)?.modified()?;
    Ok(i64::try_from(
        modified.duration_since(UNIX_EPOCH)?.as_millis(),
    )?)
}
