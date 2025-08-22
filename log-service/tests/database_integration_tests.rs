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
use tokio::time::{Duration, sleep, timeout};
use tokio_util::sync::CancellationToken;

/// Complete database system setup: storage + tables + test data
async fn setup_complete_database_system() -> Result<(TempDir, Arc<Database>)> {
    let temp_dir = TempDir::new()?;
    let db_path = temp_dir.path().to_string_lossy().to_string();
    let database = Arc::new(Database::new(db_path).await?);

    Ok((temp_dir, database))
}

/// Helper to populate database with comprehensive test data
async fn populate_test_data(database: &Arc<Database>) -> Result<()> {
    // Insert activity log data
    for i in 0..5 {
        let activity = ActivityLogRow {
            when: format!("2024-01-15T10:{}:00Z", 30 + i),
            action: format!("test_action_{}", i),
            user_id: if i % 2 == 0 {
                Some(format!("user-{}", i))
            } else {
                None
            },
            target: Some(format!("target-{}", i)),
            extra: if i % 3 == 0 {
                Some(format!("extra-{}", i))
            } else {
                None
            },
        };
        database.activity_log.insert(activity).await?;
    }

    // Insert page view data
    for i in 0..3 {
        let page_view = PageViewRow {
            when: format!("2024-01-15T10:{}:00Z", 30 + i),
            ip_address: format!("192.168.1.{}", i + 1),
            device_id: format!("session-{}", i),
            interaction_id: format!("interaction-{}", i),
            user_id: Some(format!("user-{}", i)),
            path: format!("/page-{}", i),
            referrer: if i > 0 {
                Some("https://example.com".to_string())
            } else {
                None
            },
            query: if i == 1 {
                Some("param=value".to_string())
            } else {
                None
            },
            geolocation: format!("POINT({} {})", i as f64 * 10.0, i as f64 * 5.0),
        };
        database.page_views.insert(page_view).await?;
    }

    // Insert clickthrough data
    for i in 0..4 {
        let clickthrough = ClickthroughRow {
            when: format!("2024-01-15T10:{}:00Z", 30 + i),
            ip_address: format!("10.0.0.{}", i + 1),
            device_id: format!("click-session-{}", i),
            interaction_id: format!("click-interaction-{}", i),
            user_id: Some(format!("user-{}", i)),
            from_path: format!("/from-{}", i),
            to_url: format!("https://external-{}.com", i),
            link_type: if i % 2 == 0 {
                "external".to_string()
            } else {
                "internal".to_string()
            },
            program_id: if i < 2 {
                Some(format!("program-{}", i))
            } else {
                None
            },
            geolocation: "POINT(0 0)".to_string(),
        };
        database.clickthroughs.insert(clickthrough).await?;
    }

    // Insert join data
    for i in 0..2 {
        let join = JoinRow {
            when: format!("2024-01-15T10:{}:00Z", 30 + i),
            entity_id: format!("entity-{}", i),
            user_id: format!("user-{}", i),
            active: i % 2 == 0,
            consented: if i == 0 {
                Some(format!("2024-01-15T10:{}:00Z", 25 + i))
            } else {
                None
            },
            contacted: None,
            explicit: true,
            notified: i == 1,
            activity: format!("2024-01-15T10:{}:00Z", 30 + i),
            reason: format!("Join reason {}", i),
        };
        database.join_table.insert(join).await?;
    }

    // Insert participation data
    for i in 0..3 {
        let participation = ParticipationRow {
            origin: (i + 1) as i16,
            when: format!("2024-01-15T10:{}:00Z", 30 + i),
            duration: (i + 1) as f64 * 1800.0, // 30 minutes, 1 hour, 1.5 hours
            where_point: Some(format!("POINT({} {})", i as f64 * -10.0, i as f64 * 15.0)),
            magnitude: (i + 1) * 2,
            authenticated: i % 2 == 0,
            extra: format!("Participation extra data {}", i),
            fidelity: 0.8 + (i as f64 * 0.05),
            entity_id: format!("entity-{}", i),
            user_id: format!("user-{}", i),
            task_name: if i < 2 {
                Some(format!("Task {}", i))
            } else {
                None
            },
        };
        database.participation.insert(participation).await?;
    }

    // Insert bookmark data
    for i in 0..2 {
        let bookmark = BookmarkRow {
            when: format!("2024-01-15T10:{}:00Z", 30 + i),
            entity_id: format!("bookmark-entity-{}", i),
            user_id: format!("user-{}", i),
            active: true,
            activity: format!("2024-01-15T10:{}:00Z", 30 + i),
            contacted: if i == 1 {
                Some(format!("2024-01-15T11:{}:00Z", i))
            } else {
                None
            },
        };
        database.bookmark.insert(bookmark).await?;
    }

    Ok(())
}

#[tokio::test]
async fn test_complete_database_lifecycle() -> Result<()> {
    let (_temp_dir, database) = setup_complete_database_system().await?;

    // Test 1: Verify all components initialized correctly
    assert_eq!(database.activity_log.name(), "activity_log");
    assert_eq!(database.page_views.name(), "page_views");
    assert_eq!(database.clickthroughs.name(), "clickthroughs");
    assert_eq!(database.join_table.name(), "join_table");
    assert_eq!(database.participation.name(), "participation");
    assert_eq!(database.bookmark.name(), "bookmark");

    // Test 2: Insert comprehensive test data
    populate_test_data(&database).await?;

    // Test 3: Verify WAL files are created
    sleep(Duration::from_millis(200)).await;

    let today = jiff::Timestamp::now().strftime("%Y-%m-%d").to_string();
    let wal_dir = database.path().join("wal");

    let expected_wal_files = [
        format!("activity_log_{}.jsonl", today),
        format!("page_views_{}.jsonl", today),
        format!("clickthroughs_{}.jsonl", today),
        format!("join_table_{}.jsonl", today),
        format!("participation_{}.jsonl", today),
        format!("bookmark_{}.jsonl", today),
    ];

    for wal_file in expected_wal_files {
        let wal_path = wal_dir.join(&wal_file);
        assert!(wal_path.exists(), "WAL file should exist: {}", wal_file);

        // Verify WAL file has content
        let content = fs::read_to_string(&wal_path).await?;
        assert!(
            !content.trim().is_empty(),
            "WAL file should have content: {}",
            wal_file
        );
    }

    // Test 4: Test bookmark functionality
    use log_service::database::storage::{get_sync_bookmark, set_sync_bookmark};
    set_sync_bookmark(&database, "integration-test", "2024-01-15T10:30:45Z").await?;
    let bookmark = get_sync_bookmark(&database, "integration-test").await?;
    assert_eq!(bookmark, "2024-01-15T10:30:45Z");

    // Test 5: Graceful shutdown
    let db =
        Arc::try_unwrap(database).map_err(|_| anyhow::anyhow!("Database still has references"))?;
    db.shutdown().await?;

    Ok(())
}

#[tokio::test]
async fn test_full_pipeline_with_compaction() -> Result<()> {
    let (_temp_dir, database) = setup_complete_database_system().await?;

    // Step 1: Create artificial "yesterday" WAL files with test data
    let yesterday = "2024-01-14"; // Use a fixed date for deterministic testing
    let wal_dir = database.path().join("wal");
    tokio::fs::create_dir_all(&wal_dir).await?;

    // Create sample WAL data for each table type with yesterday's date
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
        device_id: "session-123".to_string(),
        interaction_id: "interaction-123".to_string(),
        user_id: Some("user-123".to_string()),
        from_path: "/test".to_string(),
        to_url: "https://example.com".to_string(),
        link_type: "external".to_string(),
        program_id: Some("program-123".to_string()),
        geolocation: "POINT(0 0)".to_string(),
    })?;

    // Write test WAL files with yesterday's date
    let tables_data = [
        ("activity_log", &sample_activity),
        ("clickthroughs", &sample_clickthrough),
    ];

    for (table_name, sample_data) in tables_data {
        let wal_file = wal_dir.join(format!("{}_{}.jsonl", table_name, yesterday));
        fs::write(
            &wal_file,
            format!(
                "{}
",
                sample_data
            ),
        )
        .await?;
    }

    // Step 2: Run compactor to convert WAL to Parquet
    let cancellation_token = CancellationToken::new();
    use log_service::database::compactor::Compactor;
    let compactor = Compactor::with_config(database.clone(), Duration::from_millis(10), 30);

    let compactor_task = {
        let token = cancellation_token.clone();
        tokio::spawn(async move { compactor.run(token).await })
    };

    // Allow compactor to run several cycles
    sleep(Duration::from_millis(200)).await;

    // Cancel and wait for completion
    cancellation_token.cancel();
    let _ = timeout(Duration::from_secs(5), compactor_task).await;

    // Step 3: Verify Parquet files were created for the tables we created WAL files for
    let parquet_dir = database.path().join("parquet");

    let expected_tables = ["activity_log", "clickthroughs"];

    for table_name in expected_tables {
        let table_parquet_dir = parquet_dir.join(table_name);
        assert!(
            table_parquet_dir.exists(),
            "Parquet directory should exist for: {}",
            table_name
        );

        let parquet_file = table_parquet_dir.join(format!("{}_{}.parquet", table_name, yesterday));
        assert!(
            parquet_file.exists(),
            "Parquet file should exist for: {}",
            table_name
        );

        // Verify file is not empty
        let metadata = fs::metadata(&parquet_file).await?;
        assert!(
            metadata.len() > 0,
            "Parquet file should not be empty for: {}",
            table_name
        );

        // Step 4: Verify original WAL file has been moved to processed directory
        let original_wal = wal_dir.join(format!("{}_{}.jsonl", table_name, yesterday));
        assert!(
            !original_wal.exists(),
            "Original WAL file should be moved after processing: {}",
            table_name
        );

        // Verify WAL file is in processed directory
        let processed_dir = database.path().join("processed");
        let processed_wal = processed_dir.join(format!("{}_{}.jsonl", table_name, yesterday));
        assert!(
            processed_wal.exists(),
            "Processed WAL file should exist in processed directory for: {}",
            table_name
        );
    }

    let db =
        Arc::try_unwrap(database).map_err(|_| anyhow::anyhow!("Database still has references"))?;
    db.shutdown().await?;

    Ok(())
}

#[tokio::test]
async fn test_concurrent_operations_full_system() -> Result<()> {
    let (_temp_dir, database) = setup_complete_database_system().await?;

    // Spawn multiple concurrent tasks performing different operations
    let activity_task = {
        let db = Arc::clone(&database);
        tokio::spawn(async move {
            for i in 0..20 {
                let activity = ActivityLogRow {
                    when: format!("2024-01-15T10:{}:{}Z", 30 + i / 60, i % 60),
                    action: format!("concurrent_action_{}", i),
                    user_id: Some(format!("concurrent-user-{}", i % 5)),
                    target: Some(format!("concurrent-target-{}", i)),
                    extra: None,
                };
                db.activity_log.insert(activity).await.unwrap();
            }
        })
    };

    let page_view_task = {
        let db = Arc::clone(&database);
        tokio::spawn(async move {
            for i in 0..15 {
                let page_view = PageViewRow {
                    when: format!("2024-01-15T10:{}:{}Z", 30 + i / 60, i % 60),
                    ip_address: format!("10.0.{}.{}", i / 10, i % 10),
                    device_id: format!("concurrent-session-{}", i),
                    interaction_id: format!("concurrent-interaction-{}", i),
                    user_id: Some(format!("concurrent-user-{}", i % 3)),
                    path: format!("/concurrent-page-{}", i),
                    referrer: None,
                    query: None,
                    geolocation: "POINT(0 0)".to_string(),
                };
                db.page_views.insert(page_view).await.unwrap();
            }
        })
    };

    let bookmark_task = {
        let db = Arc::clone(&database);
        tokio::spawn(async move {
            for i in 0..8 {
                let bookmark = BookmarkRow {
                    when: format!("2024-01-15T10:{}:{}Z", 30 + i / 60, i % 60),
                    entity_id: format!("concurrent-entity-{}", i),
                    user_id: format!("concurrent-user-{}", i % 4),
                    active: i % 2 == 0,
                    activity: format!("2024-01-15T10:{}:{}Z", 30 + i / 60, i % 60),
                    contacted: None,
                };
                db.bookmark.insert(bookmark).await.unwrap();
            }
        })
    };

    // Concurrent bookmark operations
    let bookmark_ops_task = {
        let db = Arc::clone(&database);
        tokio::spawn(async move {
            use log_service::database::storage::set_sync_bookmark;
            for i in 0..5 {
                let key = format!("concurrent-bookmark-{}", i);
                let value = format!("2024-01-15T10:{}:00Z", 30 + i);
                set_sync_bookmark(&db, &key, &value).await.unwrap();
            }
        })
    };

    // Wait for all concurrent operations to complete
    tokio::try_join!(
        activity_task,
        page_view_task,
        bookmark_task,
        bookmark_ops_task
    )?;

    // Allow time for all writes to complete
    sleep(Duration::from_millis(300)).await;

    // Verify data was written correctly
    let today = jiff::Timestamp::now().strftime("%Y-%m-%d").to_string();
    let wal_dir = database.path().join("wal");

    // Check activity log has 20 entries
    let activity_wal = wal_dir.join(format!("activity_log_{}.jsonl", today));
    let activity_content = fs::read_to_string(&activity_wal).await?;
    let activity_lines: Vec<&str> = activity_content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    assert!(
        activity_lines.len() >= 20,
        "Should have at least 20 activity log entries"
    );

    // Check page views has 15 entries
    let page_view_wal = wal_dir.join(format!("page_views_{}.jsonl", today));
    let page_view_content = fs::read_to_string(&page_view_wal).await?;
    let page_view_lines: Vec<&str> = page_view_content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    assert!(
        page_view_lines.len() >= 15,
        "Should have at least 15 page view entries"
    );

    // Verify bookmark operations
    use log_service::database::storage::get_sync_bookmark;
    for i in 0..5 {
        let key = format!("concurrent-bookmark-{}", i);
        let expected_value = format!("2024-01-15T10:{}:00Z", 30 + i);
        let actual_value = get_sync_bookmark(&database, &key).await?;
        assert_eq!(actual_value, expected_value);
    }

    let db =
        Arc::try_unwrap(database).map_err(|_| anyhow::anyhow!("Database still has references"))?;
    db.shutdown().await?;

    Ok(())
}

#[tokio::test]
async fn test_system_recovery_after_restart() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let db_path = temp_dir.path().to_string_lossy().to_string();

    // Phase 1: Initialize system and add data
    {
        let database = Arc::new(Database::new(&db_path).await?);

        // Add some test data
        let activity = ActivityLogRow {
            when: "2024-01-15T10:30:45Z".to_string(),
            action: "recovery_test".to_string(),
            user_id: Some("recovery-user".to_string()),
            target: Some("recovery-target".to_string()),
            extra: Some("recovery-extra".to_string()),
        };
        database.activity_log.insert(activity).await?;

        // Add bookmark data
        use log_service::database::storage::set_sync_bookmark;
        set_sync_bookmark(&database, "recovery-test", "2024-01-15T10:30:45Z").await?;

        sleep(Duration::from_millis(100)).await;

        let db = Arc::try_unwrap(database)
            .map_err(|_| anyhow::anyhow!("Database still has references"))?;
        db.shutdown().await?;
    }

    // Phase 2: Simulate restart by reinitializing
    {
        let database = Arc::new(Database::new(&db_path).await?);

        // Verify files persist across "restart"
        let today = jiff::Timestamp::now().strftime("%Y-%m-%d").to_string();
        let activity_wal = database
            .path()
            .join("wal")
            .join(format!("activity_log_{}.jsonl", today));
        assert!(activity_wal.exists(), "WAL file should persist");

        let bookmark_file = database.path().join("sync_bookmarks.json");
        assert!(bookmark_file.exists(), "Bookmark file should persist");

        // Verify bookmark data persists
        use log_service::database::storage::get_sync_bookmark;
        let bookmark_value = get_sync_bookmark(&database, "recovery-test").await?;
        assert_eq!(bookmark_value, "2024-01-15T10:30:45Z");

        // Verify WAL content persists
        let wal_content = fs::read_to_string(&activity_wal).await?;
        assert!(wal_content.contains("recovery_test"));
        assert!(wal_content.contains("recovery-user"));

        let db = Arc::try_unwrap(database)
            .map_err(|_| anyhow::anyhow!("Database still has references"))?;
        db.shutdown().await?;
    }

    Ok(())
}

#[tokio::test]
async fn test_error_handling_and_resilience() -> Result<()> {
    let (_temp_dir, database) = setup_complete_database_system().await?;

    // Test 1: Insert data with various edge cases
    let edge_case_activity = ActivityLogRow {
        when: "2024-01-15T10:30:45Z".to_string(),
        action: "edge_case_test_🦀".to_string(),
        user_id: Some("edge-user-🚀".to_string()),
        target: Some("target with spaces and \"quotes\"".to_string()),
        extra: Some("Multi\nline\nextra\tdata".to_string()),
    };
    database.activity_log.insert(edge_case_activity).await?;

    // Test 2: Insert data with very long strings
    let long_string = "x".repeat(5000);
    let large_activity = ActivityLogRow {
        when: "2024-01-15T10:30:45Z".to_string(),
        action: "large_data_test".to_string(),
        user_id: Some("large-user".to_string()),
        target: Some("large-target".to_string()),
        extra: Some(long_string),
    };
    database.activity_log.insert(large_activity).await?;

    // Test 3: Insert data with empty optional fields
    let empty_fields = ActivityLogRow {
        when: "2024-01-15T10:30:45Z".to_string(),
        action: "empty_fields_test".to_string(),
        user_id: None,
        target: None,
        extra: None,
    };
    database.activity_log.insert(empty_fields).await?;

    // Test 4: Insert data with very long reason field
    let long_reason = "x".repeat(10000);
    let long_reason_join = JoinRow {
        when: "2024-01-15T10:30:45Z".to_string(),
        entity_id: "long-reason-entity".to_string(),
        user_id: "long-reason-user".to_string(),
        active: true,
        consented: Some("2024-01-15T10:30:45Z".to_string()),
        contacted: None,
        explicit: true,
        notified: false,
        activity: "2024-01-15T10:30:45Z".to_string(),
        reason: long_reason,
    };
    database.join_table.insert(long_reason_join).await?;

    // Test 5: Bookmark operations with edge cases
    use log_service::database::storage::{get_sync_bookmark, set_sync_bookmark};
    set_sync_bookmark(&database, "", "empty-key-test").await?; // Empty key
    set_sync_bookmark(&database, "unicode-🦀", "unicode-value-🚀").await?; // Unicode
    set_sync_bookmark(&database, "large-value", &"y".repeat(10000)).await?; // Large value

    sleep(Duration::from_millis(200)).await;

    // Verify all operations succeeded
    let empty_key_value = get_sync_bookmark(&database, "").await?;
    assert_eq!(empty_key_value, "empty-key-test");

    let unicode_value = get_sync_bookmark(&database, "unicode-🦀").await?;
    assert_eq!(unicode_value, "unicode-value-🚀");

    let large_value = get_sync_bookmark(&database, "large-value").await?;
    assert_eq!(large_value.len(), 10000);

    let db =
        Arc::try_unwrap(database).map_err(|_| anyhow::anyhow!("Database still has references"))?;
    db.shutdown().await?;

    Ok(())
}

#[tokio::test]
async fn test_performance_under_load() -> Result<()> {
    let (_temp_dir, database) = setup_complete_database_system().await?;

    let start_time = std::time::Instant::now();

    // Insert a large number of records quickly
    let num_records = 1000;

    let mut tasks = Vec::new();

    // Spawn multiple concurrent tasks
    for batch in 0..10 {
        let db = Arc::clone(&database);
        let task = tokio::spawn(async move {
            for i in 0..100 {
                let record_id = batch * 100 + i;
                let activity = ActivityLogRow {
                    when: format!(
                        "2024-01-15T10:{}:{}Z",
                        30 + record_id / 3600,
                        (record_id % 3600) / 60
                    ),
                    action: format!("load_test_action_{}", record_id),
                    user_id: Some(format!("load-user-{}", record_id % 50)),
                    target: Some(format!("load-target-{}", record_id)),
                    extra: Some(format!("Load test extra data {}", record_id)),
                };
                db.activity_log.insert(activity).await.unwrap();
            }
        });
        tasks.push(task);
    }

    // Wait for all tasks to complete
    for task in tasks {
        task.await?;
    }

    let insert_duration = start_time.elapsed();
    println!("Inserted {} records in {:?}", num_records, insert_duration);

    // Verify performance is reasonable (should complete well under 10 seconds)
    assert!(
        insert_duration.as_secs() < 10,
        "Performance test: inserts took too long: {:?}",
        insert_duration
    );

    // Allow time for writes to complete
    sleep(Duration::from_millis(500)).await;

    let db =
        Arc::try_unwrap(database).map_err(|_| anyhow::anyhow!("Database still has references"))?;
    db.shutdown().await?;

    Ok(())
}

#[tokio::test]
async fn test_compactor_integration_with_live_system() -> Result<()> {
    let (_temp_dir, database) = setup_complete_database_system().await?;

    // Start compactor service
    let cancellation_token = CancellationToken::new();
    let compactor_database = Arc::clone(&database);
    let compactor_task = {
        let token = cancellation_token.clone();
        tokio::spawn(async move {
            use log_service::run_compactor_service;
            run_compactor_service(compactor_database, token).await
        })
    };

    // Insert data while compactor is running
    for i in 0..10 {
        let activity = ActivityLogRow {
            when: format!("2024-01-15T10:{}:00Z", 30 + i),
            action: format!("live_compactor_test_{}", i),
            user_id: Some(format!("live-user-{}", i)),
            target: Some(format!("live-target-{}", i)),
            extra: Some(format!("Live compactor test {}", i)),
        };
        database.activity_log.insert(activity).await?;

        // Small delay between inserts
        sleep(Duration::from_millis(50)).await;
    }

    // Allow time for data to be written
    sleep(Duration::from_millis(200)).await;

    // Cancel compactor
    cancellation_token.cancel();

    // Wait for compactor to shut down
    let compactor_result = timeout(Duration::from_secs(10), compactor_task).await;
    assert!(
        compactor_result.is_ok(),
        "Compactor should shutdown gracefully"
    );

    let db =
        Arc::try_unwrap(database).map_err(|_| anyhow::anyhow!("Database still has references"))?;
    db.shutdown().await?;

    Ok(())
}
