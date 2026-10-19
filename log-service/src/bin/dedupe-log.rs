//! One-shot repair for events that were logged more than once.
//!
//! Two importers used to write a row per *delivery* rather than per event
//! (see the module docs in `sync.rs`), so a join that orrery re-delivered 715
//! times became 715 rows carrying the identical microsecond timestamp. The
//! importers no longer do that; this removes the rows they already wrote.
//!
//! It is a repair, not a maintenance job. The compactor is deliberately left
//! alone: collapsing "identical events" is a claim about what the source
//! means, and only the importer is in a position to make it. Once this has
//! run there is nothing for it to do again.
//!
//! **Stop the log service before running this.** It rewrites the Parquet and
//! WAL files in place, and the service holds the WAL open for append.
//!
//! ```text
//! cd log-service && direnv exec . cargo run --bin dedupe-log -- --dry-run
//! cd log-service && direnv exec . cargo run --bin dedupe-log
//! ```
//!
//! **Run the last pass after the re-delivery wave, not before.** The ledger in
//! `synced_source_rows` starts empty, so the first time the fixed importer
//! meets a row it has no record of it and logs it once -- correctly, as far as
//! it can tell. Every event can therefore gain one further copy after the fix
//! ships, and exactly one: from the second delivery onward the ledger
//! recognises it. Running this before that wave has passed leaves those
//! second copies behind and needs another pass later; running it afterwards
//! settles the table for good. Running it early is still useful -- it reclaims
//! the space immediately -- it just is not the final pass.
//!
//! Seeding the ledger instead, by claiming every orrery id up front, was the
//! obvious alternative and is worse: it would suppress any row that had
//! somehow never been delivered, and nothing in the logged rows can prove that
//! set is empty. One extra copy per event, cleaned up once, costs less than a
//! silent loss.

use anyhow::{Context, Result};
use datafusion::arrow::array::{Array, BooleanArray, RecordBatch, StringArray};
use datafusion::arrow::compute::filter_record_batch;
use datafusion::arrow::datatypes::Schema;
use std::collections::HashSet;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use log_service::database::rewrite::{self, DEFAULT_ROW_GROUP_SIZE, ParquetReplacement};
use log_service::database::table::TableRow;
use log_service::database::tables::{BookmarkRow, ClickthroughRow, JoinRow};

/// What identifies one event in a table, and which of its rows are in scope.
struct Repair {
    table: &'static str,
    /// The columns whose combination names a single event. Two rows agreeing
    /// on all of them are the same event logged twice: no user joins the same
    /// project twice in the same microsecond.
    key: &'static [&'static str],
    /// When set, only rows whose `.0` column equals `.1` are considered.
    /// Everything else is kept untouched.
    scope: Option<(&'static str, &'static str)>,
}

/// The three populations the two broken importers wrote.
///
/// `clickthroughs` is scoped to `link_type = 'join'` because that is the part
/// `join_synchronize` wrote. Golink and external-link clickthroughs come from
/// importers that page on an immutable column and are left alone.
const REPAIRS: &[Repair] = &[
    Repair {
        table: JoinRow::TABLE_NAME,
        key: &["when", "entity_id", "user_id"],
        scope: None,
    },
    Repair {
        table: BookmarkRow::TABLE_NAME,
        key: &["when", "entity_id", "user_id"],
        scope: None,
    },
    Repair {
        table: ClickthroughRow::TABLE_NAME,
        // `to_url` is a mutable attribute copied at delivery time -- a
        // project's URL can change between deliveries -- so the entity the
        // clickthrough points at is what identifies it.
        key: &["when", "user_id", "program_id"],
        scope: Some(("link_type", "join")),
    },
];

#[derive(Default)]
struct Counts {
    read: usize,
    kept: usize,
}

impl Counts {
    fn dropped(&self) -> usize {
        self.read - self.kept
    }
}

fn main() -> Result<()> {
    let dry_run = std::env::args().any(|arg| arg == "--dry-run");

    #[cfg(not(debug_assertions))]
    let database_path = env!("DATABASE_PATH");
    #[cfg(debug_assertions)]
    let database_path = env!("LOCAL_DATABASE_PATH");

    let base = PathBuf::from(database_path);

    if dry_run {
        println!("Dry run: counting only, nothing will be written.\n");
    } else {
        println!("Rewriting files under {}.\n", base.display());
    }

    for repair in REPAIRS {
        let counts = run(&base, repair, dry_run)?;

        println!(
            "{}: {} rows, {} kept, {} duplicate events removed",
            repair.table,
            counts.read,
            counts.kept,
            counts.dropped()
        );
    }

    Ok(())
}

/// Walk one table's files oldest-first, keeping the first copy of each event.
///
/// Order matters: a re-delivery lands in the file for the day it arrived, not
/// the day the event happened, so copies of one event are spread across many
/// files. Parquet holds what has been compacted and the WAL what has not, so
/// the WAL is read last and shares the same set of seen events -- otherwise
/// the next compaction would put duplicates straight back.
fn run(base: &Path, repair: &Repair, dry_run: bool) -> Result<Counts> {
    let schema = rewrite::schema_for(repair.table)?;
    let mut seen: HashSet<String> = HashSet::new();
    let mut counts = Counts::default();

    for path in rewrite::parquet_files(base, repair.table)? {
        rewrite_parquet(&path, &schema, repair, &mut seen, &mut counts, dry_run)
            .with_context(|| format!("rewriting {}", path.display()))?;
    }

    for path in rewrite::wal_files(base, repair.table)? {
        rewrite_wal(&path, repair, &mut seen, &mut counts, dry_run)
            .with_context(|| format!("rewriting {}", path.display()))?;
    }

    Ok(counts)
}

fn rewrite_parquet(
    path: &Path,
    schema: &Schema,
    repair: &Repair,
    seen: &mut HashSet<String>,
    counts: &mut Counts,
    dry_run: bool,
) -> Result<()> {
    let reader = rewrite::read_parquet(path, None)?;

    let mut kept_batches = Vec::new();
    let mut file_dropped = 0usize;

    for batch in reader {
        let batch = batch?;
        let mask = keep_mask(&batch, repair, seen)?;

        counts.read += batch.num_rows();
        let kept = mask.values().count_set_bits();
        counts.kept += kept;
        file_dropped += batch.num_rows() - kept;

        if !dry_run {
            kept_batches.push(filter_record_batch(&batch, &mask)?);
        }
    }

    if dry_run || file_dropped == 0 {
        return Ok(());
    }

    let mut writer = ParquetReplacement::create(path, schema, DEFAULT_ROW_GROUP_SIZE)?;
    let mut rows_written = 0usize;

    for batch in kept_batches {
        rows_written += batch.num_rows();
        writer.write(&batch)?;
    }

    let replacement = writer.finish()?;

    // A file whose rows were every one of them duplicates has nothing left to
    // say; leaving an empty Parquet behind would only slow later reads.
    if rows_written == 0 {
        replacement.abandon()?;
        std::fs::remove_file(path)?;
    } else {
        replacement.commit()?;
    }

    Ok(())
}

fn rewrite_wal(
    path: &Path,
    repair: &Repair,
    seen: &mut HashSet<String>,
    counts: &mut Counts,
    dry_run: bool,
) -> Result<()> {
    let mut kept_lines = Vec::new();
    let mut file_dropped = 0usize;

    let mut lines = BufReader::new(File::open(path)?).lines().peekable();

    while let Some(line) = lines.next() {
        let line = line?;

        if line.trim().is_empty() {
            continue;
        }

        let row: serde_json::Value = match serde_json::from_str(&line) {
            Ok(row) => row,
            // A dry run may read the file the service is appending to, and
            // catch its last line half written. That is the only line a torn
            // write can affect, and a run that is going to rewrite the file
            // has the service stopped, so anywhere else this is real damage
            // and should stop the run rather than be tidied away.
            Err(e) if dry_run && lines.peek().is_none() => {
                eprintln!(
                    "{}: ignoring an incomplete last line, which is what a write in progress looks like ({e})",
                    path.display()
                );
                break;
            }
            Err(e) => return Err(e.into()),
        };

        counts.read += 1;

        if keep_json_row(&row, repair, seen) {
            counts.kept += 1;
            if !dry_run {
                kept_lines.push(line);
            }
        } else {
            file_dropped += 1;
        }
    }

    if dry_run || file_dropped == 0 {
        return Ok(());
    }

    rewrite::replace_wal(path, kept_lines)?.commit()?;
    Ok(())
}

/// Which rows of a batch to keep: everything out of scope, plus the first copy
/// of each in-scope event.
fn keep_mask(
    batch: &RecordBatch,
    repair: &Repair,
    seen: &mut HashSet<String>,
) -> Result<BooleanArray> {
    let key_columns = repair
        .key
        .iter()
        .map(|name| column(batch, name))
        .collect::<Result<Vec<_>>>()?;

    let scope_column = repair
        .scope
        .map(|(name, _)| column(batch, name))
        .transpose()?;

    let mut keep = Vec::with_capacity(batch.num_rows());

    for row in 0..batch.num_rows() {
        let in_scope = match (scope_column, repair.scope) {
            (Some(values), Some((_, wanted))) => {
                !values.is_null(row) && values.value(row) == wanted
            }
            _ => true,
        };

        if !in_scope {
            keep.push(true);
            continue;
        }

        let mut key = String::new();
        for values in &key_columns {
            if !values.is_null(row) {
                key.push_str(values.value(row));
            }
            // A separator no ULID or timestamp can contain, so that adjacent
            // fields cannot run together into a colliding key.
            key.push('\u{1f}');
        }

        keep.push(seen.insert(key));
    }

    Ok(BooleanArray::from(keep))
}

fn keep_json_row(row: &serde_json::Value, repair: &Repair, seen: &mut HashSet<String>) -> bool {
    if let Some((name, wanted)) = repair.scope
        && row.get(name).and_then(|value| value.as_str()) != Some(wanted)
    {
        return true;
    }

    let mut key = String::new();
    for name in repair.key {
        if let Some(value) = row.get(name).and_then(|value| value.as_str()) {
            key.push_str(value);
        }
        key.push('\u{1f}');
    }

    seen.insert(key)
}

fn column<'a>(batch: &'a RecordBatch, name: &str) -> Result<&'a StringArray> {
    batch
        .column_by_name(name)
        .ok_or_else(|| anyhow::anyhow!("column {name} is missing"))?
        .as_any()
        .downcast_ref::<StringArray>()
        .ok_or_else(|| anyhow::anyhow!("column {name} is not a string column"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use datafusion::arrow::array::StringArray;
    use datafusion::arrow::datatypes::{DataType, Field, Schema};
    use std::sync::Arc;

    /// Build a `join_table`-shaped batch from `(when, entity_id, user_id)`.
    fn joins(rows: &[(&str, &str, &str)]) -> RecordBatch {
        let schema = Schema::new(vec![
            Field::new("when", DataType::Utf8, false),
            Field::new("entity_id", DataType::Utf8, false),
            Field::new("user_id", DataType::Utf8, false),
        ]);

        let column = |values: Vec<&str>| Arc::new(StringArray::from(values)) as Arc<dyn Array>;

        RecordBatch::try_new(
            Arc::new(schema),
            vec![
                column(rows.iter().map(|row| row.0).collect()),
                column(rows.iter().map(|row| row.1).collect()),
                column(rows.iter().map(|row| row.2).collect()),
            ],
        )
        .expect("batch should build")
    }

    const JOIN_REPAIR: Repair = Repair {
        table: "join_table",
        key: &["when", "entity_id", "user_id"],
        scope: None,
    };

    fn kept(batch: &RecordBatch, repair: &Repair, seen: &mut HashSet<String>) -> Vec<bool> {
        keep_mask(batch, repair, seen)
            .expect("mask should build")
            .iter()
            .map(|value| value.unwrap_or(false))
            .collect()
    }

    #[test]
    fn one_event_delivered_many_times_keeps_its_first_copy() {
        let batch = joins(&[
            ("2025-06-16T18:43:46.647969Z", "project", "person"),
            ("2025-06-16T18:43:46.647969Z", "project", "person"),
            ("2025-06-16T18:43:46.647969Z", "project", "person"),
        ]);

        let mut seen = HashSet::new();
        assert_eq!(kept(&batch, &JOIN_REPAIR, &mut seen), [true, false, false]);
    }

    /// The property that makes this safe after the orrery cutover: a person
    /// who reaches the same project again by another route at a later moment
    /// has joined twice, and both events belong in the log.
    #[test]
    fn the_same_person_joining_the_same_project_again_later_is_two_events() {
        let batch = joins(&[
            ("2025-06-16T18:43:46.647969Z", "project", "person"),
            ("2025-09-02T09:15:00.000001Z", "project", "person"),
        ]);

        let mut seen = HashSet::new();
        assert_eq!(kept(&batch, &JOIN_REPAIR, &mut seen), [true, true]);
    }

    /// Copies of one event are spread across the files for the days they were
    /// delivered on, so the set of seen events has to outlive a single batch.
    #[test]
    fn duplicates_are_found_across_batches() {
        let first = joins(&[("2025-06-16T18:43:46.647969Z", "project", "person")]);
        let second = joins(&[("2025-06-16T18:43:46.647969Z", "project", "person")]);

        let mut seen = HashSet::new();
        assert_eq!(kept(&first, &JOIN_REPAIR, &mut seen), [true]);
        assert_eq!(kept(&second, &JOIN_REPAIR, &mut seen), [false]);
    }

    /// Fields are separated, so a key cannot be forged by shifting a boundary
    /// between two adjacent columns.
    #[test]
    fn adjacent_fields_do_not_run_together_into_one_key() {
        let batch = joins(&[
            ("2025-06-16T18:43:46.647969Z", "ab", "c"),
            ("2025-06-16T18:43:46.647969Z", "a", "bc"),
        ]);

        let mut seen = HashSet::new();
        assert_eq!(kept(&batch, &JOIN_REPAIR, &mut seen), [true, true]);
    }

    #[test]
    fn rows_outside_the_scope_are_never_touched() {
        let repair = Repair {
            table: "clickthroughs",
            key: &["when"],
            scope: Some(("link_type", "join")),
        };

        let schema = Schema::new(vec![
            Field::new("when", DataType::Utf8, false),
            Field::new("link_type", DataType::Utf8, false),
        ]);

        let batch = RecordBatch::try_new(
            Arc::new(schema),
            vec![
                Arc::new(StringArray::from(vec!["t", "t", "t"])) as Arc<dyn Array>,
                Arc::new(StringArray::from(vec!["golink", "golink", "join"])) as Arc<dyn Array>,
            ],
        )
        .expect("batch should build");

        // Both golink rows survive despite sharing a timestamp; the join row
        // is the only one the repair judges.
        let mut seen = HashSet::new();
        assert_eq!(kept(&batch, &repair, &mut seen), [true, true, true]);
    }

    #[test]
    fn a_missing_field_does_not_collide_with_a_present_one() {
        let repair = Repair {
            table: "clickthroughs",
            key: &["when", "user_id", "program_id"],
            scope: None,
        };

        let anonymous = serde_json::json!({"when": "t", "program_id": "x"});
        let identified = serde_json::json!({"when": "t", "user_id": "x"});

        let mut seen = HashSet::new();
        assert!(keep_json_row(&anonymous, &repair, &mut seen));
        assert!(keep_json_row(&identified, &repair, &mut seen));
    }

    #[test]
    fn the_json_and_arrow_paths_agree_on_what_is_a_duplicate() {
        let batch = joins(&[("2025-06-16T18:43:46.647969Z", "project", "person")]);
        let row = serde_json::json!({
            "when": "2025-06-16T18:43:46.647969Z",
            "entity_id": "project",
            "user_id": "person",
        });

        let mut seen = HashSet::new();
        assert_eq!(kept(&batch, &JOIN_REPAIR, &mut seen), [true]);
        assert!(!keep_json_row(&row, &JOIN_REPAIR, &mut seen));
    }
}
