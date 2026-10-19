//! Replacing the log's files with edited copies.
//!
//! The log is append-only as it is written, but not everything about it can
//! be: the compactor turns each day's WAL into Parquet, repairs remove rows
//! that should never have been written, and participation that orrery takes
//! back is retracted. All of them replace a file the same way, here, so that
//! there is one place that decides what a replacement looks like and how it
//! becomes visible.
//!
//! A replacement is written beside its target under a name no reader lists,
//! made durable, and then renamed over the target. A reader listing the
//! directory sees the old file or the new one, never neither and never both.
//! A query already reading the old file when the rename lands can fail --
//! the object store reopens a file by path for each range it reads -- but it
//! fails rather than miscounting, and only for a replacement it overlapped.

use datafusion::arrow::array::RecordBatch;
use datafusion::arrow::datatypes::Schema;
use datafusion::arrow::json::ArrayWriter;
use datafusion::parquet::arrow::ArrowWriter;
use datafusion::parquet::arrow::arrow_reader::{
    ParquetRecordBatchReader, ParquetRecordBatchReaderBuilder,
};
use datafusion::parquet::basic::{Compression, ZstdLevel};
use datafusion::parquet::file::properties::WriterProperties;
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::database::compactor::json_records_to_batch;
use crate::database::table::get_registered_tables;
use crate::error::{LogServiceError, Result};

/// Rows per Parquet row group, unless the compactor is configured otherwise.
pub const DEFAULT_ROW_GROUP_SIZE: usize = 10_000;

/// The suffix of a replacement still being written. Neither listing table
/// matches it, so a reader never meets a half-written file.
const REPLACING: &str = "replacing";

/// The registered Arrow schema for a table, so a replacement keeps the shape
/// the compactor gives the table's files.
pub fn schema_for(table: &str) -> Result<Schema> {
    get_registered_tables()
        .into_iter()
        .find(|registration| registration.name == table)
        .map(|registration| (registration.schema)())
        .ok_or_else(|| LogServiceError::Config(format!("no registered table {table}")))
}

/// A table's Parquet files, oldest first.
pub fn parquet_files(base: &Path, table: &str) -> Result<Vec<PathBuf>> {
    sorted_files(&base.join("parquet").join(table), "parquet")
}

/// A table's WAL files, oldest first.
pub fn wal_files(base: &Path, table: &str) -> Result<Vec<PathBuf>> {
    sorted_files(&base.join("wal").join(table), "jsonl")
}

/// Every file of one kind in a directory, oldest first. Names carry the date
/// (`join_table_2025-06-16.parquet`), so sorting them sorts by day.
fn sorted_files(dir: &Path, extension: &str) -> Result<Vec<PathBuf>> {
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == extension))
        .collect();

    files.sort();
    Ok(files)
}

/// Remove replacements left half-written by an interrupted run.
///
/// Only an unfinished replacement is left under the replacing name: a
/// finished one is renamed in the same step that makes it count. Anything
/// still journaled for a rename has to be finished before this runs; see
/// [`crate::database::retraction`].
pub fn discard_unfinished(dir: &Path) -> Result<()> {
    if !dir.exists() {
        return Ok(());
    }

    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();

        if path.extension().is_some_and(|ext| ext == REPLACING) {
            std::fs::remove_file(&path)?;
        }
    }

    Ok(())
}

/// Every batch of a Parquet file, reading only the columns in `projection`
/// when one is given.
pub fn read_parquet(path: &Path, projection: Option<&[&str]>) -> Result<ParquetRecordBatchReader> {
    let builder = ParquetRecordBatchReaderBuilder::try_new(File::open(path)?)?;

    let builder = match projection {
        Some(columns) => {
            let mask = datafusion::parquet::arrow::ProjectionMask::columns(
                builder.parquet_schema(),
                columns.iter().copied(),
            );
            builder.with_projection(mask)
        }
        None => builder,
    };

    Ok(builder.build()?)
}

/// A batch as the rows it was built from.
///
/// The way back from Arrow goes through JSON, as the way in does in the
/// compactor, so a row read here is the row that was written: an absent
/// value comes back as `None`, not as a default.
pub fn rows_from_batch<T: DeserializeOwned>(batch: &RecordBatch) -> Result<Vec<T>> {
    let mut writer = ArrayWriter::new(Vec::new());
    writer.write(batch)?;
    writer.finish()?;

    let json = writer.into_inner();

    if json.is_empty() {
        return Ok(Vec::new());
    }

    Ok(serde_json::from_slice(&json)?)
}

/// Rows as a batch, built exactly as the compactor builds one from the WAL.
pub fn batch_from_rows<T: Serialize>(rows: &[T], schema: &Schema) -> Result<RecordBatch> {
    let records = rows
        .iter()
        .map(serde_json::to_value)
        .collect::<std::result::Result<Vec<_>, _>>()?;

    json_records_to_batch(&records, schema)
}

/// A finished, durable replacement for `target`, not yet in its place.
#[must_use = "a replacement does nothing until it is committed"]
pub struct Replacement {
    temporary: PathBuf,
    target: PathBuf,
}

impl Replacement {
    /// Where the replacement is waiting.
    pub fn temporary(&self) -> &Path {
        &self.temporary
    }

    pub fn target(&self) -> &Path {
        &self.target
    }

    /// Put the replacement in its target's place.
    pub fn commit(self) -> Result<()> {
        move_into_place(&self.temporary, &self.target)
    }

    /// Throw the replacement away, leaving the target as it was.
    pub fn abandon(self) -> Result<()> {
        std::fs::remove_file(&self.temporary)?;
        Ok(())
    }
}

/// Rename `from` over `to` and make the rename durable.
///
/// Also how an interrupted commit is finished: [`Replacement`] only names
/// two paths, so a journal holding them can complete the rename later.
pub fn move_into_place(from: &Path, to: &Path) -> Result<()> {
    std::fs::rename(from, to)?;

    if let Some(dir) = to.parent() {
        File::open(dir)?.sync_all()?;
    }

    Ok(())
}

/// A Parquet replacement being written a batch at a time.
pub struct ParquetReplacement {
    writer: ArrowWriter<File>,
    temporary: PathBuf,
    target: PathBuf,
}

impl ParquetReplacement {
    pub fn create(target: &Path, schema: &Schema, row_group_size: usize) -> Result<Self> {
        let temporary = replacing_path(target);

        let props = WriterProperties::builder()
            .set_compression(Compression::ZSTD(ZstdLevel::try_new(3)?))
            .set_max_row_group_size(row_group_size)
            .build();

        let writer = ArrowWriter::try_new(
            File::create(&temporary)?,
            Arc::new(schema.clone()),
            Some(props),
        )?;

        Ok(ParquetReplacement {
            writer,
            temporary,
            target: target.to_path_buf(),
        })
    }

    pub fn write(&mut self, batch: &RecordBatch) -> Result<()> {
        self.writer.write(batch)?;
        Ok(())
    }

    /// Finish the file and make it durable, ready to commit.
    pub fn finish(mut self) -> Result<Replacement> {
        self.writer.finish()?;
        self.writer.inner().sync_all()?;

        Ok(Replacement {
            temporary: self.temporary,
            target: self.target,
        })
    }
}

/// A WAL replacement holding `lines`, ready to commit.
pub fn replace_wal(target: &Path, lines: impl IntoIterator<Item = String>) -> Result<Replacement> {
    let temporary = replacing_path(target);

    {
        let file = File::create(&temporary)?;
        let mut writer = BufWriter::new(&file);

        for line in lines {
            writeln!(writer, "{line}")?;
        }

        writer.flush()?;
        drop(writer);
        file.sync_all()?;
    }

    Ok(Replacement {
        temporary,
        target: target.to_path_buf(),
    })
}

fn replacing_path(target: &Path) -> PathBuf {
    let mut name = target.as_os_str().to_owned();
    name.push(".");
    name.push(REPLACING);
    PathBuf::from(name)
}

#[cfg(test)]
mod tests {
    use assert2::check;

    use super::*;
    use crate::database::table::TableRow;
    use crate::database::tables::ParticipationRow;

    fn row(task_name: Option<&str>, user_id: Option<&str>) -> ParticipationRow {
        ParticipationRow {
            origin: 3,
            when: "2026-10-01T12:00:00.123456Z".to_string(),
            duration: 1.0,
            where_point: None,
            magnitude: 1,
            authenticated: true,
            extra: String::new(),
            fidelity: 0.5,
            entity_id: "01JKXMGGMD577ZA8Q57S2GY1V8".to_string(),
            user_id: user_id.map(str::to_string),
            task_name: task_name.map(str::to_string),
        }
    }

    fn json(rows: &[ParticipationRow]) -> Vec<String> {
        rows.iter()
            .map(|row| serde_json::to_string(row).unwrap())
            .collect()
    }

    #[test]
    fn rows_survive_the_trip_through_arrow() {
        let rows = vec![
            row(Some("Data collection"), Some("01JKXMFYFZ0000000000LINKED")),
            row(None, None),
        ];

        let batch = batch_from_rows(&rows, &ParticipationRow::schema()).unwrap();
        let back: Vec<ParticipationRow> = rows_from_batch(&batch).unwrap();

        check!(json(&back) == json(&rows));
    }

    #[test]
    fn a_replacement_takes_its_targets_place_only_when_committed() {
        let dir = tempfile::tempdir().unwrap();
        let table_dir = dir.path().join("parquet").join("participation");
        std::fs::create_dir_all(&table_dir).unwrap();
        let target = table_dir.join("participation_2026-10-01.parquet");
        let schema = ParticipationRow::schema();

        let mut original = ParquetReplacement::create(&target, &schema, 10).unwrap();
        original
            .write(&batch_from_rows(&[row(None, None), row(None, None)], &schema).unwrap())
            .unwrap();
        original.finish().unwrap().commit().unwrap();

        let mut edited = ParquetReplacement::create(&target, &schema, 10).unwrap();
        edited
            .write(&batch_from_rows(&[row(None, None)], &schema).unwrap())
            .unwrap();
        let edited = edited.finish().unwrap();

        let count = |path: &Path| -> usize {
            read_parquet(path, None)
                .unwrap()
                .map(|batch| batch.unwrap().num_rows())
                .sum()
        };

        // Written, but not yet what the table holds, nor listed beside it.
        check!(count(&target) == 2);
        check!(parquet_files(dir.path(), "participation").unwrap() == [target.clone()]);

        edited.commit().unwrap();
        check!(count(&target) == 1);
    }

    #[test]
    fn an_unfinished_replacement_is_discarded_and_its_target_kept() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("participation_2026-10-01.jsonl");
        std::fs::write(&target, "{}\n").unwrap();

        let unfinished = replace_wal(&target, ["{}".to_string(), "{}".to_string()]).unwrap();
        check!(unfinished.temporary().exists());

        discard_unfinished(dir.path()).unwrap();

        check!(!unfinished.temporary().exists());
        check!(std::fs::read_to_string(&target).unwrap() == "{}\n");
    }
}
