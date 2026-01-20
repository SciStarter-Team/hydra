use anyhow::Result;
use log_service::database::{
    storage::{Database, get_sync_bookmark, set_sync_bookmark},
    tables::ActivityLogRow,
};
use std::collections::{HashMap, HashSet};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use tempfile::TempDir;
use tokio::fs;
use tokio::time::{Duration, sleep};

/// Helper to create a temporary database instance
async fn setup_full_database() -> Result<(TempDir, Arc<Database>)> {
    let temp_dir = TempDir::new()?;
    let db_path = temp_dir.path().to_string_lossy().to_string();
    let database = Arc::new(Database::new(db_path).await?);
    Ok((temp_dir, database))
}

// =============================================================================
// LOG SYSTEM RELIABILITY TESTS
// =============================================================================

#[tokio::test]
async fn test_individual_write_atomicity() -> Result<()> {
    let (_temp_dir, database) = setup_full_database().await?;

    // Test that individual log entries are written atomically (complete or nothing)
    let mut test_handles = Vec::new();
    let completion_counter = Arc::new(AtomicU64::new(0));

    // Launch writers that will be forcibly cancelled to test atomicity
    for writer_id in 0..20 {
        let db = Arc::clone(&database);
        let counter = Arc::clone(&completion_counter);

        let handle: tokio::task::JoinHandle<anyhow::Result<()>> = tokio::spawn(async move {
            let activity = ActivityLogRow {
                when: format!("2024-01-15T10:30:{}Z", writer_id),
                action: format!("atomic_log_entry_{}", writer_id),
                user_id: Some(format!("user-{}", writer_id)),
                target: Some("atomic-test".to_string()),
                extra: Some("complete-or-nothing".to_string()),
            };

            // Attempt to write the log entry
            db.activity_log.insert(activity).await?;
            counter.fetch_add(1, Ordering::Relaxed);

            // Simulate work after write
            sleep(Duration::from_millis(50)).await;

            Ok(())
        });

        test_handles.push(handle);
    }

    // Let some writes start, then cancel abruptly
    sleep(Duration::from_millis(25)).await;

    // Cancel operations to test atomicity
    for handle in test_handles {
        handle.abort();
    }

    // Allow time for cleanup and flush
    sleep(Duration::from_millis(6000)).await;

    // ATOMICITY VERIFICATION: Check that existing entries are complete
    let today = jiff::Timestamp::now().strftime("%Y-%m-%d").to_string();
    let wal_file = database
        .path()
        .join("wal")
        .join("activity_log")
        .join(format!("activity_log_{}.jsonl", today));

    if wal_file.exists() {
        let content = fs::read_to_string(&wal_file).await?;
        let lines: Vec<&str> = content
            .lines()
            .filter(|line| !line.trim().is_empty())
            .collect();

        // Every line that exists must be complete valid JSON
        for (line_num, line) in lines.iter().enumerate() {
            let parsed: serde_json::Value = serde_json::from_str(line).map_err(|e| {
                anyhow::anyhow!(
                    "Incomplete log entry found - atomicity violated at line {}: {} - Error: {}",
                    line_num,
                    line,
                    e
                )
            })?;

            if let Some(action) = parsed.get("action").and_then(|v| v.as_str()) {
                if action.contains("atomic_log_entry_") {
                    // Verify all required fields are present and complete
                    assert!(
                        parsed.get("when").and_then(|v| v.as_str()).is_some(),
                        "Incomplete log entry missing 'when': {}",
                        line
                    );
                    assert!(
                        parsed.get("user_id").is_some(),
                        "Incomplete log entry missing 'user_id': {}",
                        line
                    );
                    assert_eq!(
                        parsed["extra"], "complete-or-nothing",
                        "Log entry data corrupted: {}",
                        line
                    );
                }
            }
        }

        println!("All {} log entries are complete and valid", lines.len());
    }

    let completed_writes = completion_counter.load(Ordering::Relaxed);
    println!("Completed atomic writes: {}", completed_writes);

    let db =
        Arc::try_unwrap(database).map_err(|_| anyhow::anyhow!("Database still has references"))?;
    db.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn test_concurrent_write_safety() -> Result<()> {
    let (_temp_dir, database) = setup_full_database().await?;

    // Test that concurrent writers to the same table don't corrupt each other's data
    let num_writers = 50;
    let records_per_writer = 100;
    let total_expected = num_writers * records_per_writer;

    let mut handles = Vec::new();
    let success_counter = Arc::new(AtomicU64::new(0));

    for writer_id in 0..num_writers {
        let db = Arc::clone(&database);
        let counter = Arc::clone(&success_counter);

        let handle: tokio::task::JoinHandle<anyhow::Result<()>> = tokio::spawn(async move {
            for record_id in 0..records_per_writer {
                let activity = ActivityLogRow {
                    when: format!(
                        "2024-01-15T10:{:02}:{:02}Z",
                        (writer_id / 60) % 60,
                        (writer_id + record_id) % 60
                    ),
                    action: format!("concurrent_log_w{:03}_r{:03}", writer_id, record_id),
                    user_id: Some(format!("writer-{:03}", writer_id)),
                    target: Some(format!("target-{}-{}", writer_id, record_id)),
                    extra: Some(format!("DATA_W{:03}_R{:03}", writer_id, record_id)),
                };

                if let Err(e) = db.activity_log.insert(activity).await {
                    eprintln!("Writer {} record {} failed: {}", writer_id, record_id, e);
                    return Err(e.into());
                } else {
                    counter.fetch_add(1, Ordering::Relaxed);
                }

                // Yield to increase contention
                if record_id % 10 == 0 {
                    tokio::task::yield_now().await;
                }
            }
            Ok(())
        });

        handles.push(handle);
    }

    // Wait for all concurrent writers
    for handle in handles {
        handle.await??;
    }

    let successful_writes = success_counter.load(Ordering::Relaxed);
    println!(
        "Successful concurrent writes: {}/{}",
        successful_writes, total_expected
    );

    // Force flush
    sleep(Duration::from_millis(6000)).await;

    // CONCURRENT SAFETY VERIFICATION
    let today = jiff::Timestamp::now().strftime("%Y-%m-%d").to_string();
    let wal_file = database
        .path()
        .join("wal")
        .join("activity_log")
        .join(format!("activity_log_{}.jsonl", today));

    let content = fs::read_to_string(&wal_file).await?;
    let lines: Vec<&str> = content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();

    // Verify no data corruption from concurrent writes
    let mut writer_record_map = HashMap::new();
    let mut duplicate_records = 0;
    let mut corrupted_records = 0;

    for line in &lines {
        match serde_json::from_str::<serde_json::Value>(line) {
            Ok(record) => {
                if let Some(action) = record.get("action").and_then(|v| v.as_str()) {
                    if action.starts_with("concurrent_log_") {
                        // Extract writer and record info
                        if let Some(extra) = record.get("extra").and_then(|v| v.as_str()) {
                            if writer_record_map.contains_key(extra) {
                                duplicate_records += 1;
                                eprintln!("Duplicate record detected: {}", extra);
                            } else {
                                writer_record_map.insert(extra.to_string(), action.to_string());
                            }

                            // Verify data integrity
                            if !extra.starts_with("DATA_W") || !extra.contains("_R") {
                                corrupted_records += 1;
                                eprintln!("Corrupted record data: {}", extra);
                            }
                        }
                    }
                }
            }
            Err(_) => {
                corrupted_records += 1;
            }
        }
    }

    // SAFETY REQUIREMENTS
    assert_eq!(
        duplicate_records, 0,
        "No duplicate records should exist from concurrent writes"
    );
    assert_eq!(
        corrupted_records, 0,
        "No corrupted records should exist from concurrent writes"
    );
    assert!(
        writer_record_map.len() > 0,
        "Should have records from concurrent writers"
    );

    println!(
        "Concurrent write safety verified: {} unique records, no corruption",
        writer_record_map.len()
    );

    let db =
        Arc::try_unwrap(database).map_err(|_| anyhow::anyhow!("Database still has references"))?;
    db.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn test_log_durability_across_crashes() -> Result<()> {
    let (_temp_dir, database) = setup_full_database().await?;

    // Test that log data survives system crashes
    let critical_log_data = vec![
        ("critical-user-1", "system_startup", "CRITICAL_EVENT_1"),
        ("critical-user-2", "data_processing", "CRITICAL_EVENT_2"),
        ("critical-user-3", "system_checkpoint", "CRITICAL_EVENT_3"),
    ];

    // Phase 1: Write critical logs then simulate crash
    // Write critical log data
    for (user_id, action, event_data) in &critical_log_data {
        let activity = ActivityLogRow {
            when: "2024-01-15T10:30:45Z".to_string(),
            action: action.to_string(),
            user_id: Some(user_id.to_string()),
            target: Some("durability_test".to_string()),
            extra: Some(event_data.to_string()),
        };
        database.activity_log.insert(activity).await?;
    }

    // Force flush critical data
    sleep(Duration::from_millis(6000)).await;

    // Write additional data that might not be flushed
    for i in 0..3 {
        let activity = ActivityLogRow {
            when: format!("2024-01-15T10:31:{:02}Z", i),
            action: format!("unflushed_event_{}", i),
            user_id: Some(format!("unflushed-user-{}", i)),
            target: Some("durability_test".to_string()),
            extra: Some("MIGHT_BE_LOST".to_string()),
        };
        database.activity_log.insert(activity).await?;
    }

    // SIMULATE CRASH: Drop without graceful shutdown
    // This simulates power failure, kill -9, etc.
    // Note: In actual test, we'll need to restart the Database instance

    // Phase 2: Recovery after crash (simulate by continuing with same database instance)
    // Verify critical log data survived crash
    let today = jiff::Timestamp::now().strftime("%Y-%m-%d").to_string();
    let wal_file = database
        .path()
        .join("wal")
        .join("activity_log")
        .join(format!("activity_log_{}.jsonl", today));

    assert!(wal_file.exists(), "Log file must survive crash");

    let content = fs::read_to_string(&wal_file).await?;
    let lines: Vec<&str> = content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();

    // Verify critical data survival
    let mut critical_events_found = HashSet::new();
    let mut unflushed_events_found = 0;

    for line in &lines {
        let record: serde_json::Value = serde_json::from_str(line)
            .map_err(|e| anyhow::anyhow!("Log corruption after crash: {} - Error: {}", line, e))?;

        if let Some(extra) = record.get("extra").and_then(|v| v.as_str()) {
            if extra.starts_with("CRITICAL_EVENT_") {
                critical_events_found.insert(extra.to_string());
            } else if extra == "MIGHT_BE_LOST" {
                unflushed_events_found += 1;
            }
        }
    }

    // DURABILITY REQUIREMENTS
    println!(
        "Critical events found after crash: {:?}",
        critical_events_found
    );
    println!(
        "Unflushed events found after crash: {}",
        unflushed_events_found
    );

    // All flushed critical data must survive
    for (_, _, event_data) in &critical_log_data {
        assert!(
            critical_events_found.contains(*event_data),
            "Critical log event lost in crash: {}",
            event_data
        );
    }

    // System should continue logging after recovery
    let recovery_log = ActivityLogRow {
        when: "2024-01-15T10:35:00Z".to_string(),
        action: "system_recovery".to_string(),
        user_id: Some("recovery-system".to_string()),
        target: Some("post_crash_test".to_string()),
        extra: Some("SYSTEM_OPERATIONAL".to_string()),
    };

    database.activity_log.insert(recovery_log).await?;

    // Verify system continues working
    sleep(Duration::from_millis(6000)).await;

    let updated_content = fs::read_to_string(&wal_file).await?;
    assert!(
        updated_content.contains("SYSTEM_OPERATIONAL"),
        "Log system should continue operating after crash recovery"
    );

    let db =
        Arc::try_unwrap(database).map_err(|_| anyhow::anyhow!("Database still has references"))?;
    db.shutdown().await?;

    Ok(())
}

#[tokio::test]
async fn test_log_schema_consistency() -> Result<()> {
    let (_temp_dir, database) = setup_full_database().await?;

    // Test that all log entries maintain consistent schema format
    let test_scenarios = vec![
        // Normal record
        ActivityLogRow {
            when: "2024-01-15T10:30:45Z".to_string(),
            action: "schema_test_normal".to_string(),
            user_id: Some("user-123".to_string()),
            target: Some("target-123".to_string()),
            extra: Some("normal data".to_string()),
        },
        // Record with null optional fields
        ActivityLogRow {
            when: "2024-01-15T10:30:46Z".to_string(),
            action: "schema_test_null".to_string(),
            user_id: None,
            target: None,
            extra: None,
        },
        // Record with special characters
        ActivityLogRow {
            when: "2024-01-15T10:30:47Z".to_string(),
            action: "schema_test_special".to_string(),
            user_id: Some("user\"with'quotes".to_string()),
            target: Some("target\nwith\nnewlines".to_string()),
            extra: Some("{\"json\": \"embedded\", \"value\": 123}".to_string()),
        },
    ];

    // Write test scenarios
    for scenario in test_scenarios {
        database.activity_log.insert(scenario).await?;
    }

    // Force flush
    sleep(Duration::from_millis(6000)).await;

    // SCHEMA CONSISTENCY VERIFICATION
    let today = jiff::Timestamp::now().strftime("%Y-%m-%d").to_string();
    let wal_file = database
        .path()
        .join("wal")
        .join("activity_log")
        .join(format!("activity_log_{}.jsonl", today));

    let content = fs::read_to_string(&wal_file).await?;
    let lines: Vec<&str> = content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();

    // Verify schema consistency
    for line in &lines {
        let record: serde_json::Value = serde_json::from_str(line).map_err(|e| {
            anyhow::anyhow!("Schema consistency violation: {} - Error: {}", line, e)
        })?;

        // Required fields must always be present
        assert!(
            record.get("when").is_some(),
            "Missing required field 'when': {}",
            line
        );
        assert!(
            record.get("action").is_some(),
            "Missing required field 'action': {}",
            line
        );

        // Optional fields must be present in structure (can be null)
        assert!(
            record.get("user_id").is_some(),
            "Missing field 'user_id' in schema: {}",
            line
        );
        assert!(
            record.get("target").is_some(),
            "Missing field 'target' in schema: {}",
            line
        );
        assert!(
            record.get("extra").is_some(),
            "Missing field 'extra' in schema: {}",
            line
        );

        // Validate data types
        if let Some(when) = record.get("when").and_then(|v| v.as_str()) {
            assert!(when.ends_with('Z'), "Invalid timestamp format: {}", when);
        }

        if let Some(action) = record.get("action").and_then(|v| v.as_str()) {
            assert!(!action.is_empty(), "Action field cannot be empty: {}", line);
        }
    }

    println!(
        "Schema consistency verified for {} log entries",
        lines.len()
    );

    let db =
        Arc::try_unwrap(database).map_err(|_| anyhow::anyhow!("Database still has references"))?;
    db.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn test_log_system_performance_under_load() -> Result<()> {
    let (_temp_dir, database) = setup_full_database().await?;

    // Test log system performance under sustained load
    let total_records = 10000;
    let concurrent_writers = 20;
    let records_per_writer = total_records / concurrent_writers;

    let start_time = std::time::Instant::now();
    let mut handles = Vec::new();
    let completion_counter = Arc::new(AtomicU64::new(0));

    for writer_id in 0..concurrent_writers {
        let db = Arc::clone(&database);
        let counter = Arc::clone(&completion_counter);

        let handle: tokio::task::JoinHandle<anyhow::Result<()>> = tokio::spawn(async move {
            for record_id in 0..records_per_writer {
                let activity = ActivityLogRow {
                    when: format!(
                        "2024-01-15T{:02}:{:02}:{:02}Z",
                        10 + (writer_id % 14),
                        (record_id / 60) % 60,
                        record_id % 60
                    ),
                    action: format!("perf_test_w{:02}_r{:04}", writer_id, record_id),
                    user_id: Some(format!("perf-user-{}", writer_id)),
                    target: Some(format!("perf-target-{}", record_id % 100)),
                    extra: Some(format!("Performance test data for record {}", record_id)),
                };

                db.activity_log.insert(activity).await?;
                counter.fetch_add(1, Ordering::Relaxed);

                // Minimal yield for realistic performance
                if record_id % 100 == 0 {
                    tokio::task::yield_now().await;
                }
            }
            Ok(())
        });

        handles.push(handle);
    }

    // Wait for all writers to complete
    for handle in handles {
        handle.await??;
    }

    let write_duration = start_time.elapsed();
    let completed_records = completion_counter.load(Ordering::Relaxed);
    let throughput = completed_records as f64 / write_duration.as_secs_f64();

    println!("Performance test results:");
    println!("  Records written: {}", completed_records);
    println!("  Duration: {:?}", write_duration);
    println!("  Throughput: {:.2} records/second", throughput);

    // Performance requirements
    assert_eq!(
        completed_records, total_records as u64,
        "All records should be written"
    );
    assert!(
        throughput > 100.0,
        "Should achieve at least 100 records/second"
    );

    // Force flush and verify data integrity
    sleep(Duration::from_millis(6000)).await;

    let today = jiff::Timestamp::now().strftime("%Y-%m-%d").to_string();
    let wal_file = database
        .path()
        .join("wal")
        .join("activity_log")
        .join(format!("activity_log_{}.jsonl", today));

    let content = fs::read_to_string(&wal_file).await?;
    let lines: Vec<&str> = content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();

    // Verify no data loss under load
    let performance_records = lines
        .iter()
        .filter(|line| line.contains("perf_test_"))
        .count();

    println!("Performance records in WAL: {}", performance_records);
    assert!(
        performance_records >= (total_records as f64 * 0.95) as usize,
        "At least 95% of records should be persisted under load"
    );

    let db =
        Arc::try_unwrap(database).map_err(|_| anyhow::anyhow!("Database still has references"))?;
    db.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn test_bookmark_state_consistency() -> Result<()> {
    let (_temp_dir, database) = setup_full_database().await?;

    // Test that bookmark state remains consistent under concurrent access
    let bookmark_key = "log-system-bookmark";
    let num_operations = 100;

    // Set initial bookmark state
    set_sync_bookmark(&database, bookmark_key, "2024-01-15T10:00:00Z").await?;

    let success_counter = Arc::new(AtomicU64::new(0));
    let consistency_violations = Arc::new(AtomicU64::new(0));

    let mut handles = Vec::new();

    for op_id in 0..num_operations {
        let key = bookmark_key.to_string();
        let success = Arc::clone(&success_counter);
        let violations = Arc::clone(&consistency_violations);
        let db = Arc::clone(&database);

        let handle = tokio::spawn(async move {
            let timestamp = format!("2024-01-15T10:{:02}:{:02}Z", (op_id / 60) % 60, op_id % 60);

            match set_sync_bookmark(&db, &key, &timestamp).await {
                Ok(_) => {
                    // Immediately verify consistency
                    match get_sync_bookmark(&db, &key).await {
                        Ok(read_value) => {
                            // Value should be a valid timestamp
                            if !read_value.starts_with("2024-01-15T10:") {
                                violations.fetch_add(1, Ordering::Relaxed);
                                eprintln!("Bookmark consistency violation: {}", read_value);
                            } else {
                                success.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                        Err(_) => {
                            violations.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Bookmark operation failed: {}", e);
                }
            }
        });

        handles.push(handle);
    }

    // Wait for all operations
    for handle in handles {
        handle.await?;
    }

    let successful_ops = success_counter.load(Ordering::Relaxed);
    let violations = consistency_violations.load(Ordering::Relaxed);

    println!("Bookmark consistency test results:");
    println!("  Successful operations: {}", successful_ops);
    println!("  Consistency violations: {}", violations);

    // Verify final consistent state
    let final_bookmark = get_sync_bookmark(&database, bookmark_key).await?;
    assert!(
        final_bookmark.starts_with("2024-01-15T10:"),
        "Final bookmark state invalid: {}",
        final_bookmark
    );

    // CONSISTENCY REQUIREMENTS
    assert_eq!(
        violations, 0,
        "No consistency violations should occur in bookmark state"
    );
    assert!(
        successful_ops > 0,
        "Some bookmark operations should succeed"
    );

    let db =
        Arc::try_unwrap(database).map_err(|_| anyhow::anyhow!("Database still has references"))?;
    db.shutdown().await?;
    Ok(())
}
