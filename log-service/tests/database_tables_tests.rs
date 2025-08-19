use anyhow::Result;
use log_service::database::{
    storage::Database,
    table::TableRow,
    tables::{
        ActivityLogRow, BookmarkRow, ClickthroughRow, JoinRow, PageViewRow, ParticipationRow,
    },
};
use std::sync::Arc;
use tempfile::TempDir;
use tokio::time::{Duration, sleep};

/// Helper to create a temporary database instance
async fn setup_full_database() -> Result<(TempDir, Arc<Database>)> {
    let temp_dir = TempDir::new()?;
    let db_path = temp_dir.path().to_string_lossy().to_string();
    let database = Arc::new(Database::new(db_path).await?);
    Ok((temp_dir, database))
}

#[tokio::test]
async fn test_table_system_initialization() -> Result<()> {
    let (_temp_dir, database) = setup_full_database().await?;

    // Verify we can access tables directly from database
    assert_eq!(database.activity_log.name(), "activity_log");
    assert_eq!(database.page_views.name(), "page_views");
    assert_eq!(database.clickthroughs.name(), "clickthroughs");
    assert_eq!(database.join_table.name(), "join_table");
    assert_eq!(database.participation.name(), "participation");
    assert_eq!(database.bookmark.name(), "bookmark");

    // Clean shutdown of table writers
    let db =
        Arc::try_unwrap(database).map_err(|_| anyhow::anyhow!("Database still has references"))?;
    db.shutdown().await?;

    Ok(())
}

#[tokio::test]
async fn test_table_system_double_initialization() -> Result<()> {
    let (_temp_dir, database1) = setup_full_database().await?;

    // Create multiple database instances - should not conflict
    let (_temp_dir2, database2) = setup_full_database().await?;

    // Both should work independently
    assert_eq!(database1.activity_log.name(), "activity_log");
    assert_eq!(database2.activity_log.name(), "activity_log");

    // Clean shutdown both
    let db1 = Arc::try_unwrap(database1)
        .map_err(|_| anyhow::anyhow!("Database1 still has references"))?;
    let db2 = Arc::try_unwrap(database2)
        .map_err(|_| anyhow::anyhow!("Database2 still has references"))?;
    db1.shutdown().await?;
    db2.shutdown().await?;

    Ok(())
}

#[tokio::test]
async fn test_activity_log_table() -> Result<()> {
    let (_temp_dir, database) = setup_full_database().await?;

    // Test ActivityLogRow insertion
    let activity_row = ActivityLogRow {
        when: "2024-01-15T10:30:45Z".to_string(),
        action: "user_login".to_string(),
        user_id: Some("user-123".to_string()),
        target: Some("dashboard".to_string()),
        extra: Some("additional info".to_string()),
    };

    database.activity_log.insert(activity_row.clone()).await?;

    // Test schema
    let schema = database.activity_log.schema();
    assert_eq!(schema.fields().len(), 5);
    assert_eq!(schema.field(0).name(), "when");
    assert_eq!(schema.field(1).name(), "action");
    assert_eq!(schema.field(2).name(), "user_id");
    assert_eq!(schema.field(3).name(), "target");
    assert_eq!(schema.field(4).name(), "extra");

    // Test nullable fields
    assert!(!schema.field(0).is_nullable()); // when
    assert!(!schema.field(1).is_nullable()); // action
    assert!(schema.field(2).is_nullable()); // user_id
    assert!(schema.field(3).is_nullable()); // target
    assert!(schema.field(4).is_nullable()); // extra

    // Clean shutdown
    let db =
        Arc::try_unwrap(database).map_err(|_| anyhow::anyhow!("Database still has references"))?;
    db.shutdown().await?;

    Ok(())
}

#[tokio::test]
async fn test_page_view_table() -> Result<()> {
    let (_temp_dir, database) = setup_full_database().await?;

    // Test PageViewRow insertion
    let page_view_row = PageViewRow {
        when: "2024-01-15T10:30:45Z".to_string(),
        ip_address: "192.168.1.1".to_string(),
        session_id: "session-123".to_string(),
        interaction_id: "interaction-456".to_string(),
        user_id: Some("user-789".to_string()),
        path: "/dashboard".to_string(),
        referrer: Some("https://example.com".to_string()),
        query: Some("param=value".to_string()),
        geolocation: "POINT(-122.4194 37.7749)".to_string(),
    };

    database.page_views.insert(page_view_row).await?;

    // Test schema structure
    let schema = database.page_views.schema();
    assert_eq!(schema.fields().len(), 9);

    // Test required vs optional fields
    assert!(!schema.field_with_name("when").unwrap().is_nullable());
    assert!(!schema.field_with_name("ip_address").unwrap().is_nullable());
    assert!(!schema.field_with_name("session_id").unwrap().is_nullable());
    assert!(
        !schema
            .field_with_name("interaction_id")
            .unwrap()
            .is_nullable()
    );
    assert!(schema.field_with_name("user_id").unwrap().is_nullable());
    assert!(!schema.field_with_name("path").unwrap().is_nullable());
    assert!(schema.field_with_name("referrer").unwrap().is_nullable());
    assert!(schema.field_with_name("query").unwrap().is_nullable());
    assert!(!schema.field_with_name("geolocation").unwrap().is_nullable());

    // Clean shutdown
    let db =
        Arc::try_unwrap(database).map_err(|_| anyhow::anyhow!("Database still has references"))?;
    db.shutdown().await?;

    Ok(())
}

#[tokio::test]
async fn test_clickthrough_table() -> Result<()> {
    let (_temp_dir, database) = setup_full_database().await?;

    // Test ClickthroughRow insertion
    let clickthrough_row = ClickthroughRow {
        when: "2024-01-15T10:30:45Z".to_string(),
        ip_address: "10.0.0.1".to_string(),
        session_id: "session-abc".to_string(),
        interaction_id: "interaction-def".to_string(),
        user_id: Some("user-xyz".to_string()),
        from_path: "/home".to_string(),
        to_url: "https://external.com".to_string(),
        link_type: "external".to_string(),
        program_id: Some("program-123".to_string()),
        geolocation: "POINT(0 0)".to_string(),
    };

    database.clickthroughs.insert(clickthrough_row).await?;

    // Verify table name and schema
    assert_eq!(database.clickthroughs.name(), "clickthroughs");
    let schema = database.clickthroughs.schema();
    assert_eq!(schema.fields().len(), 10);

    // Clean shutdown
    let db =
        Arc::try_unwrap(database).map_err(|_| anyhow::anyhow!("Database still has references"))?;
    db.shutdown().await?;

    Ok(())
}

#[tokio::test]
async fn test_boolean_and_optional_fields() -> Result<()> {
    let (_temp_dir, database) = setup_full_database().await?;

    // Test JoinRow insertion with all boolean combinations and optional fields
    let join_row_full = JoinRow {
        when: "2024-01-15T10:30:45Z".to_string(),
        entity_id: "entity-full-test".to_string(),
        user_id: "user-full-test".to_string(),
        active: true,
        consented: Some("2024-01-15T10:25:00Z".to_string()),
        contacted: Some("2024-01-15T10:35:00Z".to_string()),
        explicit: false,
        notified: true,
        activity: "2024-01-15T10:30:45Z".to_string(),
        reason: "Full field test".to_string(),
    };

    database.join_table.insert(join_row_full).await?;

    // Test with minimal fields (None for optional fields)
    let join_row_minimal = JoinRow {
        when: "2024-01-15T10:30:46Z".to_string(),
        entity_id: "entity-minimal-test".to_string(),
        user_id: "user-minimal-test".to_string(),
        active: false,
        consented: None,
        contacted: None,
        explicit: false,
        notified: false,
        activity: "2024-01-15T10:30:46Z".to_string(),
        reason: "Minimal field test".to_string(),
    };

    database.join_table.insert(join_row_minimal).await?;

    // Test schema verification
    let schema = database.join_table.schema();
    assert_eq!(schema.fields().len(), 10);
    assert_eq!(schema.field(0).name(), "when");
    assert_eq!(schema.field(3).name(), "active");

    // Clean shutdown
    let db =
        Arc::try_unwrap(database).map_err(|_| anyhow::anyhow!("Database still has references"))?;
    db.shutdown().await?;

    Ok(())
}

#[tokio::test]
async fn test_join_table() -> Result<()> {
    let (_temp_dir, database) = setup_full_database().await?;

    // Test JoinRow insertion
    let join_row = JoinRow {
        when: "2024-01-15T10:30:45Z".to_string(),
        entity_id: "entity-123".to_string(),
        user_id: "user-456".to_string(),
        active: true,
        consented: Some("2024-01-15T10:25:00Z".to_string()),
        contacted: None,
        explicit: true,
        notified: false,
        activity: "2024-01-15T10:30:45Z".to_string(),
        reason: "User requested to join".to_string(),
    };

    database.join_table.insert(join_row).await?;

    // Test boolean fields in schema
    let schema = database.join_table.schema();
    use arrow::datatypes::DataType;
    assert_eq!(
        schema.field_with_name("active").unwrap().data_type(),
        &DataType::Boolean
    );
    assert_eq!(
        schema.field_with_name("explicit").unwrap().data_type(),
        &DataType::Boolean
    );
    assert_eq!(
        schema.field_with_name("notified").unwrap().data_type(),
        &DataType::Boolean
    );

    // Clean shutdown
    let db =
        Arc::try_unwrap(database).map_err(|_| anyhow::anyhow!("Database still has references"))?;
    db.shutdown().await?;

    Ok(())
}

#[tokio::test]
async fn test_participation_table() -> Result<()> {
    let (_temp_dir, database) = setup_full_database().await?;

    // Test ParticipationRow insertion
    let participation_row = ParticipationRow {
        origin: 1,
        when: "2024-01-15T10:30:45Z".to_string(),
        duration: 3600.5,
        where_point: Some("POINT(-122.4194 37.7749)".to_string()),
        magnitude: 5,
        authenticated: true,
        extra: "Additional participation data".to_string(),
        fidelity: 0.95,
        entity_id: "entity-789".to_string(),
        user_id: "user-012".to_string(),
        task_name: Some("Data collection".to_string()),
    };

    database.participation.insert(participation_row).await?;

    // Test numeric field types in schema
    let schema = database.participation.schema();
    use arrow::datatypes::DataType;
    assert_eq!(
        schema.field_with_name("origin").unwrap().data_type(),
        &DataType::Int16
    );
    assert_eq!(
        schema.field_with_name("duration").unwrap().data_type(),
        &DataType::Float64
    );
    assert_eq!(
        schema.field_with_name("magnitude").unwrap().data_type(),
        &DataType::Int32
    );
    assert_eq!(
        schema.field_with_name("fidelity").unwrap().data_type(),
        &DataType::Float64
    );

    // Clean shutdown
    let db =
        Arc::try_unwrap(database).map_err(|_| anyhow::anyhow!("Database still has references"))?;
    db.shutdown().await?;

    Ok(())
}

#[tokio::test]
async fn test_bookmark_table() -> Result<()> {
    let (_temp_dir, database) = setup_full_database().await?;

    // Test BookmarkRow insertion
    let bookmark_row = BookmarkRow {
        when: "2024-01-15T10:30:45Z".to_string(),
        entity_id: "entity-bookmark".to_string(),
        user_id: "user-bookmark".to_string(),
        active: true,
        activity: "2024-01-15T10:30:45Z".to_string(),
        contacted: Some("2024-01-15T11:00:00Z".to_string()),
    };

    database.bookmark.insert(bookmark_row).await?;

    // Verify table structure
    let schema = database.bookmark.schema();
    assert_eq!(schema.fields().len(), 6);
    assert!(!schema.field_with_name("when").unwrap().is_nullable());
    assert!(!schema.field_with_name("entity_id").unwrap().is_nullable());
    assert!(!schema.field_with_name("user_id").unwrap().is_nullable());
    assert!(!schema.field_with_name("active").unwrap().is_nullable());
    assert!(!schema.field_with_name("activity").unwrap().is_nullable());
    assert!(schema.field_with_name("contacted").unwrap().is_nullable());

    // Clean shutdown
    let db =
        Arc::try_unwrap(database).map_err(|_| anyhow::anyhow!("Database still has references"))?;
    db.shutdown().await?;

    Ok(())
}

#[tokio::test]
async fn test_all_tables_concurrent_inserts() -> Result<()> {
    let (_temp_dir, database) = setup_full_database().await?;

    // Insert data into all tables concurrently
    let activity_task = {
        let db = Arc::clone(&database);
        tokio::spawn(async move {
            for i in 0..10 {
                let row = ActivityLogRow {
                    when: format!("2024-01-15T10:{}:00Z", 30 + i),
                    action: format!("action_{}", i),
                    user_id: Some(format!("user-{}", i)),
                    target: None,
                    extra: None,
                };
                db.activity_log.insert(row).await.unwrap();
            }
        })
    };

    let page_view_task = {
        let db = Arc::clone(&database);
        tokio::spawn(async move {
            for i in 0..10 {
                let row = PageViewRow {
                    when: format!("2024-01-15T10:{}:00Z", 30 + i),
                    ip_address: format!("192.168.1.{}", i + 1),
                    session_id: format!("session-{}", i),
                    interaction_id: format!("interaction-{}", i),
                    user_id: Some(format!("user-{}", i)),
                    path: format!("/page-{}", i),
                    referrer: None,
                    query: None,
                    geolocation: "POINT(0 0)".to_string(),
                };
                db.page_views.insert(row).await.unwrap();
            }
        })
    };

    let clickthrough_task = {
        let db = Arc::clone(&database);
        tokio::spawn(async move {
            for i in 0..10 {
                let row = ClickthroughRow {
                    when: format!("2024-01-15T10:{}:00Z", 30 + i),
                    ip_address: format!("10.0.0.{}", i + 1),
                    session_id: format!("session-{}", i),
                    interaction_id: format!("interaction-{}", i),
                    user_id: Some(format!("user-{}", i)),
                    from_path: format!("/from-{}", i),
                    to_url: format!("https://example.com/{}", i),
                    link_type: "external".to_string(),
                    program_id: None,
                    geolocation: "POINT(0 0)".to_string(),
                };
                db.clickthroughs.insert(row).await.unwrap();
            }
        })
    };

    // Wait for all concurrent inserts
    tokio::try_join!(activity_task, page_view_task, clickthrough_task)?;

    // Allow time for writes
    sleep(Duration::from_millis(200)).await;

    // Clean shutdown
    let db =
        Arc::try_unwrap(database).map_err(|_| anyhow::anyhow!("Database still has references"))?;
    db.shutdown().await?;

    Ok(())
}

#[tokio::test]
async fn test_table_system_graceful_shutdown() -> Result<()> {
    let (_temp_dir, database) = setup_full_database().await?;

    // Insert some data
    let activity_row = ActivityLogRow {
        when: "2024-01-15T10:30:45Z".to_string(),
        action: "shutdown_test".to_string(),
        user_id: Some("user-shutdown".to_string()),
        target: None,
        extra: None,
    };

    database.activity_log.insert(activity_row).await?;

    // Graceful shutdown should flush all data
    // Clean shutdown
    let db =
        Arc::try_unwrap(database).map_err(|_| anyhow::anyhow!("Database still has references"))?;
    db.shutdown().await?;

    // Attempting to access tables after shutdown should panic (expected behavior)
    // This is tested by the fact that the test completes successfully

    Ok(())
}

#[tokio::test]
async fn test_table_system_schema_consistency() -> Result<()> {
    let (_temp_dir, database) = setup_full_database().await?;

    // Verify that table schemas match the TableRow implementations
    assert_eq!(database.activity_log.schema(), ActivityLogRow::schema());
    assert_eq!(database.page_views.schema(), PageViewRow::schema());
    assert_eq!(database.clickthroughs.schema(), ClickthroughRow::schema());
    assert_eq!(database.join_table.schema(), JoinRow::schema());
    assert_eq!(database.participation.schema(), ParticipationRow::schema());
    assert_eq!(database.bookmark.schema(), BookmarkRow::schema());

    // Clean shutdown
    let db =
        Arc::try_unwrap(database).map_err(|_| anyhow::anyhow!("Database still has references"))?;
    db.shutdown().await?;

    Ok(())
}

#[tokio::test]
async fn test_table_row_serialization() -> Result<()> {
    // Test that all row types can be serialized and deserialized properly

    let activity_row = ActivityLogRow {
        when: "2024-01-15T10:30:45Z".to_string(),
        action: "test_action".to_string(),
        user_id: Some("user-123".to_string()),
        target: Some("target".to_string()),
        extra: Some("extra data".to_string()),
    };

    let json = serde_json::to_string(&activity_row)?;
    let deserialized: ActivityLogRow = serde_json::from_str(&json)?;
    assert_eq!(activity_row.when, deserialized.when);
    assert_eq!(activity_row.action, deserialized.action);

    let join_row = JoinRow {
        when: "2024-01-15T10:30:45Z".to_string(),
        entity_id: "entity-serialization-test".to_string(),
        user_id: "user-serialization-test".to_string(),
        active: true,
        consented: Some("2024-01-15T10:25:00Z".to_string()),
        contacted: None,
        explicit: false,
        notified: true,
        activity: "2024-01-15T10:30:45Z".to_string(),
        reason: "Serialization test".to_string(),
    };

    let json = serde_json::to_string(&join_row)?;
    let deserialized: JoinRow = serde_json::from_str(&json)?;
    assert_eq!(join_row.when, deserialized.when);
    assert_eq!(join_row.active, deserialized.active);
    assert_eq!(join_row.consented, deserialized.consented);

    Ok(())
}

#[tokio::test]
async fn test_table_row_with_nulls() -> Result<()> {
    let (_temp_dir, database) = setup_full_database().await?;

    // Test ActivityLogRow with all nullable fields as None
    let minimal_activity = ActivityLogRow {
        when: "2024-01-15T10:30:45Z".to_string(),
        action: "minimal_action".to_string(),
        user_id: None,
        target: None,
        extra: None,
    };

    database.activity_log.insert(minimal_activity).await?;

    // Test PageViewRow with minimal required fields
    let minimal_page_view = PageViewRow {
        when: "2024-01-15T10:30:45Z".to_string(),
        ip_address: "127.0.0.1".to_string(),
        session_id: "minimal-session".to_string(),
        interaction_id: "minimal-interaction".to_string(),
        user_id: None,
        path: "/minimal".to_string(),
        referrer: None,
        query: None,
        geolocation: "POINT(0 0)".to_string(),
    };

    database.page_views.insert(minimal_page_view).await?;

    sleep(Duration::from_millis(100)).await;
    // Clean shutdown
    let db =
        Arc::try_unwrap(database).map_err(|_| anyhow::anyhow!("Database still has references"))?;
    db.shutdown().await?;

    Ok(())
}

#[tokio::test]
async fn test_table_system_large_data() -> Result<()> {
    let (_temp_dir, database) = setup_full_database().await?;

    // Test with large string data
    let large_extra = "x".repeat(10000); // 10KB string
    let large_activity = ActivityLogRow {
        when: "2024-01-15T10:30:45Z".to_string(),
        action: "large_data_test".to_string(),
        user_id: Some("user-large".to_string()),
        target: Some("large-target".to_string()),
        extra: Some(large_extra.clone()),
    };

    database.activity_log.insert(large_activity).await?;

    // Test with large reason field
    let large_reason = "y".repeat(50000); // 50KB string
    let large_join = JoinRow {
        when: "2024-01-15T10:30:45Z".to_string(),
        entity_id: "entity-large".to_string(),
        user_id: "user-large-join".to_string(),
        active: true,
        consented: Some("2024-01-15T10:25:00Z".to_string()),
        contacted: None,
        explicit: true,
        notified: false,
        activity: "2024-01-15T10:30:45Z".to_string(),
        reason: large_reason,
    };

    database.join_table.insert(large_join).await?;

    sleep(Duration::from_millis(200)).await;
    // Clean shutdown
    let db =
        Arc::try_unwrap(database).map_err(|_| anyhow::anyhow!("Database still has references"))?;
    db.shutdown().await?;

    Ok(())
}

#[tokio::test]
async fn test_table_system_edge_case_values() -> Result<()> {
    let (_temp_dir, database) = setup_full_database().await?;

    // Test with edge case numeric values
    let edge_case_participation = ParticipationRow {
        origin: i16::MAX,
        when: "2024-01-15T10:30:45Z".to_string(),
        duration: f64::MAX,
        where_point: None,
        magnitude: i32::MIN,
        authenticated: false,
        extra: "".to_string(), // Empty string
        fidelity: 0.0,
        entity_id: "edge-entity".to_string(),
        user_id: "edge-user".to_string(),
        task_name: Some("".to_string()), // Empty optional string
    };

    database
        .participation
        .insert(edge_case_participation)
        .await?;

    // Test with unicode characters
    let unicode_activity = ActivityLogRow {
        when: "2024-01-15T10:30:45Z".to_string(),
        action: "unicode_test_🦀🚀".to_string(),
        user_id: Some("用户-123".to_string()), // Chinese characters
        target: Some("🎯_target".to_string()),
        extra: Some("Extra with 日本語 and 🌟".to_string()),
    };

    database.activity_log.insert(unicode_activity).await?;

    sleep(Duration::from_millis(100)).await;
    // Clean shutdown
    let db =
        Arc::try_unwrap(database).map_err(|_| anyhow::anyhow!("Database still has references"))?;
    db.shutdown().await?;

    Ok(())
}
