use anyhow::Result;
use log_service::database::{
    storage::Database,
    tables::{ClickthroughRow, PageViewRow},
};
use std::sync::Arc;
use tempfile::TempDir;
use tokio::time::{sleep, Duration};

/// Helper to create a temporary database instance
async fn setup_database() -> Result<(TempDir, Arc<Database>)> {
    let temp_dir = TempDir::new()?;
    let db_path = temp_dir.path().to_string_lossy().to_string();
    let database = Arc::new(Database::new(db_path).await?);
    Ok((temp_dir, database))
}

/// Test that WAL tables are properly isolated and don't read each other's data
#[tokio::test]
async fn test_wal_table_isolation() -> Result<()> {
    let (_temp_dir, database) = setup_database().await?;
    
    // Insert test data into clickthroughs table
    let clickthrough_data = ClickthroughRow {
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
    };
    
    // Insert test data into page_views table  
    let page_view_data = PageViewRow {
        when: "2024-01-15T10:35:00Z".to_string(),
        ip_address: "192.168.1.1".to_string(),
        session_id: "session-456".to_string(),
        interaction_id: "interaction-456".to_string(),
        user_id: Some("user-789".to_string()),
        path: "/dashboard".to_string(),
        referrer: Some("https://example.com".to_string()),
        query: Some("param=value".to_string()),
        geolocation: "POINT(-122.4194 37.7749)".to_string(),
    };
    
    // Insert data into both tables
    database.clickthroughs.insert(clickthrough_data.clone()).await?;
    database.page_views.insert(page_view_data.clone()).await?;
    
    // Allow time for WAL writes to complete
    sleep(Duration::from_millis(200)).await;
    
    // Test that clickthroughs_wal only contains clickthrough data
    let ctx = database.context();
    let clickthrough_result = ctx
        .sql("SELECT * FROM clickthroughs_wal")
        .await?
        .collect()
        .await?;
        
    // Should have exactly 1 record with clickthrough data
    assert_eq!(clickthrough_result.len(), 1, "Expected exactly 1 batch from clickthroughs_wal");
    let clickthrough_batch = &clickthrough_result[0];
    assert_eq!(clickthrough_batch.num_rows(), 1, "Expected exactly 1 row in clickthroughs_wal");
    
    // Verify it contains the expected clickthrough data
    let ip_column = clickthrough_batch.column_by_name("ip_address").unwrap();
    let ip_array = ip_column.as_any().downcast_ref::<arrow::array::StringArray>().unwrap();
    assert_eq!(ip_array.value(0), "10.0.0.1", "Clickthroughs WAL should contain clickthrough IP");
    
    let session_column = clickthrough_batch.column_by_name("session_id").unwrap();
    let session_array = session_column.as_any().downcast_ref::<arrow::array::StringArray>().unwrap();
    assert_eq!(session_array.value(0), "session-123", "Clickthroughs WAL should contain clickthrough session");
    
    // Test that page_views_wal only contains page view data
    let page_view_result = ctx
        .sql("SELECT * FROM page_views_wal")
        .await?
        .collect()
        .await?;
        
    // Should have exactly 1 record with page view data
    assert_eq!(page_view_result.len(), 1, "Expected exactly 1 batch from page_views_wal");
    let page_view_batch = &page_view_result[0];
    assert_eq!(page_view_batch.num_rows(), 1, "Expected exactly 1 row in page_views_wal");
    
    // Verify it contains the expected page view data
    let ip_column = page_view_batch.column_by_name("ip_address").unwrap();
    let ip_array = ip_column.as_any().downcast_ref::<arrow::array::StringArray>().unwrap();
    assert_eq!(ip_array.value(0), "192.168.1.1", "Page views WAL should contain page view IP");
    
    let session_column = page_view_batch.column_by_name("session_id").unwrap();
    let session_array = session_column.as_any().downcast_ref::<arrow::array::StringArray>().unwrap();
    assert_eq!(session_array.value(0), "session-456", "Page views WAL should contain page view session");
    
    let path_column = page_view_batch.column_by_name("path").unwrap();
    let path_array = path_column.as_any().downcast_ref::<arrow::array::StringArray>().unwrap();
    assert_eq!(path_array.value(0), "/dashboard", "Page views WAL should contain page view path");
    
    println!("✓ WAL table isolation test passed - no data intermixing detected");
    
    Ok(())
}

/// Test that realtime tables properly union parquet and WAL data without cross-contamination
#[tokio::test]
async fn test_realtime_table_isolation() -> Result<()> {
    let (_temp_dir, database) = setup_database().await?;
    
    // Insert different data into each table
    let clickthrough_data = ClickthroughRow {
        when: "2024-01-15T11:00:00Z".to_string(),
        ip_address: "10.1.1.1".to_string(),
        session_id: "click-session".to_string(),
        interaction_id: "click-interaction".to_string(),
        user_id: Some("click-user".to_string()),
        from_path: "/click-from".to_string(),
        to_url: "https://click-to.com".to_string(),
        link_type: "click-type".to_string(),
        program_id: Some("click-program".to_string()),
        geolocation: "POINT(1 1)".to_string(),
    };
    
    let page_view_data = PageViewRow {
        when: "2024-01-15T11:05:00Z".to_string(),
        ip_address: "10.2.2.2".to_string(),
        session_id: "page-session".to_string(),
        interaction_id: "page-interaction".to_string(),
        user_id: Some("page-user".to_string()),
        path: "/page-path".to_string(),
        referrer: Some("https://page-referrer.com".to_string()),
        query: Some("page=query".to_string()),
        geolocation: "POINT(2 2)".to_string(),
    };
    
    database.clickthroughs.insert(clickthrough_data).await?;
    database.page_views.insert(page_view_data).await?;
    
    // Allow time for WAL writes
    sleep(Duration::from_millis(200)).await;
    
    let ctx = database.context();
    
    // Test clickthroughs_realtime contains only clickthrough data
    let click_realtime = ctx
        .sql("SELECT ip_address, session_id, from_path FROM clickthroughs_realtime")
        .await?
        .collect()
        .await?;
        
    assert_eq!(click_realtime.len(), 1);
    assert_eq!(click_realtime[0].num_rows(), 1);
    
    let ip_array = click_realtime[0].column_by_name("ip_address").unwrap()
        .as_any().downcast_ref::<arrow::array::StringArray>().unwrap();
    assert_eq!(ip_array.value(0), "10.1.1.1");
    
    // Test page_views_realtime contains only page view data
    let page_realtime = ctx
        .sql("SELECT ip_address, session_id, path FROM page_views_realtime")
        .await?
        .collect()
        .await?;
        
    assert_eq!(page_realtime.len(), 1);
    assert_eq!(page_realtime[0].num_rows(), 1);
    
    let ip_array = page_realtime[0].column_by_name("ip_address").unwrap()
        .as_any().downcast_ref::<arrow::array::StringArray>().unwrap();
    assert_eq!(ip_array.value(0), "10.2.2.2");
    
    let path_array = page_realtime[0].column_by_name("path").unwrap()
        .as_any().downcast_ref::<arrow::array::StringArray>().unwrap();
    assert_eq!(path_array.value(0), "/page-path");
    
    println!("✓ Realtime table isolation test passed - union queries correctly isolated");
    
    Ok(())
}