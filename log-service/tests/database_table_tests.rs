use anyhow::Result;
use datafusion::arrow::datatypes::{DataType, Field, Schema};
use log_service::database::table::{Table, TableRow};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tempfile::TempDir;
use tokio::fs;
use tokio::time::{Duration, sleep};

/// Test helper to create a temporary directory and return its path
async fn setup_temp_db_path() -> Result<(TempDir, PathBuf)> {
    let temp_dir = TempDir::new()?;
    let db_path = temp_dir.path().to_path_buf();
    Ok((temp_dir, db_path))
}

/// Test row type for testing TableRow trait
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct TestRow {
    id: String,
    name: String,
    value: i32,
    active: bool,
}

impl TableRow for TestRow {
    const TABLE_NAME: &'static str = "test_table";

    fn schema() -> Schema {
        Schema::new(vec![
            Field::new("id", DataType::Utf8, false),
            Field::new("name", DataType::Utf8, false),
            Field::new("value", DataType::Int32, false),
            Field::new("active", DataType::Boolean, false),
        ])
    }
}

/// Another test row type for testing multiple table types
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct SimpleRow {
    message: String,
    timestamp: String,
}

impl TableRow for SimpleRow {
    const TABLE_NAME: &'static str = "simple_table";

    fn schema() -> Schema {
        Schema::new(vec![
            Field::new("message", DataType::Utf8, false),
            Field::new("timestamp", DataType::Utf8, false),
        ])
    }
}

#[tokio::test]
async fn test_table_creation() -> Result<()> {
    let (_temp_dir, db_path) = setup_temp_db_path().await?;

    // Create a table
    let table = Table::<TestRow>::new(&db_path).await?;

    // Verify table properties
    assert_eq!(table.name(), "test_table");

    // Verify schema
    let schema = table.schema();
    assert_eq!(schema.fields().len(), 4);
    assert_eq!(schema.field(0).name(), "id");
    assert_eq!(schema.field(1).name(), "name");
    assert_eq!(schema.field(2).name(), "value");
    assert_eq!(schema.field(3).name(), "active");

    // Gracefully shutdown
    table.shutdown().await?;

    Ok(())
}

#[tokio::test]
async fn test_table_insert_single_row() -> Result<()> {
    let (_temp_dir, db_path) = setup_temp_db_path().await?;

    let table = Table::<TestRow>::new(&db_path).await?;

    // Insert a test row
    let test_row = TestRow {
        id: "test-1".to_string(),
        name: "Test Item".to_string(),
        value: 42,
        active: true,
    };

    table.insert(test_row.clone()).await?;

    // Allow some time for the writer to process
    sleep(Duration::from_millis(100)).await;

    table.shutdown().await?;

    // Check that WAL file was created and contains the data
    let today = jiff::Timestamp::now().strftime("%Y-%m-%d").to_string();
    let wal_file = db_path
        .join("wal")
        .join("test_table")
        .join(format!("test_table_{}.jsonl", today));

    assert!(wal_file.exists(), "WAL file should be created");

    let content = fs::read_to_string(&wal_file).await?;
    assert!(content.contains("test-1"));
    assert!(content.contains("Test Item"));
    assert!(content.contains("42"));

    Ok(())
}

#[tokio::test]
async fn test_table_insert_multiple_rows() -> Result<()> {
    let (_temp_dir, db_path) = setup_temp_db_path().await?;
    let table = Table::<TestRow>::new(&db_path).await?;

    // Insert multiple test rows
    let test_rows = vec![
        TestRow {
            id: "test-1".to_string(),
            name: "First Item".to_string(),
            value: 10,
            active: true,
        },
        TestRow {
            id: "test-2".to_string(),
            name: "Second Item".to_string(),
            value: 20,
            active: false,
        },
        TestRow {
            id: "test-3".to_string(),
            name: "Third Item".to_string(),
            value: 30,
            active: true,
        },
    ];

    for row in &test_rows {
        table.insert(row.clone()).await?;
    }

    // Allow time for writes
    sleep(Duration::from_millis(100)).await;

    table.shutdown().await?;

    // Verify all rows were written
    let today = jiff::Timestamp::now().strftime("%Y-%m-%d").to_string();
    let wal_file = db_path
        .join("wal")
        .join("test_table")
        .join(format!("test_table_{}.jsonl", today));

    let content = fs::read_to_string(&wal_file).await?;
    let lines: Vec<&str> = content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    assert_eq!(lines.len(), 3, "Should have 3 JSON lines");

    // Verify each row's data is present
    assert!(content.contains("First Item"));
    assert!(content.contains("Second Item"));
    assert!(content.contains("Third Item"));

    Ok(())
}

#[tokio::test]
async fn test_table_json_serialization() -> Result<()> {
    let (_temp_dir, db_path) = setup_temp_db_path().await?;
    let table = Table::<TestRow>::new(&db_path).await?;

    let test_row = TestRow {
        id: "json-test".to_string(),
        name: "JSON Test Item".to_string(),
        value: 123,
        active: false,
    };

    table.insert(test_row.clone()).await?;
    sleep(Duration::from_millis(100)).await;
    table.shutdown().await?;

    // Read and parse the JSON
    let today = jiff::Timestamp::now().strftime("%Y-%m-%d").to_string();
    let wal_file = db_path
        .join("wal")
        .join("test_table")
        .join(format!("test_table_{}.jsonl", today));

    let content = fs::read_to_string(&wal_file).await?;
    let line = content.lines().next().unwrap();

    // Verify it's valid JSON that can be deserialized back
    let parsed_row: TestRow = serde_json::from_str(line)?;
    assert_eq!(parsed_row, test_row);

    Ok(())
}

#[tokio::test]
async fn test_table_wal_directory_creation() -> Result<()> {
    let (_temp_dir, db_path) = setup_temp_db_path().await?;

    // WAL directory should not exist initially
    let wal_dir = db_path.join("wal");
    assert!(!wal_dir.exists());

    let table = Table::<TestRow>::new(&db_path).await?;

    let test_row = TestRow {
        id: "dir-test".to_string(),
        name: "Directory Test".to_string(),
        value: 1,
        active: true,
    };

    table.insert(test_row).await?;
    sleep(Duration::from_millis(100)).await;

    // WAL directory should now exist
    assert!(wal_dir.exists());
    assert!(wal_dir.is_dir());

    table.shutdown().await?;

    Ok(())
}

#[tokio::test]
async fn test_table_daily_rotation() -> Result<()> {
    let (_temp_dir, db_path) = setup_temp_db_path().await?;
    let table = Table::<TestRow>::new(&db_path).await?;

    // Insert a row
    let test_row = TestRow {
        id: "rotation-test".to_string(),
        name: "Rotation Test".to_string(),
        value: 1,
        active: true,
    };

    table.insert(test_row).await?;
    sleep(Duration::from_millis(100)).await;
    table.shutdown().await?;

    // Verify the file was created with today's date
    let today = jiff::Timestamp::now().strftime("%Y-%m-%d").to_string();
    let expected_file = db_path
        .join("wal")
        .join("test_table")
        .join(format!("test_table_{}.jsonl", today));

    assert!(expected_file.exists());

    // Note: Testing actual date rotation would require mocking time,
    // which is complex. The daily rotation logic is tested by ensuring
    // the file naming includes the date.

    Ok(())
}

#[tokio::test]
async fn test_table_buffered_writes() -> Result<()> {
    let (_temp_dir, db_path) = setup_temp_db_path().await?;
    let table = Table::<TestRow>::new(&db_path).await?;

    // Insert many rows quickly
    for i in 0..100 {
        let test_row = TestRow {
            id: format!("buffer-test-{}", i),
            name: format!("Buffered Item {}", i),
            value: i,
            active: i % 2 == 0,
        };
        table.insert(test_row).await?;
    }

    // Allow time for buffered writes to complete
    sleep(Duration::from_millis(200)).await;
    table.shutdown().await?;

    // Verify all rows were written
    let today = jiff::Timestamp::now().strftime("%Y-%m-%d").to_string();
    let wal_file = db_path
        .join("wal")
        .join("test_table")
        .join(format!("test_table_{}.jsonl", today));

    let content = fs::read_to_string(&wal_file).await?;
    let lines: Vec<&str> = content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    assert_eq!(lines.len(), 100, "Should have 100 JSON lines");

    Ok(())
}

#[tokio::test]
async fn test_table_periodic_flush() -> Result<()> {
    let (_temp_dir, db_path) = setup_temp_db_path().await?;
    let table = Table::<TestRow>::new(&db_path).await?;

    let test_row = TestRow {
        id: "flush-test".to_string(),
        name: "Flush Test".to_string(),
        value: 1,
        active: true,
    };

    table.insert(test_row).await?;

    // Wait longer than the flush interval (5 seconds + buffer)
    sleep(Duration::from_millis(6000)).await;

    // Data should be flushed to disk even before shutdown
    let today = jiff::Timestamp::now().strftime("%Y-%m-%d").to_string();
    let wal_file = db_path
        .join("wal")
        .join("test_table")
        .join(format!("test_table_{}.jsonl", today));

    assert!(wal_file.exists());
    let content = fs::read_to_string(&wal_file).await?;
    assert!(content.contains("flush-test"));

    table.shutdown().await?;

    Ok(())
}

#[tokio::test]
async fn test_multiple_table_types() -> Result<()> {
    let (_temp_dir, db_path) = setup_temp_db_path().await?;

    // Create tables of different types
    let test_table = Table::<TestRow>::new(&db_path).await?;
    let simple_table = Table::<SimpleRow>::new(&db_path).await?;

    // Insert data into both tables
    let test_row = TestRow {
        id: "multi-test".to_string(),
        name: "Multi Test".to_string(),
        value: 42,
        active: true,
    };

    let simple_row = SimpleRow {
        message: "Hello World".to_string(),
        timestamp: "2024-01-15T10:30:45Z".to_string(),
    };

    test_table.insert(test_row).await?;
    simple_table.insert(simple_row).await?;

    sleep(Duration::from_millis(100)).await;

    // Verify both tables created separate WAL files
    let today = jiff::Timestamp::now().strftime("%Y-%m-%d").to_string();
    let test_wal = db_path
        .join("wal")
        .join("test_table")
        .join(format!("test_table_{}.jsonl", today));
    let simple_wal = db_path
        .join("wal")
        .join("simple_table")
        .join(format!("simple_table_{}.jsonl", today));

    assert!(test_wal.exists());
    assert!(simple_wal.exists());

    // Verify content separation
    let test_content = fs::read_to_string(&test_wal).await?;
    let simple_content = fs::read_to_string(&simple_wal).await?;

    assert!(test_content.contains("Multi Test"));
    assert!(!test_content.contains("Hello World"));

    assert!(simple_content.contains("Hello World"));
    assert!(!simple_content.contains("Multi Test"));

    test_table.shutdown().await?;
    simple_table.shutdown().await?;

    Ok(())
}

#[tokio::test]
async fn test_table_shutdown_graceful() -> Result<()> {
    let (_temp_dir, db_path) = setup_temp_db_path().await?;
    let table = Table::<TestRow>::new(&db_path).await?;

    // Insert some data
    for i in 0..5 {
        let test_row = TestRow {
            id: format!("shutdown-test-{}", i),
            name: format!("Shutdown Test {}", i),
            value: i,
            active: true,
        };
        table.insert(test_row).await?;
    }

    // Shutdown should flush remaining data
    table.shutdown().await?;

    // Verify all data was written
    let today = jiff::Timestamp::now().strftime("%Y-%m-%d").to_string();
    let wal_file = db_path
        .join("wal")
        .join("test_table")
        .join(format!("test_table_{}.jsonl", today));

    let content = fs::read_to_string(&wal_file).await?;
    let lines: Vec<&str> = content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    assert_eq!(lines.len(), 5);

    Ok(())
}

#[tokio::test]
async fn test_table_channel_closed_handling() -> Result<()> {
    let (_temp_dir, db_path) = setup_temp_db_path().await?;
    let table = Table::<TestRow>::new(&db_path).await?;

    // Insert a row to ensure the writer is working
    let test_row = TestRow {
        id: "channel-test".to_string(),
        name: "Channel Test".to_string(),
        value: 1,
        active: true,
    };

    table.insert(test_row).await?;

    // Shutdown should close the channel and exit gracefully
    table.shutdown().await?;

    // Any subsequent attempts to use the table would fail,
    // but the shutdown itself should have succeeded

    Ok(())
}

#[tokio::test]
async fn test_table_concurrent_inserts() -> Result<()> {
    let (_temp_dir, db_path) = setup_temp_db_path().await?;

    let table = Arc::new(Table::<TestRow>::new(&db_path).await?);

    // Spawn multiple concurrent insert tasks
    let handles = (0..50)
        .map(|i| {
            let table_clone = Arc::clone(&table);
            tokio::spawn(async move {
                let test_row = TestRow {
                    id: format!("concurrent-{}", i),
                    name: format!("Concurrent Item {}", i),
                    value: i,
                    active: i % 2 == 0,
                };
                table_clone.insert(test_row).await.unwrap();
            })
        })
        .collect::<Vec<_>>();

    // Wait for all inserts to complete
    for handle in handles {
        handle.await?;
    }

    sleep(Duration::from_millis(200)).await;

    // Shutdown the table
    match Arc::try_unwrap(table) {
        Ok(table) => table.shutdown().await?,
        Err(_) => panic!("Could not unwrap Arc - other references still exist"),
    }

    // Verify all rows were written
    let today = jiff::Timestamp::now().strftime("%Y-%m-%d").to_string();
    let wal_file = db_path
        .join("wal")
        .join("test_table")
        .join(format!("test_table_{}.jsonl", today));

    let content = fs::read_to_string(&wal_file).await?;
    let lines: Vec<&str> = content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    assert_eq!(lines.len(), 50);

    Ok(())
}

#[tokio::test]
async fn test_table_special_characters() -> Result<()> {
    let (_temp_dir, db_path) = setup_temp_db_path().await?;
    let table = Table::<TestRow>::new(&db_path).await?;

    // Test with special characters
    let test_row = TestRow {
        id: "special-chars-test".to_string(),
        name: "Special chars: 🦀 \"quotes\" \n newlines \t tabs".to_string(),
        value: -42,
        active: false,
    };

    table.insert(test_row.clone()).await?;
    sleep(Duration::from_millis(100)).await;
    table.shutdown().await?;

    // Verify the data was serialized and can be deserialized correctly
    let today = jiff::Timestamp::now().strftime("%Y-%m-%d").to_string();
    let wal_file = db_path
        .join("wal")
        .join("test_table")
        .join(format!("test_table_{}.jsonl", today));

    let content = fs::read_to_string(&wal_file).await?;
    let line = content.lines().next().unwrap();

    let parsed_row: TestRow = serde_json::from_str(line)?;
    assert_eq!(parsed_row, test_row);

    Ok(())
}

#[tokio::test]
async fn test_table_schema_consistency() -> Result<()> {
    // Test that the schema returned by the table matches the TableRow implementation
    let (_temp_dir, db_path) = setup_temp_db_path().await?;
    let table = Table::<TestRow>::new(&db_path).await?;

    let table_schema = table.schema();
    let row_schema = TestRow::schema();

    // Schemas should be identical
    assert_eq!(table_schema.fields().len(), row_schema.fields().len());

    for (table_field, row_field) in table_schema.fields().iter().zip(row_schema.fields().iter()) {
        assert_eq!(table_field.name(), row_field.name());
        assert_eq!(table_field.data_type(), row_field.data_type());
        assert_eq!(table_field.is_nullable(), row_field.is_nullable());
    }

    table.shutdown().await?;

    Ok(())
}
