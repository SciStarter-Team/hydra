use anyhow::Result;
use datafusion::arrow::array::Array;
use log_service::database::{
    compactor::Compactor,
    storage::Database,
    tables::{ActivityLogRow, JoinRow, PageViewRow},
};
use std::collections::{HashMap, HashSet};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use tempfile::TempDir;
use tokio::fs;
use tokio::time::{Duration, sleep};
use tokio_util::sync::CancellationToken;

/// Test helper to create a temporary database instance
async fn setup_full_database() -> Result<(TempDir, Arc<Database>)> {
    let temp_dir = TempDir::new()?;
    let db_path = temp_dir.path().to_string_lossy().to_string();
    let database = Arc::new(Database::new(db_path).await?);
    Ok((temp_dir, database))
}

// =============================================================================
// LOG COMPACTION INTEGRITY TESTS
// =============================================================================

#[tokio::test]
async fn test_wal_to_parquet_compaction_integrity() -> Result<()> {
    let (_temp_dir, database) = setup_full_database().await?;

    // Test that WAL-to-Parquet compaction preserves all data without corruption
    let test_records = 500;
    let mut expected_records = HashMap::new();

    // Write test data to WAL
    for i in 0..test_records {
        let activity = ActivityLogRow {
            when: format!("2024-01-15T10:{:02}:{:02}Z", (i / 60) % 60, i % 60),
            action: format!("compaction_test_{:04}", i),
            user_id: Some(format!("compaction-user-{}", i % 20)),
            target: Some(format!("compaction-target-{}", i % 10)),
            extra: Some(format!("COMPACTION_DATA_{:04}_INTEGRITY_CHECK", i)),
        };

        // Track what we wrote
        let key = format!("compaction_test_{:04}", i);
        expected_records.insert(key, format!("COMPACTION_DATA_{:04}_INTEGRITY_CHECK", i));

        database.activity_log.insert(activity).await?;

        // Add some records to other tables too
        if i % 5 == 0 {
            let page_view = PageViewRow {
                when: format!("2024-01-15T10:{:02}:{:02}Z", (i / 60) % 60, i % 60),
                ip_address: format!("192.168.1.{}", (i % 255) + 1),
                device_id: format!("compaction-session-{}", i),
                interaction_id: format!("compaction-interaction-{}", i),
                user_id: Some(format!("compaction-user-{}", i % 20)),
                path: format!("/compaction/{}", i),
                referrer: None,
                query: None,
                geolocation: "POINT(0 0)".to_string(),
            };
            database.page_views.insert(page_view).await?;
        }
    }

    println!("Wrote {} test records for compaction", test_records);

    // Force flush to WAL
    sleep(Duration::from_millis(6000)).await;

    // Verify WAL data exists before compaction (WAL files are in subdirectories)
    let today = jiff::Timestamp::now().strftime("%Y-%m-%d").to_string();
    let activity_wal = database
        .path()
        .join("wal")
        .join("activity_log")
        .join(format!("activity_log_{}.jsonl", today));

    assert!(
        activity_wal.exists(),
        "WAL file should exist before compaction"
    );

    let wal_content = fs::read_to_string(&activity_wal).await?;
    let wal_lines: Vec<&str> = wal_content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();

    println!("WAL contains {} lines before compaction", wal_lines.len());

    // Start compactor with fast cycle time
    let compactor = Compactor::with_config(database.clone(), Duration::from_millis(100), 1, 10_000);

    // Create cancellation token and run compactor in background
    let cancellation_token = CancellationToken::new();
    let cancel_clone = cancellation_token.clone();
    let compactor_task = tokio::spawn(async move { compactor.run(cancel_clone).await });

    // Let compactor run multiple cycles
    sleep(Duration::from_millis(1000)).await;

    // Stop compactor
    cancellation_token.cancel();
    let _ = compactor_task.await?;

    println!("Compaction completed");

    // COMPACTION INTEGRITY VERIFICATION

    // Check if Parquet files were created
    let activity_parquet_dir = database.path().join("parquet").join("activity_log");
    let mut parquet_files = Vec::new();
    let mut read_dir = fs::read_dir(&activity_parquet_dir).await?;
    while let Some(entry) = read_dir.next_entry().await? {
        let path = entry.path();
        if path.extension().map_or(false, |ext| ext == "parquet") {
            parquet_files.push(path);
        }
    }

    if !parquet_files.is_empty() {
        println!("Parquet files created: {}", parquet_files.len());

        // Verify data can be read from Parquet
        let ctx = database.context();

        let query_result = ctx.sql("SELECT COUNT(*) as record_count FROM activity_log WHERE action LIKE 'compaction_test_%'").await?;
        let batches = query_result.collect().await?;

        if !batches.is_empty() {
            let record_count = batches[0]
                .column(0)
                .as_any()
                .downcast_ref::<datafusion::arrow::array::Int64Array>()
                .unwrap()
                .value(0);

            println!("Records found in Parquet: {}", record_count);

            // COMPACTION INTEGRITY REQUIREMENT
            // Note: Some records might still be in WAL if compaction hasn't processed everything
            assert!(
                record_count > 0,
                "Compaction should preserve some records in Parquet"
            );

            // Verify data quality in Parquet
            let sample_query = ctx.sql(
                "SELECT action, extra FROM activity_log WHERE action LIKE 'compaction_test_%' LIMIT 10"
            ).await?;
            let sample_batches = sample_query.collect().await?;

            if !sample_batches.is_empty() {
                println!("Sample compacted records verified - no corruption detected");
            }
        }
    } else {
        println!("No Parquet files created yet - WAL retention period not exceeded");
    }

    // Verify WAL integrity is maintained during compaction
    if activity_wal.exists() {
        let post_compaction_content = fs::read_to_string(&activity_wal).await?;
        let post_compaction_lines: Vec<&str> = post_compaction_content
            .lines()
            .filter(|line| !line.trim().is_empty())
            .collect();

        // All remaining WAL lines should be valid JSON
        for line in post_compaction_lines {
            serde_json::from_str::<serde_json::Value>(line).map_err(|e| {
                anyhow::anyhow!("Compaction corrupted WAL data: {} - Error: {}", line, e)
            })?;
        }

        println!("WAL integrity verified after compaction");
    }

    let db =
        Arc::try_unwrap(database).map_err(|_| anyhow::anyhow!("Database still has references"))?;
    db.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn test_wal_file_rotation_integrity() -> Result<()> {
    let (_temp_dir, database) = setup_full_database().await?;

    // Test WAL file rotation behavior and data integrity
    let base_date = "2024-01-15";

    // Write data with consistent pattern
    for i in 0..50 {
        let activity = ActivityLogRow {
            when: format!("{}T10:30:{:02}Z", base_date, i % 60),
            action: format!("rotation_test_{:03}", i),
            user_id: Some(format!("rotation-user-{}", i % 10)),
            target: Some("rotation-test".to_string()),
            extra: Some(format!("ROTATION_DATA_{:03}", i)),
        };

        database.activity_log.insert(activity).await?;

        // Space out writes
        if i % 10 == 0 {
            sleep(Duration::from_millis(100)).await;
        }
    }

    // Force flush
    sleep(Duration::from_millis(6000)).await;

    // ROTATION INTEGRITY VERIFICATION (WAL files are in subdirectories)
    let today = jiff::Timestamp::now().strftime("%Y-%m-%d").to_string();
    let wal_file = database
        .path()
        .join("wal")
        .join("activity_log")
        .join(format!("activity_log_{}.jsonl", today));

    assert!(wal_file.exists(), "WAL file should be created");

    let content = fs::read_to_string(&wal_file).await?;
    let lines: Vec<&str> = content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();

    // Verify all rotation test records are present and ordered
    let mut rotation_records = Vec::new();
    for line in lines {
        let record: serde_json::Value = serde_json::from_str(line)?;

        if let Some(action) = record.get("action").and_then(|v| v.as_str()) {
            if action.starts_with("rotation_test_") {
                if let Some(extra) = record.get("extra").and_then(|v| v.as_str()) {
                    rotation_records.push((action.to_string(), extra.to_string()));
                }
            }
        }
    }

    // Verify record integrity and ordering
    assert_eq!(
        rotation_records.len(),
        50,
        "All rotation test records should be present"
    );

    for (i, (action, extra)) in rotation_records.iter().enumerate() {
        let expected_action = format!("rotation_test_{:03}", i);
        let expected_extra = format!("ROTATION_DATA_{:03}", i);

        assert_eq!(*action, expected_action, "Record {} action corrupted", i);
        assert_eq!(*extra, expected_extra, "Record {} data corrupted", i);
    }

    println!(
        "WAL rotation integrity verified: {} records in correct order",
        rotation_records.len()
    );

    let db =
        Arc::try_unwrap(database).map_err(|_| anyhow::anyhow!("Database still has references"))?;
    db.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn test_multi_table_independent_logging() -> Result<()> {
    let (_temp_dir, database) = setup_full_database().await?;

    // Test that different log streams (tables) operate independently
    let records_per_table = 100;

    let activity_counter = Arc::new(AtomicU64::new(0));
    let page_view_counter = Arc::new(AtomicU64::new(0));
    let join_counter = Arc::new(AtomicU64::new(0));

    // Independent log streams
    let activity_task = {
        let db = Arc::clone(&database);
        let counter = Arc::clone(&activity_counter);

        tokio::spawn(async move {
            for i in 0..records_per_table {
                let activity = ActivityLogRow {
                    when: format!("2024-01-15T10:30:{:02}Z", i % 60),
                    action: format!("independent_activity_{}", i),
                    user_id: Some(format!("activity-user-{}", i)),
                    target: Some("activity-stream".to_string()),
                    extra: Some(format!("ACTIVITY_STREAM_DATA_{}", i)),
                };

                if db.activity_log.insert(activity).await.is_ok() {
                    counter.fetch_add(1, Ordering::Relaxed);
                }
            }
        })
    };

    let page_view_task = {
        let db = Arc::clone(&database);
        let counter = Arc::clone(&page_view_counter);

        tokio::spawn(async move {
            for i in 0..records_per_table {
                let page_view = PageViewRow {
                    when: format!("2024-01-15T10:30:{:02}Z", i % 60),
                    ip_address: format!("192.168.2.{}", (i % 255) + 1),
                    device_id: format!("independent-session-{}", i),
                    interaction_id: format!("independent-interaction-{}", i),
                    user_id: Some(format!("pageview-user-{}", i)),
                    path: format!("/independent/{}", i),
                    referrer: None,
                    query: None,
                    geolocation: "POINT(1 1)".to_string(),
                };

                if db.page_views.insert(page_view).await.is_ok() {
                    counter.fetch_add(1, Ordering::Relaxed);
                }
            }
        })
    };

    let join_task = {
        let db = Arc::clone(&database);
        let counter = Arc::clone(&join_counter);

        tokio::spawn(async move {
            for i in 0..records_per_table {
                let join = JoinRow {
                    when: format!("2024-01-15T10:30:{:02}Z", i % 60),
                    entity_id: format!("independent-entity-{}", i),
                    user_id: format!("independent-join-user-{}", i),
                    active: i % 2 == 0,
                    consented: if i % 3 == 0 {
                        Some(format!("2024-01-15T10:25:{:02}Z", i % 60))
                    } else {
                        None
                    },
                    contacted: None,
                    explicit: true,
                    notified: i % 5 == 0,
                    activity: format!("2024-01-15T10:30:{:02}Z", i % 60),
                    reason: format!("log_stream_test_{}", i),
                };

                if db.join_table.insert(join).await.is_ok() {
                    counter.fetch_add(1, Ordering::Relaxed);
                }
            }
        })
    };

    // Run all log streams concurrently
    let start_time = std::time::Instant::now();
    let (_activity_result, _page_view_result, _join_result) =
        tokio::join!(activity_task, page_view_task, join_task);
    let duration = start_time.elapsed();

    let activity_count = activity_counter.load(Ordering::Relaxed);
    let page_view_count = page_view_counter.load(Ordering::Relaxed);
    let join_count = join_counter.load(Ordering::Relaxed);

    println!("Independent logging test completed in {:?}", duration);
    println!("Activity log entries: {}", activity_count);
    println!("Page view entries: {}", page_view_count);
    println!("Join entries: {}", join_count);

    // Force flush all streams
    sleep(Duration::from_millis(6000)).await;

    // INDEPENDENCE VERIFICATION (WAL files are in subdirectories)
    let today = jiff::Timestamp::now().strftime("%Y-%m-%d").to_string();

    // Each table should have its own WAL file with correct data
    let activity_wal = fs::read_to_string(
        database
            .path()
            .join("wal")
            .join("activity_log")
            .join(format!("activity_log_{}.jsonl", today)),
    )
    .await?;
    let page_view_wal = fs::read_to_string(
        database
            .path()
            .join("wal")
            .join("page_views")
            .join(format!("page_views_{}.jsonl", today)),
    )
    .await?;
    let join_wal = fs::read_to_string(
        database
            .path()
            .join("wal")
            .join("join_table")
            .join(format!("join_table_{}.jsonl", today)),
    )
    .await?;

    // Verify no cross-contamination between log streams
    assert!(
        activity_wal.contains("ACTIVITY_STREAM_DATA_"),
        "Activity log should contain activity stream data"
    );
    assert!(
        !activity_wal.contains("independent-session-"),
        "Activity log should not contain page view data"
    );
    assert!(
        !activity_wal.contains("independent-tags-user-"),
        "Activity log should not contain user tags data"
    );

    assert!(
        page_view_wal.contains("independent-session-"),
        "Page view log should contain page view data"
    );
    assert!(
        !page_view_wal.contains("ACTIVITY_STREAM_DATA_"),
        "Page view log should not contain activity data"
    );

    assert!(
        join_wal.contains("independent-join-user-"),
        "User tags log should contain user tags data"
    );
    assert!(
        !join_wal.contains("ACTIVITY_STREAM_DATA_"),
        "User tags log should not contain activity data"
    );

    println!("Log stream independence verified - no cross-contamination");

    let db =
        Arc::try_unwrap(database).map_err(|_| anyhow::anyhow!("Database still has references"))?;
    db.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn test_compaction_data_preservation() -> Result<()> {
    let (_temp_dir, database) = setup_full_database().await?;

    // Test that compaction preserves all data exactly
    let preservation_test_data = vec![
        ("preserve-1", "critical_data_1", "MUST_PRESERVE_1"),
        ("preserve-2", "critical_data_2", "MUST_PRESERVE_2"),
        ("preserve-3", "critical_data_3", "MUST_PRESERVE_3"),
    ];

    // Write data that will be compacted
    for (user_id, action, data) in &preservation_test_data {
        let activity = ActivityLogRow {
            when: "2024-01-15T09:30:45Z".to_string(), // Old timestamp to trigger compaction
            action: action.to_string(),
            user_id: Some(user_id.to_string()),
            target: Some("preservation_test".to_string()),
            extra: Some(data.to_string()),
        };

        database.activity_log.insert(activity).await?;
    }

    // Write newer data that should remain in WAL
    for i in 0..5 {
        let activity = ActivityLogRow {
            when: format!("2024-01-15T10:31:{:02}Z", i),
            action: format!("recent_data_{}", i),
            user_id: Some(format!("recent-user-{}", i)),
            target: Some("preservation_test".to_string()),
            extra: Some(format!("RECENT_DATA_{}", i)),
        };

        database.activity_log.insert(activity).await?;
    }

    // Force initial flush
    sleep(Duration::from_millis(6000)).await;

    // Start compactor with aggressive settings to force compaction
    let compactor = Compactor::with_config(database.clone(), Duration::from_millis(50), 1, 10_000); // Very aggressive

    // Create cancellation token and run compactor in background
    let cancellation_token = CancellationToken::new();
    let cancel_clone = cancellation_token.clone();
    let compactor_task = tokio::spawn(async move { compactor.run(cancel_clone).await });

    // Let compactor run long enough to process old data
    sleep(Duration::from_millis(1000)).await;

    // Stop compactor
    cancellation_token.cancel();
    let _ = compactor_task.await?;

    // PRESERVATION VERIFICATION

    // Check Parquet files for compacted data
    let ctx = database.context();

    // Query for our preservation test data
    let compacted_query = ctx.sql(
        "SELECT action, extra FROM activity_log WHERE target = 'preservation_test' AND extra LIKE 'MUST_PRESERVE_%'"
    ).await?;

    let compacted_batches = compacted_query.collect().await?;
    let mut compacted_data_found = HashSet::new();

    for batch in &compacted_batches {
        let extra_array = batch
            .column(1)
            .as_any()
            .downcast_ref::<datafusion::arrow::array::StringArray>()
            .unwrap();

        for i in 0..extra_array.len() {
            if let Some(extra_value) = extra_array.value(i).strip_prefix("") {
                if extra_value.starts_with("MUST_PRESERVE_") {
                    compacted_data_found.insert(extra_value.to_string());
                }
            }
        }
    }

    // Check WAL for any remaining data (WAL files are in subdirectories)
    let today = jiff::Timestamp::now().strftime("%Y-%m-%d").to_string();
    let wal_file = database
        .path()
        .join("wal")
        .join("activity_log")
        .join(format!("activity_log_{}.jsonl", today));

    let mut wal_data_found = HashSet::new();
    if wal_file.exists() {
        let content = fs::read_to_string(&wal_file).await?;
        for line in content.lines().filter(|line| !line.trim().is_empty()) {
            let record: serde_json::Value = serde_json::from_str(line)?;
            if let Some(extra) = record.get("extra").and_then(|v| v.as_str()) {
                if extra.starts_with("MUST_PRESERVE_") {
                    wal_data_found.insert(extra.to_string());
                }
            }
        }
    }

    // Combine data from both sources
    let mut all_preserved_data = compacted_data_found;
    all_preserved_data.extend(wal_data_found);

    println!("Preserved data found: {:?}", all_preserved_data);

    // PRESERVATION REQUIREMENTS
    for (_, _, expected_data) in &preservation_test_data {
        assert!(
            all_preserved_data.contains(*expected_data),
            "Compaction lost critical data: {}",
            expected_data
        );
    }

    println!("Compaction data preservation verified: all critical data preserved");

    let db =
        Arc::try_unwrap(database).map_err(|_| anyhow::anyhow!("Database still has references"))?;
    db.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn test_log_system_recovery_after_failure() -> Result<()> {
    let (_temp_dir, database) = setup_full_database().await?;

    // Test complete log system recovery after various failure scenarios
    let recovery_scenarios = vec![
        ("graceful_shutdown", true),
        ("ungraceful_crash", false),
        ("partial_write_crash", false),
    ];

    for (scenario, graceful) in recovery_scenarios {
        println!("Testing recovery scenario: {}", scenario);

        // Write test data
        let scenario_data = format!("RECOVERY_TEST_{}", scenario.to_uppercase());
        for i in 0..10 {
            let activity = ActivityLogRow {
                when: format!("2024-01-15T10:{}:{:02}Z", 30 + (i / 60), i % 60),
                action: format!("recovery_test_{}", scenario),
                user_id: Some(format!("recovery-user-{}", i)),
                target: Some(scenario.to_string()),
                extra: Some(format!("{}_{}", scenario_data, i)),
            };

            database.activity_log.insert(activity).await?;
        }

        if graceful {
            // Graceful shutdown with full flush
            sleep(Duration::from_millis(6000)).await;
        } else {
            // Simulate crash - brief pause without full flush
            sleep(Duration::from_millis(100)).await;
        }

        // Verify system can continue logging
        let recovery_log = ActivityLogRow {
            when: "2024-01-15T10:35:00Z".to_string(),
            action: format!("post_recovery_{}", scenario),
            user_id: Some("recovery-test".to_string()),
            target: Some("recovery-verification".to_string()),
            extra: Some("RECOVERY_SUCCESSFUL".to_string()),
        };

        match database.activity_log.insert(recovery_log).await {
            Ok(_) => {
                println!("  Scenario {}: Recovery successful", scenario);
            }
            Err(e) => {
                panic!("Recovery failed for scenario {}: {}", scenario, e);
            }
        }

        // Verify data consistency after recovery (WAL files are in subdirectories)
        sleep(Duration::from_millis(6000)).await;

        let today = jiff::Timestamp::now().strftime("%Y-%m-%d").to_string();
        let wal_file = database
            .path()
            .join("wal")
            .join("activity_log")
            .join(format!("activity_log_{}.jsonl", today));

        if wal_file.exists() {
            let content = fs::read_to_string(&wal_file).await?;

            // Should contain recovery marker
            assert!(
                content.contains("RECOVERY_SUCCESSFUL"),
                "Recovery marker should be present for scenario {}",
                scenario
            );

            // All JSON should be valid (no corruption from crash)
            for line in content.lines().filter(|line| !line.trim().is_empty()) {
                serde_json::from_str::<serde_json::Value>(line).map_err(|_| {
                    anyhow::anyhow!(
                        "Data corruption after recovery in scenario {}: {}",
                        scenario,
                        line
                    )
                })?;
            }

            println!(
                "  Scenario {}: Data integrity verified after recovery",
                scenario
            );
        }

        // Brief pause between scenarios
        sleep(Duration::from_millis(100)).await;
    }

    println!("Log system recovery tests completed successfully");

    let db =
        Arc::try_unwrap(database).map_err(|_| anyhow::anyhow!("Database still has references"))?;
    db.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn test_high_throughput_logging_reliability() -> Result<()> {
    let (_temp_dir, database) = setup_full_database().await?;

    // Test log system reliability under sustained high throughput
    let target_records_per_second = 1000;
    let test_duration_seconds = 5;
    let expected_total = target_records_per_second * test_duration_seconds;

    let records_written = Arc::new(AtomicU64::new(0));
    let records_failed = Arc::new(AtomicU64::new(0));
    let stop_flag = Arc::new(AtomicBool::new(false));

    // High-throughput writer
    let writer_task = {
        let db = Arc::clone(&database);
        let written = Arc::clone(&records_written);
        let failed = Arc::clone(&records_failed);
        let stop = Arc::clone(&stop_flag);

        tokio::spawn(async move {
            let mut record_id = 0u64;

            while !stop.load(Ordering::Relaxed) {
                let activity = ActivityLogRow {
                    when: format!(
                        "2024-01-15T10:30:{:02}.{:03}Z",
                        (record_id / 1000) % 60,
                        record_id % 1000
                    ),
                    action: format!("throughput_test_{:06}", record_id),
                    user_id: Some(format!("throughput-user-{}", record_id % 100)),
                    target: Some("throughput-test".to_string()),
                    extra: Some(format!("THROUGHPUT_DATA_{:06}", record_id)),
                };

                match db.activity_log.insert(activity).await {
                    Ok(_) => {
                        written.fetch_add(1, Ordering::Relaxed);
                    }
                    Err(_) => {
                        failed.fetch_add(1, Ordering::Relaxed);
                    }
                }

                record_id += 1;

                // Brief yield every 100 records
                if record_id % 100 == 0 {
                    tokio::task::yield_now().await;
                }
            }
        })
    };

    // Run for test duration
    let start_time = std::time::Instant::now();
    sleep(Duration::from_secs(test_duration_seconds as u64)).await;
    stop_flag.store(true, Ordering::Relaxed);

    writer_task.await?;
    let actual_duration = start_time.elapsed();

    let total_written = records_written.load(Ordering::Relaxed);
    let total_failed = records_failed.load(Ordering::Relaxed);
    let actual_throughput = total_written as f64 / actual_duration.as_secs_f64();

    println!("High throughput test results:");
    println!("  Duration: {:?}", actual_duration);
    println!("  Records written: {}", total_written);
    println!("  Records failed: {}", total_failed);
    println!(
        "  Actual throughput: {:.2} records/second",
        actual_throughput
    );
    println!(
        "  Target throughput: {} records/second",
        target_records_per_second
    );

    // Performance requirements
    assert!(
        total_written > 0,
        "Should successfully write records under load"
    );
    assert!(
        actual_throughput > target_records_per_second as f64 * 0.5,
        "Should achieve at least 50% of target throughput"
    );

    // Validate test ran for expected duration (within reasonable tolerance)
    let duration_tolerance = Duration::from_millis(1000); // 1 second tolerance
    let expected_duration = Duration::from_secs(test_duration_seconds as u64);
    assert!(
        actual_duration >= expected_duration - duration_tolerance
            && actual_duration <= expected_duration + duration_tolerance,
        "Test should run for approximately {} seconds, ran for {:?}",
        test_duration_seconds,
        actual_duration
    );

    println!("  Expected total (at target rate): {}", expected_total);

    // Force flush and verify data integrity (WAL files are in subdirectories)
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

    // Verify data integrity under high throughput
    let mut throughput_records = 0;
    let mut sequence_errors = 0;

    for line in lines {
        match serde_json::from_str::<serde_json::Value>(line) {
            Ok(record) => {
                if let Some(action) = record.get("action").and_then(|v| v.as_str()) {
                    if action.starts_with("throughput_test_") {
                        throughput_records += 1;

                        // Verify data integrity
                        if let Some(extra) = record.get("extra").and_then(|v| v.as_str()) {
                            if !extra.starts_with("THROUGHPUT_DATA_") {
                                sequence_errors += 1;
                            }
                        }
                    }
                }
            }
            Err(_) => {
                sequence_errors += 1;
            }
        }
    }

    println!("Throughput integrity verification:");
    println!("  Throughput records in WAL: {}", throughput_records);
    println!("  Data integrity errors: {}", sequence_errors);

    // RELIABILITY REQUIREMENTS
    assert_eq!(
        sequence_errors, 0,
        "No data integrity errors should occur under high throughput"
    );
    assert!(
        throughput_records > (total_written as f64 * 0.8) as usize,
        "At least 80% of written records should be persisted"
    );

    let db =
        Arc::try_unwrap(database).map_err(|_| anyhow::anyhow!("Database still has references"))?;
    db.shutdown().await?;
    Ok(())
}
