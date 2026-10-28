//! One-shot repair for orrery housekeeping that was logged as page updates.
//!
//! Between 2026-10-21 and the small hours of 2026-10-28, orrery moved the
//! `updated` of 94 projects and 3 events for changes that were not updates
//! to them: publishing, linking iNaturalist projects, and clearing old
//! boilerplate out of How to Join. The orrery sync logged each of those as a
//! `project-edit` or `event-edit` at the moved time, so the pages look as if
//! they were updated that week. Orrery now keeps a separate `changed` for
//! such writes, and the sync logs them as housekeeping (see
//! `common::message::log::Housekeeping`); orrery's core migrations 0272
//! (projects) and 0273 (events) put the records' `updated` back. This rewrites the rows the sync already
//! logged to match, changing only their action.
//!
//! A row is reclassified when it was logged by the orrery sync (no user, and
//! an edit description ending "(from orrery)") against one of the records
//! below, within that week. The sync's first sight of a record ("all ...")
//! is kept as the update it records: it is when this side learnt the record.
//!
//! **Stop the log service before running this.** It rewrites the Parquet and
//! WAL files in place, and the service holds the WAL open for append.
//!
//! ```text
//! cd log-service && direnv exec . cargo run --bin reclassify-orrery-edits -- --dry-run
//! cd log-service && direnv exec . cargo run --bin reclassify-orrery-edits
//! ```
//!
//! Running it twice changes nothing the second time.

use anyhow::{Context, Result};
use common::message::log::{Housekeeping, PageChange};
use datafusion::arrow::datatypes::Schema;
use jiff::Timestamp;
use std::collections::HashSet;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use log_service::database::rewrite::{self, DEFAULT_ROW_GROUP_SIZE, ParquetReplacement};
use log_service::database::table::TableRow;
use log_service::database::tables::ActivityLogRow;

/// The projects and events whose `updated` orrery's core migrations 0272
/// and 0273 put back.
const RECORDS: &[&str] = &[
    "01JKXMGGMD577ZA8Q57S2GY1V8",
    "01JKXMGGG4HW0F2YYJQEFKJX1B",
    "01JKXMGG90W0K47C5K75BAK1Y3",
    "01JKXMGG8F8MQFJ5VPQW5144B3",
    "01JKXMGG6C8JH5F2JSGH2E70F6",
    "01JKXMGG0B5HXX1B28MN8T4MDR",
    "01JKXMGFC7FW3YN2WB6XCEG4X1",
    "01JKXMGF5QJXRHE83P66VTAZWB",
    "01JKXMGF3CV76E89CDTJ9JE8RP",
    "01JKXMGF3A56K4ST4TE5QDZPST",
    "01JKXMGF381HS5STCFNN91DSEC",
    "01JKXMGF35Y5HJEM0WMRJY24PF",
    "01JKXMGF322F12XMPB57FQ7JV1",
    "01JKXMGDY7TGCN7FZZ0KVPWHB2",
    "01JKXMGD6MEKZX67BFMGSJXEZV",
    "01JKXMGD5H9401W6ZJ6SM622HK",
    "01JKXMGBY5T2P1MF8S4SA0RPJ3",
    "01JKXMGBJA3D35KVS75PC09BPX",
    "01JKXMGB52T74WGTNAFD829XEY",
    "01JKXMGA5GJMM32VES9BN9CCSF",
    "01JKXMG9BZDSTEK8PJZQRGG267",
    "01JKXMG8XT2YCP8XF7CY5MYBFT",
    "01JKXMG8RAP4CKTFST2T7JGDQ7",
    "01JKXMG8K7ACZTCFMM7DKJR3FE",
    "01JKXMG8K38KYK6CQ8YWS3X850",
    "01JKXMG8ECCM9T544QH0A7P7JV",
    "01JKXMG8E29SX96YVZMYHQ1RJD",
    "01JKXMG7WQVB6PFNYXRPQ1GXE8",
    "01JKXMG6ZTJ8ZE3EMBXQPJHBDR",
    "01JKXMG61K76JRYC1C3W5X57NR",
    "01JKXMG5B13Z1ATTQN82GRD09Q",
    "01JKXMG57J0DJRDSGQTJY3EAAV",
    "01JKXMG5376B99A79R7SXX2WFF",
    "01JKXMG4M0PQDVKA4KX75W5BKV",
    "01JKXMG4KTHQD9KN8BB885AZMW",
    "01JKXMG4F6SZTNJX769PZ40PHD",
    "01JKXMG4EJ4WN8VCZ1SPPC1D2F",
    "01JKXMG4EGNQ333N5D3TZ0SCN8",
    "01JKXMG4C7MCRHGMV9K9Y4DWHH",
    "01JKXMG4BVBDY0N5YKRXYX7VDX",
    "01JKXMG3JGMSZ35B1FT1XJRG06",
    "01JKXMG37YKVHSX1WRZE6RZ81J",
    "01JKXMG331DGKSCYAJA388GEMC",
    "01JKXMG2RQG3EY8KY2R2Q7S91J",
    "01JKXMFZEBG71XKBGEMRTG5XV4",
    "01JKXMFZ005KNT0JPGCAWC17P2",
    "01JKXMFXSAQF3WN5254RP1CQGM",
    "01JKXMFXP9JHWRGE8XCW4K2WGQ",
    "01JKXMFXP63XGWXVWAPT13K9M0",
    "01JKXMFXG11NWC7TW1S3RY85KA",
    "01JKXMFXE5Q7DM6CF4NHKFPZT2",
    "01JKXMFXD3EZ9QJWZWN539BNNC",
    "01JKXMFXCVYGDDBP0CWG8N54CR",
    "01JKXMFXASFPT8XN54S9DJA25M",
    "01JKXMFX8ZMQAB7PXKK0D1PH0A",
    "01JKXMFX5MNTJWNQ2HD34Q38HN",
    "01JKXMFX19RG8NZKNP7868ZK8P",
    "01JKXMFWXMYR4TAV2VJZ3J5AHG",
    "01JKXMFWWKQ0TVWG1ZS0Y2H2RN",
    "01JKXMFWVCVE1P7TRDBK1AEHXF",
    "01JKXMFWRMH0E2QH12C54E80AT",
    "01JKXMFW6J945D3MAVG1YYGWGA",
    "01JKXMFVVAZAZXC86ZJKMR15XF",
    "01JKXMFVSWW6TSSDPRX7ZHGRTM",
    "01JKXMFVPNX23701QTP64M26PD",
    "01JKXMFVDX5QECM30XNNBNEHA0",
    "01JKXMFV1TK2ZS7JH8ZBYJG5TE",
    "01JKXMFSKJ79QMM56RQ2832R29",
    "01JKXMFSCQ6WK2YQ92QKME1EE0",
    "01JKXMFSA1QBS6X6C1S0TQW62J",
    "01JKXMFS3YPDJ06QW8B69E66DE",
    "01JKXMFRGS6R911X6295YQY0H7",
    "01JKXMFRG6GYR1PPDVKHCYY6NS",
    "01JKXMFRBT848SPKCG1QYXHE5B",
    "01JKXMFR8YJH2SFHG0EGKJV9KY",
    "01JKXMFQB700ZMPVWX2WZ5VXQJ",
    "01JKXMFNPFHMYXPSKMT79P6PSJ",
    "01JKXMFNEJ8FH50802N1X9HGMZ",
    "01JKXMFKSVWFRYZVYTJ1W7KKYF",
    "01JKXMFKSSCPT8S8SNFBFRJM47",
    "01JKXMFKSQVA5HW3Y26XCENR2Y",
    "01JQ4RDXBD7JZCS28WHWEGQ5NW",
    "01M5DJGJ6QFDYVVH66A7T2RKV9",
    "01M5DJH3T12BWVAHSMMTEJY1D2",
    "01M5DJHKY0RJ8Q16Z6SC489TSZ",
    "01M5DJJ52XFSCQBC7SNTM6W4CB",
    "01M5DJMBS3GEHE8K9JXANXTFEW",
    "01M5DJMTWJQGMCNFA47TG9WXVP",
    "01M5DJNATFH78PPWTNM05DCWZ7",
    "01M5DJNT2EKWQ1JCZ0RQHT3YF8",
    "01M5DJPD5AC9ERB6D88ACZFN4Y",
    "01M5DJQ11DBH9B4XMWSKF5548B",
    "01M5DJQKJTMGKBMZM5MDM887Q6",
    "01M5DJRP1KNXDR4TB6VSXMAEKD",
    "01M5VYT0Z29G3PYCE6CKA3KBB1",
    "01M5Z6CTQ8DJAVQ9YAYS5SR58J",
    "01M603Q6RR0R8Z7TRJ36Z1V0RY",
];

static RECORD_SET: LazyLock<HashSet<&'static str>> =
    LazyLock::new(|| RECORDS.iter().copied().collect());

/// The week the records' `updated` was moved, as orrery's migrations 0272
/// and 0273 bound it between them.
const SINCE: &str = "2026-10-21T00:00:00Z";
const UNTIL: &str = "2026-10-28T03:00:00Z";

/// How the orrery sync ends the description of an edit it logs.
const FROM_ORRERY: &str = " (from orrery)";

/// The action `row` should have been logged as, when it is one of the rows
/// this repairs.
fn reclassified(row: &ActivityLogRow) -> Option<&'static str> {
    let housekeeping = if row.action == PageChange::ProjectEdit.action() {
        Housekeeping::Project
    } else if row.action == PageChange::EventEdit.action() {
        Housekeeping::Event
    } else {
        return None;
    };

    if row.user_id.is_some() {
        return None;
    }

    let extra = row.extra.as_deref()?;
    if !extra.ends_with(FROM_ORRERY) || extra.starts_with("all") {
        return None;
    }

    if !RECORD_SET.contains(row.target.as_deref()?) {
        return None;
    }

    let when: Timestamp = row.when.parse().ok()?;
    let since: Timestamp = SINCE.parse().expect("a valid timestamp");
    let until: Timestamp = UNTIL.parse().expect("a valid timestamp");
    if when < since || when >= until {
        return None;
    }

    Some(housekeeping.action())
}

/// Reclassifies the rows that need it, reporting each, and says how many
/// there were.
fn repair_rows(rows: &mut [ActivityLogRow], file: &Path) -> usize {
    let mut changed = 0;

    for row in rows.iter_mut() {
        if let Some(action) = reclassified(row) {
            println!(
                "{}: {} {} {} -> {action}",
                file.display(),
                row.when,
                row.target.as_deref().unwrap_or_default(),
                row.action,
            );
            row.action = action.to_string();
            changed += 1;
        }
    }

    changed
}

fn main() -> Result<()> {
    let dry_run = std::env::args().any(|arg| arg == "--dry-run");

    #[cfg(not(debug_assertions))]
    let database_path = env!("DATABASE_PATH");
    #[cfg(debug_assertions)]
    let database_path = env!("LOCAL_DATABASE_PATH");

    let base = PathBuf::from(database_path);

    if dry_run {
        println!("Dry run: reporting only, nothing will be written.\n");
    } else {
        println!("Rewriting files under {}.\n", base.display());
    }

    let schema = rewrite::schema_for(ActivityLogRow::TABLE_NAME)?;
    let mut changed = 0;

    for path in rewrite::parquet_files(&base, ActivityLogRow::TABLE_NAME)? {
        changed += rewrite_parquet(&path, &schema, dry_run)
            .with_context(|| format!("rewriting {}", path.display()))?;
    }

    for path in rewrite::wal_files(&base, ActivityLogRow::TABLE_NAME)? {
        changed +=
            rewrite_wal(&path, dry_run).with_context(|| format!("rewriting {}", path.display()))?;
    }

    let verb = if dry_run {
        "Would reclassify"
    } else {
        "Reclassified"
    };
    println!("\n{verb} {changed} rows.");

    Ok(())
}

fn rewrite_parquet(path: &Path, schema: &Schema, dry_run: bool) -> Result<usize> {
    let mut batches = Vec::new();
    let mut changed = 0;

    for batch in rewrite::read_parquet(path, None)? {
        let mut rows: Vec<ActivityLogRow> = rewrite::rows_from_batch(&batch?)?;
        changed += repair_rows(&mut rows, path);
        batches.push(rows);
    }

    if dry_run || changed == 0 {
        return Ok(changed);
    }

    let mut writer = ParquetReplacement::create(path, schema, DEFAULT_ROW_GROUP_SIZE)?;
    for rows in batches {
        writer.write(&rewrite::batch_from_rows(&rows, schema)?)?;
    }
    writer.finish()?.commit()?;

    Ok(changed)
}

fn rewrite_wal(path: &Path, dry_run: bool) -> Result<usize> {
    let mut lines = Vec::new();
    let mut changed = 0;

    let mut reader = BufReader::new(File::open(path)?).lines().peekable();

    while let Some(line) = reader.next() {
        let line = line?;

        if line.trim().is_empty() {
            continue;
        }

        let mut row: ActivityLogRow = match serde_json::from_str(&line) {
            Ok(row) => row,
            // As in dedupe-log: a dry run may catch the service's last line
            // half written, and nothing else can be torn.
            Err(e) if dry_run && reader.peek().is_none() => {
                eprintln!(
                    "{}: ignoring an incomplete last line, which is what a write in progress looks like ({e})",
                    path.display()
                );
                break;
            }
            Err(e) => return Err(e.into()),
        };

        if repair_rows(std::slice::from_mut(&mut row), path) == 0 {
            lines.push(line);
        } else {
            lines.push(serde_json::to_string(&row)?);
            changed += 1;
        }
    }

    if dry_run || changed == 0 {
        return Ok(changed);
    }

    rewrite::replace_wal(path, lines)?.commit()?;
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert2::check;

    const RECORD: &str = "01JKXMGGMD577ZA8Q57S2GY1V8";

    fn row(
        when: &str,
        action: &str,
        user: Option<&str>,
        target: &str,
        extra: &str,
    ) -> ActivityLogRow {
        ActivityLogRow {
            when: when.to_string(),
            action: action.to_string(),
            user_id: user.map(str::to_string),
            target: Some(target.to_string()),
            extra: Some(extra.to_string()),
        }
    }

    #[test]
    fn a_synced_edit_that_week_is_housekeeping() {
        let project = row(
            "2026-10-27T15:33:41.619906Z",
            "project-edit",
            None,
            RECORD,
            "how_to_join (from orrery)",
        );
        let event = row(
            "2026-10-26T11:17:06.510223Z",
            "event-edit",
            None,
            RECORD,
            "publish (from orrery)",
        );

        check!(reclassified(&project) == Some("project-housekeeping"));
        check!(reclassified(&event) == Some("event-housekeeping"));
    }

    #[test]
    fn the_first_sight_of_a_record_stays_an_update() {
        let first = row(
            "2026-10-26T11:17:06Z",
            "event-edit",
            None,
            RECORD,
            "all, page (from orrery)",
        );

        check!(reclassified(&first).is_none());
    }

    #[test]
    fn edits_outside_that_week_are_left_alone() {
        let before = row(
            "2026-10-20T23:59:59Z",
            "project-edit",
            None,
            RECORD,
            "name (from orrery)",
        );
        let after = row(
            "2026-10-28T03:00:00Z",
            "project-edit",
            None,
            RECORD,
            "name (from orrery)",
        );

        check!(reclassified(&before).is_none());
        check!(reclassified(&after).is_none());
    }

    #[test]
    fn other_records_and_edits_made_here_are_left_alone() {
        let other = row(
            "2026-10-27T15:22:14Z",
            "project-edit",
            None,
            "01JKXMFZ6Y2A9S1P0R5C3H8N4T",
            "name (from orrery)",
        );
        let here = row(
            "2026-10-27T15:33:41Z",
            "project-edit",
            Some("01K0000000000000000000USER"),
            RECORD,
            "name",
        );
        let api = row(
            "2026-10-27T15:33:41Z",
            "project-edit",
            None,
            RECORD,
            "name (from the API)",
        );

        check!(reclassified(&other).is_none());
        check!(reclassified(&here).is_none());
        check!(reclassified(&api).is_none());
    }

    #[test]
    fn a_reclassified_row_is_not_reclassified_again() {
        let mut rows = [row(
            "2026-10-27T15:33:41Z",
            "project-edit",
            None,
            RECORD,
            "how_to_join (from orrery)",
        )];

        check!(repair_rows(&mut rows, Path::new("test")) == 1);
        check!(repair_rows(&mut rows, Path::new("test")) == 0);
    }

    #[test]
    fn every_record_is_a_ulid() {
        for record in RECORDS {
            check!(record.parse::<ulid::Ulid>().is_ok(), "{record}");
        }
        check!(RECORD_SET.len() == 97);
    }
}
