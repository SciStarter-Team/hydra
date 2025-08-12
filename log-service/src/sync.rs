use anyhow::Result;
use duckdb::{params, Connection};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use std::{str::FromStr, time::Duration};
use tokio::time::sleep;

use crate::database::{create_connection, get_sync_bookmark, set_sync_bookmark, get_sync_last_id, set_sync_last_id};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AffiliateEventData {
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
    pub activity: String, // ISO 8601 timestamp string
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

// Affiliate sync functions
pub async fn affiliate_synchronize() {
    loop {
        let last = get_sync_bookmark("affiliate-sync-bookmark").unwrap_or_else(|_| String::from("1900-01-01T00:00:00Z"));
        let last_id = get_sync_last_id("affiliate-sync-last-id").unwrap_or_else(|_| 0);
        
        println!("Syncing affiliate events since {} with last_id {}", last, last_id);
        
        let sync_token = std::env::var("SYNC_TOKEN").expect("SYNC_TOKEN environment variable must be set");
        
        let client = reqwest::Client::new();
        
        let affiliate_events: Vec<AffiliateEventData> = match client
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
        
        let got_events = affiliate_events.len();
        
        if got_events == 0 {
            println!("No new affiliate events to sync");
        } else {
            let conn = create_connection().expect("Failed to connect to database");
            let mut bookmark = last.clone();
            let mut bookmark_id = last_id;
            
            for event in affiliate_events {
                if let Err(e) = insert_affiliate_event(&conn, &event) {
                    eprintln!("Failed to insert affiliate event: {}", e);
                    continue;
                }
                
                // Update bookmark to the latest timestamp and id
                if event.when > bookmark || (event.when == bookmark && event.id > bookmark_id) {
                    bookmark = event.when.clone();
                    bookmark_id = event.id;
                }
                
                println!("Inserted {} repetitions for affiliate event {}", event.repetitions, event.id);
            }
            
            set_sync_bookmark("affiliate-sync-bookmark", &bookmark).expect("Failed to save sync bookmark");
            set_sync_last_id("affiliate-sync-last-id", bookmark_id).expect("Failed to save sync last_id");
        }
        
        // If we got the full batch size, sync again quickly, otherwise wait longer
        if got_events == 500 {
            sleep(Duration::from_secs(60)).await;
        } else {
            sleep(Duration::from_secs(60 * 60)).await;
        }
    }
}

pub async fn bookmark_synchronize() {
    loop {
        let last = get_sync_bookmark("bookmark-sync-bookmark").unwrap_or_else(|_| String::from("1900-01-01T00:00:00Z"));
        let last_id = get_sync_last_id("bookmark-sync-last-id").unwrap_or_else(|_| 0);
        
        println!("Syncing bookmarks since {} with last_id {}", last, last_id);
        
        let sync_token = std::env::var("SYNC_TOKEN").expect("SYNC_TOKEN environment variable must be set");
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
            let conn = create_connection().expect("Failed to connect to database");
            let mut bookmark = last.clone();
            let mut bookmark_id = last_id;
            
            for bm in bookmarks {
                if let Err(e) = insert_bookmark(&conn, &bm) {
                    eprintln!("Failed to insert bookmark: {}", e);
                    continue;
                }
                
                if bm.activity > bookmark || (bm.activity == bookmark && bm.id > bookmark_id) {
                    bookmark = bm.activity.clone();
                    bookmark_id = bm.id;
                }
                
                println!("Inserted bookmark {}", bm.id);
            }
            
            set_sync_bookmark("bookmark-sync-bookmark", &bookmark).expect("Failed to save sync bookmark");
            set_sync_last_id("bookmark-sync-last-id", bookmark_id).expect("Failed to save sync last_id");
        }
        
        if got_bookmarks == 500 {
            sleep(Duration::from_secs(60)).await;
        } else {
            sleep(Duration::from_secs(60 * 60)).await;
        }
    }
}

pub async fn join_synchronize() {
    loop {
        let last = get_sync_bookmark("join-sync-bookmark").unwrap_or_else(|_| String::from("1900-01-01T00:00:00Z"));
        let last_id = get_sync_last_id("join-sync-last-id").unwrap_or_else(|_| 0);
        
        println!("Syncing joins since {} with last_id {}", last, last_id);
        
        let sync_token = std::env::var("SYNC_TOKEN").expect("SYNC_TOKEN environment variable must be set");
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
            let conn = create_connection().expect("Failed to connect to database");
            let mut bookmark = last.clone();
            let mut bookmark_id = last_id;
            
            for join in joins {
                match join.kind {
                    0 => {
                        if let Err(e) = insert_join_as_clickthrough(&conn, &join) {
                            eprintln!("Failed to insert join as clickthrough: {}", e);
                            continue;
                        }
                        println!("Inserted join {} as clickthrough", join.id);
                    },
                    1 => {
                        if let Err(e) = insert_join_as_clickthrough(&conn, &join) {
                            eprintln!("Failed to insert join as clickthrough: {}", e);
                            continue;
                        }
                        if let Err(e) = insert_join(&conn, &join) {
                            eprintln!("Failed to insert join: {}", e);
                            continue;
                        }
                        println!("Inserted join {} as both clickthrough and join", join.id);
                    },
                    _ => {
                        println!("Skipping join {} with kind {}", join.id, join.kind);
                        continue;
                    }
                }
                
                if join.activity > bookmark || (join.activity == bookmark && join.id > bookmark_id) {
                    bookmark = join.activity.clone();
                    bookmark_id = join.id;
                }
            }
            
            set_sync_bookmark("join-sync-bookmark", &bookmark).expect("Failed to save sync bookmark");
            set_sync_last_id("join-sync-last-id", bookmark_id).expect("Failed to save sync last_id");
        }
        
        if got_joins == 500 {
            sleep(Duration::from_secs(60)).await;
        } else {
            sleep(Duration::from_secs(60 * 60)).await;
        }
    }
}



fn insert_affiliate_event(conn: &Connection, event: &AffiliateEventData) -> Result<()> {
    // Parse the timestamp - jiff can parse ISO 8601 directly
    let timestamp = Timestamp::from_str(&event.when)?;
    let timestamp_str = timestamp.to_string();
    
    // Insert one record for each repetition
    for _rep in 0..event.repetitions {
        conn.execute(
            r#"INSERT INTO affiliate (
                origin, when_timestamp, duration, where_point, magnitude, 
                authenticated, extra, fidelity, entity_id, user_id, task_name
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
            params![
                event.origin,
                timestamp_str,
                event.duration,
                event.where_point,
                event.magnitude,
                event.authenticated,
                event.extra,
                event.fidelity,
                event.entity_ulid,
                event.profile_user_ulid,
                event.task_name
            ],
        )?;
    }
    
    Ok(())
}

fn insert_bookmark(conn: &Connection, bookmark: &BookmarkData) -> Result<()> {
    let when_timestamp = Timestamp::from_str(&bookmark.when)?;
    let activity_timestamp = Timestamp::from_str(&bookmark.activity)?;
    let contacted_timestamp = bookmark.contacted.as_ref()
        .map(|s| Timestamp::from_str(s))
        .transpose()?;
    
    conn.execute(
        r#"INSERT INTO bookmark (
            when_timestamp, entity_id, user_id, active, activity, contacted
        ) VALUES (?, ?, ?, ?, ?, ?)"#,
        params![
            when_timestamp.to_string(),
            bookmark.entity_ulid,
            bookmark.user_ulid,
            bookmark.active,
            activity_timestamp.to_string(),
            contacted_timestamp.map(|t| t.to_string())
        ],
    )?;
    Ok(())
}

fn insert_join(conn: &Connection, join: &JoinData) -> Result<()> {
    let when_timestamp = Timestamp::from_str(&join.when)?;
    let activity_timestamp = Timestamp::from_str(&join.activity)?;
    let consented_timestamp = join.consented.as_ref()
        .map(|s| Timestamp::from_str(s))
        .transpose()?;
    let contacted_timestamp = join.contacted.as_ref()
        .map(|s| Timestamp::from_str(s))
        .transpose()?;
    
    conn.execute(
        r#"INSERT INTO join_table (
            when_timestamp, entity_id, user_id, active, consented, contacted,
            explicit, notified, activity, reason
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
        params![
            when_timestamp.to_string(),
            join.entity_ulid,
            join.user_ulid,
            join.active,
            consented_timestamp.map(|t| t.to_string()),
            contacted_timestamp.map(|t| t.to_string()),
            join.explicit,
            join.notified,
            activity_timestamp.to_string(),
            join.reason
        ],
    )?;
    Ok(())
}

fn insert_join_as_clickthrough(conn: &Connection, join: &JoinData) -> Result<()> {
    let when_timestamp = Timestamp::from_str(&join.when)?;
    
    conn.execute(
        r#"INSERT INTO clickthroughs (
            when_timestamp, ip_address, session_id, interaction_id, user_id,
            from_path, to_url, link_type, program_id, geolocation
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
        params![
            when_timestamp.to_string(),
            "0.0.0.0",
            format!("join-{}", join.id),
            format!("join-{}", join.id),  
            join.user_ulid,
            "join",
            format!("entity/{}", join.entity_ulid),
            "join",
            join.entity_ulid,
            "POINT(0 0)"
        ],
    )?;
    Ok(())
}