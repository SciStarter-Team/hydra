//! Importers for orrery's sync feeds.
//!
//! Each feed is paged with a watermark: the importer remembers how far it has
//! read and asks for everything after that. Which column the watermark follows
//! decides whether a delivery and an event are the same thing.
//!
//! Most feeds page on `when`, the moment the thing happened, which orrery never
//! changes. A row crosses that watermark exactly once, so every delivery is a
//! new event and logging each one is correct.
//!
//! Joins and bookmarks page on `activity`, which orrery bumps whenever the row
//! is touched. Those rows cross the watermark again on every change, and the
//! importer is handed an event it has already logged. For those two feeds a
//! delivery is *not* an event, and they consult [`claim_source_row`] before
//! writing. Adding a feed that pages on a mutable column means doing the same.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;

use log_service::database::{
    Database, get_sync_bookmark, get_sync_last_id, set_sync_bookmark, set_sync_last_id,
};
use std::sync::Arc;

/// Feed names for [`claim_source_row`]. orrery's ids are only unique within
/// one of its tables, so the ledger is keyed by both.
const JOIN_FEED: &str = "joins";
const BOOKMARK_FEED: &str = "bookmarks";

/// Claim `source_id` for `feed`, reporting whether this importer had it already.
///
/// `true` means the row has not been logged before and its events should be
/// written; `false` means this is a re-delivery of a row whose `activity`
/// moved, and writing it again would log the same event twice.
///
/// The claim is taken before the write rather than after, so that a crash
/// between the two costs one duplicated event rather than a silently dropped
/// one. A write that fails outright gives the claim back via
/// [`release_source_row`].
async fn claim_source_row(feed: &str, source_id: i32) -> Result<bool> {
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
async fn release_source_row(feed: &str, source_id: i32) {
    let released = sqlx::query!(
        r#"DELETE FROM synced_source_rows WHERE "feed" = ? AND "source_id" = ?"#,
        feed,
        source_id
    )
    .execute(common::shared::db())
    .await;

    if let Err(e) = released {
        eprintln!("Failed to release {feed} row {source_id} after a failed write: {e}");
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParticipationEventData {
    pub id: i32,
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

/// Watermark: the immutable `when`, so every delivery is a new event.
pub async fn participation_synchronize(
    database: Arc<Database>,
    cancellation_token: CancellationToken,
) {
    loop {
        // Check for cancellation at start of loop
        if cancellation_token.is_cancelled() {
            println!("Participation sync cancelled, exiting gracefully");
            return;
        }

        let last = get_sync_bookmark(&database, "participation-sync-bookmark")
            .await
            .unwrap_or_else(|_| String::from("1900-01-01T00:00:00Z"));
        let last_id = get_sync_last_id(&database, "participation-sync-last-id")
            .await
            .unwrap_or_else(|_| 0);

        println!(
            "Syncing participation events since {} with last_id {}",
            last, last_id
        );

        let sync_token = env!("SYNC_TOKEN");

        let client = reqwest::Client::new();

        let participation_events: Vec<ParticipationEventData> = match client
            .get("https://scistarter.org/staff/sync/affiliate_events")
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

        let got_events = participation_events.len();

        if got_events == 0 {
            println!("No new participation events to sync");
        } else {
            let mut bookmark = last.clone();
            let mut bookmark_id = last_id;

            for event in participation_events {
                if let Err(e) = insert_participation(&database, &event).await {
                    eprintln!("Failed to insert participation event: {}", e);
                    continue;
                }

                // Update bookmark to the latest timestamp and id
                if event.when > bookmark || (event.when == bookmark && event.id > bookmark_id) {
                    bookmark = event.when.clone();
                    bookmark_id = event.id;
                }

                println!(
                    "Inserted {} repetitions for participation event {}",
                    event.repetitions, event.id
                );
            }

            set_sync_bookmark(&database, "participation-sync-bookmark", &bookmark)
                .await
                .expect("Failed to save sync bookmark");
            set_sync_last_id(&database, "participation-sync-last-id", bookmark_id)
                .await
                .expect("Failed to save sync last_id");
        }

        // If we got the full batch size, sync again quickly, otherwise wait longer
        let sleep_duration = if got_events == 500 {
            Duration::from_secs(60)
        } else {
            Duration::from_secs(60 * 60)
        };

        // Use cancellation-aware sleep
        tokio::select! {
            _ = sleep(sleep_duration) => {}
            _ = cancellation_token.cancelled() => {
                println!("Participation sync cancelled during sleep, exiting gracefully");
                return;
            }
        }
    }
}

/// Watermark: the mutable `activity`, so a delivery is not an event and
/// rows are claimed through [`claim_source_row`] before being logged.
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
                let first_delivery = match claim_source_row(BOOKMARK_FEED, bm.id).await {
                    Ok(claimed) => claimed,
                    Err(e) => {
                        eprintln!("Failed to claim bookmark {}: {}", bm.id, e);
                        continue;
                    }
                };

                if first_delivery {
                    if let Err(e) = insert_bookmark(&database, &bm).await {
                        eprintln!("Failed to insert bookmark {}: {}", bm.id, e);
                        release_source_row(BOOKMARK_FEED, bm.id).await;
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
/// rows are claimed through [`claim_source_row`] before being logged.
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
                // first delivery is an event; see `claim_source_row`.
                let first_delivery = match join.kind {
                    0 | 1 => match claim_source_row(JOIN_FEED, join.id).await {
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
                        release_source_row(JOIN_FEED, join.id).await;
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

async fn insert_participation(database: &Database, event: &ParticipationEventData) -> Result<()> {
    use log_service::database::tables::ParticipationRow;

    // Insert one record for each repetition
    for _rep in 0..event.repetitions {
        let row = ParticipationRow {
            origin: event.origin,
            when: event.when.clone(),
            duration: event.duration.unwrap_or(1.0),
            where_point: event.where_point.clone(),
            magnitude: event.magnitude.unwrap_or(1),
            authenticated: event.authenticated.unwrap_or(false),
            extra: event.extra.clone().unwrap_or_default(),
            fidelity: event.fidelity.unwrap_or(0.5),
            entity_id: event.entity_ulid.clone(),
            user_id: event.profile_user_ulid.clone(),
            task_name: event.task_name.clone(),
        };

        database.participation.insert(row).await?;
    }

    // Update cache and send SSE notification if user participated
    if let Some(user_id_str) = &event.profile_user_ulid {
        // Update cache (ignore if already exists)
        sqlx::query!(
            r#"
            INSERT OR IGNORE INTO project_participations_cache (user_id, project_id)
            VALUES (?, ?)
            "#,
            user_id_str,
            event.entity_ulid
        )
        .execute(common::shared::db())
        .await?;

        // Send SSE notification to user
        if let Ok(user_id) = user_id_str.parse::<ulid::Ulid>() {
            let _ = common::channel::send_to_user(
                user_id,
                "completion",
                common::channel::Event::new("project_participation", &event.entity_ulid),
            )
            .await;
        }
    }

    Ok(())
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
