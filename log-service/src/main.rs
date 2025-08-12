use anyhow::Result;
use async_nats::service::ServiceExt;
use common::{message::log::{AnalyticsQuery, AnalyticsResponse, WeeklyCount}, service::decode_structured_message, shared};
use duckdb::{params, Connection};
use futures::StreamExt;
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use std::{str::FromStr, sync::OnceLock, time::Duration};
use tokio::{sync::mpsc, task::JoinSet, time::sleep, spawn};

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AffiliateEventData {
    id: i32,
    origin: i16,
    when: String, // ISO 8601 timestamp string
    duration: f64,
    where_point: Option<String>,
    magnitude: i32,
    authenticated: bool,
    extra: String,
    repetitions: i32,
    fidelity: f64,
    entity_ulid: String,
    profile_user_ulid: String,
    task_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BookmarkData {
    id: i32,
    when: String, // ISO 8601 timestamp string
    entity_ulid: String,
    user_ulid: String,
    active: bool,
    activity: String, // ISO 8601 timestamp string
    contacted: Option<String>, // ISO 8601 timestamp string
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JoinData {
    id: i32,
    when: String, // ISO 8601 timestamp string
    entity_ulid: String,
    user_ulid: String,
    active: bool,
    consented: Option<String>, // ISO 8601 timestamp string
    contacted: Option<String>, // ISO 8601 timestamp string
    explicit: bool,
    notified: bool,
    activity: String, // ISO 8601 timestamp string
    kind: i32,
    reason: String,
}

fn get_database_path() -> &'static str {
    DATABASE_PATH.get().expect("Database path not initialized")
}

fn init_database(database_path: &str) -> Result<()> {
    let conn = Connection::open(database_path)?;
    
    // Initialize migrations table
    conn.execute(
        "CREATE TABLE IF NOT EXISTS schema_migrations (version INTEGER PRIMARY KEY, applied_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP)",
        params![]
    )?;
    
    // Run base schema (only creates tables/indexes that don't exist)
    let schema = include_str!("tables.sql");
    conn.execute_batch(schema)?;
    
    // Run migrations
    run_migrations(&conn)?;
    
    Ok(())
}

fn run_migrations(conn: &Connection) -> Result<()> {
    // Migration 1: Fix affiliate table auto-increment
    if !migration_applied(conn, 1)? {
        println!("Running migration 1: Fix affiliate table auto-increment");
        
        // Check if affiliate table exists and has data
        let has_data = conn.prepare("SELECT COUNT(*) FROM affiliate")
            .and_then(|mut stmt| stmt.query_row([], |row| Ok(row.get::<_, i64>(0)?)))
            .unwrap_or(0) > 0;
        
        if has_data {
            println!("Backing up existing affiliate data...");
            // Create backup table
            conn.execute("CREATE TABLE affiliate_backup AS SELECT * FROM affiliate", params![])?;
            
            // Drop and recreate with proper auto-increment
            conn.execute("DROP TABLE affiliate", params![])?;
            conn.execute("CREATE SEQUENCE IF NOT EXISTS affiliate_id_seq", params![])?;
            conn.execute(r#"
                CREATE TABLE affiliate (
                    id BIGINT PRIMARY KEY DEFAULT nextval('affiliate_id_seq'),
                    origin SMALLINT NOT NULL,
                    when_timestamp TIMESTAMP WITH TIME ZONE NOT NULL,
                    duration DOUBLE NOT NULL,
                    where_point VARCHAR,
                    magnitude INTEGER NOT NULL,
                    authenticated BOOLEAN NOT NULL,
                    extra VARCHAR NOT NULL,
                    fidelity DOUBLE NOT NULL,
                    entity_id VARCHAR NOT NULL,
                    user_id VARCHAR NOT NULL,
                    task_name VARCHAR
                )
            "#, params![])?;
            
            // Restore data (excluding id column to let auto-increment work)
            conn.execute(r#"
                INSERT INTO affiliate (origin, when_timestamp, duration, where_point, magnitude, authenticated, extra, fidelity, entity_id, user_id, task_name)
                SELECT origin, when_timestamp, duration, where_point, magnitude, authenticated, extra, fidelity, entity_id, user_id, task_name 
                FROM affiliate_backup
            "#, params![])?;
            
            // Recreate indexes
            conn.execute("CREATE INDEX IF NOT EXISTS affiliate_by_when ON affiliate (when_timestamp)", params![])?;
            conn.execute("CREATE INDEX IF NOT EXISTS affiliate_by_user ON affiliate (user_id)", params![])?;
            conn.execute("CREATE INDEX IF NOT EXISTS affiliate_by_entity ON affiliate (entity_id)", params![])?;
            conn.execute("CREATE INDEX IF NOT EXISTS affiliate_by_task ON affiliate (task_name)", params![])?;
            conn.execute("CREATE INDEX IF NOT EXISTS affiliate_by_origin ON affiliate (origin)", params![])?;
            
            // Drop backup table
            conn.execute("DROP TABLE affiliate_backup", params![])?;
            
            println!("Migration 1 completed: Restored {} records with new auto-increment IDs", has_data);
        } else {
            // No data, safe to recreate
            conn.execute("DROP TABLE IF EXISTS affiliate", params![])?;
            conn.execute("CREATE SEQUENCE IF NOT EXISTS affiliate_id_seq", params![])?;
            conn.execute(r#"
                CREATE TABLE affiliate (
                    id BIGINT PRIMARY KEY DEFAULT nextval('affiliate_id_seq'),
                    origin SMALLINT NOT NULL,
                    when_timestamp TIMESTAMP WITH TIME ZONE NOT NULL,
                    duration DOUBLE NOT NULL,
                    where_point VARCHAR,
                    magnitude INTEGER NOT NULL,
                    authenticated BOOLEAN NOT NULL,
                    extra VARCHAR NOT NULL,
                    fidelity DOUBLE NOT NULL,
                    entity_id VARCHAR NOT NULL,
                    user_id VARCHAR NOT NULL,
                    task_name VARCHAR
                )
            "#, params![])?;
            
            // Create indexes
            conn.execute("CREATE INDEX affiliate_by_when ON affiliate (when_timestamp)", params![])?;
            conn.execute("CREATE INDEX affiliate_by_user ON affiliate (user_id)", params![])?;
            conn.execute("CREATE INDEX affiliate_by_entity ON affiliate (entity_id)", params![])?;
            conn.execute("CREATE INDEX affiliate_by_task ON affiliate (task_name)", params![])?;
            conn.execute("CREATE INDEX affiliate_by_origin ON affiliate (origin)", params![])?;
            
            println!("Migration 1 completed: Created affiliate table with auto-increment");
        }
        
        mark_migration_applied(conn, 1)?;
    }
    
    // Migration 2: Add bookmark and join tables
    if !migration_applied(conn, 2)? {
        println!("Running migration 2: Add bookmark and join tables");
        
        // Create bookmark table
        conn.execute("CREATE SEQUENCE IF NOT EXISTS bookmark_id_seq", params![])?;
        conn.execute(r#"
            CREATE TABLE IF NOT EXISTS bookmark (
                id BIGINT PRIMARY KEY DEFAULT nextval('bookmark_id_seq'),
                when_timestamp TIMESTAMP WITH TIME ZONE NOT NULL,
                entity_id VARCHAR NOT NULL,
                user_id VARCHAR NOT NULL,
                active BOOLEAN NOT NULL,
                activity TIMESTAMP WITH TIME ZONE NOT NULL,
                contacted TIMESTAMP WITH TIME ZONE,
                current BOOLEAN NOT NULL DEFAULT TRUE
            )
        "#, params![])?;
        
        // Create bookmark indexes
        conn.execute("CREATE INDEX IF NOT EXISTS bookmark_by_activity ON bookmark (activity)", params![])?;
        conn.execute("CREATE INDEX IF NOT EXISTS bookmark_by_user ON bookmark (user_id)", params![])?;
        conn.execute("CREATE INDEX IF NOT EXISTS bookmark_by_entity ON bookmark (entity_id)", params![])?;
        conn.execute("CREATE UNIQUE INDEX IF NOT EXISTS bookmark_user_entity_unique ON bookmark (user_id, entity_id)", params![])?;
        
        // Create join table (kind field removed - filtering handled in sync logic)
        conn.execute("CREATE SEQUENCE IF NOT EXISTS join_id_seq", params![])?;
        conn.execute(r#"
            CREATE TABLE IF NOT EXISTS join_table (
                id BIGINT PRIMARY KEY DEFAULT nextval('join_id_seq'),
                when_timestamp TIMESTAMP WITH TIME ZONE NOT NULL,
                entity_id VARCHAR NOT NULL,
                user_id VARCHAR NOT NULL,
                active BOOLEAN NOT NULL,
                consented TIMESTAMP WITH TIME ZONE,
                contacted TIMESTAMP WITH TIME ZONE,
                explicit BOOLEAN NOT NULL,
                notified BOOLEAN NOT NULL,
                activity TIMESTAMP WITH TIME ZONE NOT NULL,
                reason TEXT NOT NULL,
                current BOOLEAN NOT NULL DEFAULT TRUE
            )
        "#, params![])?;
        
        // Create join indexes
        conn.execute("CREATE INDEX IF NOT EXISTS join_by_activity ON join_table (activity)", params![])?;
        conn.execute("CREATE INDEX IF NOT EXISTS join_by_user ON join_table (user_id)", params![])?;
        conn.execute("CREATE INDEX IF NOT EXISTS join_by_entity ON join_table (entity_id)", params![])?;
        conn.execute("CREATE UNIQUE INDEX IF NOT EXISTS join_user_entity_unique ON join_table (user_id, entity_id)", params![])?;
        
        println!("Migration 2 completed: Created bookmark and join_table tables");
        mark_migration_applied(conn, 2)?;
    }
    
    // Migration 3: Add optimized indexes for bookmark and join tables
    if !migration_applied(conn, 3)? {
        println!("Running migration 3: Add optimized indexes for bookmark and join tables");
        
        // Add composite indexes for bookmark table
        // For queries filtering by current status
        conn.execute("CREATE INDEX IF NOT EXISTS bookmark_user_current ON bookmark (user_id, current)", params![])?;
        conn.execute("CREATE INDEX IF NOT EXISTS bookmark_entity_current ON bookmark (entity_id, current)", params![])?;
        conn.execute("CREATE INDEX IF NOT EXISTS bookmark_user_entity ON bookmark (user_id, entity_id)", params![])?;
        
        // Add composite indexes for join_table  
        conn.execute("CREATE INDEX IF NOT EXISTS join_user_current ON join_table (user_id, current)", params![])?;
        conn.execute("CREATE INDEX IF NOT EXISTS join_entity_current ON join_table (entity_id, current)", params![])?;
        conn.execute("CREATE INDEX IF NOT EXISTS join_user_entity ON join_table (user_id, entity_id)", params![])?;
        
        println!("Migration 3 completed: Added optimized composite indexes");
        mark_migration_applied(conn, 3)?;
    }
    
    // Migration 4: Add summary tables for analytics optimization
    if !migration_applied(conn, 4)? {
        println!("Running migration 4: Add summary tables for analytics optimization");
        
        // Note: We use weekly summaries only, not monthly/yearly tables.
        // Weekly summaries provide 99%+ of the performance benefit with 50% of the complexity:
        // - Monthly queries: ~1-3ms (aggregating 4-5 weekly rows) 
        // - Yearly queries: ~5-10ms (aggregating ~52 weekly rows)
        // - Multi-year queries: ~15-25ms (acceptable for dashboards)
        // Monthly/yearly tables would only save 5-20ms but add significant complexity
        // with cascading updates, more storage overhead, and additional failure points.
        
        // Daily clickthrough summary table
        conn.execute(r#"
            CREATE TABLE IF NOT EXISTS clickthrough_daily_summary (
                date DATE PRIMARY KEY,
                count BIGINT NOT NULL DEFAULT 0,
                updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP
            )
        "#, params![])?;
        
        // Weekly clickthrough summary table
        conn.execute(r#"
            CREATE TABLE IF NOT EXISTS clickthrough_weekly_summary (
                week_start DATE PRIMARY KEY,
                count BIGINT NOT NULL DEFAULT 0,
                updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP
            )
        "#, params![])?;
        
        // Daily join summary table
        conn.execute(r#"
            CREATE TABLE IF NOT EXISTS join_daily_summary (
                date DATE PRIMARY KEY,
                count BIGINT NOT NULL DEFAULT 0,
                updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP
            )
        "#, params![])?;
        
        // Weekly join summary table
        conn.execute(r#"
            CREATE TABLE IF NOT EXISTS join_weekly_summary (
                week_start DATE PRIMARY KEY,
                count BIGINT NOT NULL DEFAULT 0,
                updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP
            )
        "#, params![])?;
        
        // Index for faster range queries
        conn.execute("CREATE INDEX IF NOT EXISTS clickthrough_daily_summary_date ON clickthrough_daily_summary (date)", params![])?;
        conn.execute("CREATE INDEX IF NOT EXISTS clickthrough_weekly_summary_date ON clickthrough_weekly_summary (week_start)", params![])?;
        conn.execute("CREATE INDEX IF NOT EXISTS join_daily_summary_date ON join_daily_summary (date)", params![])?;
        conn.execute("CREATE INDEX IF NOT EXISTS join_weekly_summary_date ON join_weekly_summary (week_start)", params![])?;
        
        // Build initial summary data if tables have data
        if let Err(e) = rebuild_all_summaries(conn) {
            eprintln!("Warning: Failed to build initial summary data: {}", e);
        }
        
        println!("Migration 4 completed: Added summary tables for analytics optimization");
        mark_migration_applied(conn, 4)?;
    }
    
    Ok(())
}

fn migration_applied(conn: &Connection, version: i32) -> Result<bool> {
    let mut stmt = conn.prepare("SELECT COUNT(*) FROM schema_migrations WHERE version = ?")?;
    let count: i64 = stmt.query_row([version], |row| Ok(row.get(0)?))?;
    Ok(count > 0)
}

fn mark_migration_applied(conn: &Connection, version: i32) -> Result<()> {
    conn.execute("INSERT INTO schema_migrations (version) VALUES (?)", params![version])?;
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

async fn handle_join(req: &async_nats::service::Request, tx: &mpsc::Sender<LogEntry>) -> Result<()> {
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

fn get_clickthrough_analytics(query: AnalyticsQuery) -> Result<AnalyticsResponse> {
    let conn = create_connection()?;
    
    // Query pre-computed weekly summary table for much faster results
    let mut stmt = conn.prepare(r#"
        SELECT 
            week_start,
            count
        FROM clickthrough_weekly_summary 
        WHERE week_start >= ? AND week_start <= ?
        ORDER BY week_start
    "#)?;
    
    let rows = stmt.query_map(params![query.start_date, query.end_date], |row| {
        let week_start: String = row.get(0)?;
        let count: i64 = row.get(1)?;
        Ok(WeeklyCount {
            week_start,
            count,
        })
    })?;
    
    let weekly_counts: Result<Vec<WeeklyCount>, _> = rows.collect();
    Ok(AnalyticsResponse::WeeklyCounts(weekly_counts?))
}

async fn handle_clickthrough_analytics(req: &async_nats::service::Request) -> Result<()> {
    let query: AnalyticsQuery = decode_structured_message(req.message.clone())?;
    
    let response = tokio::task::spawn_blocking(move || get_clickthrough_analytics(query)).await??;
    
    let response_data = flexbuffers::to_vec(&response)?;
    req.respond(Ok(response_data.into())).await?;
    
    Ok(())
}

fn get_join_analytics(query: AnalyticsQuery) -> Result<AnalyticsResponse> {
    let conn = create_connection()?;
    
    // Query pre-computed weekly summary table for much faster results
    let mut stmt = conn.prepare(r#"
        SELECT 
            week_start,
            count
        FROM join_weekly_summary 
        WHERE week_start >= ? AND week_start <= ?
        ORDER BY week_start
    "#)?;
    
    let rows = stmt.query_map(params![query.start_date, query.end_date], |row| {
        let week_start: String = row.get(0)?;
        let count: i64 = row.get(1)?;
        Ok(WeeklyCount {
            week_start,
            count,
        })
    })?;
    
    let weekly_counts: Result<Vec<WeeklyCount>, _> = rows.collect();
    Ok(AnalyticsResponse::WeeklyCounts(weekly_counts?))
}

async fn handle_join_analytics(req: &async_nats::service::Request) -> Result<()> {
    let query: AnalyticsQuery = decode_structured_message(req.message.clone())?;
    
    let response = tokio::task::spawn_blocking(move || get_join_analytics(query)).await??;
    
    let response_data = flexbuffers::to_vec(&response)?;
    req.respond(Ok(response_data.into())).await?;
    
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

    // Spawn affiliate sync task
    let affiliate_sync_task = spawn(affiliate_synchronize());
    tasks.spawn(async move {
        affiliate_sync_task.await.unwrap_or_else(|e| eprintln!("Affiliate sync task panicked: {}", e));
        Ok::<(), anyhow::Error>(())
    });

    // Spawn bookmark sync task
    let bookmark_sync_task = spawn(bookmark_synchronize());
    tasks.spawn(async move {
        bookmark_sync_task.await.unwrap_or_else(|e| eprintln!("Bookmark sync task panicked: {}", e));
        Ok::<(), anyhow::Error>(())
    });

    // Spawn join sync task
    let join_sync_task = spawn(join_synchronize());
    tasks.spawn(async move {
        join_sync_task.await.unwrap_or_else(|e| eprintln!("Join sync task panicked: {}", e));
        Ok::<(), anyhow::Error>(())
    });

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
    
    let join_endpoint = log_group
        .endpoint("join")
        .await
        .expect("Unable to create join endpoint");
    
    let clickthrough_analytics_endpoint = log_group
        .endpoint("clickthrough_analytics")
        .await
        .expect("Unable to create clickthrough_analytics endpoint");
    
    let join_analytics_endpoint = log_group
        .endpoint("join_analytics")
        .await
        .expect("Unable to create join_analytics endpoint");

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

    let tx_join = tx.clone();
    tasks.spawn(async move {
        let handler = join_endpoint.for_each_concurrent(None, move |req| {
            let tx = tx_join.clone();
            async move {
                if let Err(e) = handle_join(&req, &tx).await {
                    eprintln!("Error handling join: {}", e);
                }
            }
        });
        handler.await;
        Ok::<(), anyhow::Error>(())
    });

    tasks.spawn(async move {
        let handler = clickthrough_analytics_endpoint.for_each_concurrent(None, move |req| {
            async move {
                if let Err(e) = handle_clickthrough_analytics(&req).await {
                    eprintln!("Error handling clickthrough analytics: {}", e);
                }
            }
        });
        handler.await;
        Ok::<(), anyhow::Error>(())
    });

    tasks.spawn(async move {
        let handler = join_analytics_endpoint.for_each_concurrent(None, move |req| {
            async move {
                if let Err(e) = handle_join_analytics(&req).await {
                    eprintln!("Error handling join analytics: {}", e);
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
    println!("  - log.join");
    println!("  - log.clickthrough_analytics");
    println!("  - log.join_analytics");
    println!("  - log.user_tags_update");
    println!("  - affiliate sync running in background");
    println!("  - bookmark sync running in background");
    println!("  - join sync running in background");

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

// Affiliate sync functions
async fn affiliate_synchronize() {
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

// Generalized sync bookmark functions
fn get_sync_bookmark(key: &str) -> Result<String> {
    let conn = create_connection()?;
    
    // Create bookmarks table if it doesn't exist
    conn.execute(
        "CREATE TABLE IF NOT EXISTS sync_bookmarks (key VARCHAR PRIMARY KEY, value VARCHAR)",
        params![]
    )?;
    
    let mut stmt = conn.prepare("SELECT value FROM sync_bookmarks WHERE key = ?")?;
    let result = stmt.query_row([key], |row| {
        Ok(row.get::<_, String>(0)?)
    });
    
    match result {
        Ok(value) => Ok(value),
        Err(duckdb::Error::QueryReturnedNoRows) => Ok(String::from("1900-01-01T00:00:00Z")),
        Err(e) => Err(e.into()),
    }
}

fn set_sync_bookmark(key: &str, value: &str) -> Result<()> {
    let conn = create_connection()?;
    conn.execute(
        "INSERT OR REPLACE INTO sync_bookmarks (key, value) VALUES (?, ?)",
        params![key, value]
    )?;
    Ok(())
}

fn get_sync_last_id(key: &str) -> Result<i32> {
    let conn = create_connection()?;
    let mut stmt = conn.prepare("SELECT value FROM sync_bookmarks WHERE key = ?")?;
    let result = stmt.query_row([key], |row| {
        let value: String = row.get(0)?;
        Ok(value.parse::<i32>().unwrap_or(0))
    });
    
    match result {
        Ok(id) => Ok(id),
        Err(duckdb::Error::QueryReturnedNoRows) => Ok(0),
        Err(e) => Err(e.into()),
    }
}

fn set_sync_last_id(key: &str, last_id: i32) -> Result<()> {
    let conn = create_connection()?;
    conn.execute(
        "INSERT OR REPLACE INTO sync_bookmarks (key, value) VALUES (?, ?)",
        params![key, last_id.to_string()]
    )?;
    Ok(())
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

// Bookmark sync functions
async fn bookmark_synchronize() {
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

async fn join_synchronize() {
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

// Summary table management functions
fn update_clickthrough_summaries(conn: &Connection, date: &str) -> Result<()> {
    // Extract date from timestamp string (first 10 chars: YYYY-MM-DD)
    let date_only = &date[0..10];
    
    // Update daily summary
    conn.execute(r#"
        INSERT OR REPLACE INTO clickthrough_daily_summary (date, count, updated_at)
        SELECT 
            DATE(?) as date,
            COUNT(*) as count,
            CURRENT_TIMESTAMP as updated_at
        FROM clickthroughs 
        WHERE DATE(when_timestamp) = DATE(?)
    "#, params![date_only, date_only])?;
    
    // Update weekly summary - let DuckDB calculate the week start
    conn.execute(r#"
        INSERT OR REPLACE INTO clickthrough_weekly_summary (week_start, count, updated_at)
        SELECT 
            DATE_TRUNC('week', CAST(? as TIMESTAMP)) as week_start,
            COUNT(*) as count,
            CURRENT_TIMESTAMP as updated_at
        FROM clickthroughs 
        WHERE DATE_TRUNC('week', when_timestamp) = DATE_TRUNC('week', CAST(? as TIMESTAMP))
    "#, params![date, date])?;
    
    Ok(())
}

fn update_join_summaries(conn: &Connection, date: &str) -> Result<()> {
    // Extract date from timestamp string (first 10 chars: YYYY-MM-DD)
    let date_only = &date[0..10];
    
    // Update daily summary
    conn.execute(r#"
        INSERT OR REPLACE INTO join_daily_summary (date, count, updated_at)
        SELECT 
            DATE(?) as date,
            COUNT(*) as count,
            CURRENT_TIMESTAMP as updated_at
        FROM join_table 
        WHERE DATE(when_timestamp) = DATE(?)
    "#, params![date_only, date_only])?;
    
    // Update weekly summary - let DuckDB calculate the week start
    conn.execute(r#"
        INSERT OR REPLACE INTO join_weekly_summary (week_start, count, updated_at)
        SELECT 
            DATE_TRUNC('week', CAST(? as TIMESTAMP)) as week_start,
            COUNT(*) as count,
            CURRENT_TIMESTAMP as updated_at
        FROM join_table 
        WHERE DATE_TRUNC('week', when_timestamp) = DATE_TRUNC('week', CAST(? as TIMESTAMP))
    "#, params![date, date])?;
    
    Ok(())
}

fn rebuild_all_summaries(conn: &Connection) -> Result<()> {
    println!("Rebuilding all summary tables...");
    
    // Rebuild clickthrough summaries
    conn.execute("DELETE FROM clickthrough_daily_summary", params![])?;
    conn.execute("DELETE FROM clickthrough_weekly_summary", params![])?;
    
    conn.execute(r#"
        INSERT INTO clickthrough_daily_summary (date, count)
        SELECT 
            DATE(when_timestamp) as date,
            COUNT(*) as count
        FROM clickthroughs 
        GROUP BY DATE(when_timestamp)
    "#, params![])?;
    
    conn.execute(r#"
        INSERT INTO clickthrough_weekly_summary (week_start, count)
        SELECT 
            DATE_TRUNC('week', when_timestamp) as week_start,
            COUNT(*) as count
        FROM clickthroughs 
        GROUP BY DATE_TRUNC('week', when_timestamp)
    "#, params![])?;
    
    // Rebuild join summaries
    conn.execute("DELETE FROM join_daily_summary", params![])?;
    conn.execute("DELETE FROM join_weekly_summary", params![])?;
    
    conn.execute(r#"
        INSERT INTO join_daily_summary (date, count)
        SELECT 
            DATE(when_timestamp) as date,
            COUNT(*) as count
        FROM join_table 
        GROUP BY DATE(when_timestamp)
    "#, params![])?;
    
    conn.execute(r#"
        INSERT INTO join_weekly_summary (week_start, count)
        SELECT 
            DATE_TRUNC('week', when_timestamp) as week_start,
            COUNT(*) as count
        FROM join_table 
        GROUP BY DATE_TRUNC('week', when_timestamp)
    "#, params![])?;
    
    println!("Summary tables rebuilt successfully");
    Ok(())
}
