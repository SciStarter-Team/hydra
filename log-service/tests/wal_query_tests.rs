use anyhow::Result;
use log_service::database::{storage::Database, tables::ActivityLogRow};
use std::sync::Arc;
use tempfile::TempDir;
use tokio::time::{Duration, sleep};

#[tokio::test]
async fn test_wal_data_queryable() -> Result<()> {
    // Create a temporary database
    let temp_dir = TempDir::new()?;
    let db_path = temp_dir.path().to_string_lossy().to_string();
    let database = Arc::new(Database::new(db_path).await?);

    // Insert some test data (this goes to WAL)
    let activity = ActivityLogRow {
        when: "2024-01-15T10:30:45Z".to_string(),
        action: "test_wal_visibility".to_string(),
        user_id: Some("test_user".to_string()),
        target: Some("wal_test".to_string()),
        extra: Some("This data is only in WAL".to_string()),
    };

    database.activity_log.insert(activity).await?;

    // Give it a moment for the write to complete
    sleep(Duration::from_millis(100)).await;

    println!("Data inserted into WAL. Now querying...");

    // Try to query the data immediately from the realtime view (should include WAL data)
    let ctx = database.context();
    let query_result = ctx.sql("SELECT COUNT(*) as count FROM activity_log_realtime WHERE action = 'test_wal_visibility'").await?;
    let batches = query_result.collect().await?;

    // Check if any data was returned
    let count = if !batches.is_empty() && batches[0].num_rows() > 0 {
        batches[0]
            .column(0)
            .as_any()
            .downcast_ref::<datafusion::arrow::array::Int64Array>()
            .unwrap()
            .value(0)
    } else {
        0
    };

    // The test should FAIL if WAL data is not queryable
    assert!(
        count > 0,
        "WAL data should be queryable immediately after insertion. \
         Found {} records, expected at least 1. \
         This indicates that queries only see Parquet data, not fresh WAL data.",
        count
    );

    // If we get here, WAL data IS queryable - verify the content
    let detail_query = ctx
        .sql("SELECT action, extra FROM activity_log_realtime WHERE action = 'test_wal_visibility'")
        .await?;
    let detail_batches = detail_query.collect().await?;

    assert!(
        !detail_batches.is_empty() && detail_batches[0].num_rows() > 0,
        "Should be able to query the specific WAL record"
    );

    let action_array = detail_batches[0]
        .column(0)
        .as_any()
        .downcast_ref::<datafusion::arrow::array::StringArray>()
        .unwrap();
    let extra_array = detail_batches[0]
        .column(1)
        .as_any()
        .downcast_ref::<datafusion::arrow::array::StringArray>()
        .unwrap();

    assert_eq!(action_array.value(0), "test_wal_visibility");
    assert_eq!(extra_array.value(0), "This data is only in WAL");

    // Clean shutdown
    let db =
        Arc::try_unwrap(database).map_err(|_| anyhow::anyhow!("Database still has references"))?;
    db.shutdown().await?;

    Ok(())
}
