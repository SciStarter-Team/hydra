//! Importers for orrery's sync feeds.
//!
//! Each feed is paged with a watermark: the importer remembers how far it has
//! read and asks for everything after that. Which column the watermark follows
//! decides two things: whether every row crosses it, and whether a delivery
//! and an event are the same thing.
//!
//! Every row crosses a watermark only if the watermark follows the order rows
//! *arrive* in; one that follows any other order passes over every row that
//! arrives behind it, for good. Golinks, ratings and reviews page on `when`,
//! which orrery stamps as the row is created, so for them it is arrival order
//! and each row crosses exactly once: every delivery is a new event.
//!
//! Participation does not. Its `when` is the moment the contribution was
//! made, which imports set from the past -- and one row dated in the future
//! once held the whole feed back for six months. Nor is an event written
//! once: later contributions fold into it. So it pages on the revision orrery
//! numbers every write with, and a delivery is a new state of an event that
//! [`participation_ledger`] turns into a change to the log; see
//! [`log_service::participation_feed`].
//!
//! Joins and bookmarks page on `activity`, which orrery bumps whenever the row
//! is touched. Those rows cross the watermark again on every change, and the
//! importer is handed an event it has already logged.
//!
//! So for joins and bookmarks a delivery is *not* an event, and they consult
//! [`ledger::claim`] before writing. Adding a feed that can deliver a row
//! twice means doing the same.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;

use anyhow::Context;
use futures::TryStreamExt;
use jiff::Timestamp;
use log_service::database::tables::ParticipationRow;
use log_service::database::{
    Database, get_sync_bookmark, get_sync_last_id, set_sync_bookmark, set_sync_last_id,
};
use log_service::ledger::{self, Feed};
use log_service::participation_feed::{self, FeedCursor, FeedItem};
use log_service::participation_ledger;
use std::sync::Arc;

use crate::bridges::{BridgeCoverage, Contribution};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BookmarkData {
    pub id: i32,
    pub when: String, // ISO 8601 timestamp string
    pub entity_ulid: String,
    pub user_ulid: String,
    pub active: bool,
    pub activity: String,          // ISO 8601 timestamp string
    pub contacted: Option<String>, // ISO 8601 timestamp string
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JoinData {
    pub id: i32,
    pub when: String, // ISO 8601 timestamp string
    pub entity_ulid: String,
    pub user_ulid: String,
    pub active: bool,
    pub consented: Option<String>, // ISO 8601 timestamp string
    pub contacted: Option<String>, // ISO 8601 timestamp string
    pub explicit: bool,
    pub notified: bool,
    pub activity: String, // ISO 8601 timestamp string
    pub kind: i32,
    pub reason: String,
    pub url: Option<String>,
    pub slug: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GolinkData {
    pub when: String,
    pub slug: String,
    pub target: String,
    pub file: String,
    pub user_ulid: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RatingData {
    pub id: i32,
    pub when: String, // ISO 8601 timestamp string
    pub entity_ulid: String,
    pub user_ulid: Option<String>, // None for anonymous orrery ratings
    pub amount: i16,               // > 0 is a like; mirrors orrery rate_review_rating
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewData {
    pub id: i32,
    pub when: String, // ISO 8601 timestamp string
    pub entity_ulid: String,
    pub user_ulid: String,
    pub text: String,
    pub private: bool,
    pub checked: bool,
}

/// Watermark: the revision orrery numbers each write to an event with, with
/// a settling window re-read behind it. A delivery is the event as the write
/// left it, reconciled with what the log holds through
/// [`participation_ledger`]. See [`log_service::participation_feed`].
pub async fn participation_synchronize(
    database: Arc<Database>,
    cancellation_token: CancellationToken,
) {
    let client = reqwest::Client::new();

    loop {
        if cancellation_token.is_cancelled() {
            println!("Participation sync cancelled, exiting gracefully");
            return;
        }

        let sleep_duration = match participation_pass(&database, &client, &cancellation_token).await
        {
            Ok(Pass::Read(rows)) => {
                println!("Participation sync read {rows} rows");
                Duration::from_secs(60 * 60)
            }
            Ok(Pass::NotStarted) => {
                eprintln!(
                    "Participation sync has no cursor; the participation-repair tool \
                     sets where it starts"
                );
                Duration::from_secs(60 * 60)
            }
            Ok(Pass::Cancelled) => return,
            Err(e) => {
                // The cursor is left where it was, so the next pass reads
                // from the same place, the failed write included.
                eprintln!("Participation sync pass failed: {e}");
                Duration::from_secs(60)
            }
        };

        tokio::select! {
            _ = sleep(sleep_duration) => {}
            _ = cancellation_token.cancelled() => {
                println!("Participation sync cancelled during sleep, exiting gracefully");
                return;
            }
        }
    }
}

enum Pass {
    /// Read the feed to its end, this many writes of it.
    Read(usize),
    NotStarted,
    Cancelled,
}

/// Read everything after the settled mark, in revision order, and move the
/// cursor.
///
/// Stops at the first write it cannot finish with: one it passed over would
/// be passed over for good once the cursor settled past it.
async fn participation_pass(
    database: &Database,
    client: &reqwest::Client,
    cancellation_token: &CancellationToken,
) -> Result<Pass> {
    let Some(cursor) = FeedCursor::load(database).await? else {
        return Ok(Pass::NotStarted);
    };

    // Fetched per pass, since accounts link and projects join between them.
    let coverage = BridgeCoverage::fetch().await?;
    take_back_newly_covered(&coverage).await?;
    let started = Timestamp::now();
    let mut read_through = cursor.settled;
    let mut rows = 0;

    loop {
        if cancellation_token.is_cancelled() {
            return Ok(Pass::Cancelled);
        }

        let page = participation_feed::fetch_page(client, read_through).await?;

        for item in &page {
            import_participation(database, &coverage, item).await?;
            read_through = item.revision;
        }

        rows += page.len();

        if page.len() < participation_feed::PAGE_SIZE {
            break;
        }
    }

    cursor
        .after_pass(started, read_through, Timestamp::now())
        .store(database)
        .await?;

    Ok(Pass::Read(rows))
}

/// Take back what the log holds for events a bridge has come to cover since
/// they were last delivered.
///
/// A delivery is checked against coverage as it stands when it is read. But
/// an account can link a platform after its events were read, or its link
/// can reach here after them, and a project can join a bridge; orrery writes
/// nothing to the events then, so no delivery would bring them back.
async fn take_back_newly_covered(coverage: &BridgeCoverage) -> Result<()> {
    let newly_covered: Vec<i32> = participation_ledger::uncovered()
        .try_filter_map(|(source_id, row)| {
            let covered = coverage.covers(Contribution::of_row(&row));
            std::future::ready(Ok(covered.then_some(source_id)))
        })
        .try_collect()
        .await?;

    for source_id in newly_covered {
        let entry = participation_ledger::load(source_id).await?;
        let reconciliation = participation_ledger::cover(&entry);

        if !reconciliation.is_change(&entry) {
            continue;
        }

        participation_ledger::record(
            source_id,
            &reconciliation,
            participation_ledger::Cause::Coverage,
        )
        .await
        .with_context(|| format!("recording participation event {source_id}"))?
        .commit()
        .await
        .with_context(|| format!("recording participation event {source_id}"))?;

        report(source_id, &reconciliation);
    }

    Ok(())
}

/// Bring the log into line with one write to an event.
async fn import_participation(
    database: &Database,
    coverage: &BridgeCoverage,
    item: &FeedItem,
) -> Result<()> {
    let entry = participation_ledger::load(item.id).await?;
    let covered = item
        .event
        .as_ref()
        .is_some_and(|event| coverage.covers(Contribution::of_event(event)));

    let reconciliation = participation_ledger::reconcile(
        &entry,
        item.id,
        item.event.as_ref(),
        covered,
        Timestamp::now(),
    );

    if !reconciliation.is_change(&entry) {
        return Ok(());
    }

    // Outside the ledger's transaction, which holds the cache's write lock;
    // noting a participant twice is harmless.
    if let Some((row, _)) = &reconciliation.append {
        note_participant(row).await?;
    }

    let recording = participation_ledger::record(
        item.id,
        &reconciliation,
        participation_ledger::Cause::Delivery(item.revision),
    )
    .await
    .with_context(|| format!("recording participation event {}", item.id))?;

    if let Some((row, copies)) = &reconciliation.append {
        database
            .participation
            .insert_all(vec![row.clone(); *copies as usize])
            .await
            .with_context(|| format!("logging participation event {}", item.id))?;
    }

    recording
        .commit()
        .await
        .with_context(|| format!("recording participation event {}", item.id))?;

    if let Some((row, _)) = &reconciliation.append {
        announce_participation(row).await;
    }

    report(item.id, &reconciliation);

    Ok(())
}

fn report(source_id: i32, reconciliation: &participation_ledger::Reconciliation) {
    if let Some((_, copies)) = &reconciliation.append {
        println!("Logged {copies} repetitions of participation event {source_id}");
    }

    if let Some((_, copies, reason)) = &reconciliation.retract {
        println!(
            "Queued {copies} repetitions of participation event {source_id} for retraction: {}",
            reason.describe()
        );
    }
}

/// Watermark: the mutable `activity`, so a delivery is not an event and
/// rows are claimed through [`ledger::claim`] before being logged.
pub async fn bookmark_synchronize(database: Arc<Database>, cancellation_token: CancellationToken) {
    loop {
        // Check for cancellation at start of loop
        if cancellation_token.is_cancelled() {
            println!("Bookmark sync cancelled, exiting gracefully");
            return;
        }
        let last = get_sync_bookmark(&database, "bookmark-sync-bookmark")
            .await
            .unwrap_or_else(|_| String::from("1900-01-01T00:00:00Z"));
        let last_id = get_sync_last_id(&database, "bookmark-sync-last-id")
            .await
            .unwrap_or_else(|_| 0);

        println!("Syncing bookmarks since {} with last_id {}", last, last_id);

        let sync_token = env!("SYNC_TOKEN");
        let client = reqwest::Client::new();

        let bookmarks: Vec<BookmarkData> = match client
            .get("https://scistarter.org/staff/sync/bookmarks")
            .bearer_auth(sync_token)
            .query(&[("since", &last), ("last_id", &last_id.to_string())])
            .send()
            .await
        {
            Ok(response) => match response.error_for_status() {
                Ok(resp) => {
                    let response_text = match resp.text().await {
                        Ok(text) => text,
                        Err(e) => {
                            eprintln!("Failed to read response body: {}", e);
                            sleep(Duration::from_secs(60)).await;
                            continue;
                        }
                    };

                    match serde_json::from_str(&response_text) {
                        Ok(events) => events,
                        Err(e) => {
                            eprintln!("JSON parsing error: {}", e);
                            eprintln!("Response body: {}", response_text);
                            sleep(Duration::from_secs(60)).await;
                            continue;
                        }
                    }
                }
                Err(e) => {
                    eprintln!("HTTP error: {}", e);
                    sleep(Duration::from_secs(60)).await;
                    continue;
                }
            },
            Err(e) => {
                eprintln!("Request error: {}", e);
                sleep(Duration::from_secs(60)).await;
                continue;
            }
        };

        let got_bookmarks = bookmarks.len();

        if got_bookmarks == 0 {
            println!("No new bookmarks to sync");
        } else {
            let mut bookmark = last.clone();
            let mut bookmark_id = last_id;

            for bm in bookmarks {
                // Pages on the mutable `activity`, exactly as joins do, so the
                // same bookmark arrives again every time it is touched.
                let first_delivery = match ledger::claim(Feed::Bookmarks, bm.id.into()).await {
                    Ok(claimed) => claimed,
                    Err(e) => {
                        eprintln!("Failed to claim bookmark {}: {}", bm.id, e);
                        continue;
                    }
                };

                if first_delivery {
                    if let Err(e) = insert_bookmark(&database, &bm).await {
                        eprintln!("Failed to insert bookmark {}: {}", bm.id, e);
                        ledger::release(Feed::Bookmarks, bm.id.into()).await;
                        continue;
                    }

                    println!("Inserted bookmark {}", bm.id);
                }

                if bm.activity > bookmark || (bm.activity == bookmark && bm.id > bookmark_id) {
                    bookmark = bm.activity.clone();
                    bookmark_id = bm.id;
                }
            }

            set_sync_bookmark(&database, "bookmark-sync-bookmark", &bookmark)
                .await
                .expect("Failed to save sync bookmark");
            set_sync_last_id(&database, "bookmark-sync-last-id", bookmark_id)
                .await
                .expect("Failed to save sync last_id");
        }

        let sleep_duration = if got_bookmarks == 500 {
            Duration::from_secs(60)
        } else {
            Duration::from_secs(60 * 60)
        };

        // Use cancellation-aware sleep
        tokio::select! {
            _ = sleep(sleep_duration) => {}
            _ = cancellation_token.cancelled() => {
                println!("Bookmark sync cancelled during sleep, exiting gracefully");
                return;
            }
        }
    }
}

/// Watermark: the mutable `activity`, so a delivery is not an event and
/// rows are claimed through [`ledger::claim`] before being logged.
pub async fn join_synchronize(database: Arc<Database>, cancellation_token: CancellationToken) {
    loop {
        // Check for cancellation at start of loop
        if cancellation_token.is_cancelled() {
            println!("Join sync cancelled, exiting gracefully");
            return;
        }
        let last = get_sync_bookmark(&database, "join-sync-bookmark")
            .await
            .unwrap_or_else(|_| String::from("1900-01-01T00:00:00Z"));
        let last_id = get_sync_last_id(&database, "join-sync-last-id")
            .await
            .unwrap_or_else(|_| 0);

        println!("Syncing joins since {} with last_id {}", last, last_id);

        let sync_token = env!("SYNC_TOKEN");
        let client = reqwest::Client::new();

        let joins: Vec<JoinData> = match client
            .get("https://scistarter.org/staff/sync/joins")
            .bearer_auth(sync_token)
            .query(&[("since", &last), ("last_id", &last_id.to_string())])
            .send()
            .await
        {
            Ok(response) => match response.error_for_status() {
                Ok(resp) => {
                    let response_text = match resp.text().await {
                        Ok(text) => text,
                        Err(e) => {
                            eprintln!("Failed to read response body: {}", e);
                            sleep(Duration::from_secs(60)).await;
                            continue;
                        }
                    };

                    match serde_json::from_str(&response_text) {
                        Ok(events) => events,
                        Err(e) => {
                            eprintln!("JSON parsing error: {}", e);
                            eprintln!("Response body: {}", response_text);
                            sleep(Duration::from_secs(60)).await;
                            continue;
                        }
                    }
                }
                Err(e) => {
                    eprintln!("HTTP error: {}", e);
                    sleep(Duration::from_secs(60)).await;
                    continue;
                }
            },
            Err(e) => {
                eprintln!("Request error: {}", e);
                sleep(Duration::from_secs(60)).await;
                continue;
            }
        };

        let got_joins = joins.len();

        if got_joins == 0 {
            println!("No new joins to sync");
        } else {
            let mut bookmark = last.clone();
            let mut bookmark_id = last_id;

            for join in joins {
                // This feed pages on `activity`, which orrery mutates, so the
                // same join arrives again every time it is touched. Only the
                // first delivery is an event; see `ledger::claim`.
                let first_delivery = match join.kind {
                    0 | 1 => match ledger::claim(Feed::Joins, join.id.into()).await {
                        Ok(claimed) => claimed,
                        Err(e) => {
                            eprintln!("Failed to claim join {}: {}", join.id, e);
                            // The watermark must not move past a row we have
                            // not decided about, or the event is lost.
                            continue;
                        }
                    },
                    _ => {
                        println!("Skipping join {} with kind {}", join.id, join.kind);
                        false
                    }
                };

                if first_delivery {
                    let written = match join.kind {
                        0 => insert_join_as_clickthrough(&database, &join)
                            .await
                            .inspect(|()| println!("Inserted join {} as clickthrough", join.id)),
                        _ => match insert_join_as_clickthrough(&database, &join).await {
                            Ok(()) => insert_join(&database, &join).await.inspect(|()| {
                                println!("Inserted join {} as both clickthrough and join", join.id)
                            }),
                            Err(e) => Err(e),
                        },
                    };

                    if let Err(e) = written {
                        eprintln!("Failed to insert join {}: {}", join.id, e);
                        // Hand the row back so the retry logs it rather than
                        // mistaking it for a delivery it has already seen.
                        ledger::release(Feed::Joins, join.id.into()).await;
                        continue;
                    }
                }

                // Reached for a logged event, a re-delivery and a kind we do
                // not log alike: all three are finished with, and holding the
                // watermark back for any of them stalls the feed.
                if join.activity > bookmark || (join.activity == bookmark && join.id > bookmark_id)
                {
                    bookmark = join.activity.clone();
                    bookmark_id = join.id;
                }
            }

            set_sync_bookmark(&database, "join-sync-bookmark", &bookmark)
                .await
                .expect("Failed to save sync bookmark");
            set_sync_last_id(&database, "join-sync-last-id", bookmark_id)
                .await
                .expect("Failed to save sync last_id");
        }

        let sleep_duration = if got_joins == 500 {
            Duration::from_secs(60)
        } else {
            Duration::from_secs(60 * 60)
        };

        // Use cancellation-aware sleep
        tokio::select! {
            _ = sleep(sleep_duration) => {}
            _ = cancellation_token.cancelled() => {
                println!("Join sync cancelled during sleep, exiting gracefully");
                return;
            }
        }
    }
}

/// Watermark: the immutable `when`, so every delivery is a new event.
pub async fn rating_synchronize(database: Arc<Database>, cancellation_token: CancellationToken) {
    loop {
        // Check for cancellation at start of loop
        if cancellation_token.is_cancelled() {
            println!("Rating sync cancelled, exiting gracefully");
            return;
        }
        let last = get_sync_bookmark(&database, "rating-sync-bookmark")
            .await
            .unwrap_or_else(|_| String::from("1900-01-01T00:00:00Z"));
        let last_id = get_sync_last_id(&database, "rating-sync-last-id")
            .await
            .unwrap_or(0);

        println!("Syncing ratings since {} with last_id {}", last, last_id);

        let sync_token = env!("SYNC_TOKEN");
        let client = reqwest::Client::new();

        let ratings: Vec<RatingData> = match client
            .get("https://scistarter.org/staff/sync/ratings")
            .bearer_auth(sync_token)
            .query(&[("since", &last), ("last_id", &last_id.to_string())])
            .send()
            .await
        {
            Ok(response) => match response.error_for_status() {
                Ok(resp) => {
                    let response_text = match resp.text().await {
                        Ok(text) => text,
                        Err(e) => {
                            eprintln!("Failed to read response body: {}", e);
                            sleep(Duration::from_secs(60)).await;
                            continue;
                        }
                    };

                    match serde_json::from_str(&response_text) {
                        Ok(events) => events,
                        Err(e) => {
                            eprintln!("JSON parsing error: {}", e);
                            eprintln!("Response body: {}", response_text);
                            sleep(Duration::from_secs(60)).await;
                            continue;
                        }
                    }
                }
                Err(e) => {
                    eprintln!("HTTP error: {}", e);
                    sleep(Duration::from_secs(60)).await;
                    continue;
                }
            },
            Err(e) => {
                eprintln!("Request error: {}", e);
                sleep(Duration::from_secs(60)).await;
                continue;
            }
        };

        let got_ratings = ratings.len();

        if got_ratings == 0 {
            println!("No new ratings to sync");
        } else {
            let mut bookmark = last.clone();
            let mut bookmark_id = last_id;

            for rating in ratings {
                if let Err(e) = insert_rating(&rating).await {
                    eprintln!("Failed to insert rating: {}", e);
                    continue;
                }

                if rating.when > bookmark || (rating.when == bookmark && rating.id > bookmark_id) {
                    bookmark = rating.when.clone();
                    bookmark_id = rating.id;
                }
            }

            set_sync_bookmark(&database, "rating-sync-bookmark", &bookmark)
                .await
                .expect("Failed to save sync bookmark");
            set_sync_last_id(&database, "rating-sync-last-id", bookmark_id)
                .await
                .expect("Failed to save sync last_id");
        }

        let sleep_duration = if got_ratings == 500 {
            Duration::from_secs(60)
        } else {
            Duration::from_secs(60 * 60)
        };

        // Use cancellation-aware sleep
        tokio::select! {
            _ = sleep(sleep_duration) => {}
            _ = cancellation_token.cancelled() => {
                println!("Rating sync cancelled during sleep, exiting gracefully");
                return;
            }
        }
    }
}

/// Watermark: the immutable `when`, so every delivery is a new event.
pub async fn review_synchronize(database: Arc<Database>, cancellation_token: CancellationToken) {
    loop {
        // Check for cancellation at start of loop
        if cancellation_token.is_cancelled() {
            println!("Review sync cancelled, exiting gracefully");
            return;
        }
        let last = get_sync_bookmark(&database, "review-sync-bookmark")
            .await
            .unwrap_or_else(|_| String::from("1900-01-01T00:00:00Z"));
        let last_id = get_sync_last_id(&database, "review-sync-last-id")
            .await
            .unwrap_or(0);

        println!("Syncing reviews since {} with last_id {}", last, last_id);

        let sync_token = env!("SYNC_TOKEN");
        let client = reqwest::Client::new();

        let reviews: Vec<ReviewData> = match client
            .get("https://scistarter.org/staff/sync/reviews")
            .bearer_auth(sync_token)
            .query(&[("since", &last), ("last_id", &last_id.to_string())])
            .send()
            .await
        {
            Ok(response) => match response.error_for_status() {
                Ok(resp) => {
                    let response_text = match resp.text().await {
                        Ok(text) => text,
                        Err(e) => {
                            eprintln!("Failed to read response body: {}", e);
                            sleep(Duration::from_secs(60)).await;
                            continue;
                        }
                    };

                    match serde_json::from_str(&response_text) {
                        Ok(events) => events,
                        Err(e) => {
                            eprintln!("JSON parsing error: {}", e);
                            eprintln!("Response body: {}", response_text);
                            sleep(Duration::from_secs(60)).await;
                            continue;
                        }
                    }
                }
                Err(e) => {
                    eprintln!("HTTP error: {}", e);
                    sleep(Duration::from_secs(60)).await;
                    continue;
                }
            },
            Err(e) => {
                eprintln!("Request error: {}", e);
                sleep(Duration::from_secs(60)).await;
                continue;
            }
        };

        let got_reviews = reviews.len();

        if got_reviews == 0 {
            println!("No new reviews to sync");
        } else {
            let mut bookmark = last.clone();
            let mut bookmark_id = last_id;

            for review in reviews {
                if let Err(e) = insert_review(&review).await {
                    eprintln!("Failed to insert review: {}", e);
                    continue;
                }

                if review.when > bookmark || (review.when == bookmark && review.id > bookmark_id) {
                    bookmark = review.when.clone();
                    bookmark_id = review.id;
                }
            }

            set_sync_bookmark(&database, "review-sync-bookmark", &bookmark)
                .await
                .expect("Failed to save sync bookmark");
            set_sync_last_id(&database, "review-sync-last-id", bookmark_id)
                .await
                .expect("Failed to save sync last_id");
        }

        let sleep_duration = if got_reviews == 500 {
            Duration::from_secs(60)
        } else {
            Duration::from_secs(60 * 60)
        };

        // Use cancellation-aware sleep
        tokio::select! {
            _ = sleep(sleep_duration) => {}
            _ = cancellation_token.cancelled() => {
                println!("Review sync cancelled during sleep, exiting gracefully");
                return;
            }
        }
    }
}

/// Watermark: the immutable `when`, so every delivery is a new event.
pub async fn golink_synchronize(database: Arc<Database>, cancellation_token: CancellationToken) {
    loop {
        // Check for cancellation at start of loop
        if cancellation_token.is_cancelled() {
            println!("Golink sync cancelled, exiting gracefully");
            return;
        }
        let last = get_sync_bookmark(&database, "golink-sync-bookmark")
            .await
            .unwrap_or_else(|_| String::from("1900-01-01T00:00:00Z"));
        let last_id = get_sync_last_id(&database, "golink-sync-last-id")
            .await
            .unwrap_or_else(|_| 0);

        println!("Syncing golinks since {} with last_id {}", last, last_id);

        let sync_token = env!("SYNC_TOKEN");
        let client = reqwest::Client::new();

        let golinks: Vec<GolinkData> = match client
            .get("https://scistarter.org/staff/sync/golinks")
            .bearer_auth(sync_token)
            .query(&[("since", &last), ("last_id", &last_id.to_string())])
            .send()
            .await
        {
            Ok(response) => match response.error_for_status() {
                Ok(resp) => {
                    let response_text = match resp.text().await {
                        Ok(text) => text,
                        Err(e) => {
                            eprintln!("Failed to read response body: {}", e);
                            sleep(Duration::from_secs(60)).await;
                            continue;
                        }
                    };

                    match serde_json::from_str(&response_text) {
                        Ok(events) => events,
                        Err(e) => {
                            eprintln!("JSON parsing error: {}", e);
                            eprintln!("Response body: {}", response_text);
                            sleep(Duration::from_secs(60)).await;
                            continue;
                        }
                    }
                }
                Err(e) => {
                    eprintln!("HTTP error: {}", e);
                    sleep(Duration::from_secs(60)).await;
                    continue;
                }
            },
            Err(e) => {
                eprintln!("Request error: {}", e);
                sleep(Duration::from_secs(60)).await;
                continue;
            }
        };

        let got_golinks = golinks.len();

        if got_golinks == 0 {
            println!("No new golinks to sync");
        } else {
            let mut bookmark = last.clone();

            for golink in golinks {
                if let Err(e) = insert_golink_as_clickthrough(&database, &golink).await {
                    eprintln!("Failed to insert golink as clickthrough: {}", e);
                    continue;
                }

                // Update bookmark to the latest timestamp
                if golink.when > bookmark {
                    bookmark = golink.when.clone();
                }

                println!("Inserted golink {} as clickthrough", golink.slug);
            }

            set_sync_bookmark(&database, "golink-sync-bookmark", &bookmark)
                .await
                .expect("Failed to save sync bookmark");
        }

        let sleep_duration = if got_golinks == 500 {
            Duration::from_secs(60)
        } else {
            Duration::from_secs(60 * 60)
        };

        // Use cancellation-aware sleep
        tokio::select! {
            _ = sleep(sleep_duration) => {}
            _ = cancellation_token.cancelled() => {
                println!("Golink sync cancelled during sleep, exiting gracefully");
                return;
            }
        }
    }
}

/// Note that the row's account has participated in the row's project.
async fn note_participant(row: &ParticipationRow) -> Result<()> {
    if let Some(user_id_str) = &row.user_id {
        sqlx::query!(
            r#"
            INSERT OR IGNORE INTO project_participations_cache (user_id, project_id)
            VALUES (?, ?)
            "#,
            user_id_str,
            row.entity_id
        )
        .execute(common::shared::db())
        .await?;
    }

    Ok(())
}

/// Tell the row's account, if it is online, that its participation arrived.
async fn announce_participation(row: &ParticipationRow) {
    if let Some(user_id) = row
        .user_id
        .as_deref()
        .and_then(|user_id| user_id.parse::<ulid::Ulid>().ok())
    {
        let _ = common::channel::send_to_user(
            user_id,
            "completion",
            common::channel::Event::new("project_participation", &row.entity_id),
        )
        .await;
    }
}

async fn insert_bookmark(database: &Database, bookmark: &BookmarkData) -> Result<()> {
    use log_service::database::tables::BookmarkRow;

    let row = BookmarkRow {
        when: bookmark.when.clone(),
        entity_id: bookmark.entity_ulid.clone(),
        user_id: bookmark.user_ulid.clone(),
        active: bookmark.active,
        activity: bookmark.activity.clone(),
        contacted: bookmark.contacted.clone(),
    };

    database.bookmark.insert(row).await?;
    Ok(())
}

async fn insert_rating(rating: &RatingData) -> Result<()> {
    // Anonymous orrery ratings have no user; give each a distinct synthetic id so
    // they count once (via amount > 0) and never match an authenticated user's like.
    let user_id = rating
        .user_ulid
        .clone()
        .unwrap_or_else(|| format!("anon:{}", rating.id));
    let amount = rating.amount as i64;

    sqlx::query!(
        r#"INSERT INTO project_ratings_cache (project_id, user_id, amount, "when")
           VALUES (?, ?, ?, ?)
           ON CONFLICT (project_id, user_id)
           DO UPDATE SET amount = excluded.amount, "when" = excluded."when""#,
        rating.entity_ulid,
        user_id,
        amount,
        rating.when,
    )
    .execute(common::shared::db())
    .await?;

    Ok(())
}

async fn insert_review(review: &ReviewData) -> Result<()> {
    let private: i64 = if review.private { 1 } else { 0 };
    let checked: i64 = if review.checked { 1 } else { 0 };

    sqlx::query!(
        r#"INSERT INTO project_reviews_cache (project_id, user_id, text, private, checked, "when")
           VALUES (?, ?, ?, ?, ?, ?)
           ON CONFLICT (project_id, user_id)
           DO UPDATE SET text = excluded.text, private = excluded.private,
               checked = excluded.checked, "when" = excluded."when""#,
        review.entity_ulid,
        review.user_ulid,
        review.text,
        private,
        checked,
        review.when,
    )
    .execute(common::shared::db())
    .await?;

    Ok(())
}

async fn insert_join(database: &Database, join: &JoinData) -> Result<()> {
    use log_service::database::tables::JoinRow;

    let row = JoinRow {
        when: join.when.clone(),
        entity_id: join.entity_ulid.clone(),
        user_id: join.user_ulid.clone(),
        active: join.active,
        consented: join.consented.clone(),
        contacted: join.contacted.clone(),
        explicit: join.explicit,
        notified: join.notified,
        activity: join.activity.clone(),
        reason: join.reason.clone(),
    };

    database.join_table.insert(row).await?;
    Ok(())
}

async fn insert_join_as_clickthrough(database: &Database, join: &JoinData) -> Result<()> {
    use log_service::database::tables::ClickthroughRow;

    let row = ClickthroughRow {
        when: join.when.clone(),
        ip_address: "0.0.0.0".to_string(),
        device_id: "sync".to_string(),
        interaction_id: "sync".to_string(),
        user_id: Some(join.user_ulid.clone()),
        from_path: format!("/{}", join.slug),
        to_url: join
            .url
            .clone()
            .unwrap_or_else(|| format!("entity/{}", join.entity_ulid)),
        link_type: "join".to_string(),
        program_id: Some(join.entity_ulid.clone()),
        geolocation: r#"{"type":"Point","coordinates":[0,0]}"#.to_string(),
    };

    database.clickthroughs.insert(row).await?;
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActsOfScienceData {
    pub id: i32,
    pub when: String,
    pub email: String,
    pub country: String,
    pub newsletter: i16,
    pub name: String,
    pub contributions: i32,
    pub act: String,
    pub organization: String,
    pub num_others: i32,
    pub others: String,
    pub project: String,
    pub notes: String,
    pub source: i16,
    pub accepted: bool,
    pub display: Option<bool>,
    pub featured: Option<bool>,
    pub sampled: Option<bool>,
    pub entity_processed: bool,
    pub user_ulid: Option<String>,
    pub entity_ulid: Option<String>,
}

/// Watermark: the immutable `when`, so every delivery is a new event.
pub async fn acts_of_science_synchronize(
    database: Arc<Database>,
    cancellation_token: CancellationToken,
) {
    loop {
        if cancellation_token.is_cancelled() {
            println!("Acts of science sync cancelled, exiting gracefully");
            return;
        }

        let last = get_sync_bookmark(&database, "acts-sync-bookmark")
            .await
            .unwrap_or_else(|_| String::from("1900-01-01T00:00:00Z"));
        let last_id = get_sync_last_id(&database, "acts-sync-last-id")
            .await
            .unwrap_or_else(|_| 0);

        println!(
            "Syncing acts of science since {} with last_id {}",
            last, last_id
        );

        let sync_token = env!("SYNC_TOKEN");
        let client = reqwest::Client::new();

        let acts: Vec<ActsOfScienceData> = match client
            .get("https://scistarter.org/staff/sync/acts_of_science")
            .bearer_auth(sync_token)
            .query(&[("since", &last), ("last_id", &last_id.to_string())])
            .send()
            .await
        {
            Ok(response) => match response.error_for_status() {
                Ok(resp) => {
                    let response_text = match resp.text().await {
                        Ok(text) => text,
                        Err(e) => {
                            eprintln!("Failed to read response body: {}", e);
                            sleep(Duration::from_secs(60)).await;
                            continue;
                        }
                    };

                    match serde_json::from_str(&response_text) {
                        Ok(events) => events,
                        Err(e) => {
                            eprintln!("JSON parsing error: {}", e);
                            eprintln!("Response body: {}", response_text);
                            sleep(Duration::from_secs(60)).await;
                            continue;
                        }
                    }
                }
                Err(e) => {
                    eprintln!("HTTP error: {}", e);
                    sleep(Duration::from_secs(60)).await;
                    continue;
                }
            },
            Err(e) => {
                eprintln!("Request error: {}", e);
                sleep(Duration::from_secs(60)).await;
                continue;
            }
        };

        let got_acts = acts.len();

        if got_acts == 0 {
            println!("No new acts of science to sync");
        } else {
            let mut bookmark = last.clone();
            let mut bookmark_id = last_id;

            for act in acts {
                if let Err(e) = insert_act_of_science(&act).await {
                    eprintln!("Failed to insert act of science: {}", e);
                    continue;
                }

                if act.when > bookmark || (act.when == bookmark && act.id > bookmark_id) {
                    bookmark = act.when.clone();
                    bookmark_id = act.id;
                }

                println!("Inserted act of science {}", act.id);
            }

            set_sync_bookmark(&database, "acts-sync-bookmark", &bookmark)
                .await
                .expect("Failed to save sync bookmark");
            set_sync_last_id(&database, "acts-sync-last-id", bookmark_id)
                .await
                .expect("Failed to save sync last_id");
        }

        let sleep_duration = if got_acts == 500 {
            Duration::from_secs(60)
        } else {
            Duration::from_secs(60 * 60)
        };

        tokio::select! {
            _ = sleep(sleep_duration) => {}
            _ = cancellation_token.cancelled() => {
                println!("Acts of science sync cancelled during sleep, exiting gracefully");
                return;
            }
        }
    }
}

async fn insert_act_of_science(act: &ActsOfScienceData) -> Result<()> {
    let act_id = ulid::Ulid::new().to_string();
    let newsletter = act.newsletter as i32;
    let contributions = act.contributions as i64;
    let num_others = act.num_others as i64;
    let source = act.source as i32;
    let accepted = act.accepted as i32;
    let entity_slug = act.entity_ulid.as_deref();

    let display = act.display.unwrap_or(true) as i32;
    let featured = act.featured.unwrap_or(false) as i32;
    let sampled = act.sampled.unwrap_or(false) as i32;

    sqlx::query!(
        r#"INSERT OR IGNORE INTO acts_of_science
           (act_id, user_id, created_at, email, country, newsletter, name,
            contributions, act, organization, num_others, others, project,
            notes, source, entity_slug, accepted, display, featured, sampled)
           VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
        act_id,
        act.user_ulid,
        act.when,
        act.email,
        act.country,
        newsletter,
        act.name,
        contributions,
        act.act,
        act.organization,
        num_others,
        act.others,
        act.project,
        act.notes,
        source,
        entity_slug,
        accepted,
        display,
        featured,
        sampled,
    )
    .execute(common::shared::db())
    .await?;

    Ok(())
}

async fn insert_golink_as_clickthrough(database: &Database, golink: &GolinkData) -> Result<()> {
    use log_service::database::tables::ClickthroughRow;

    let row = ClickthroughRow {
        when: golink.when.clone(),
        ip_address: "0.0.0.0".to_string(),
        device_id: "sync".to_string(),
        interaction_id: "sync".to_string(),
        user_id: golink.user_ulid.clone(),
        from_path: format!("/go/{}", golink.slug),
        to_url: golink.target.clone(),
        link_type: "golink".to_string(),
        program_id: None,
        geolocation: r#"{"type":"Point","coordinates":[0,0]}"#.to_string(),
    };

    database.clickthroughs.insert(row).await?;
    Ok(())
}

/// One exchange event as orrery's `/staff/sync/exchange_events` sends it.
#[derive(Debug, Clone, Deserialize)]
pub struct ExchangeEventData {
    pub id: i32,
    pub exchange: String,
    pub when: String,
    pub device_id: String,
    pub interaction_id: String,
    pub who: Option<String>,
    pub page: String,
    pub page_id: Option<String>,
    pub action: String,
    pub extra: String,
}

/// The size of a page of exchange events, as orrery sends them.
const EXCHANGE_EVENT_PAGE: usize = 1000;

/// Carry orrery's exchange events into `exchange_events`, where hydra's own
/// are recorded, so an exchange's data page reads the same history on either
/// side of the cutover.
///
/// Watermark: orrery's id, which only ever grows, so every delivery is a new
/// event. Each keeps an id derived from orrery's, which no event hydra
/// records can share.
///
/// An event recorded before orrery kept analytics ids has empty ones here
/// too, and is left out of visits and visitors: most of them were anonymous,
/// with no session cookie to count by instead. See `get_engagement_stats`.
pub async fn exchange_event_synchronize(
    database: Arc<Database>,
    cancellation_token: CancellationToken,
) {
    use log_service::database::tables::ExchangeEventRow;

    loop {
        if cancellation_token.is_cancelled() {
            println!("Exchange event sync cancelled, exiting gracefully");
            return;
        }

        let last_id = get_sync_last_id(&database, "exchange-event-sync-last-id")
            .await
            .unwrap_or(0);

        let client = reqwest::Client::new();

        let events: Vec<ExchangeEventData> = match client
            .get("https://scistarter.org/staff/sync/exchange_events")
            .bearer_auth(env!("SYNC_TOKEN"))
            .query(&[("after", last_id.to_string())])
            .send()
            .await
        {
            Ok(response) => match response.error_for_status() {
                Ok(resp) => match resp.text().await {
                    Ok(text) => match serde_json::from_str(&text) {
                        Ok(events) => events,
                        Err(e) => {
                            eprintln!("Exchange events JSON parsing error: {}", e);
                            sleep(Duration::from_secs(60)).await;
                            continue;
                        }
                    },
                    Err(e) => {
                        eprintln!("Failed to read exchange events body: {}", e);
                        sleep(Duration::from_secs(60)).await;
                        continue;
                    }
                },
                Err(e) => {
                    eprintln!("Exchange events HTTP error: {}", e);
                    sleep(Duration::from_secs(60)).await;
                    continue;
                }
            },
            Err(e) => {
                eprintln!("Exchange events request error: {}", e);
                sleep(Duration::from_secs(60)).await;
                continue;
            }
        };

        let got_events = events.len();
        let mut bookmark_id = last_id;

        for event in events {
            let when = match common::timestring::TimeString::try_from(event.when.clone()) {
                Ok(when) => when.to_string(),
                Err(e) => {
                    eprintln!("Exchange event {} has an unreadable time: {}", event.id, e);
                    break;
                }
            };

            let row = ExchangeEventRow {
                when,
                event_id: ulid::Ulid::from_parts(0, event.id as u128).to_string(),
                exchange_id: event.exchange,
                device_id: event.device_id,
                interaction_id: event.interaction_id,
                user_id: event.who,
                page: event.page,
                page_id: event.page_id,
                action: event.action,
                extra: (!event.extra.is_empty()).then_some(event.extra),
            };

            if let Err(e) = database.exchange_events.insert(row).await {
                eprintln!("Failed to insert exchange event {}: {}", event.id, e);
                break;
            }

            bookmark_id = event.id;
        }

        if bookmark_id != last_id {
            set_sync_last_id(&database, "exchange-event-sync-last-id", bookmark_id)
                .await
                .expect("Failed to save exchange event sync last_id");
        }

        // A full page means more are waiting; otherwise check back hourly.
        let sleep_duration = if got_events == EXCHANGE_EVENT_PAGE {
            Duration::from_secs(5)
        } else {
            Duration::from_secs(60 * 60)
        };

        tokio::select! {
            _ = sleep(sleep_duration) => {}
            _ = cancellation_token.cancelled() => {
                println!("Exchange event sync cancelled during sleep, exiting gracefully");
                return;
            }
        }
    }
}
