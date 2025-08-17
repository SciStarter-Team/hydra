use anyhow::Result;
use async_nats::service::ServiceExt;
use common::shared;
use futures::StreamExt;
use std::sync::Arc;
use tokio::{signal, task::JoinSet};
use tokio_util::sync::CancellationToken;

mod handlers;
mod sync;

use handlers::{
    handle_activity_log, handle_clickthrough, handle_clickthrough_analytics, handle_join,
    handle_join_analytics, handle_page_view, handle_user_emails_update, handle_user_tags_update,
};
use log_service::{Database, run_compactor_service};
use sync::{bookmark_synchronize, join_synchronize, participation_synchronize};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize shared database connection (from common)
    shared::init("log_cahe").await;

    let database_path = std::env::var("DATABASE_URL").expect("DATABASE_URL is not set");

    // Initialize database and table system
    let database = Arc::new(Database::new(database_path).await?);

    let mut background_tasks = JoinSet::new();
    let mut nats_handler_tasks = JoinSet::new();

    // Create cancellation tokens for graceful shutdown
    let participation_cancel = CancellationToken::new();
    let bookmark_cancel = CancellationToken::new();
    let join_cancel = CancellationToken::new();
    let compactor_cancel = CancellationToken::new();

    // Spawn WAL compactor service
    let compactor_cancel_clone = compactor_cancel.clone();
    let compactor_database = Arc::clone(&database);
    background_tasks.spawn(async move {
        if let Err(e) = run_compactor_service(&compactor_database, compactor_cancel_clone).await {
            eprintln!("WAL compactor service error: {}", e);
        }
        Ok::<(), anyhow::Error>(())
    });

    // Spawn participation sync task
    let participation_cancel_clone = participation_cancel.clone();
    let participation_database = Arc::clone(&database);
    background_tasks.spawn(async move {
        participation_synchronize(participation_database, participation_cancel_clone).await;
        Ok::<(), anyhow::Error>(())
    });

    // Spawn bookmark sync task
    let bookmark_cancel_clone = bookmark_cancel.clone();
    let bookmark_database = Arc::clone(&database);
    background_tasks.spawn(async move {
        bookmark_synchronize(bookmark_database, bookmark_cancel_clone).await;
        Ok::<(), anyhow::Error>(())
    });

    // Spawn join sync task
    let join_cancel_clone = join_cancel.clone();
    let join_database = Arc::clone(&database);
    background_tasks.spawn(async move {
        join_synchronize(join_database, join_cancel_clone).await;
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

    let cache_group = svc.group("log.cache");

    let user_tags_endpoint = cache_group
        .endpoint("user_tags_update")
        .await
        .expect("Unable to create user_tags_update endpoint");

    let user_emails_endpoint = cache_group
        .endpoint("user_emails_update")
        .await
        .expect("Unable to create user_emails_update endpoint");

    // Spawn handlers for each endpoint
    let activity_database = Arc::clone(&database);
    nats_handler_tasks.spawn(async move {
        let db = Arc::clone(&activity_database);
        let handler = activity_endpoint.for_each_concurrent(None, move |req| {
            let db = Arc::clone(&db);
            async move {
                if let Err(e) = handle_activity_log(db, &req).await {
                    eprintln!("Error handling activity log: {}", e);
                }
            }
        });
        handler.await;
        Ok::<(), anyhow::Error>(())
    });

    let pageview_database = Arc::clone(&database);
    nats_handler_tasks.spawn(async move {
        let db = Arc::clone(&pageview_database);
        let handler = pageview_endpoint.for_each_concurrent(None, move |req| {
            let db = Arc::clone(&db);
            async move {
                if let Err(e) = handle_page_view(db, &req).await {
                    eprintln!("Error handling page view: {}", e);
                }
            }
        });
        handler.await;
        Ok::<(), anyhow::Error>(())
    });

    let clickthrough_database = Arc::clone(&database);
    nats_handler_tasks.spawn(async move {
        let db = Arc::clone(&clickthrough_database);
        let handler = clickthrough_endpoint.for_each_concurrent(None, move |req| {
            let db = Arc::clone(&db);
            async move {
                if let Err(e) = handle_clickthrough(db, &req).await {
                    eprintln!("Error handling clickthrough: {}", e);
                }
            }
        });
        handler.await;
        Ok::<(), anyhow::Error>(())
    });

    let user_tags_database = Arc::clone(&database);
    nats_handler_tasks.spawn(async move {
        let db = Arc::clone(&user_tags_database);
        let handler = user_tags_endpoint.for_each_concurrent(None, move |req| {
            let db = Arc::clone(&db);
            async move {
                if let Err(e) = handle_user_tags_update(db, &req).await {
                    eprintln!("Error handling user tags update: {}", e);
                }
            }
        });
        handler.await;
        Ok::<(), anyhow::Error>(())
    });

    let join_database = Arc::clone(&database);
    nats_handler_tasks.spawn(async move {
        let db = Arc::clone(&join_database);
        let handler = join_endpoint.for_each_concurrent(None, move |req| {
            let db = Arc::clone(&db);
            async move {
                if let Err(e) = handle_join(db, &req).await {
                    eprintln!("Error handling join: {}", e);
                }
            }
        });
        handler.await;
        Ok::<(), anyhow::Error>(())
    });

    let clickthrough_analytics_database = Arc::clone(&database);
    nats_handler_tasks.spawn(async move {
        let db = Arc::clone(&clickthrough_analytics_database);
        let handler = clickthrough_analytics_endpoint.for_each_concurrent(None, move |req| {
            let db = Arc::clone(&db);
            async move {
                if let Err(e) = handle_clickthrough_analytics(db, &req).await {
                    eprintln!("Error handling clickthrough analytics: {}", e);
                }
            }
        });
        handler.await;
        Ok::<(), anyhow::Error>(())
    });

    let join_analytics_database = Arc::clone(&database);
    nats_handler_tasks.spawn(async move {
        let db = Arc::clone(&join_analytics_database);
        let handler = join_analytics_endpoint.for_each_concurrent(None, move |req| {
            let db = Arc::clone(&db);
            async move {
                if let Err(e) = handle_join_analytics(db, &req).await {
                    eprintln!("Error handling join analytics: {}", e);
                }
            }
        });
        handler.await;
        Ok::<(), anyhow::Error>(())
    });

    let user_emails_database = Arc::clone(&database);
    nats_handler_tasks.spawn(async move {
        let db = Arc::clone(&user_emails_database);
        let handler = user_emails_endpoint.for_each_concurrent(None, move |req| {
            let db = Arc::clone(&db);
            async move {
                if let Err(e) = handle_user_emails_update(db, &req).await {
                    eprintln!("Error handling user emails update: {}", e);
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
    println!("  - log.cache.user_tags_update");
    println!("  - log.cache.user_emails_update");
    println!("  - participation sync running in background");
    println!("  - bookmark sync running in background");
    println!("  - join sync running in background");
    println!("  - WAL compactor running in background");

    // Set up signal handling
    let mut sigint = signal::unix::signal(signal::unix::SignalKind::interrupt())?;
    let mut sigterm = signal::unix::signal(signal::unix::SignalKind::terminate())?;

    tokio::select! {
        // Wait for SIGINT (Ctrl+C)
        _ = sigint.recv() => {
            println!("Received SIGINT, initiating graceful shutdown...");
        }
        // Wait for SIGTERM
        _ = sigterm.recv() => {
            println!("Received SIGTERM, initiating graceful shutdown...");
        }
        // Wait for any background task to complete (which would indicate an error)
        result = background_tasks.join_next() => {
            if let Some(result) = result {
                match result {
                    Ok(task_result) => {
                        if let Err(e) = task_result {
                            eprintln!("Background task completed with error: {}", e);
                        }
                    }
                    Err(e) => {
                        eprintln!("Background task panicked: {}", e);
                    }
                }
                eprintln!("A background task has completed unexpectedly, shutting down...");
            }
        }
        // Wait for any NATS handler to complete (which would indicate an error)
        result = nats_handler_tasks.join_next() => {
            if let Some(result) = result {
                match result {
                    Ok(task_result) => {
                        if let Err(e) = task_result {
                            eprintln!("NATS handler completed with error: {}", e);
                        }
                    }
                    Err(e) => {
                        eprintln!("NATS handler panicked: {}", e);
                    }
                }
                eprintln!("A NATS handler has completed unexpectedly, shutting down...");
            }
        }
    }

    // First, stop accepting new work by terminating NATS handlers
    println!("Terminating NATS handlers to stop accepting new requests...");
    nats_handler_tasks.abort_all();

    // Wait for NATS handlers to be aborted with short timeout
    let nats_timeout = tokio::time::Duration::from_secs(5);
    let _ = tokio::time::timeout(nats_timeout, async {
        while let Some(_) = nats_handler_tasks.join_next().await {
            // Wait for NATS handlers to be aborted
        }
    })
    .await;

    // Now signal graceful shutdown to background tasks
    println!("Signaling shutdown to background tasks...");
    participation_cancel.cancel();
    bookmark_cancel.cancel();
    join_cancel.cancel();
    compactor_cancel.cancel();

    // Wait for critical background tasks to finish gracefully (no force timeout for data safety)
    println!("Waiting for critical background tasks to finish gracefully...");

    // Wait indefinitely for background tasks to preserve data integrity
    while !background_tasks.is_empty() {
        if let Some(result) = background_tasks.join_next().await {
            match result {
                Ok(_) => {
                    println!(
                        "Background task finished ({} remaining)",
                        background_tasks.len()
                    );
                }
                Err(e) if e.is_cancelled() => {
                    println!(
                        "Background task stopped ({} remaining)",
                        background_tasks.len()
                    );
                }
                Err(e) => {
                    eprintln!(
                        "Background task error during shutdown: {} ({} remaining)",
                        e,
                        background_tasks.len()
                    );
                }
            }
        } else {
            // No more background tasks
            break;
        }
    }

    println!("All critical background tasks finished - data integrity preserved");

    // Perform graceful shutdown of database and table system
    println!("Shutting down database and table system...");
    match Arc::try_unwrap(database) {
        Ok(database) => {
            if let Err(e) = database.shutdown().await {
                eprintln!("Error during database shutdown: {}", e);
            }
        }
        Err(_) => {
            eprintln!(
                "Warning: Could not shutdown database cleanly - other references still exist"
            );
        }
    }

    println!("Graceful shutdown complete.");
    Ok(())
}
