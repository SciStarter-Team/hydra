use anyhow::Result;
use jiff::Timestamp;
use log_service::database::storage::{
    Database, get_sync_bookmark, get_sync_last_id, set_sync_bookmark, set_sync_last_id,
};
use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;
use tempfile::TempDir;
use tokio::fs;

// Helper function to convert jiff timestamp to microsecond timestamp (test helper)
fn timestamp_to_microseconds(timestamp_str: &str) -> Result<i64> {
    let timestamp = Timestamp::from_str(timestamp_str)?;
    // Convert to microseconds since epoch
    Ok(timestamp.as_second() * 1_000_000 + (timestamp.subsec_nanosecond() / 1000) as i64)
}

/// Test helper to create a temporary database instance
async fn setup_temp_database() -> Result<(TempDir, Arc<Database>)> {
    let temp_dir = TempDir::new()?;
    let db_path = temp_dir.path().to_string_lossy().to_string();
    let database = Arc::new(Database::new(db_path).await?);
    Ok((temp_dir, database))
}

/// Helper to cleanly shutdown a database for tests
async fn shutdown_database(database: Arc<Database>) -> Result<()> {
    match Arc::try_unwrap(database) {
        Ok(db) => Ok(db.shutdown().await?),
        Err(_) => {
            // Database still has references, which shouldn't happen in tests
            // but let's handle it gracefully
            anyhow::bail!("Database still has active references - cannot shutdown cleanly")
        }
    }
}

#[tokio::test]
async fn test_database_path_management() -> Result<()> {
    let (_temp_dir, database) = setup_temp_database().await?;

    // Test database path access
    let db_path = database.path();
    assert!(db_path.exists());

    // Test that multiple Database instances can be created with different paths
    let temp_dir2 = TempDir::new()?;
    let db_path2 = temp_dir2.path().to_string_lossy().to_string();
    let database2 = Arc::new(Database::new(db_path2).await?);

    assert_ne!(database.path(), database2.path());

    Ok(())
}

#[tokio::test]
async fn test_database_initialization() -> Result<()> {
    let (_temp_dir, database) = setup_temp_database().await?;

    // Verify that the base database directory was created
    let base_path = database.path();
    assert!(
        base_path.exists(),
        "Base database directory should exist"
    );
    assert!(
        base_path.is_dir(),
        "Base database path should be a directory"
    );

    // Parquet subdirectories are created on demand when tables are registered
    // WAL subdirectories are created on demand when data is written

    // Verify that session context is available and properly initialized
    let ctx = database.context();

    // Verify context has essential components
    assert!(
        !ctx.session_id().is_empty(),
        "Session should have a valid ID"
    );
    assert!(
        !ctx.catalog_names().is_empty(),
        "Session should have at least one catalog"
    );

    // Verify runtime environment is accessible
    let _runtime_env = ctx.runtime_env();

    // Verify session has a creation time (indicates proper initialization)
    let _start_time = ctx.session_start_time();

    // Verify we can access the default catalog
    let default_catalog = ctx
        .catalog("datafusion")
        .expect("Should have default catalog");
    assert!(
        !default_catalog.schema_names().is_empty(),
        "Default catalog should have schemas"
    );

    println!(
        "Session context verified - ID: {}, Catalogs: {:?}, Schemas in default catalog: {:?}",
        ctx.session_id(),
        ctx.catalog_names(),
        default_catalog.schema_names()
    );

    Ok(())
}

#[tokio::test]
async fn test_database_initialization_nested_path() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let nested_path = temp_dir
        .path()
        .join("deeply")
        .join("nested")
        .join("database");
    let db_path = nested_path.to_string_lossy().to_string();

    // Initialize database with deeply nested path that doesn't exist yet
    let database = Arc::new(Database::new(db_path).await?);

    // Verify that the nested directory structure was created
    assert!(nested_path.exists());
    assert!(nested_path.is_dir());

    // Proper cleanup
    let db =
        Arc::try_unwrap(database).map_err(|_| anyhow::anyhow!("Database still has references"))?;
    db.shutdown().await?;

    Ok(())
}

#[tokio::test]
async fn test_database_initialization_idempotent() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let db_path = temp_dir.path().to_string_lossy().to_string();

    // Initialize database twice - should not fail
    let database1 = Arc::new(Database::new(&db_path).await?);
    let database2 = Arc::new(Database::new(&db_path).await?);

    // Verify base directory exists and is correct
    let base_path = Path::new(&db_path);
    assert!(base_path.exists());
    assert!(base_path.is_dir());

    // Proper cleanup for both databases
    let db1 = Arc::try_unwrap(database1)
        .map_err(|_| anyhow::anyhow!("Database1 still has references"))?;
    let db2 = Arc::try_unwrap(database2)
        .map_err(|_| anyhow::anyhow!("Database2 still has references"))?;
    db1.shutdown().await?;
    db2.shutdown().await?;

    Ok(())
}

#[tokio::test]
async fn test_database_initialization_existing_directory() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let db_path = temp_dir.path().to_string_lossy().to_string();

    // Pre-create the base directory
    let base_path = Path::new(&db_path);
    fs::create_dir_all(&base_path).await?;

    // Pre-create a parquet subdirectory with some content to test preservation
    let parquet_dir = base_path.join("parquet");
    fs::create_dir_all(&parquet_dir).await?;
    fs::write(parquet_dir.join("existing_file.txt"), "test content").await?;

    // Initialize database - should work with existing directories
    let database = Arc::new(Database::new(&db_path).await?);

    // Verify base directory exists
    assert!(base_path.exists());
    assert!(base_path.is_dir());

    // Verify existing content is preserved
    let existing_file = parquet_dir.join("existing_file.txt");
    assert!(existing_file.exists());
    let content = fs::read_to_string(existing_file).await?;
    assert_eq!(content, "test content");

    // Proper cleanup
    let db =
        Arc::try_unwrap(database).map_err(|_| anyhow::anyhow!("Database still has references"))?;
    db.shutdown().await?;

    Ok(())
}


#[tokio::test]
async fn test_timestamp_conversion() -> Result<()> {
    // Test valid timestamp conversion
    let timestamp_str = "2024-01-15T10:30:45.123456Z";
    let microseconds = timestamp_to_microseconds(timestamp_str)?;
    assert!(microseconds > 0);

    // Test another valid timestamp
    let timestamp_str2 = "2023-12-31T23:59:59.999999Z";
    let microseconds2 = timestamp_to_microseconds(timestamp_str2)?;
    assert!(microseconds2 > 0);
    assert!(microseconds > microseconds2); // First timestamp should be later

    // Test invalid timestamp
    let invalid_timestamp = "invalid-timestamp";
    let result = timestamp_to_microseconds(invalid_timestamp);
    assert!(result.is_err());

    // Test timestamp without microseconds
    let simple_timestamp = "2024-01-15T10:30:45Z";
    let microseconds3 = timestamp_to_microseconds(simple_timestamp)?;
    assert!(microseconds3 > 0);

    // Test epoch timestamp
    let epoch = "1970-01-01T00:00:00.000000Z";
    let epoch_microseconds = timestamp_to_microseconds(epoch)?;
    assert_eq!(epoch_microseconds, 0);

    // Test timestamp with nanosecond precision (should truncate to microseconds)
    let nano_timestamp = "2024-01-15T10:30:45.123456789Z";
    let micro_result = timestamp_to_microseconds(nano_timestamp)?;
    assert!(micro_result > 0);

    Ok(())
}

#[tokio::test]
async fn test_bookmark_storage_operations() -> Result<()> {
    let (_temp_dir, database) = setup_temp_database().await?;

    // Test setting and getting bookmark
    let test_key = "test-bookmark";
    let test_value = "2024-01-15T10:30:45Z";

    set_sync_bookmark(&database, test_key, test_value).await?;
    let retrieved_value = get_sync_bookmark(&database, test_key).await?;
    assert_eq!(retrieved_value, test_value);

    // Test getting non-existent bookmark (should return default)
    let default_value = get_sync_bookmark(&database, "non-existent-key").await?;
    assert_eq!(default_value, "1900-01-01T00:00:00Z");

    // Test updating existing bookmark
    let new_value = "2024-01-16T11:45:30Z";
    set_sync_bookmark(&database, test_key, new_value).await?;
    let updated_value = get_sync_bookmark(&database, test_key).await?;
    assert_eq!(updated_value, new_value);

    Ok(())
}

#[tokio::test]
async fn test_last_id_storage_operations() -> Result<()> {
    let (_temp_dir, database) = setup_temp_database().await?;

    // Test setting and getting last ID
    let test_key = "test-last-id";
    let test_id = 12345;

    set_sync_last_id(&database, test_key, test_id).await?;
    let retrieved_id = get_sync_last_id(&database, test_key).await?;
    assert_eq!(retrieved_id, test_id);

    // Test getting non-existent last ID (should return 0)
    let default_id = get_sync_last_id(&database, "non-existent-key").await?;
    assert_eq!(default_id, 0);

    // Test updating existing last ID
    let new_id = 67890;
    set_sync_last_id(&database, test_key, new_id).await?;
    let updated_id = get_sync_last_id(&database, test_key).await?;
    assert_eq!(updated_id, new_id);

    // Test negative ID
    let negative_id = -100;
    set_sync_last_id(&database, "negative-test", negative_id).await?;
    let retrieved_negative = get_sync_last_id(&database, "negative-test").await?;
    assert_eq!(retrieved_negative, negative_id);

    Ok(())
}

#[tokio::test]
async fn test_bookmark_file_creation_and_location() -> Result<()> {
    let (_temp_dir, database) = setup_temp_database().await?;

    // Initially, bookmark file should not exist
    let bookmark_file = database.path().join("sync_bookmarks.json");
    assert!(!bookmark_file.exists());

    // After setting a bookmark, file should be created
    set_sync_bookmark(&database, "test", "value").await?;
    assert!(bookmark_file.exists());
    assert!(bookmark_file.is_file());

    Ok(())
}

#[tokio::test]
async fn test_bookmark_file_persistence() -> Result<()> {
    let (_temp_dir, database) = setup_temp_database().await?;

    // Set multiple bookmarks and last IDs
    set_sync_bookmark(&database, "bookmark1", "value1").await?;
    set_sync_bookmark(&database, "bookmark2", "value2").await?;
    set_sync_last_id(&database, "lastid1", 100).await?;
    set_sync_last_id(&database, "lastid2", 200).await?;

    // Verify the JSON file was created in the correct location
    let bookmark_file = database.path().join("sync_bookmarks.json");
    assert!(bookmark_file.exists());

    // Read and verify file contents
    let content = fs::read_to_string(&bookmark_file).await?;
    assert!(content.contains("bookmark1"));
    assert!(content.contains("value1"));
    assert!(content.contains("lastid1"));
    assert!(content.contains("100"));

    // Verify values persist across function calls
    let value1 = get_sync_bookmark(&database, "bookmark1").await?;
    let value2 = get_sync_bookmark(&database, "bookmark2").await?;
    let id1 = get_sync_last_id(&database, "lastid1").await?;
    let id2 = get_sync_last_id(&database, "lastid2").await?;

    assert_eq!(value1, "value1");
    assert_eq!(value2, "value2");
    assert_eq!(id1, 100);
    assert_eq!(id2, 200);

    // Clean shutdown
    shutdown_database(database).await?;

    Ok(())
}

#[tokio::test]
async fn test_bookmark_json_format_and_structure() -> Result<()> {
    let (_temp_dir, database) = setup_temp_database().await?;

    // Set some test data
    set_sync_bookmark(&database, "test-bookmark", "test-value").await?;
    set_sync_last_id(&database, "test-id", 42).await?;

    // Read the JSON file directly
    let bookmark_file = database.path().join("sync_bookmarks.json");
    let content = fs::read_to_string(&bookmark_file).await?;

    // Verify it's valid JSON with pretty formatting
    let parsed: serde_json::Value = serde_json::from_str(&content)?;
    assert!(parsed.is_object());
    assert!(content.contains('\n')); // Should be pretty-printed

    // Verify structure matches expected BookmarkStorage format
    let bookmarks = parsed["bookmarks"]
        .as_object()
        .expect("Should have 'bookmarks' object");
    assert_eq!(bookmarks["test-bookmark"], "test-value");
    assert_eq!(bookmarks["test-id"], "42");

    Ok(())
}

#[tokio::test]
async fn test_concurrent_bookmark_operations() -> Result<()> {
    let (_temp_dir, database) = setup_temp_database().await?;

    // Spawn multiple concurrent operations
    let handles = (0..10)
        .map(|i| {
            let db = Arc::clone(&database);
            let key = format!("concurrent-{}", i);
            let value = format!("value-{}", i);
            tokio::spawn(async move {
                set_sync_bookmark(&db, &key, &value).await.unwrap();
                set_sync_last_id(&db, &format!("id-{}", i), i * 10)
                    .await
                    .unwrap();
            })
        })
        .collect::<Vec<_>>();

    // Wait for all operations to complete
    for handle in handles {
        handle.await?;
    }

    // Verify all values were set correctly
    for i in 0..10 {
        let key = format!("concurrent-{}", i);
        let expected_value = format!("value-{}", i);
        let actual_value = get_sync_bookmark(&database, &key).await?;
        assert_eq!(actual_value, expected_value);

        let id_key = format!("id-{}", i);
        let expected_id = i * 10;
        let actual_id = get_sync_last_id(&database, &id_key).await?;
        assert_eq!(actual_id, expected_id);
    }

    Ok(())
}

#[tokio::test]
async fn test_bookmark_special_characters_and_edge_cases() -> Result<()> {
    let (_temp_dir, database) = setup_temp_database().await?;

    // Test with special characters in keys and values
    let special_cases = vec![
        ("key with spaces", "value with spaces"),
        ("key-with-dashes", "value-with-dashes"),
        ("key_with_underscores", "value_with_underscores"),
        ("key.with.dots", "value.with.dots"),
        ("unicode-key-🦀", "unicode-value-🚀"),
        ("", "empty-key-test"),   // Empty key
        ("empty-value-test", ""), // Empty value
        ("json-special", r#"{"quotes": "value", "array": [1,2,3]}"#), // JSON-like content
        ("newlines", "line1\nline2\nline3"), // Newlines
        ("tabs", "value\twith\ttabs"), // Tabs
    ];

    for (key, value) in special_cases {
        set_sync_bookmark(&database, key, value).await?;
        let retrieved = get_sync_bookmark(&database, key).await?;
        assert_eq!(retrieved, value, "Failed for key: '{}'", key);
    }

    Ok(())
}

#[tokio::test]
async fn test_bookmark_large_values_and_limits() -> Result<()> {
    let (_temp_dir, database) = setup_temp_database().await?;

    // Test with large values
    let large_value = "x".repeat(10000); // 10KB string
    set_sync_bookmark(&database, "large-value-test", &large_value).await?;
    let retrieved = get_sync_bookmark(&database, "large-value-test").await?;
    assert_eq!(retrieved, large_value);

    // Test with very large ID
    let large_id = i32::MAX;
    set_sync_last_id(&database, "large-id-test", large_id).await?;
    let retrieved_id = get_sync_last_id(&database, "large-id-test").await?;
    assert_eq!(retrieved_id, large_id);

    // Test ID boundary values
    set_sync_last_id(&database, "min-id", i32::MIN).await?;
    let min_retrieved = get_sync_last_id(&database, "min-id").await?;
    assert_eq!(min_retrieved, i32::MIN);

    Ok(())
}

#[tokio::test]
async fn test_directory_permissions_simulation() -> Result<()> {
    // This test simulates permission issues by trying to create database
    // in a path that would typically cause permission errors
    // Note: This test may behave differently on different platforms

    // Try to initialize database in root directory (should fail on most systems)
    #[cfg(unix)]
    {
        let restricted_path = "/root/restricted_database";
        let result = Database::new(restricted_path).await;
        // This might succeed if running as root, so we check the behavior
        if let Err(e) = result {
            assert!(
                e.to_string().contains("Permission denied")
                    || e.to_string().contains("permission")
                    || e.to_string().contains("access")
                    || e.to_string().contains("denied")
            );
        }
    }

    Ok(())
}

#[tokio::test]
async fn test_datafusion_table_registration() -> Result<()> {
    let (_temp_dir, database) = setup_temp_database().await?;

    // Get context and verify tables are registered
    let ctx = database.context();

    // Test that all expected tables are registered with DataFusion
    let expected_tables = [
        "activity_log",
        "page_views",
        "clickthroughs",
        "participation",
        "bookmark",
        "join_table",
        "clickthrough_daily_summary",
        "clickthrough_weekly_summary",
        "join_daily_summary",
        "join_weekly_summary",
    ];

    for table_name in expected_tables {
        let table_exists = ctx.table_exist(table_name).unwrap_or(false);
        assert!(
            table_exists,
            "Table {} should be registered with DataFusion",
            table_name
        );
        
        // Also check that WAL and realtime variants exist
        let wal_table_exists = ctx.table_exist(&format!("{}_wal", table_name)).unwrap_or(false);
        let realtime_table_exists = ctx.table_exist(&format!("{}_realtime", table_name)).unwrap_or(false);
        
        // WAL tables might not exist if no WAL directory exists yet, so we don't assert on these
        // But we can log their status for debugging
        if wal_table_exists {
            println!("✓ {}_wal table is available", table_name);
        }
        if realtime_table_exists {
            println!("✓ {}_realtime view is available", table_name);
        }
    }

    Ok(())
}
