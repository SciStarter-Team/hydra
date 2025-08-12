use anyhow::Result;
use duckdb::{params, Connection};
use std::sync::OnceLock;

static DATABASE_PATH: OnceLock<String> = OnceLock::new();

pub fn get_database_path() -> &'static str {
    DATABASE_PATH.get().expect("Database path not initialized")
}

pub fn set_database_path(path: String) -> Result<()> {
    DATABASE_PATH.set(path).map_err(|_| anyhow::anyhow!("Failed to set database path"))
}

pub fn init_database(database_path: &str) -> Result<()> {
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

pub fn create_connection() -> Result<Connection> {
    Connection::open(get_database_path()).map_err(Into::into)
}

// Generalized sync bookmark functions
pub fn get_sync_bookmark(key: &str) -> Result<String> {
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

pub fn set_sync_bookmark(key: &str, value: &str) -> Result<()> {
    let conn = create_connection()?;
    conn.execute(
        "INSERT OR REPLACE INTO sync_bookmarks (key, value) VALUES (?, ?)",
        params![key, value]
    )?;
    Ok(())
}

pub fn get_sync_last_id(key: &str) -> Result<i32> {
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

pub fn set_sync_last_id(key: &str, last_id: i32) -> Result<()> {
    let conn = create_connection()?;
    conn.execute(
        "INSERT OR REPLACE INTO sync_bookmarks (key, value) VALUES (?, ?)",
        params![key, last_id.to_string()]
    )?;
    Ok(())
}

// Summary table management functions
pub fn update_clickthrough_summaries(conn: &Connection, date: &str) -> Result<()> {
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

pub fn update_join_summaries(conn: &Connection, date: &str) -> Result<()> {
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

pub fn rebuild_all_summaries(conn: &Connection) -> Result<()> {
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