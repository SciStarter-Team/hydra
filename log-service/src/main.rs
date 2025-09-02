use anyhow::Result;
use async_nats::service::ServiceExt;
use common::{shared, handler, recv_handler};
use futures::StreamExt;
use std::sync::Arc;
use tokio::{signal, task::JoinSet};
use tokio_util::sync::CancellationToken;

mod handlers;
mod sync;

use handlers::{
    handle_activity_log, handle_clickthrough, handle_clickthrough_analytics, handle_join,
    handle_join_analytics, handle_page_view, handle_user_emails_update, handle_user_tags_update,
    handle_health_check,
};
use log_service::{Database, run_compactor_service};
use sync::{bookmark_synchronize, golink_synchronize, join_synchronize, participation_synchronize};
use std::sync::OnceLock;

// Global database instance for handler access
pub static DATABASE: OnceLock<Arc<Database>> = OnceLock::new();


#[tokio::main]
async fn main() -> Result<()> {
    // Initialize shared database connection (from common)
    shared::init("log_cache").await;

    #[cfg(not(debug_assertions))]
    let database_path = env!("DATABASE_PATH");
    #[cfg(debug_assertions)]
    let database_path = env!("LOCAL_DATABASE_PATH");

    // Initialize database and table system
    let database = Arc::new(Database::new(database_path).await?);
    
    // Set global database for handler access
    DATABASE.set(Arc::clone(&database)).map_err(|_| anyhow::anyhow!("Failed to set global database"))?;

    let mut background_tasks = JoinSet::new();

    // Create cancellation tokens for graceful shutdown
    let participation_cancel = CancellationToken::new();
    let bookmark_cancel = CancellationToken::new();
    let join_cancel = CancellationToken::new();
    let golink_cancel = CancellationToken::new();
    let compactor_cancel = CancellationToken::new();

    // Spawn WAL compactor service
    let compactor_cancel_clone = compactor_cancel.clone();
    let compactor_database = Arc::clone(&database);
    background_tasks.spawn(async move {
        if let Err(e) = run_compactor_service(compactor_database, compactor_cancel_clone).await {
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

    // Spawn golink sync task
    let golink_cancel_clone = golink_cancel.clone();
    let golink_database = Arc::clone(&database);
    background_tasks.spawn(async move {
        golink_synchronize(golink_database, golink_cancel_clone).await;
        Ok::<(), anyhow::Error>(())
    });

    let svc = shared::nats()
        .service_builder()
        .start("log", "0.1.0")
        .await
        .map_err(|e| anyhow::anyhow!("Failed to start service: {}", e))?;

    // Use handler! and recv_handler! macros for clean endpoint management
    let mut toplevel = JoinSet::new();

    // Log endpoints
    let log_group = svc.group("log");
    toplevel.spawn(recv_handler!(log_group, "activity", handle_activity_log));
    toplevel.spawn(recv_handler!(log_group, "pageview", handle_page_view));
    toplevel.spawn(recv_handler!(log_group, "clickthrough", handle_clickthrough));
    toplevel.spawn(recv_handler!(log_group, "join", handle_join));
    toplevel.spawn(handler!(log_group, "clickthrough_analytics", handle_clickthrough_analytics));
    toplevel.spawn(handler!(log_group, "join_analytics", handle_join_analytics));

    // Cache endpoints
    let cache_group = svc.group("log.cache");
    toplevel.spawn(recv_handler!(cache_group, "user_tags_update", handle_user_tags_update));
    toplevel.spawn(recv_handler!(cache_group, "user_emails_update", handle_user_emails_update));

    // Health endpoint
    let health_group = svc.group("log.health");
    toplevel.spawn(handler!(health_group, "check", handle_health_check));

    println!("Log service started and listening for messages on:");
    println!("  - log.activity");
    println!("  - log.pageview");
    println!("  - log.clickthrough");
    println!("  - log.join");
    println!("  - log.clickthrough_analytics");
    println!("  - log.join_analytics");
    println!("  - log.cache.user_tags_update");
    println!("  - log.cache.user_emails_update");
    println!("  - log.health.check");
    println!("  - participation sync running in background");
    println!("  - bookmark sync running in background");
    println!("  - join sync running in background");
    println!("  - golink sync running in background");
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
        result = toplevel.join_next() => {
            if let Some(result) = result {
                match result {
                    Ok(_) => {
                        eprintln!("NATS handler completed unexpectedly");
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
    toplevel.abort_all();

    // Wait for NATS handlers to be aborted with short timeout
    let nats_timeout = tokio::time::Duration::from_secs(5);
    let _ = tokio::time::timeout(nats_timeout, async {
        while let Some(_) = toplevel.join_next().await {
            // Wait for NATS handlers to be aborted
        }
    })
    .await;

    // Now signal graceful shutdown to background tasks
    println!("Signaling shutdown to background tasks...");
    participation_cancel.cancel();
    bookmark_cancel.cancel();
    join_cancel.cancel();
    golink_cancel.cancel();
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
