use anyhow::Result;
use async_nats::service::ServiceExt;
use common::shared;
use futures::StreamExt;
use tokio::{sync::mpsc, task::JoinSet, spawn};

mod analytics;
mod cache;
mod database;
mod handlers;
mod sync;

use database::{init_database, set_database_path};
use handlers::{
    database_writer, handle_activity_log, handle_clickthrough, handle_clickthrough_analytics, 
    handle_join, handle_join_analytics, handle_page_view, handle_user_tags_update, LogEntry
};
use sync::{affiliate_synchronize, bookmark_synchronize, join_synchronize};

#[tokio::main]
async fn main() -> Result<()> {
    shared::init().await;

    let database_path = std::env::var("DATABASE_URL")
        .expect("DATABASE_URL is not set");
    
    set_database_path(database_path.clone())?;
    
    init_database(&database_path)?;

    let (tx, rx) = mpsc::channel::<LogEntry>(1000);
    
    let mut tasks = JoinSet::new();

    // Spawn database writer task
    tasks.spawn(database_writer(rx));

    // Spawn affiliate sync task
    let affiliate_sync_task = spawn(affiliate_synchronize());
    tasks.spawn(async move {
        affiliate_sync_task.await.unwrap_or_else(|e| eprintln!("Affiliate sync task panicked: {}", e));
        Ok::<(), anyhow::Error>(())
    });

    // Spawn bookmark sync task
    let bookmark_sync_task = spawn(bookmark_synchronize());
    tasks.spawn(async move {
        bookmark_sync_task.await.unwrap_or_else(|e| eprintln!("Bookmark sync task panicked: {}", e));
        Ok::<(), anyhow::Error>(())
    });

    // Spawn join sync task
    let join_sync_task = spawn(join_synchronize());
    tasks.spawn(async move {
        join_sync_task.await.unwrap_or_else(|e| eprintln!("Join sync task panicked: {}", e));
        Ok::<(), anyhow::Error>(())
    });

    let svc = shared::nats()
        .service_builder()
        .start("log", "0.1.0")
        .await
        .map_err(|e| anyhow::anyhow!("Failed to start service: {}", e))?;

    let log_group = svc.group("log");

    // Create all endpoints first
    let activity_endpoint = log_group
        .endpoint("activity")
        .await
        .expect("Unable to create activity endpoint");
    
    let pageview_endpoint = log_group
        .endpoint("pageview")
        .await
        .expect("Unable to create pageview endpoint");
    
    let clickthrough_endpoint = log_group
        .endpoint("clickthrough")
        .await
        .expect("Unable to create clickthrough endpoint");
    
    let user_tags_endpoint = log_group
        .endpoint("user_tags_update")
        .await
        .expect("Unable to create user_tags_update endpoint");
    
    let join_endpoint = log_group
        .endpoint("join")
        .await
        .expect("Unable to create join endpoint");
    
    let clickthrough_analytics_endpoint = log_group
        .endpoint("clickthrough_analytics")
        .await
        .expect("Unable to create clickthrough_analytics endpoint");
    
    let join_analytics_endpoint = log_group
        .endpoint("join_analytics")
        .await
        .expect("Unable to create join_analytics endpoint");

    // Spawn handlers for each endpoint
    let tx_activity = tx.clone();
    tasks.spawn(async move {
        let handler = activity_endpoint.for_each_concurrent(None, move |req| {
            let tx = tx_activity.clone();
            async move {
                if let Err(e) = handle_activity_log(&req, &tx).await {
                    eprintln!("Error handling activity log: {}", e);
                }
            }
        });
        handler.await;
        Ok::<(), anyhow::Error>(())
    });

    let tx_pageview = tx.clone();
    tasks.spawn(async move {
        let handler = pageview_endpoint.for_each_concurrent(None, move |req| {
            let tx = tx_pageview.clone();
            async move {
                if let Err(e) = handle_page_view(&req, &tx).await {
                    eprintln!("Error handling page view: {}", e);
                }
            }
        });
        handler.await;
        Ok::<(), anyhow::Error>(())
    });

    let tx_clickthrough = tx.clone();
    tasks.spawn(async move {
        let handler = clickthrough_endpoint.for_each_concurrent(None, move |req| {
            let tx = tx_clickthrough.clone();
            async move {
                if let Err(e) = handle_clickthrough(&req, &tx).await {
                    eprintln!("Error handling clickthrough: {}", e);
                }
            }
        });
        handler.await;
        Ok::<(), anyhow::Error>(())
    });

    tasks.spawn(async move {
        let handler = user_tags_endpoint.for_each_concurrent(None, move |req| {
            async move {
                if let Err(e) = handle_user_tags_update(&req).await {
                    eprintln!("Error handling user tags update: {}", e);
                }
            }
        });
        handler.await;
        Ok::<(), anyhow::Error>(())
    });

    let tx_join = tx.clone();
    tasks.spawn(async move {
        let handler = join_endpoint.for_each_concurrent(None, move |req| {
            let tx = tx_join.clone();
            async move {
                if let Err(e) = handle_join(&req, &tx).await {
                    eprintln!("Error handling join: {}", e);
                }
            }
        });
        handler.await;
        Ok::<(), anyhow::Error>(())
    });

    tasks.spawn(async move {
        let handler = clickthrough_analytics_endpoint.for_each_concurrent(None, move |req| {
            async move {
                if let Err(e) = handle_clickthrough_analytics(&req).await {
                    eprintln!("Error handling clickthrough analytics: {}", e);
                }
            }
        });
        handler.await;
        Ok::<(), anyhow::Error>(())
    });

    tasks.spawn(async move {
        let handler = join_analytics_endpoint.for_each_concurrent(None, move |req| {
            async move {
                if let Err(e) = handle_join_analytics(&req).await {
                    eprintln!("Error handling join analytics: {}", e);
                }
            }
        });
        handler.await;
        Ok::<(), anyhow::Error>(())
    });

    println!("Log service started and listening for messages on:");
    println!("  - log.activity");
    println!("  - log.pageview");
    println!("  - log.clickthrough");
    println!("  - log.join");
    println!("  - log.clickthrough_analytics");
    println!("  - log.join_analytics");
    println!("  - log.user_tags_update");
    println!("  - affiliate sync running in background");
    println!("  - bookmark sync running in background");
    println!("  - join sync running in background");

    // Wait for any task to complete (which would indicate an error)
    while let Some(result) = tasks.join_next().await {
        match result {
            Ok(task_result) => {
                if let Err(e) = task_result {
                    eprintln!("Task completed with error: {}", e);
                }
            }
            Err(e) => {
                eprintln!("Task panicked: {}", e);
            }
        }
        eprintln!("A task has completed unexpectedly, shutting down...");
        break;
    }

    Ok(())
}