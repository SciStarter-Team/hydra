use anyhow::Result;
use async_nats::service::ServiceExt;
use common::{service::decode_structured_message, shared};
use duckdb::{params, Connection};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;
use tokio::{sync::mpsc, task::JoinSet};

static DATABASE_PATH: OnceLock<String> = OnceLock::new();

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LogEntry {
    Activity {
        when: String,
        action: String,
        user_id: Option<String>,
        target: Option<String>,
        extra: Option<String>,
    },
    PageView {
        when: String,
        ip_address: String,
        session_id: String,
        interaction_id: String, // Unique identifier for this interaction
        user_id: Option<String>, // Can be null for anonymous users
        path: String,
        referrer: Option<String>, // Can be null for direct visits
        query: Option<String>, // Can be null if no query params
        geolocation: String, // GeoJSON point (required)
    },
    Clickthrough {
        when: String,
        ip_address: String,
        session_id: String,
        interaction_id: String, // Unique identifier for this interaction
        user_id: Option<String>, // Can be null for anonymous users
        from_path: String,
        to_url: String,
        link_type: String, // "project_page", "actual_project", etc.
        program_id: Option<String>, // Can be null for non-program links
        geolocation: String, // GeoJSON point (required)
    },
    UserTagsUpdate {
        user_id: String,
        tags: Vec<String>,
    },
}

fn get_database_path() -> &'static str {
    DATABASE_PATH.get().expect("Database path not initialized")
}

fn init_database(database_path: &str) -> Result<()> {
    let conn = Connection::open(database_path)?;
    let schema = include_str!("tables.sql");
    conn.execute_batch(schema)?;
    Ok(())
}

impl LogEntry {
    fn write_to_database(&self, conn: &Connection) -> Result<()> {
        match self {
            LogEntry::Activity { when, action, user_id, target, extra } => {
                conn.execute(
                    "INSERT INTO activity_log (when_timestamp, action, user_id, target, extra) VALUES (?, ?, ?, ?, ?)",
                    params![when, action, user_id, target, extra],
                )?;
            }
            LogEntry::PageView { when, ip_address, session_id, interaction_id, user_id, path, referrer, query, geolocation } => {
                conn.execute(
                    "INSERT INTO page_views (when_timestamp, ip_address, session_id, interaction_id, user_id, path, referrer, query, geolocation) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
                    params![when, ip_address, session_id, interaction_id, user_id, path, referrer, query, geolocation],
                )?;
            }
            LogEntry::Clickthrough { when, ip_address, session_id, interaction_id, user_id, from_path, to_url, link_type, program_id, geolocation } => {
                conn.execute(
                    "INSERT INTO clickthroughs (when_timestamp, ip_address, session_id, interaction_id, user_id, from_path, to_url, link_type, program_id, geolocation) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                    params![when, ip_address, session_id, interaction_id, user_id, from_path, to_url, link_type, program_id, geolocation],
                )?;
            }
            LogEntry::UserTagsUpdate { user_id: _, tags: _ } => {
                // This is handled directly by update_user_tags_cache, not through the database writer
                return Ok(());
            }
        }
        Ok(())
    }
}

async fn database_writer(mut rx: mpsc::Receiver<LogEntry>) -> Result<()> {
    let conn = Connection::open(get_database_path())?;
    
    while let Some(entry) = rx.recv().await {
        if let Err(e) = entry.write_to_database(&conn) {
            eprintln!("Database write error: {}", e);
        }
    }
    
    Ok(())
}

async fn handle_activity_log(req: &async_nats::service::Request, tx: &mpsc::Sender<LogEntry>) -> Result<()> {
    let (when, action, user_id, target, extra): (String, String, Option<String>, Option<String>, Option<String>) = 
        decode_structured_message(req.message.clone())?;
    
    let entry = LogEntry::Activity { when, action, user_id, target, extra };
    
    if let Err(e) = tx.send(entry).await {
        eprintln!("Failed to queue activity log entry: {}", e);
    }
    
    Ok(())
}

async fn handle_page_view(req: &async_nats::service::Request, tx: &mpsc::Sender<LogEntry>) -> Result<()> {
    let (when, ip_address, session_id, interaction_id, user_id, path, referrer, query, geolocation): 
        (String, String, String, String, Option<String>, String, Option<String>, Option<String>, String) = 
        decode_structured_message(req.message.clone())?;
    
    let entry = LogEntry::PageView { 
        when, ip_address, session_id, interaction_id, user_id, path, referrer, query, geolocation 
    };
    
    if let Err(e) = tx.send(entry).await {
        eprintln!("Failed to queue page view entry: {}", e);
    }
    
    Ok(())
}

async fn handle_clickthrough(req: &async_nats::service::Request, tx: &mpsc::Sender<LogEntry>) -> Result<()> {
    let (when, ip_address, session_id, interaction_id, user_id, from_path, to_url, link_type, program_id, geolocation): 
        (String, String, String, String, Option<String>, String, String, String, Option<String>, String) = 
        decode_structured_message(req.message.clone())?;
    
    let entry = LogEntry::Clickthrough { 
        when, ip_address, session_id, interaction_id, user_id, from_path, to_url, link_type, program_id, geolocation 
    };
    
    if let Err(e) = tx.send(entry).await {
        eprintln!("Failed to queue clickthrough entry: {}", e);
    }
    
    Ok(())
}

async fn handle_user_tags_update(req: &async_nats::service::Request) -> Result<()> {
    let (user_id, tags): (String, Vec<String>) = decode_structured_message(req.message.clone())?;
    
    // Update cache directly, with retry mechanism
    for _attempt in 0..3 {
        if update_user_tags_cache(user_id.clone(), tags.clone()).await.is_ok() {
            break;
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
    }
    
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    shared::init().await;

    let database_path = std::env::var("DATABASE_URL")
        .expect("DATABASE_URL is not set");
    
    DATABASE_PATH.set(database_path.clone()).map_err(|_| anyhow::anyhow!("Failed to set database path"))?;
    
    init_database(&database_path)?;

    let (tx, rx) = mpsc::channel::<LogEntry>(1000);
    
    let mut tasks = JoinSet::new();

    // Spawn database writer task
    tasks.spawn(database_writer(rx));

    let svc = shared::nats()
        .service_builder()
        .start("log", "0.1.0")
        .await
        .map_err(|e| anyhow::anyhow!("Failed to start service: {}", e))?;

    let log_group = svc.group("log");

    // Create all endpoints first
    let activity_endpoint = log_group
        .endpoint("activity")
        .await
        .expect("Unable to create activity endpoint");
    
    let pageview_endpoint = log_group
        .endpoint("pageview")
        .await
        .expect("Unable to create pageview endpoint");
    
    let clickthrough_endpoint = log_group
        .endpoint("clickthrough")
        .await
        .expect("Unable to create clickthrough endpoint");
    
    let user_tags_endpoint = log_group
        .endpoint("user_tags_update")
        .await
        .expect("Unable to create user_tags_update endpoint");

    // Spawn handlers for each endpoint
    let tx_activity = tx.clone();
    tasks.spawn(async move {
        let handler = activity_endpoint.for_each_concurrent(None, move |req| {
            let tx = tx_activity.clone();
            async move {
                if let Err(e) = handle_activity_log(&req, &tx).await {
                    eprintln!("Error handling activity log: {}", e);
                }
            }
        });
        handler.await;
        Ok::<(), anyhow::Error>(())
    });

    let tx_pageview = tx.clone();
    tasks.spawn(async move {
        let handler = pageview_endpoint.for_each_concurrent(None, move |req| {
            let tx = tx_pageview.clone();
            async move {
                if let Err(e) = handle_page_view(&req, &tx).await {
                    eprintln!("Error handling page view: {}", e);
                }
            }
        });
        handler.await;
        Ok::<(), anyhow::Error>(())
    });

    let tx_clickthrough = tx.clone();
    tasks.spawn(async move {
        let handler = clickthrough_endpoint.for_each_concurrent(None, move |req| {
            let tx = tx_clickthrough.clone();
            async move {
                if let Err(e) = handle_clickthrough(&req, &tx).await {
                    eprintln!("Error handling clickthrough: {}", e);
                }
            }
        });
        handler.await;
        Ok::<(), anyhow::Error>(())
    });

    tasks.spawn(async move {
        let handler = user_tags_endpoint.for_each_concurrent(None, move |req| {
            async move {
                if let Err(e) = handle_user_tags_update(&req).await {
                    eprintln!("Error handling user tags update: {}", e);
                }
            }
        });
        handler.await;
        Ok::<(), anyhow::Error>(())
    });

    println!("Log service started and listening for messages on:");
    println!("  - log.activity");
    println!("  - log.pageview");
    println!("  - log.clickthrough");
    println!("  - log.user_tags_update");

    // Wait for any task to complete (which would indicate an error)
    while let Some(result) = tasks.join_next().await {
        match result {
            Ok(task_result) => {
                if let Err(e) = task_result {
                    eprintln!("Task completed with error: {}", e);
                }
            }
            Err(e) => {
                eprintln!("Task panicked: {}", e);
            }
        }
        eprintln!("A task has completed unexpectedly, shutting down...");
        break;
    }

    Ok(())
}

fn create_connection() -> Result<Connection> {
    Connection::open(get_database_path()).map_err(Into::into)
}

// User tags cache management functions
async fn update_user_tags_cache(user_id: String, tags: Vec<String>) -> Result<()> {
    let conn = create_connection()?;
    
    // Use DuckDB's conflict resolution instead of explicit transactions
    // First, remove existing tags for this user
    conn.execute("DELETE FROM user_tags WHERE user_id = ?", params![user_id])?;
    
    // Insert new tags using INSERT OR IGNORE for conflict resolution
    for tag in tags {
        conn.execute(
            "INSERT OR IGNORE INTO user_tags (user_id, tag) VALUES (?, ?)",
            params![user_id, tag]
        )?;
    }
    
    Ok(())
}

#[allow(dead_code)]
async fn get_user_tags_from_cache(user_id: &str) -> Result<Option<Vec<String>>> {
    let conn = create_connection()?;
    
    let mut stmt = conn.prepare("SELECT tag FROM user_tags WHERE user_id = ? ORDER BY tag")?;
    let tag_iter = stmt.query_map([user_id], |row| {
        Ok(row.get::<_, String>(0)?)
    })?;
    
    let tags: Result<Vec<String>, _> = tag_iter.collect();
    let tags = tags?;
    
    if tags.is_empty() {
        Ok(None) // No cache entry found
    } else {
        Ok(Some(tags))
    }
}

#[allow(dead_code)]
async fn fetch_user_tags_from_service(user_id: &str) -> Result<Vec<String>> {
    use common::service::request;
    
    // Call user service to get user tags
    let tags: Vec<String> = request("user.tags.get", user_id).await.unwrap_or_default();
    Ok(tags)
}

#[allow(dead_code)]
async fn get_user_tags(user_id: &str) -> Result<Vec<String>> {
    // Try cache first
    if let Some(tags) = get_user_tags_from_cache(user_id).await? {
        return Ok(tags);
    }
    
    // Cache miss - fetch from user service and cache
    let tags = fetch_user_tags_from_service(user_id).await?;
    if !tags.is_empty() {
        // Use a retry mechanism for cache updates in case of conflicts
        for _attempt in 0..3 {
            if update_user_tags_cache(user_id.to_string(), tags.clone()).await.is_ok() {
                break;
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        }
    }
    
    Ok(tags)
}

#[allow(dead_code)]
async fn add_user_tag(user_id: &str, tag: &str) -> Result<()> {
    let conn = create_connection()?;
    conn.execute(
        "INSERT OR IGNORE INTO user_tags (user_id, tag) VALUES (?, ?)",
        params![user_id, tag]
    )?;
    Ok(())
}

#[allow(dead_code)]
async fn remove_user_tag(user_id: &str, tag: &str) -> Result<()> {
    let conn = create_connection()?;
    conn.execute(
        "DELETE FROM user_tags WHERE user_id = ? AND tag = ?",
        params![user_id, tag]
    )?;
    Ok(())
}

#[allow(dead_code)]
async fn remove_user_from_cache(user_id: &str) -> Result<()> {
    let conn = create_connection()?;
    conn.execute("DELETE FROM user_tags WHERE user_id = ?", params![user_id])?;
    Ok(())
}

#[allow(dead_code)]
async fn get_users_with_tag(tag: &str) -> Result<Vec<String>> {
    let conn = create_connection()?;
    
    let mut stmt = conn.prepare("SELECT DISTINCT user_id FROM user_tags WHERE tag = ?")?;
    let user_iter = stmt.query_map([tag], |row| {
        Ok(row.get::<_, String>(0)?)
    })?;
    
    let users: Result<Vec<String>, _> = user_iter.collect();
    users.map_err(Into::into)
}

#[allow(dead_code)]
async fn get_popular_tags(limit: usize) -> Result<Vec<(String, i64)>> {
    let conn = create_connection()?;
    
    let mut stmt = conn.prepare("SELECT tag, COUNT(*) as count FROM user_tags GROUP BY tag ORDER BY count DESC LIMIT ?")?;
    let tag_iter = stmt.query_map([limit], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?;
    
    let tags: Result<Vec<(String, i64)>, _> = tag_iter.collect();
    tags.map_err(Into::into)
}
