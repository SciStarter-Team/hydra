use anyhow::Result;
use log_service::database::{compactor::Compactor, storage::Database, tables::ActivityLogRow};
use std::sync::Arc;
use tempfile::TempDir;
use tokio::time::{Duration, sleep};
use tokio_util::sync::CancellationToken;

/// Test that verifies correct WAL compaction behavior:
/// - Yesterday's WAL files SHOULD be compacted
/// - Today's WAL files should NOT be compacted (prevents data loss bug)
#[tokio::test]
async fn test_compaction_respects_date_boundaries() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let db_path = temp_dir.path().to_string_lossy().to_string();
    let database = Arc::new(Database::new(db_path).await?);

    let today = jiff::Timestamp::now().strftime("%Y-%m-%d").to_string();
    let yesterday = "2024-01-14".to_string(); // Static date for testing

    // === SETUP: Create artificial yesterday's WAL file ===
    // WAL files are stored in subdirectories: wal/{table_name}/{table_name}_{date}.jsonl
    let wal_dir = database.path().join("wal").join("activity_log");
    tokio::fs::create_dir_all(&wal_dir).await?;

    let yesterday_wal = wal_dir.join(format!("activity_log_{}.jsonl", yesterday));
    let yesterday_data = r#"{"when":"2024-01-14T10:00:00Z","action":"old_data","user_id":"user_old","target":"test","extra":"This is yesterday's data"}
"#;
    tokio::fs::write(&yesterday_wal, yesterday_data).await?;

    // === Write today's data ===
    let today_data = ActivityLogRow {
        when: "2024-01-15T15:30:00Z".to_string(),
        action: "new_data".to_string(),
        user_id: Some("user_new".to_string()),
        target: Some("test".to_string()),
        extra: Some("This is today's data".to_string()),
    };

    database.activity_log.insert(today_data).await?;
    sleep(Duration::from_millis(100)).await;

    let today_wal = wal_dir.join(format!("activity_log_{}.jsonl", today));
    assert!(today_wal.exists(), "Today's WAL file should exist at {:?}", today_wal);
    assert!(yesterday_wal.exists(), "Yesterday's WAL file should exist at {:?}", yesterday_wal);

    // === Run compactor ===
    let compactor = Compactor::with_config(database.clone(), Duration::from_millis(50), 30, 10_000);
    let cancel_token = CancellationToken::new();

    let compactor_task = {
        let token = cancel_token.clone();
        tokio::spawn(async move { compactor.run(token).await })
    };

    sleep(Duration::from_millis(200)).await;
    cancel_token.cancel();
    let _ = compactor_task.await;

    // === VERIFICATION: Yesterday's file should be compacted ===
    let processed_dir = database.path().join("processed");
    let yesterday_processed = processed_dir.join(format!("activity_log_{}.jsonl", yesterday));
    assert!(
        yesterday_processed.exists(),
        "Yesterday's WAL should be moved to processed/ after compaction"
    );
    assert!(
        !yesterday_wal.exists(),
        "Yesterday's WAL should no longer be in wal/ directory"
    );

    let parquet_dir = database.path().join("parquet").join("activity_log");
    let yesterday_parquet = parquet_dir.join(format!("activity_log_{}.parquet", yesterday));
    assert!(
        yesterday_parquet.exists(),
        "Yesterday's parquet file should be created"
    );

    // === VERIFICATION: Today's file should NOT be compacted ===
    let today_processed = processed_dir.join(format!("activity_log_{}.jsonl", today));
    assert!(
        !today_processed.exists(),
        "Today's WAL should NOT be moved to processed/"
    );
    assert!(
        today_wal.exists(),
        "Today's WAL should remain in wal/ directory"
    );

    let today_parquet = parquet_dir.join(format!("activity_log_{}.parquet", today));
    assert!(
        !today_parquet.exists(),
        "Today's parquet should NOT be created yet"
    );

    // === VERIFICATION: Data queries work correctly ===
    let ctx = database.context();

    // Yesterday's data should be in parquet table
    let yesterday_query = ctx
        .sql("SELECT COUNT(*) as count FROM activity_log WHERE action = 'old_data'")
        .await?;
    let yesterday_batches = yesterday_query.collect().await?;
    let yesterday_count = if !yesterday_batches.is_empty() && yesterday_batches[0].num_rows() > 0 {
        yesterday_batches[0]
            .column(0)
            .as_any()
            .downcast_ref::<datafusion::arrow::array::Int64Array>()
            .unwrap()
            .value(0)
    } else {
        0
    };

    // Today's data should NOT be in parquet table yet
    let today_parquet_query = ctx
        .sql("SELECT COUNT(*) as count FROM activity_log WHERE action = 'new_data'")
        .await?;
    let today_parquet_batches = today_parquet_query.collect().await?;
    let today_parquet_count =
        if !today_parquet_batches.is_empty() && today_parquet_batches[0].num_rows() > 0 {
            today_parquet_batches[0]
                .column(0)
                .as_any()
                .downcast_ref::<datafusion::arrow::array::Int64Array>()
                .unwrap()
                .value(0)
        } else {
            0
        };

    // Today's data should be in realtime view
    let today_realtime_query = ctx
        .sql("SELECT COUNT(*) as count FROM activity_log_realtime WHERE action = 'new_data'")
        .await?;
    let today_realtime_batches = today_realtime_query.collect().await?;
    let today_realtime_count =
        if !today_realtime_batches.is_empty() && today_realtime_batches[0].num_rows() > 0 {
            today_realtime_batches[0]
                .column(0)
                .as_any()
                .downcast_ref::<datafusion::arrow::array::Int64Array>()
                .unwrap()
                .value(0)
        } else {
            0
        };

    // These assertions will fail if the bug reappears:
    assert_eq!(
        yesterday_count, 1,
        "Yesterday's data should be compacted into parquet table"
    );
    assert_eq!(
        today_parquet_count, 0,
        "Today's data should NOT be in parquet yet"
    );
    assert_eq!(
        today_realtime_count, 1,
        "Today's data should be accessible via realtime view"
    );

    // Clean shutdown
    let db =
        Arc::try_unwrap(database).map_err(|_| anyhow::anyhow!("Database still has references"))?;
    db.shutdown().await?;

    Ok(())
}
