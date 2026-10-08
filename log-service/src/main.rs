use anyhow::Result;
use async_nats::service::ServiceExt;
use common::{handler, recv_handler, shared};
use futures::StreamExt;
use std::sync::Arc;
use tokio::{signal, task::JoinSet};
use tokio_util::sync::CancellationToken;

mod bridges;
mod engagement;
mod exchange;
mod foundations;
mod handlers;
mod project_ids;
mod sync;

use bridges::{
    inaturalist::inaturalist_synchronize,
    zooniverse::{zooniverse_sync_projects, zooniverse_sync_users, zooniverse_synchronize},
};
use handlers::{
    handle_activity_log, handle_acts_of_science_create, handle_acts_to_date,
    handle_affiliate_participation, handle_analytics_query, handle_articulate_completion,
    handle_articulate_completion_record, handle_clickthrough, handle_clickthrough_analytics,
    handle_contributors, handle_edit_log, handle_email_event, handle_entity_list_stats,
    handle_external_link_clickthrough, handle_external_links, handle_go_url_stats,
    handle_gourl_totals, handle_health_check, handle_join, handle_last_active_for,
    handle_last_edited, handle_page_view, handle_participation_analytics,
    handle_participation_by_email, handle_participation_by_users, handle_participation_summary,
    handle_project_likes, handle_project_member_count, handle_project_participation,
    handle_project_participation_record, handle_project_reviews, handle_rating_set,
    handle_recent_activity, handle_review_set, handle_reviewers_by_projects, handle_search_event,
    handle_share, handle_top_performers_analytics, handle_user_emails_update,
    handle_user_first_participation, handle_user_last_view, handle_user_participation,
    handle_user_participation_breakdown, handle_user_recent_visits, handle_user_tags_update,
    handle_users_active_since,
};
use log_service::{Database, run_compactor_service};
use parking_lot::RwLock;
use sync::{
    acts_of_science_synchronize, bookmark_synchronize, exchange_event_synchronize,
    golink_synchronize, join_synchronize, participation_synchronize, rating_synchronize,
    review_synchronize,
};

// Global database instance for handler access - using RwLock<Option<>> so we can take ownership for shutdown
pub static DATABASE: RwLock<Option<Arc<Database>>> = RwLock::new(None);

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize shared database connection (from common)
    shared::init("log_cache").await;

    // Run migrations
    sqlx::migrate!("./migrations").run(shared::db()).await?;

    #[cfg(not(debug_assertions))]
    let database_path = env!("DATABASE_PATH");
    #[cfg(debug_assertions)]
    let database_path = env!("LOCAL_DATABASE_PATH");

    // Initialize database and table system
    let database = Arc::new(Database::new(database_path).await?);

    // Set global database for handler access
    *DATABASE.write() = Some(Arc::clone(&database));

    let mut background_tasks = JoinSet::new();

    // Create cancellation tokens for graceful shutdown
    let participation_cancel = CancellationToken::new();
    let bookmark_cancel = CancellationToken::new();
    let join_cancel = CancellationToken::new();
    let rating_cancel = CancellationToken::new();
    let review_cancel = CancellationToken::new();
    let golink_cancel = CancellationToken::new();
    let acts_cancel = CancellationToken::new();
    let exchange_events_cancel = CancellationToken::new();
    let zoo_projects_cancel = CancellationToken::new();
    let zoo_users_cancel = CancellationToken::new();
    let zoo_sync_cancel = CancellationToken::new();
    let inat_cancel = CancellationToken::new();
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

    // Spawn rating ("likes") sync task
    let rating_cancel_clone = rating_cancel.clone();
    let rating_database = Arc::clone(&database);
    background_tasks.spawn(async move {
        rating_synchronize(rating_database, rating_cancel_clone).await;
        Ok::<(), anyhow::Error>(())
    });

    // Spawn review sync task
    let review_cancel_clone = review_cancel.clone();
    let review_database = Arc::clone(&database);
    background_tasks.spawn(async move {
        review_synchronize(review_database, review_cancel_clone).await;
        Ok::<(), anyhow::Error>(())
    });

    // Spawn golink sync task
    let golink_cancel_clone = golink_cancel.clone();
    let golink_database = Arc::clone(&database);
    background_tasks.spawn(async move {
        golink_synchronize(golink_database, golink_cancel_clone).await;
        Ok::<(), anyhow::Error>(())
    });

    // Spawn acts of science sync task
    let acts_cancel_clone = acts_cancel.clone();
    let acts_database = Arc::clone(&database);
    background_tasks.spawn(async move {
        acts_of_science_synchronize(acts_database, acts_cancel_clone).await;
        Ok::<(), anyhow::Error>(())
    });

    // Spawn exchange event (exchange data page history) sync task
    let exchange_events_cancel_clone = exchange_events_cancel.clone();
    let exchange_events_database = Arc::clone(&database);
    background_tasks.spawn(async move {
        exchange_event_synchronize(exchange_events_database, exchange_events_cancel_clone).await;
        Ok::<(), anyhow::Error>(())
    });

    // Spawn Zooniverse project sync task
    let zoo_projects_cancel_clone = zoo_projects_cancel.clone();
    let zoo_projects_database = Arc::clone(&database);
    background_tasks.spawn(async move {
        zooniverse_sync_projects(zoo_projects_database, zoo_projects_cancel_clone).await;
        Ok::<(), anyhow::Error>(())
    });

    // Spawn Zooniverse user sync task
    let zoo_users_cancel_clone = zoo_users_cancel.clone();
    let zoo_users_database = Arc::clone(&database);
    background_tasks.spawn(async move {
        zooniverse_sync_users(zoo_users_database, zoo_users_cancel_clone).await;
        Ok::<(), anyhow::Error>(())
    });

    // Spawn Zooniverse classification sync task
    let zoo_sync_cancel_clone = zoo_sync_cancel.clone();
    let zoo_sync_database = Arc::clone(&database);
    background_tasks.spawn(async move {
        zooniverse_synchronize(zoo_sync_database, zoo_sync_cancel_clone).await;
        Ok::<(), anyhow::Error>(())
    });

    // Spawn iNaturalist sync task
    let inat_cancel_clone = inat_cancel.clone();
    let inat_database = Arc::clone(&database);
    background_tasks.spawn(async move {
        inaturalist_synchronize(inat_database, inat_cancel_clone).await;
        Ok::<(), anyhow::Error>(())
    });

    // The DATABASE global and the background task references should
    // be the only references to the DatFusion database that we need
    // from here on, and holding this until the end of scope would
    // prevent the shutdown code from working properly.
    drop(database);

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
    toplevel.spawn(recv_handler!(
        log_group,
        "clickthrough",
        handle_clickthrough
    ));
    toplevel.spawn(recv_handler!(log_group, "join", handle_join));
    toplevel.spawn(recv_handler!(log_group, "email_event", handle_email_event));
    toplevel.spawn(recv_handler!(
        log_group,
        "exchange_event",
        exchange::record_exchange_event
    ));
    toplevel.spawn(recv_handler!(
        log_group,
        "search_event",
        handle_search_event
    ));
    toplevel.spawn(recv_handler!(log_group, "share", handle_share));
    toplevel.spawn(handler!(
        log_group,
        "exchange_engagement",
        exchange::get_engagement_stats
    ));
    toplevel.spawn(handler!(
        log_group,
        "clickthrough_analytics",
        handle_clickthrough_analytics
    ));
    toplevel.spawn(handler!(
        log_group,
        "participation_analytics",
        handle_participation_analytics
    ));
    toplevel.spawn(handler!(
        log_group,
        "participation_summary",
        handle_participation_summary
    ));
    toplevel.spawn(handler!(
        log_group,
        "participation_by_users",
        handle_participation_by_users
    ));
    toplevel.spawn(handler!(log_group, "gourl_totals", handle_gourl_totals));
    toplevel.spawn(handler!(
        log_group,
        "user_first_participation",
        handle_user_first_participation
    ));
    toplevel.spawn(handler!(
        log_group,
        "participation_by_email",
        handle_participation_by_email
    ));
    toplevel.spawn(handler!(
        log_group,
        "top_performers_analytics",
        handle_top_performers_analytics
    ));
    toplevel.spawn(handler!(
        log_group,
        "recent_activity",
        handle_recent_activity
    ));
    toplevel.spawn(handler!(
        log_group,
        "affiliate_participants",
        handle_affiliate_participation
    ));
    toplevel.spawn(handler!(
        log_group,
        "entity_list_stats",
        handle_entity_list_stats
    ));
    toplevel.spawn(handler!(
        log_group,
        "articulate_completion",
        handle_articulate_completion
    ));
    toplevel.spawn(handler!(
        log_group,
        "project_participation",
        handle_project_participation
    ));
    toplevel.spawn(recv_handler!(
        log_group,
        "articulate_completion_record",
        handle_articulate_completion_record
    ));
    toplevel.spawn(recv_handler!(
        log_group,
        "project_participation_record",
        handle_project_participation_record
    ));
    toplevel.spawn(handler!(
        log_group,
        "project_member_count",
        handle_project_member_count
    ));
    toplevel.spawn(handler!(
        log_group,
        "users_active_since",
        handle_users_active_since
    ));
    toplevel.spawn(handler!(
        log_group,
        "last_active_for",
        handle_last_active_for
    ));
    toplevel.spawn(handler!(log_group, "project_likes", handle_project_likes));
    toplevel.spawn(handler!(log_group, "rating_set", handle_rating_set));
    toplevel.spawn(handler!(
        log_group,
        "project_reviews",
        handle_project_reviews
    ));
    toplevel.spawn(handler!(
        log_group,
        "reviewers_by_projects",
        handle_reviewers_by_projects
    ));
    toplevel.spawn(handler!(log_group, "review_set", handle_review_set));
    toplevel.spawn(handler!(log_group, "last_edited", handle_last_edited));
    toplevel.spawn(handler!(log_group, "edit_log", handle_edit_log));
    toplevel.spawn(handler!(
        log_group,
        "user_participation",
        handle_user_participation
    ));
    toplevel.spawn(handler!(
        log_group,
        "user_participation_breakdown",
        handle_user_participation_breakdown
    ));
    toplevel.spawn(handler!(
        log_group,
        "user_recent_visits",
        handle_user_recent_visits
    ));
    toplevel.spawn(handler!(log_group, "user_last_view", handle_user_last_view));

    // Stats endpoints
    let stats_group = svc.group("log.stats");
    toplevel.spawn(handler!(stats_group, "go_url", handle_go_url_stats));
    toplevel.spawn(handler!(
        stats_group,
        "annual_engagement",
        engagement::handle_annual_engagement
    ));

    // Cache endpoints
    let cache_group = svc.group("log.cache");
    toplevel.spawn(recv_handler!(
        cache_group,
        "user_tags_update",
        handle_user_tags_update
    ));
    toplevel.spawn(recv_handler!(
        cache_group,
        "user_emails_update",
        handle_user_emails_update
    ));

    // External links endpoints
    let links_group = svc.group("log.links");
    toplevel.spawn(handler!(
        links_group,
        "get_or_create",
        handle_external_links
    ));
    toplevel.spawn(handler!(
        links_group,
        "clickthrough",
        handle_external_link_clickthrough
    ));

    // Acts of Science endpoints
    let acts_group = svc.group("log.acts");
    toplevel.spawn(handler!(
        acts_group,
        "create",
        handle_acts_of_science_create
    ));
    toplevel.spawn(handler!(acts_group, "to_date", handle_acts_to_date));
    toplevel.spawn(handler!(acts_group, "contributors", handle_contributors));

    // Health endpoint
    let health_group = svc.group("log.health");
    toplevel.spawn(handler!(health_group, "check", handle_health_check));

    // Ad-hoc read-only SQL for the staff query tool in the frontend's MCP
    // endpoint. The log service has two query surfaces: `select` covers
    // the SQLite cache, `analytics` the DataFusion context over Parquet.
    toplevel.spawn(common::query::serve(svc.group("log.query")));
    let analytics_query = svc.group("log.query");
    toplevel.spawn(handler!(
        analytics_query,
        "analytics",
        handle_analytics_query
    ));

    println!("Log service started and listening for messages on:");
    println!("  - log.activity");
    println!("  - log.pageview");
    println!("  - log.clickthrough");
    println!("  - log.join");
    println!("  - log.email_event");
    println!("  - log.exchange_event");
    println!("  - log.share");
    println!("  - log.exchange_engagement");
    println!("  - log.links.get_or_create");
    println!("  - log.links.clickthrough");
    println!("  - log.clickthrough_analytics");
    println!("  - log.participation_analytics");
    println!("  - log.recent_activity");
    println!("  - log.affiliate_participants");
    println!("  - log.entity_list_stats");
    println!("  - log.stats.go_url");
    println!("  - log.acts.contributors");
    println!("  - log.cache.user_tags_update");
    println!("  - log.cache.user_emails_update");
    println!("  - log.health.check");
    println!("  - log.project_likes");
    println!("  - log.rating_set");
    println!("  - log.project_reviews");
    println!("  - log.reviewers_by_projects");
    println!("  - log.participation_by_users");
    println!("  - log.gourl_totals");
    println!("  - log.review_set");
    println!("  - participation sync running in background");
    println!("  - bookmark sync running in background");
    println!("  - join sync running in background");
    println!("  - rating sync running in background");
    println!("  - review sync running in background");
    println!("  - golink sync running in background");
    println!("  - acts of science sync running in background");
    println!("  - zooniverse project sync running in background");
    println!("  - zooniverse user sync running in background");
    println!("  - zooniverse classification sync running in background");
    println!("  - iNaturalist sync running in background");
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
    rating_cancel.cancel();
    review_cancel.cancel();
    golink_cancel.cancel();
    acts_cancel.cancel();
    exchange_events_cancel.cancel();
    zoo_projects_cancel.cancel();
    zoo_users_cancel.cancel();
    zoo_sync_cancel.cancel();
    inat_cancel.cancel();
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

    // Take ownership of the database from the global
    let database = DATABASE.write().take();

    if let Some(mut database) = database {
        // Wait up to 5 minutes for all other references to be dropped
        let shutdown_timeout = std::time::Duration::from_secs(5 * 60);
        let start_time = std::time::Instant::now();

        println!("Waiting for handler tasks to release database references...");

        let mut last_log_time = 0u64;

        loop {
            match Arc::try_unwrap(database) {
                Ok(database) => {
                    println!("All database references released, performing clean shutdown");
                    if let Err(e) = database.shutdown().await {
                        eprintln!("Error during database shutdown: {}", e);
                    }
                    break;
                }
                Err(arc_db) => {
                    database = arc_db;
                    let elapsed = start_time.elapsed();

                    if elapsed >= shutdown_timeout {
                        eprintln!(
                            "Timeout waiting for database references to be released after {:?}",
                            elapsed
                        );
                        eprintln!(
                            "Warning: Could not shutdown database cleanly - {} references still exist",
                            Arc::strong_count(&database)
                        );
                        break;
                    }

                    let elapsed_secs = elapsed.as_secs();
                    if elapsed_secs > 0 && elapsed_secs % 10 == 0 && elapsed_secs != last_log_time {
                        println!(
                            "Still waiting for database references to be released... ({} references, {:?} elapsed)",
                            Arc::strong_count(&database),
                            elapsed
                        );
                        last_log_time = elapsed_secs;
                    }

                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                }
            }
        }
    } else {
        eprintln!("Warning: Database was already taken for shutdown");
    }

    println!("Graceful shutdown complete.");
    Ok(())
}
