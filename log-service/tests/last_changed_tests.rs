//! When a page last changed, as the sitemap and the detail page's "Last
//! Updated" read it: the latest page change logged against the page,
//! whichever kind of change it was.

use anyhow::Result;
use assert2::check;
use common::message::log::{LastChangedRequest, PageChange};
use log_service::analytics::get_last_changed;
use log_service::database::storage::Database;
use log_service::database::tables::ActivityLogRow;
use std::collections::BTreeMap;
use std::sync::Arc;
use tempfile::TempDir;
use tokio::time::{Duration, sleep};
use ulid::Ulid;

const PROJECT: &str = "01K000000000000000000PRJ01";
const PAGE: &str = "01K000000000000000000PAGE1";
const VIEWED_ONLY: &str = "01K000000000000000000PRJ02";
const UNREQUESTED: &str = "01K000000000000000000PRJ03";
const UNLOGGED: &str = "01K000000000000000000PRJ04";

fn activity(when: &str, action: impl Into<String>, target: &str) -> ActivityLogRow {
    ActivityLogRow {
        when: when.into(),
        action: action.into(),
        user_id: None,
        target: Some(target.into()),
        extra: None,
    }
}

async fn database() -> Result<(TempDir, Arc<Database>)> {
    let temp_dir = TempDir::new()?;
    let database = Arc::new(Database::new(temp_dir.path().to_string_lossy().to_string()).await?);

    for row in [
        // A project created, edited, then published: the latest counts,
        // whatever order the rows arrive in.
        activity("2026-10-01T10:00:00Z", PageChange::ProjectCreate, PROJECT),
        activity("2026-10-03T10:00:00Z", PageChange::ProjectPublish, PROJECT),
        activity("2026-10-02T10:00:00Z", PageChange::ProjectEdit, PROJECT),
        // A view is logged against the project later still, and is not a
        // change to it.
        activity("2026-10-09T10:00:00Z", "project-view", PROJECT),
        // A content page, edited in the page builder.
        activity("2026-10-05T10:00:00Z", PageChange::ContentEdit, PAGE),
        // A project nothing but viewed.
        activity("2026-10-06T10:00:00Z", "project-view", VIEWED_ONLY),
        // A change to a page nobody asked about.
        activity("2026-10-07T10:00:00Z", PageChange::ProjectEdit, UNREQUESTED),
    ] {
        database.activity_log.insert(row).await?;
    }

    // Writes land in the WAL; give them a moment to be readable.
    sleep(Duration::from_millis(100)).await;

    Ok((temp_dir, database))
}

fn ulid(id: &str) -> Ulid {
    id.parse().expect("test ids are ULIDs")
}

#[tokio::test]
async fn each_page_reports_its_latest_change() -> Result<()> {
    let (_dir, database) = database().await?;

    let response = get_last_changed(
        database,
        LastChangedRequest {
            targets: [PROJECT, PAGE, VIEWED_ONLY, UNLOGGED]
                .into_iter()
                .map(ulid)
                .collect(),
        },
    )
    .await?;

    let changes: BTreeMap<Ulid, String> = response
        .changes
        .into_iter()
        .map(|change| (change.target, change.when.to_string()))
        .collect();

    check!(
        changes
            == BTreeMap::from([
                (ulid(PROJECT), "2026-10-03T10:00:00Z".to_string()),
                (ulid(PAGE), "2026-10-05T10:00:00Z".to_string()),
            ]),
        "views are not changes, unlogged and unrequested pages are absent"
    );

    Ok(())
}

#[tokio::test]
async fn asking_about_nothing_answers_nothing() -> Result<()> {
    let (_dir, database) = database().await?;

    let response = get_last_changed(database, LastChangedRequest { targets: vec![] }).await?;

    check!(response.changes.is_empty());

    Ok(())
}
