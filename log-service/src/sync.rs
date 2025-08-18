use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;

use log_service::database::{
    Database, get_sync_bookmark, get_sync_last_id, set_sync_bookmark, set_sync_last_id,
};
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParticipationEventData {
    pub id: i32,
    pub origin: i16,
    pub when: String, // ISO 8601 timestamp string
    pub duration: f64,
    pub where_point: Option<String>,
    pub magnitude: i32,
    pub authenticated: bool,
    pub extra: String,
    pub repetitions: i32,
    pub fidelity: f64,
    pub entity_ulid: String,
    pub profile_user_ulid: String,
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
}

// Participation sync functions
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

        let sync_token =
            std::env::var("SYNC_TOKEN").expect("SYNC_TOKEN environment variable must be set");

        let client = reqwest::Client::new();

        let participation_events: Vec<ParticipationEventData> = match client
            .get("https://scistarter.org/staff/sync/affiliate_events")
            .bearer_auth(sync_token)
            .query(&[("since", &last), ("last_id", &last_id.to_string())])
            .send()
            .await
        {
            Ok(response) => match response.error_for_status() {
                Ok(resp) => match resp.json().await {
                    Ok(events) => events,
                    Err(e) => {
                        eprintln!("JSON parsing error: {}", e);
                        sleep(Duration::from_secs(60)).await;
                        continue;
                    }
                },
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

        let sync_token =
            std::env::var("SYNC_TOKEN").expect("SYNC_TOKEN environment variable must be set");
        let client = reqwest::Client::new();

        let bookmarks: Vec<BookmarkData> = match client
            .get("https://scistarter.org/staff/sync/bookmarks")
            .bearer_auth(sync_token)
            .query(&[("since", &last), ("last_id", &last_id.to_string())])
            .send()
            .await
        {
            Ok(response) => match response.error_for_status() {
                Ok(resp) => match resp.json().await {
                    Ok(events) => events,
                    Err(e) => {
                        eprintln!("JSON parsing error: {}", e);
                        sleep(Duration::from_secs(60)).await;
                        continue;
                    }
                },
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
                if let Err(e) = insert_bookmark(&database, &bm).await {
                    eprintln!("Failed to insert bookmark: {}", e);
                    continue;
                }

                if bm.activity > bookmark || (bm.activity == bookmark && bm.id > bookmark_id) {
                    bookmark = bm.activity.clone();
                    bookmark_id = bm.id;
                }

                println!("Inserted bookmark {}", bm.id);
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

        let sync_token =
            std::env::var("SYNC_TOKEN").expect("SYNC_TOKEN environment variable must be set");
        let client = reqwest::Client::new();

        let joins: Vec<JoinData> = match client
            .get("https://scistarter.org/staff/sync/joins")
            .bearer_auth(sync_token)
            .query(&[("since", &last), ("last_id", &last_id.to_string())])
            .send()
            .await
        {
            Ok(response) => match response.error_for_status() {
                Ok(resp) => match resp.json().await {
                    Ok(events) => events,
                    Err(e) => {
                        eprintln!("JSON parsing error: {}", e);
                        sleep(Duration::from_secs(60)).await;
                        continue;
                    }
                },
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
                match join.kind {
                    0 => {
                        if let Err(e) = insert_join_as_clickthrough(&database, &join).await {
                            eprintln!("Failed to insert join as clickthrough: {}", e);
                            continue;
                        }
                        println!("Inserted join {} as clickthrough", join.id);
                    }
                    1 => {
                        if let Err(e) = insert_join_as_clickthrough(&database, &join).await {
                            eprintln!("Failed to insert join as clickthrough: {}", e);
                            continue;
                        }
                        if let Err(e) = insert_join(&database, &join).await {
                            eprintln!("Failed to insert join: {}", e);
                            continue;
                        }
                        println!("Inserted join {} as both clickthrough and join", join.id);
                    }
                    _ => {
                        println!("Skipping join {} with kind {}", join.id, join.kind);
                        continue;
                    }
                }

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

async fn insert_participation(database: &Database, event: &ParticipationEventData) -> Result<()> {
    use log_service::database::tables::ParticipationRow;

    // Insert one record for each repetition
    for _rep in 0..event.repetitions {
        let row = ParticipationRow {
            origin: event.origin,
            when: event.when.clone(),
            duration: event.duration,
            where_point: event.where_point.clone(),
            magnitude: event.magnitude,
            authenticated: event.authenticated,
            extra: event.extra.clone(),
            fidelity: event.fidelity,
            entity_id: event.entity_ulid.clone(),
            user_id: event.profile_user_ulid.clone(),
            task_name: event.task_name.clone(),
        };

        database.participation.insert(row).await?;
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
        session_id: format!("join-{}", join.id),
        interaction_id: format!("join-{}", join.id),
        user_id: Some(join.user_ulid.clone()),
        from_path: "join".to_string(),
        to_url: format!("entity/{}", join.entity_ulid),
        link_type: "join".to_string(),
        program_id: Some(join.entity_ulid.clone()),
        geolocation: "POINT(0 0)".to_string(),
    };

    database.clickthroughs.insert(row).await?;
    Ok(())
}
