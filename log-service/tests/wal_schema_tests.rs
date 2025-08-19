use anyhow::Result;
use log_service::database::{
    storage::Database,
    tables::{
        ActivityLogRow, BookmarkRow, ClickthroughRow, JoinRow, PageViewRow, ParticipationRow,
    },
};
use std::sync::Arc;
use tempfile::TempDir;
use tokio::fs;
use tokio::time::{sleep, Duration};

/// Helper to create a temporary database instance
async fn setup_database() -> Result<(TempDir, Arc<Database>)> {
    let temp_dir = TempDir::new()?;
    let db_path = temp_dir.path().to_string_lossy().to_string();
    let database = Arc::new(Database::new(db_path).await?);
    Ok((temp_dir, database))
}

/// Test that WAL tables can be queried and have correct schema alignment
#[tokio::test]
async fn test_wal_table_schema_alignment() -> Result<()> {
    let (_temp_dir, database) = setup_database().await?;
    
    // Create sample WAL data for each table type with today's date (so they appear in _wal tables)
    let today = jiff::Timestamp::now().strftime("%Y-%m-%d").to_string();
    let wal_dir = database.path().join("wal");
    tokio::fs::create_dir_all(&wal_dir).await?;

    // Sample data for each table type
    let sample_activity = serde_json::to_string(&ActivityLogRow {
        when: "2024-01-15T10:30:00Z".to_string(),
        action: "test_action".to_string(),
        user_id: Some("user-123".to_string()),
        target: Some("target".to_string()),
        extra: Some("extra".to_string()),
    })?;

    let sample_clickthrough = serde_json::to_string(&ClickthroughRow {
        when: "2024-01-15T10:30:00Z".to_string(),
        ip_address: "10.0.0.1".to_string(),
        session_id: "session-123".to_string(),
        interaction_id: "interaction-123".to_string(),
        user_id: Some("user-123".to_string()),
        from_path: "/test".to_string(),
        to_url: "https://example.com".to_string(),
        link_type: "external".to_string(),
        program_id: Some("program-123".to_string()),
        geolocation: "POINT(0 0)".to_string(),
    })?;

    let sample_page_view = serde_json::to_string(&PageViewRow {
        when: "2024-01-15T10:30:00Z".to_string(),
        ip_address: "192.168.1.1".to_string(),
        session_id: "session-123".to_string(),
        interaction_id: "interaction-456".to_string(),
        user_id: Some("user-789".to_string()),
        path: "/dashboard".to_string(),
        referrer: Some("https://example.com".to_string()),
        query: Some("param=value".to_string()),
        geolocation: "POINT(-122.4194 37.7749)".to_string(),
    })?;

    let sample_join = serde_json::to_string(&JoinRow {
        when: "2024-01-15T10:30:00Z".to_string(),
        entity_id: "entity-123".to_string(),
        user_id: "user-123".to_string(),
        active: true,
        consented: Some("2024-01-15T10:30:00Z".to_string()),
        contacted: None,
        explicit: true,
        notified: false,
        activity: "2024-01-15T10:30:00Z".to_string(),
        reason: "test join".to_string(),
    })?;

    let sample_participation = serde_json::to_string(&ParticipationRow {
        origin: 1,
        when: "2024-01-15T10:30:00Z".to_string(),
        duration: 3600.0,
        where_point: Some("POINT(-122.4194 37.7749)".to_string()),
        magnitude: 5,
        authenticated: true,
        extra: "test participation".to_string(),
        fidelity: 0.95,
        entity_id: "entity-123".to_string(),
        user_id: "user-123".to_string(),
        task_name: Some("test task".to_string()),
    })?;

    let sample_bookmark = serde_json::to_string(&BookmarkRow {
        when: "2024-01-15T10:30:00Z".to_string(),
        entity_id: "entity-123".to_string(),
        user_id: "user-123".to_string(),
        active: true,
        activity: "2024-01-15T10:30:00Z".to_string(),
        contacted: Some("2024-01-15T11:00:00Z".to_string()),
    })?;

    // Write sample data to WAL files with today's date
    let test_tables = [
        ("activity_log", &sample_activity),
        ("clickthroughs", &sample_clickthrough),
        ("page_views", &sample_page_view),
        ("join_table", &sample_join),
        ("participation", &sample_participation),
        ("bookmark", &sample_bookmark),
    ];

    for (table_name, sample_data) in test_tables {
        let table_wal_dir = wal_dir.join(table_name);
        fs::create_dir_all(&table_wal_dir).await?;
        let wal_file = table_wal_dir.join(format!("{}_{}.jsonl", table_name, today));
        fs::write(&wal_file, format!("{}\n", sample_data)).await?;
    }

    // Allow some time for file system operations to complete
    sleep(Duration::from_millis(100)).await;

    // Test querying each _wal table to ensure schema alignment
    let ctx = database.context();
    
    for (table_name, _) in test_tables {
        let wal_table_name = format!("{}_wal", table_name);
        
        // This query should succeed without schema errors
        let query = format!("SELECT * FROM {} LIMIT 1", wal_table_name);
        
        let result = match ctx.sql(&query).await {
            Ok(df) => df.collect().await,
            Err(e) => Err(e),
        };
        
        match result {
            Ok(result) => {
                // Verify we got data back
                assert!(!result.is_empty(), "Expected data from WAL table: {}", wal_table_name);
                
                // Log successful query for debugging
                let total_rows: usize = result.iter().map(|batch| batch.num_rows()).sum();
                println!("✓ Successfully queried {} - got {} records", wal_table_name, total_rows);
            },
            Err(e) => {
                panic!("Schema alignment error for {}: {}", wal_table_name, e);
            }
        }
        
        // Also test the realtime (union) table
        let realtime_table_name = format!("{}_realtime", table_name);
        let realtime_query = format!("SELECT * FROM {} LIMIT 1", realtime_table_name);
        
        let realtime_result = match ctx.sql(&realtime_query).await {
            Ok(df) => df.collect().await,
            Err(e) => Err(e),
        };
        
        match realtime_result {
            Ok(result) => {
                let total_rows: usize = result.iter().map(|batch| batch.num_rows()).sum();
                println!("✓ Successfully queried {} - got {} records", realtime_table_name, total_rows);
            },
            Err(e) => {
                panic!("Schema alignment error for {}: {}", realtime_table_name, e);
            }
        }
    }

    // Clean shutdown
    let db = Arc::try_unwrap(database)
        .map_err(|_| anyhow::anyhow!("Database still has references"))?;
    db.shutdown().await?;

    Ok(())
}

/// Test specifically for nullable field validation
#[tokio::test]
async fn test_wal_nullable_fields() -> Result<()> {
    let (_temp_dir, database) = setup_database().await?;
    
    let today = jiff::Timestamp::now().strftime("%Y-%m-%d").to_string();
    let wal_dir = database.path().join("wal");
    tokio::fs::create_dir_all(&wal_dir).await?;

    // Create clickthrough data with null values for nullable fields
    let clickthrough_with_nulls = serde_json::to_string(&ClickthroughRow {
        when: "2024-01-15T10:30:00Z".to_string(),
        ip_address: "0.0.0.0".to_string(),        // Use placeholder for required fields
        session_id: "unknown".to_string(),         // Use placeholder for required fields  
        interaction_id: "unknown".to_string(),     // Use placeholder for required fields
        user_id: None,          // nullable
        from_path: "/unknown".to_string(),         // Use placeholder for required fields
        to_url: "https://example.com".to_string(),
        link_type: "external".to_string(),
        program_id: None,       // nullable
        geolocation: "POINT(0 0)".to_string(),
    })?;

    // Write to WAL file
    let wal_file = wal_dir.join(format!("clickthroughs_{}.jsonl", today));
    fs::write(&wal_file, format!("{}\n", clickthrough_with_nulls)).await?;

    sleep(Duration::from_millis(100)).await;

    // Query should work without "unmasked nulls" errors
    let ctx = database.context();
    let result = match ctx.sql("SELECT * FROM clickthroughs_wal LIMIT 1").await {
        Ok(df) => df.collect().await,
        Err(e) => Err(e),
    };
    
    match result {
        Ok(data) => {
            assert!(!data.is_empty(), "Expected data from clickthroughs_wal");
            let total_rows: usize = data.iter().map(|batch| batch.num_rows()).sum();
            println!("✓ Successfully queried clickthroughs with null fields - got {} records", total_rows);
        },
        Err(e) => {
            panic!("Nullable field schema error: {}", e);
        }
    }

    // Clean shutdown
    let db = Arc::try_unwrap(database)
        .map_err(|_| anyhow::anyhow!("Database still has references"))?;
    db.shutdown().await?;

    Ok(())
}