//! One-shot repair that brings the participation log in line with orrery and
//! starts the participation ledger.
//!
//! The importer used to take each of orrery's events once, at whatever it held
//! then, so the log missed every repetition folded into an event afterwards,
//! kept every event orrery since deleted or rewrote, and lost a week of
//! participation behind every event dated in the future. It now follows
//! every write to every event, and reconciles each with a ledger of what the
//! log holds for it (see `participation_ledger`). This builds that ledger
//! from the log as it stands, and clears out what nothing in orrery accounts
//! for, so that the importer can then walk orrery's whole history and leave
//! the log holding exactly what orrery does.
//!
//! In order, over every row of the participation log:
//!
//! - The word "null" in a string column becomes NULL, in every table: the
//!   compactor used to spell a JSON null out.
//! - Rows the iNaturalist bridge credited to "inaturalist", which is no
//!   project's id, are removed; the bridge walks every account again and
//!   credits each observation to the projects it was recorded in.
//! - Rows the Zooniverse bridge logged under origin 10, which orrery has
//!   since given to Foundations, move to origin 5 with the rest of them.
//! - Each of orrery's events is matched with the rows logged for it -- every
//!   column alike, the origin aside, which orrery renumbered -- and the ledger
//!   is seeded with what each one holds. Rows beyond an event's repetitions,
//!   and rows orrery spells the time of but has no event for, were imported
//!   from an event orrery has since rewritten or deleted, and are removed.
//! - Each Zooniverse project is given the start of the era in which the
//!   bridge credited every account (see `BridgeCoverage`).
//! - The importer's cursor is set to read orrery's whole history.
//!
//! The rows the bridges log, and rows hydra recorded itself -- whose times
//! are spelled to the nanosecond, never as orrery spells them -- are left
//! alone. Whatever the ledger then finds different, the importer settles
//! through the ledger as it reads: rows a bridge is now the importer of
//! record for, events dated in the future, and the origins orrery renumbered
//! are taken back and logged again.
//!
//! Run it in two steps. The first reads orrery's feed into a snapshot, and
//! can run beside the service; the second needs the service stopped, since
//! it rewrites the log's files:
//!
//! ```text
//! cd log-service && direnv exec . cargo run --release --bin participation-repair -- fetch SNAPSHOT
//! # stop the log service
//! cd log-service && direnv exec . cargo run --release --bin participation-repair -- apply SNAPSHOT --dry-run
//! cd log-service && direnv exec . cargo run --release --bin participation-repair -- apply SNAPSHOT
//! # start the log service
//! ```
//!
//! An interrupted `apply` can be run again: the rows it already removed are
//! no longer there to be counted, and what remains is classified the same
//! way. Once it has seeded the ledger it refuses to run again.

use anyhow::{Context, Result, bail};
use datafusion::arrow::array::{Array, ArrayRef, RecordBatch, StringArray};
use datafusion::arrow::datatypes::{DataType, Fields, Schema};
use datafusion::arrow::record_batch::RecordBatchReader;
use jiff::Timestamp;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::File;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;

use common::linked_platform::RETROACTIVE_IMPORT_CUTOVER;
use log_service::database::Database;
use log_service::database::rewrite::{
    self, DEFAULT_ROW_GROUP_SIZE, ParquetReplacement, batch_from_rows, rows_from_batch,
};
use log_service::database::table::{TableRow, get_registered_tables};
use log_service::database::tables::ParticipationRow;
use log_service::participation_feed::{self, FeedCursor, FeedItem};
use log_service::participation_ledger::{self, Entry, Shape};

/// The origin both bridges log under, and the one orrery files their
/// platforms' participation under.
const IMPORT_ORIGIN: i16 = 5;

/// The origin the Zooniverse bridge logged under until orrery gave it to
/// Foundations.
const FORMER_ZOONIVERSE_ORIGIN: i16 = 10;

/// What the iNaturalist bridge used to credit every observation to.
const MISCREDITED_INATURALIST: &str = "inaturalist";

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["fetch", snapshot] => fetch(Path::new(snapshot)).await,
        ["apply", snapshot] => apply(Path::new(snapshot), false).await,
        ["apply", snapshot, "--dry-run"] | ["apply", "--dry-run", snapshot] => {
            apply(Path::new(snapshot), true).await
        }
        _ => bail!("usage: participation-repair fetch SNAPSHOT | apply SNAPSHOT [--dry-run]"),
    }
}

/// Read every event orrery holds into `snapshot`, one per line.
async fn fetch(snapshot: &Path) -> Result<()> {
    let client = reqwest::Client::new();
    let partial = snapshot.with_extension("partial");
    let mut out = BufWriter::new(File::create(&partial)?);
    let mut after = 0;
    let mut events = 0usize;

    loop {
        let page = participation_feed::fetch_page(&client, after).await?;

        for item in &page {
            // A tombstone is an event orrery no longer holds, which is what
            // leaving it out of the snapshot says.
            if item.event.is_some() {
                serde_json::to_writer(&mut out, item)?;
                out.write_all(b"\n")?;
                events += 1;
            }

            after = item.revision;
        }

        if page.len() < participation_feed::PAGE_SIZE {
            break;
        }

        if events % 100_000 < participation_feed::PAGE_SIZE {
            println!("{events} events read, through revision {after}");
        }
    }

    out.flush()?;
    drop(out);
    std::fs::rename(&partial, snapshot)?;

    println!("{events} events read into {}", snapshot.display());
    Ok(())
}

/// One of orrery's events, as matching needs it.
#[derive(Debug, Clone, Copy)]
struct Event {
    source_id: i32,
    repetitions: u32,
    shape: u128,
    /// Which line of the snapshot holds it.
    line: usize,
}

/// The rows of the log an event is matched with: copies of its own row,
/// with this origin and this spelling of its time.
#[derive(Debug, Clone, PartialEq)]
struct Assignment {
    origin: i16,
    when: String,
    logged: u32,
}

/// A distinct row the log holds for some shape: shapes ignore the origin and
/// how the time is spelled, rows do not.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct Variant {
    shape: u128,
    origin: i16,
    when: String,
}

/// Every event in the snapshot, by id. An event the feed moved past while it
/// was being read appears more than once, and its latest revision is the one
/// that counts.
fn read_snapshot(snapshot: &Path) -> Result<HashMap<i32, Event>> {
    let mut events = HashMap::new();

    for (line, text) in BufReader::new(File::open(snapshot)?).lines().enumerate() {
        let item: FeedItem = serde_json::from_str(&text?)?;
        let Some(event) = &item.event else { continue };

        let row = participation_ledger::row_of(event);

        events.insert(
            item.id,
            Event {
                source_id: item.id,
                repetitions: event.repetitions.max(0) as u32,
                shape: shape_of(&row),
                line,
            },
        );
    }

    Ok(events)
}

/// A digest of a row's [`Shape`], wide enough that no two of the log's
/// shapes share one.
fn shape_of(row: &ParticipationRow) -> u128 {
    let shape = Shape::of(row);
    let half = |salt: u8| {
        let mut hasher = DefaultHasher::new();
        salt.hash(&mut hasher);
        shape.hash(&mut hasher);
        hasher.finish()
    };

    (u128::from(half(0)) << 64) | u128::from(half(1))
}

/// Whether a row's time is spelled the way orrery's feed spells times: UTC,
/// to the second, millisecond or microsecond. hydra stamps what it records
/// itself to the nanosecond.
fn spelled_by_orrery(when: &str) -> bool {
    let fraction_digits = match when.len() {
        20 => 0,
        24 => 3,
        27 => 6,
        _ => return false,
    };

    when.ends_with('Z')
        && Timestamp::from_str(when).is_ok()
        && (fraction_digits == 0 || when.as_bytes()[19] == b'.')
}

/// A bridge's row: neither bridge records a task, and every event orrery
/// files under the bridges' origins has one.
fn logged_by_a_bridge(row: &ParticipationRow) -> bool {
    row.task_name.is_none() && matches!(row.origin, IMPORT_ORIGIN | FORMER_ZOONIVERSE_ORIGIN)
}

/// The compactor spelled a JSON null out; it means no value.
fn unspell_null(value: &mut Option<String>) -> bool {
    if value.as_deref() == Some("null") {
        *value = None;
        true
    } else {
        false
    }
}

/// What happens to one row of the participation log.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Fate {
    /// A bridge's row, renumbered if it carries the Zooniverse bridge's old
    /// origin.
    Bridged {
        renumbered: bool,
    },
    Miscredited,
    /// Logged for one of orrery's events.
    Imported(Variant),
    /// Imported from an event orrery no longer holds as it was.
    Stale {
        origin: i16,
        year: i16,
    },
    /// Recorded by hydra itself.
    Native,
}

#[derive(Debug, Default)]
struct Census {
    read: u64,
    nulls: u64,
    miscredited: u64,
    bridged: u64,
    renumbered: u64,
    imported: u64,
    surplus: u64,
    stale: BTreeMap<(i16, i16), u64>,
    native: u64,
}

impl Census {
    fn report(&self) {
        println!("{} participation rows read", self.read);
        println!("  {} \"null\" strings that become NULL", self.nulls);
        println!(
            "  {} credited to {MISCREDITED_INATURALIST:?}, removed",
            self.miscredited
        );
        println!(
            "  {} logged by a bridge, kept ({} moved from origin {FORMER_ZOONIVERSE_ORIGIN} to {IMPORT_ORIGIN})",
            self.bridged, self.renumbered
        );
        println!("  {} imported from orrery's events, kept", self.imported);
        println!(
            "  {} beyond their event's repetitions, removed",
            self.surplus
        );
        println!(
            "  {} imported from events orrery no longer holds as they were, removed:",
            self.stale.values().sum::<u64>()
        );
        for ((origin, year), rows) in &self.stale {
            println!("      origin {origin}, {year}: {rows}");
        }
        println!("  {} recorded by hydra itself, kept", self.native);
    }
}

/// Classifies rows, knowing which of orrery's events each could be.
struct Classifier {
    shapes: HashSet<u128>,
    zooniverse_projects: Vec<String>,
}

impl Classifier {
    /// Fix the row's spelled-out nulls and its origin, and say what becomes
    /// of it.
    fn classify(&self, row: &mut ParticipationRow, census: &mut Census) -> Fate {
        census.read += 1;

        for value in [&mut row.where_point, &mut row.user_id, &mut row.task_name] {
            if unspell_null(value) {
                census.nulls += 1;
            }
        }

        if row.entity_id == MISCREDITED_INATURALIST {
            return Fate::Miscredited;
        }

        if logged_by_a_bridge(row) {
            let renumbered = row.origin == FORMER_ZOONIVERSE_ORIGIN;
            if renumbered {
                row.origin = IMPORT_ORIGIN;
            }
            return Fate::Bridged { renumbered };
        }

        let shape = shape_of(row);

        if self.shapes.contains(&shape) {
            return Fate::Imported(Variant {
                shape,
                origin: row.origin,
                when: row.when.clone(),
            });
        }

        if spelled_by_orrery(&row.when) {
            let year = Timestamp::from_str(&row.when)
                .map(|when| when.to_zoned(jiff::tz::TimeZone::UTC).year())
                .unwrap_or_default();

            return Fate::Stale {
                origin: row.origin,
                year,
            };
        }

        Fate::Native
    }

    fn is_zooniverse(&self, entity_id: &str) -> bool {
        self.zooniverse_projects
            .iter()
            .any(|project| project == entity_id)
    }
}

/// What the read-only pass learns.
#[derive(Debug, Default)]
struct Survey {
    /// How many copies of each distinct imported row the log holds.
    variants: HashMap<Variant, u64>,
    /// The earliest row the Zooniverse bridge logged for each project before
    /// the import policy.
    credited_since: HashMap<String, Timestamp>,
}

async fn apply(snapshot: &Path, dry_run: bool) -> Result<()> {
    common::shared::init("log_cache").await;
    sqlx::migrate!("./migrations")
        .run(common::shared::db())
        .await?;

    #[cfg(not(debug_assertions))]
    let database_path = env!("DATABASE_PATH");
    #[cfg(debug_assertions)]
    let database_path = env!("LOCAL_DATABASE_PATH");

    let base = PathBuf::from(database_path);
    let database = Database::new(&base).await?;

    // The cursor is set last, so it is what says the repair has finished.
    if FeedCursor::load(&database).await?.is_some() {
        bail!("the participation importer already has a cursor; this repair has run");
    }

    if dry_run {
        println!("Dry run: counting only, nothing will be written.\n");
    }

    let events = read_snapshot(snapshot).context("reading the snapshot")?;
    println!("{} events in the snapshot", events.len());

    let zooniverse_projects = sqlx::query_scalar!(
        r#"SELECT DISTINCT entity_id AS "entity_id!" FROM zooniverse_projects"#
    )
    .fetch_all(common::shared::db())
    .await?;

    let classifier = Classifier {
        shapes: events.values().map(|event| event.shape).collect(),
        zooniverse_projects,
    };

    let survey = tokio::task::spawn_blocking({
        let base = base.clone();
        move || survey(&base, &classifier).map(|survey| (survey, classifier))
    })
    .await??;
    let (survey, classifier) = survey;

    let (assignments, keep) = assign(&events, &survey.variants);
    let claimed: u64 = assignments
        .values()
        .map(|assignment| u64::from(assignment.logged))
        .sum();
    println!(
        "{} events matched to {claimed} logged rows",
        assignments.len()
    );

    for (project, since) in &survey.credited_since {
        println!("Zooniverse project {project} credited to every account since {since}");
    }

    let census = tokio::task::spawn_blocking({
        let base = base.clone();
        move || rewrite_participation(&base, &classifier, keep, dry_run)
    })
    .await??;

    census.report();

    let nulls = tokio::task::spawn_blocking({
        let base = base.clone();
        move || unspell_nulls_elsewhere(&base, dry_run)
    })
    .await??;

    for (table, count) in &nulls {
        println!("{table}: {count} \"null\" strings that become NULL");
    }

    if dry_run {
        return Ok(());
    }

    // A run interrupted while seeding left part of the ledger behind; the
    // files are as this run left them, so seeding starts over.
    sqlx::query!("DELETE FROM participation_sources")
        .execute(common::shared::db())
        .await?;

    seed(snapshot, &events, &assignments).await?;

    for (project, since) in &survey.credited_since {
        let since = since.to_string();
        sqlx::query!(
            r#"UPDATE zooniverse_projects SET credited_since = ? WHERE entity_id = ?"#,
            since,
            project
        )
        .execute(common::shared::db())
        .await?;
    }

    // Last, so that until everything above is done the importer stays idle.
    FeedCursor::starting_after(0).store(&database).await?;

    println!(
        "Ledger seeded with {} events; the importer will read orrery's history from the start.",
        assignments.len()
    );
    Ok(())
}

/// The read-only pass over the log.
fn survey(base: &Path, classifier: &Classifier) -> Result<Survey> {
    let cutover = Timestamp::from_str(RETROACTIVE_IMPORT_CUTOVER)?;
    let mut survey = Survey::default();
    let mut census = Census::default();

    for_each_participation_row(base, |row| {
        let mut row = row.clone();
        let fate = classifier.classify(&mut row, &mut census);

        match fate {
            Fate::Imported(variant) => *survey.variants.entry(variant).or_default() += 1,
            Fate::Bridged { .. } if classifier.is_zooniverse(&row.entity_id) => {
                if let Ok(when) = Timestamp::from_str(&row.when)
                    && when < cutover
                {
                    survey
                        .credited_since
                        .entry(row.entity_id.clone())
                        .and_modify(|since| *since = (*since).min(when))
                        .or_insert(when);
                }
            }
            _ => {}
        }

        Ok(())
    })?;

    Ok(survey)
}

/// Match orrery's events with the rows logged for them.
///
/// Each event takes the most plentiful of the distinct rows logged for its
/// shape, up to its repetitions. Events identical in every column share the
/// rows between them. Returns what each matched event takes, and how many
/// copies of each distinct row to keep.
fn assign(
    events: &HashMap<i32, Event>,
    variants: &HashMap<Variant, u64>,
) -> (HashMap<i32, Assignment>, HashMap<Variant, u64>) {
    let mut by_shape: HashMap<u128, Vec<(&Variant, u64)>> = HashMap::new();
    for (variant, count) in variants {
        by_shape
            .entry(variant.shape)
            .or_default()
            .push((variant, *count));
    }

    let mut ordered: Vec<&Event> = events.values().collect();
    ordered.sort_by_key(|event| event.source_id);

    let mut assignments = HashMap::new();
    let mut keep: HashMap<Variant, u64> = HashMap::new();

    for event in ordered {
        let Some(pool) = by_shape.get_mut(&event.shape) else {
            continue;
        };

        // Most plentiful first, ties broken by the row itself, so that a run
        // after an interruption chooses the same.
        pool.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));

        let Some((variant, remaining)) = pool.first_mut().filter(|(_, remaining)| *remaining > 0)
        else {
            continue;
        };

        let taken = (*remaining).min(u64::from(event.repetitions));
        *remaining -= taken;
        *keep.entry((*variant).clone()).or_default() += taken;

        assignments.insert(
            event.source_id,
            Assignment {
                origin: variant.origin,
                when: variant.when.clone(),
                logged: taken as u32,
            },
        );
    }

    (assignments, keep)
}

/// Seed the ledger: each matched event's own row, with the origin and the
/// spelling of the time the log holds it under, so that the entry names
/// exactly the rows in the log.
async fn seed(
    snapshot: &Path,
    events: &HashMap<i32, Event>,
    assignments: &HashMap<i32, Assignment>,
) -> Result<()> {
    const BATCH: usize = 10_000;
    let mut batch = Vec::with_capacity(BATCH);

    for (line, text) in BufReader::new(File::open(snapshot)?).lines().enumerate() {
        let item: FeedItem = serde_json::from_str(&text?)?;

        // Only the line `read_snapshot` kept for the event.
        if events.get(&item.id).is_none_or(|event| event.line != line) {
            continue;
        }

        let (Some(event), Some(assignment)) = (&item.event, assignments.get(&item.id)) else {
            continue;
        };

        let mut row = participation_ledger::row_of(event);
        row.origin = assignment.origin;
        row.when = assignment.when.clone();

        batch.push((
            item.id,
            Entry {
                row: Some(row),
                logged: assignment.logged,
                covered: false,
            },
        ));

        if batch.len() == BATCH {
            participation_ledger::seed(&batch).await?;
            batch.clear();
        }
    }

    participation_ledger::seed(&batch).await?;
    Ok(())
}

/// The rewriting pass over the participation log.
fn rewrite_participation(
    base: &Path,
    classifier: &Classifier,
    mut keep: HashMap<Variant, u64>,
    dry_run: bool,
) -> Result<Census> {
    let schema = ParticipationRow::schema();
    let mut census = Census::default();

    let mut decide = |row: &mut ParticipationRow, census: &mut Census| -> bool {
        match classifier.classify(row, census) {
            Fate::Bridged { renumbered } => {
                census.bridged += 1;
                census.renumbered += u64::from(renumbered);
                true
            }
            Fate::Miscredited => {
                census.miscredited += 1;
                false
            }
            Fate::Imported(variant) => match keep.get_mut(&variant) {
                Some(left) if *left > 0 => {
                    *left -= 1;
                    census.imported += 1;
                    true
                }
                _ => {
                    census.surplus += 1;
                    false
                }
            },
            Fate::Stale { origin, year } => {
                *census.stale.entry((origin, year)).or_default() += 1;
                false
            }
            Fate::Native => {
                census.native += 1;
                true
            }
        }
    };

    for path in rewrite::parquet_files(base, ParticipationRow::TABLE_NAME)? {
        let mut writer = (!dry_run)
            .then(|| ParquetReplacement::create(&path, &schema, DEFAULT_ROW_GROUP_SIZE))
            .transpose()?;

        for batch in rewrite::read_parquet(&path, None)? {
            let mut kept = Vec::new();

            for mut row in rows_from_batch::<ParticipationRow>(&batch?)? {
                if decide(&mut row, &mut census) {
                    kept.push(row);
                }
            }

            if let Some(writer) = &mut writer
                && !kept.is_empty()
            {
                writer.write(&batch_from_rows(&kept, &schema)?)?;
            }
        }

        if let Some(writer) = writer {
            writer
                .finish()?
                .commit()
                .with_context(|| format!("rewriting {}", path.display()))?;
        }
    }

    for path in rewrite::wal_files(base, ParticipationRow::TABLE_NAME)? {
        let mut kept = Vec::new();

        for line in BufReader::new(File::open(&path)?).lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }

            let mut row: ParticipationRow = serde_json::from_str(&line)
                .with_context(|| format!("reading {}", path.display()))?;

            if decide(&mut row, &mut census) {
                kept.push(serde_json::to_string(&row)?);
            }
        }

        if !dry_run {
            rewrite::replace_wal(&path, kept)?
                .commit()
                .with_context(|| format!("rewriting {}", path.display()))?;
        }
    }

    Ok(census)
}

/// Every row of the participation log, Parquet and WAL alike.
fn for_each_participation_row(
    base: &Path,
    mut visit: impl FnMut(&ParticipationRow) -> Result<()>,
) -> Result<()> {
    for path in rewrite::parquet_files(base, ParticipationRow::TABLE_NAME)? {
        for batch in rewrite::read_parquet(&path, None)? {
            for row in rows_from_batch::<ParticipationRow>(&batch?)? {
                visit(&row)?;
            }
        }
    }

    for path in rewrite::wal_files(base, ParticipationRow::TABLE_NAME)? {
        for line in BufReader::new(File::open(&path)?).lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }

            let row: ParticipationRow = serde_json::from_str(&line)
                .with_context(|| format!("reading {}", path.display()))?;
            visit(&row)?;
        }
    }

    Ok(())
}

/// Make the word "null" NULL in every other table's Parquet. The WAL never
/// had the problem: it holds JSON, where a null is a null.
fn unspell_nulls_elsewhere(base: &Path, dry_run: bool) -> Result<BTreeMap<&'static str, u64>> {
    let mut counts = BTreeMap::new();

    for registration in get_registered_tables() {
        if registration.name == ParticipationRow::TABLE_NAME {
            continue;
        }

        let registered = (registration.schema)();
        let nullable: Vec<&str> = registered
            .fields()
            .iter()
            .filter(|field| field.is_nullable() && *field.data_type() == DataType::Utf8)
            .map(|field| field.name().as_str())
            .collect();

        if nullable.is_empty() {
            continue;
        }

        let mut table_count = 0u64;

        for path in rewrite::parquet_files(base, registration.name)? {
            let mut file_count = 0u64;
            for batch in rewrite::read_parquet(&path, Some(&nullable))? {
                let batch = batch?;
                for name in &nullable {
                    file_count += spelled_nulls(&batch, name)?;
                }
            }

            table_count += file_count;

            if dry_run || file_count == 0 {
                continue;
            }

            let batches = rewrite::read_parquet(&path, None)?;
            let schema = unspelled_schema(&batches.schema(), &nullable);
            let mut writer = ParquetReplacement::create(&path, &schema, DEFAULT_ROW_GROUP_SIZE)?;
            for batch in batches {
                writer.write(&unspell_batch(&batch?, &schema, &nullable)?)?;
            }
            writer
                .finish()?
                .commit()
                .with_context(|| format!("rewriting {}", path.display()))?;
        }

        if table_count > 0 {
            counts.insert(registration.name, table_count);
        }
    }

    Ok(counts)
}

fn spelled_nulls(batch: &RecordBatch, name: &str) -> Result<u64> {
    let Some(column) = batch.column_by_name(name) else {
        return Ok(0);
    };

    let values = column
        .as_any()
        .downcast_ref::<StringArray>()
        .with_context(|| format!("column {name} is not a string column"))?;

    Ok(values.iter().filter(|value| *value == Some("null")).count() as u64)
}

/// The schema a file is rewritten with: its own, since a file compacted
/// before a column was added to its table has no such column, with each
/// column that is unspelled able to hold the NULLs it now gets.
fn unspelled_schema(file: &Schema, nullable: &[&str]) -> Schema {
    let fields: Fields = file
        .fields()
        .iter()
        .map(|field| {
            if nullable.contains(&field.name().as_str()) {
                Arc::new(field.as_ref().clone().with_nullable(true))
            } else {
                field.clone()
            }
        })
        .collect();

    Schema::new_with_metadata(fields, file.metadata().clone())
}

/// `batch`, read from a file whose rewrite is written with `schema` (see
/// [`unspelled_schema`]), with the word "null" made NULL in each `nullable`
/// column.
fn unspell_batch(batch: &RecordBatch, schema: &Schema, nullable: &[&str]) -> Result<RecordBatch> {
    let columns = schema
        .fields()
        .iter()
        .map(|field| -> Result<ArrayRef> {
            let column = batch
                .column_by_name(field.name())
                .with_context(|| format!("column {} is missing", field.name()))?;

            if !nullable.contains(&field.name().as_str()) {
                return Ok(column.clone());
            }

            let values = column
                .as_any()
                .downcast_ref::<StringArray>()
                .with_context(|| format!("column {} is not a string column", field.name()))?;

            Ok(Arc::new(
                values
                    .iter()
                    .map(|value| value.filter(|value| *value != "null"))
                    .collect::<StringArray>(),
            ))
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(RecordBatch::try_new(Arc::new(schema.clone()), columns)?)
}

#[cfg(test)]
mod tests {
    use assert2::check;

    use super::*;

    fn row(origin: i16, when: &str, task: Option<&str>) -> ParticipationRow {
        ParticipationRow {
            origin,
            when: when.to_string(),
            duration: 1.0,
            where_point: None,
            magnitude: 1,
            authenticated: true,
            extra: String::new(),
            fidelity: 0.5,
            entity_id: "01JKXMGGMD577ZA8Q57S2GY1V8".to_string(),
            user_id: Some("01JKXMFYFZ0000000000LINKED".to_string()),
            task_name: task.map(str::to_string),
        }
    }

    fn classifier(events: &[&ParticipationRow]) -> Classifier {
        Classifier {
            shapes: events.iter().map(|row| shape_of(row)).collect(),
            zooniverse_projects: vec![],
        }
    }

    #[test]
    fn orrerys_spellings_are_recognized_and_hydras_are_not() {
        check!(spelled_by_orrery("2017-06-20T00:00:00Z"));
        check!(spelled_by_orrery("2021-11-15T11:38:52.680Z"));
        check!(spelled_by_orrery("2019-10-04T22:37:25.853055Z"));
        check!(!spelled_by_orrery("2026-04-13T11:59:12.854313355Z"));
        check!(!spelled_by_orrery("2011-06-01T21:20:46-05:00"));
    }

    #[test]
    fn a_row_matches_its_event_whatever_its_origin_and_spelling() {
        let event = row(5, "2021-11-15T11:38:52.680Z", Some("Data collection"));
        let logged = row(10, "2021-11-15T11:38:52.680000Z", Some("Data collection"));

        let mut census = Census::default();
        let mut logged_copy = logged.clone();
        let fate = classifier(&[&event]).classify(&mut logged_copy, &mut census);

        check!(
            fate == Fate::Imported(Variant {
                shape: shape_of(&event),
                origin: 10,
                when: "2021-11-15T11:38:52.680000Z".to_string(),
            })
        );
    }

    #[test]
    fn a_bridge_row_is_kept_and_its_old_origin_corrected() {
        let mut bridged = row(10, "2026-03-11T07:42:10.508Z", Some("null"));
        let mut census = Census::default();

        let fate = classifier(&[]).classify(&mut bridged, &mut census);

        check!(fate == Fate::Bridged { renumbered: true });
        check!(bridged.origin == IMPORT_ORIGIN);
        check!(bridged.task_name == None);
        check!(census.nulls == 1);
    }

    #[test]
    fn an_unmatched_import_is_stale_and_a_native_row_is_not() {
        let mut stale = row(3, "2027-04-02T12:00:00Z", Some("Data collection"));
        let mut native = row(0, "2026-04-13T11:59:12.854313355Z", Some("null"));
        let mut census = Census::default();

        check!(
            classifier(&[]).classify(&mut stale, &mut census)
                == Fate::Stale {
                    origin: 3,
                    year: 2027
                }
        );
        check!(classifier(&[]).classify(&mut native, &mut census) == Fate::Native);
    }

    #[test]
    fn each_event_takes_its_repetitions_and_the_surplus_is_left() {
        let event = row(3, "2021-11-15T11:38:52.680Z", Some("Data collection"));
        let shape = shape_of(&event);

        let events = HashMap::from([(
            7,
            Event {
                source_id: 7,
                repetitions: 3,
                shape,
                line: 0,
            },
        )]);

        let majority = Variant {
            shape,
            origin: 3,
            when: event.when.clone(),
        };
        let minority = Variant {
            shape,
            origin: 10,
            when: event.when.clone(),
        };

        let variants = HashMap::from([(majority.clone(), 5), (minority.clone(), 1)]);
        let (assignments, keep) = assign(&events, &variants);

        check!(
            assignments.get(&7)
                == Some(&Assignment {
                    origin: 3,
                    when: event.when.clone(),
                    logged: 3,
                })
        );
        check!(keep.get(&majority) == Some(&3));
        check!(keep.get(&minority) == None);
    }

    #[test]
    fn an_event_logged_short_takes_what_there_is() {
        let event = row(3, "2021-11-15T11:38:52.680Z", Some("Data collection"));
        let shape = shape_of(&event);

        let events = HashMap::from([(
            7,
            Event {
                source_id: 7,
                repetitions: 310,
                shape,
                line: 0,
            },
        )]);

        let variant = Variant {
            shape,
            origin: 3,
            when: event.when.clone(),
        };

        let (assignments, keep) = assign(&events, &HashMap::from([(variant.clone(), 34)]));

        check!(assignments[&7].logged == 34);
        check!(keep.get(&variant) == Some(&34));
    }

    #[test]
    fn a_file_compacted_before_a_column_was_added_keeps_its_own_columns() {
        use log_service::database::tables::SearchEventRow;

        let dir = tempfile::tempdir().unwrap();
        let table = dir.path().join("parquet").join(SearchEventRow::TABLE_NAME);
        std::fs::create_dir_all(&table).unwrap();
        let path = table.join("search_events_2025-01-01.parquet");

        let added_since = ["kinds", "show_past", "event_start", "event_end"];
        let current = SearchEventRow::schema();
        let older = Schema::new(
            current
                .fields()
                .iter()
                .filter(|field| !added_since.contains(&field.name().as_str()))
                .cloned()
                .collect::<Fields>(),
        );

        let rows = [serde_json::json!({
            "when": "2025-01-01T12:00:00.123456789Z",
            "device_id": "device",
            "interaction_id": "interaction",
            "query_text": "null",
            "location_name": "Bristol",
            "show_completed": false,
            "page": 1,
            "result_count": 3,
            "search_type": "projects",
        })];

        let mut writer = ParquetReplacement::create(&path, &older, DEFAULT_ROW_GROUP_SIZE).unwrap();
        writer
            .write(&batch_from_rows(&rows, &older).unwrap())
            .unwrap();
        writer.finish().unwrap().commit().unwrap();

        let counts = unspell_nulls_elsewhere(dir.path(), false).unwrap();
        check!(counts.get(SearchEventRow::TABLE_NAME) == Some(&1));

        let batches = rewrite::read_parquet(&path, None).unwrap();
        check!(batches.schema().field_with_name("kinds").is_err());

        let back: Vec<serde_json::Value> = batches
            .map(|batch| rows_from_batch::<serde_json::Value>(&batch.unwrap()).unwrap())
            .flatten()
            .collect();

        check!(back.len() == 1);
        check!(
            back[0]
                .get("query_text")
                .is_none_or(|value| value.is_null())
        );
        check!(back[0]["location_name"] == "Bristol");
        check!(back[0]["search_type"] == "projects");
    }
}
