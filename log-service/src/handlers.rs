use anyhow::Result;
use common::{message::log::AnalyticsQuery, service::decode_structured_message};
use duckdb::{params, Connection};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use tokio::sync::mpsc;

use crate::{analytics::{get_clickthrough_analytics, get_join_analytics}, cache::update_user_tags_cache, database::update_clickthrough_summaries, database::update_join_summaries};

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
    Join {
        when: String,
        entity_id: String,
        user_id: String,
        active: bool,
        consented: Option<String>,
        contacted: Option<String>,
        notified: bool,
        activity: String,
        reason: String,
    },
}

impl LogEntry {
    pub fn write_to_database(&self, conn: &Connection) -> Result<()> {
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
                // Update summary tables for this date
                if let Err(e) = update_clickthrough_summaries(conn, when) {
                    eprintln!("Warning: Failed to update clickthrough summaries: {}", e);
                }
            }
            LogEntry::UserTagsUpdate { user_id: _, tags: _ } => {
                // This is handled directly by update_user_tags_cache, not through the database writer
                return Ok(());
            }
            LogEntry::Join { when, entity_id, user_id, active, consented, contacted, notified, activity, reason } => {
                let when_timestamp = Timestamp::from_str(when)?;
                let activity_timestamp = Timestamp::from_str(activity)?;
                let consented_timestamp = consented.as_ref()
                    .map(|s| Timestamp::from_str(s))
                    .transpose()?;
                let contacted_timestamp = contacted.as_ref()
                    .map(|s| Timestamp::from_str(s))
                    .transpose()?;
                
                conn.execute(
                    r#"INSERT INTO join_table (
                        when_timestamp, entity_id, user_id, active, consented, contacted,
                        explicit, notified, activity, reason
                    ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
                    params![
                        when_timestamp.to_string(),
                        entity_id,
                        user_id,
                        active,
                        consented_timestamp.map(|t| t.to_string()),
                        contacted_timestamp.map(|t| t.to_string()),
                        true, // All new joins are explicit
                        notified,
                        activity_timestamp.to_string(),
                        reason
                    ],
                )?;
                // Update summary tables for this date
                if let Err(e) = update_join_summaries(conn, when) {
                    eprintln!("Warning: Failed to update join summaries: {}", e);
                }
            }
        }
        Ok(())
    }
}

pub async fn database_writer(mut rx: mpsc::Receiver<LogEntry>) -> Result<()> {
    use crate::database::create_connection;
    let conn = create_connection()?;
    
    while let Some(entry) = rx.recv().await {
        if let Err(e) = entry.write_to_database(&conn) {
            eprintln!("Database write error: {}", e);
        }
    }
    
    Ok(())
}

pub async fn handle_activity_log(req: &async_nats::service::Request, tx: &mpsc::Sender<LogEntry>) -> Result<()> {
    let (when, action, user_id, target, extra): (String, String, Option<String>, Option<String>, Option<String>) = 
        decode_structured_message(req.message.clone())?;
    
    let entry = LogEntry::Activity { when, action, user_id, target, extra };
    
    if let Err(e) = tx.send(entry).await {
        eprintln!("Failed to queue activity log entry: {}", e);
    }
    
    Ok(())
}

pub async fn handle_page_view(req: &async_nats::service::Request, tx: &mpsc::Sender<LogEntry>) -> Result<()> {
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

pub async fn handle_clickthrough(req: &async_nats::service::Request, tx: &mpsc::Sender<LogEntry>) -> Result<()> {
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

pub async fn handle_user_tags_update(req: &async_nats::service::Request) -> Result<()> {
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

pub async fn handle_join(req: &async_nats::service::Request, tx: &mpsc::Sender<LogEntry>) -> Result<()> {
    let (when, entity_id, user_id, active, consented, contacted, notified, activity, reason): 
        (String, String, String, bool, Option<String>, Option<String>, bool, String, String) = 
        decode_structured_message(req.message.clone())?;
    
    let entry = LogEntry::Join { 
        when, entity_id, user_id, active, consented, contacted, notified, activity, reason 
    };
    
    if let Err(e) = tx.send(entry).await {
        eprintln!("Failed to queue join entry: {}", e);
    }
    
    Ok(())
}

pub async fn handle_clickthrough_analytics(req: &async_nats::service::Request) -> Result<()> {
    let query: AnalyticsQuery = decode_structured_message(req.message.clone())?;
    
    let response = tokio::task::spawn_blocking(move || get_clickthrough_analytics(query)).await??;
    
    let response_data = flexbuffers::to_vec(&response)?;
    req.respond(Ok(response_data.into())).await?;
    
    Ok(())
}

pub async fn handle_join_analytics(req: &async_nats::service::Request) -> Result<()> {
    let query: AnalyticsQuery = decode_structured_message(req.message.clone())?;
    
    let response = tokio::task::spawn_blocking(move || get_join_analytics(query)).await??;
    
    let response_data = flexbuffers::to_vec(&response)?;
    req.respond(Ok(response_data.into())).await?;
    
    Ok(())
}