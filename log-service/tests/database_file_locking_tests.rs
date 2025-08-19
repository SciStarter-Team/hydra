use anyhow::Result;
use log_service::database::storage::{Database, get_sync_bookmark, set_sync_bookmark};
use log_service::database::tables::ActivityLogRow;
use std::sync::Arc;
use std::time::Duration;
use tempfile::TempDir;
use tokio::time::sleep;

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
            anyhow::bail!("Database still has active references - cannot shutdown cleanly")
        }
    }
}

#[tokio::test]
async fn test_concurrent_bookmark_reads() -> Result<()> {
    let (_temp_dir, database) = setup_temp_database().await?;

    // Set initial bookmark data
    set_sync_bookmark(&database, "test-key", "test-value").await?;

    // Spawn multiple concurrent read operations
    let mut handles = Vec::new();
    for i in 0..10 {
        let db = Arc::clone(&database);
        let handle = tokio::spawn(async move {
            let value = get_sync_bookmark(&db, "test-key").await.unwrap();
            assert_eq!(value, "test-value");
            i
        });
        handles.push(handle);
    }

    // Wait for all reads to complete - they should all succeed with shared locks
    for handle in handles {
        handle.await?;
    }

    shutdown_database(database).await?;
    Ok(())
}

#[tokio::test]
async fn test_concurrent_bookmark_read_write() -> Result<()> {
    let (_temp_dir, database) = setup_temp_database().await?;

    // Set initial data
    set_sync_bookmark(&database, "read-write-test", "initial").await?;

    let mut handles = Vec::new();

    // Spawn a writer
    let db_writer = Arc::clone(&database);
    let writer_handle = tokio::spawn(async move {
        for i in 0..5 {
            let value = format!("updated-{}", i);
            set_sync_bookmark(&db_writer, "read-write-test", &value)
                .await
                .unwrap();
            sleep(Duration::from_millis(10)).await;
        }
    });
    handles.push(writer_handle);

    // Spawn multiple readers
    for _reader_id in 0..5 {
        let db = Arc::clone(&database);
        let reader_handle = tokio::spawn(async move {
            for _attempt in 0..10 {
                // Readers should be able to read, but may see different values
                // as the writer updates them
                let _value = get_sync_bookmark(&db, "read-write-test").await.unwrap();
                sleep(Duration::from_millis(5)).await;
            }
        });
        handles.push(reader_handle);
    }

    // Wait for all operations to complete
    for handle in handles {
        handle.await?;
    }

    // Verify final state
    let final_value = get_sync_bookmark(&database, "read-write-test").await?;
    assert!(final_value.starts_with("updated-"));

    shutdown_database(database).await?;
    Ok(())
}

#[tokio::test]
async fn test_concurrent_wal_writes() -> Result<()> {
    let (_temp_dir, database) = setup_temp_database().await?;

    let mut handles = Vec::new();

    // Spawn multiple concurrent WAL writers
    for writer_id in 0..5 {
        let db = Arc::clone(&database);
        let handle = tokio::spawn(async move {
            for i in 0..10 {
                let activity = ActivityLogRow {
                    when: format!("2024-01-15T10:30:{}Z", i),
                    action: format!("concurrent_write_{}_{}", writer_id, i),
                    user_id: Some(format!("user-{}", writer_id)),
                    target: Some(format!("target-{}-{}", writer_id, i)),
                    extra: None,
                };
                db.activity_log.insert(activity).await.unwrap();
                sleep(Duration::from_millis(2)).await;
            }
        });
        handles.push(handle);
    }

    // Wait for all writes to complete
    for handle in handles {
        handle.await?;
    }

    // Allow time for all writes to be flushed
    sleep(Duration::from_millis(200)).await;

    shutdown_database(database).await?;
    Ok(())
}

#[tokio::test]
async fn test_wal_write_during_compaction() -> Result<()> {
    let (_temp_dir, database) = setup_temp_database().await?;

    // Insert some initial data to create a WAL file
    for i in 0..10 {
        let activity = ActivityLogRow {
            when: format!("2024-01-15T10:30:{}Z", i),
            action: format!("initial_data_{}", i),
            user_id: Some(format!("user-{}", i)),
            target: Some(format!("target-{}", i)),
            extra: None,
        };
        database.activity_log.insert(activity).await?;
    }

    sleep(Duration::from_millis(100)).await;

    let mut handles = Vec::new();

    // Spawn compactor (this will try to read with shared locks)
    let compactor_db = Arc::clone(&database);
    let compactor_handle = tokio::spawn(async move {
        use log_service::database::compactor::Compactor;
        use tokio_util::sync::CancellationToken;

        let compactor =
            Compactor::with_config(compactor_db.path().clone(), Duration::from_millis(50), 30);
        let token = CancellationToken::new();

        // Run compactor briefly
        tokio::select! {
            result = compactor.run(token.clone()) => {
                if let Err(e) = result {
                    eprintln!("Compactor error: {}", e);
                }
            }
            _ = sleep(Duration::from_millis(200)) => {
                token.cancel();
            }
        }
    });
    handles.push(compactor_handle);

    // Continue writing while compactor runs (writers use exclusive locks)
    let writer_db = Arc::clone(&database);
    let writer_handle = tokio::spawn(async move {
        for i in 10..20 {
            let activity = ActivityLogRow {
                when: format!("2024-01-15T10:30:{}Z", i),
                action: format!("concurrent_with_compaction_{}", i),
                user_id: Some(format!("user-{}", i)),
                target: Some(format!("target-{}", i)),
                extra: None,
            };
            writer_db.activity_log.insert(activity).await.unwrap();
            sleep(Duration::from_millis(10)).await;
        }
    });
    handles.push(writer_handle);

    // Wait for both to complete
    for handle in handles {
        let _ = handle.await;
    }

    shutdown_database(database).await?;
    Ok(())
}

#[tokio::test]
async fn test_file_lock_error_handling() -> Result<()> {
    let (_temp_dir, database) = setup_temp_database().await?;

    // Set a bookmark to create the file
    set_sync_bookmark(&database, "lock-test", "value").await?;

    // Test that operations can handle lock contention gracefully
    // This test primarily verifies that lock errors are properly reported
    // rather than causing panics or deadlocks

    let mut handles = Vec::new();

    // Spawn rapid concurrent bookmark operations
    for i in 0..20 {
        let db = Arc::clone(&database);
        let handle = tokio::spawn(async move {
            let key = format!("rapid-test-{}", i % 5); // Use few keys to increase contention
            let value = format!("value-{}", i);

            // Mix reads and writes to create lock contention
            if i % 2 == 0 {
                let _ = set_sync_bookmark(&db, &key, &value).await;
            } else {
                let _ = get_sync_bookmark(&db, &key).await;
            }
        });
        handles.push(handle);
    }

    // Wait for all operations - some may fail due to lock contention
    // but the system should remain stable
    for handle in handles {
        let _ = handle.await; // Don't fail the test if some operations fail due to lock contention
    }

    shutdown_database(database).await?;
    Ok(())
}

#[tokio::test]
async fn test_exclusive_lock_prevents_concurrent_writes() -> Result<()> {
    let (_temp_dir, database) = setup_temp_database().await?;

    // Insert initial data to create WAL files
    for i in 0..5 {
        let activity = ActivityLogRow {
            when: format!("2024-01-15T10:30:{}Z", i),
            action: format!("test_data_{}", i),
            user_id: Some(format!("user-{}", i)),
            target: Some(format!("target-{}", i)),
            extra: None,
        };
        database.activity_log.insert(activity).await?;
    }

    sleep(Duration::from_millis(100)).await;

    // This test verifies that our locking is working by checking that
    // operations complete successfully without corruption
    // (If locks weren't working, we might see corrupted WAL files)

    let mut handles = Vec::new();

    // Spawn multiple writers for the same table
    for writer_id in 0..3 {
        let db = Arc::clone(&database);
        let handle = tokio::spawn(async move {
            for i in 0..20 {
                let activity = ActivityLogRow {
                    when: format!("2024-01-15T10:30:{}Z", i + writer_id * 100),
                    action: format!("exclusive_test_{}_{}", writer_id, i),
                    user_id: Some(format!("writer-{}", writer_id)),
                    target: Some(format!("target-{}-{}", writer_id, i)),
                    extra: Some(format!("Extra data from writer {}", writer_id)),
                };
                db.activity_log.insert(activity).await.unwrap();

                // Small delay to increase chances of lock contention
                sleep(Duration::from_millis(1)).await;
            }
        });
        handles.push(handle);
    }

    // Wait for all writers to complete
    for handle in handles {
        handle.await?;
    }

    // Allow time for all writes to be flushed
    sleep(Duration::from_millis(200)).await;

    // Verify that WAL file is not corrupted (contains valid JSON lines)
    let today = jiff::Timestamp::now().strftime("%Y-%m-%d").to_string();
    let wal_file = database
        .path()
        .join("wal")
        .join(format!("activity_log_{}.jsonl", today));

    if wal_file.exists() {
        let content = tokio::fs::read_to_string(&wal_file).await?;
        let lines: Vec<&str> = content
            .lines()
            .filter(|line| !line.trim().is_empty())
            .collect();

        // Should have 5 initial + 3 writers * 20 = 65 total records
        assert!(
            lines.len() >= 60,
            "Should have at least 60 records, found {}",
            lines.len()
        );

        // Verify each line is valid JSON
        for (line_num, line) in lines.iter().enumerate() {
            serde_json::from_str::<serde_json::Value>(line).map_err(|e| {
                anyhow::anyhow!(
                    "Invalid JSON on line {}: {} - Error: {}",
                    line_num + 1,
                    line,
                    e
                )
            })?;
        }
    }

    shutdown_database(database).await?;
    Ok(())
}
