use async_nats::service::ServiceExt;
use common::{
    handler, recv_handler,
    shared::{db, nats},
};
use futures_util::stream::StreamExt;
use tokio::task::JoinSet;
use tracing::Level;

mod analytics;
mod datasets;
mod datasets_info;
mod events;
mod format;
mod health;
mod oneoff_emails;
mod processor;
mod ratelimit;
mod scheduled;
mod sends;
mod ses;
mod stored_emails;
mod sync;
mod template;
mod triggers;

#[tokio::main]
async fn main() {
    // Set up tracing - check for LOG_LEVEL environment variable
    let log_level = std::env::var("LOG_LEVEL")
        .ok()
        .and_then(|s| match s.to_lowercase().as_str() {
            "trace" => Some(Level::TRACE),
            "debug" => Some(Level::DEBUG),
            "info" => Some(Level::INFO),
            "warn" => Some(Level::WARN),
            "error" => Some(Level::ERROR),
            _ => None,
        })
        .unwrap_or_else(|| {
            if cfg!(debug_assertions) {
                Level::INFO
            } else {
                Level::WARN
            }
        });

    // Use pretty formatting in debug mode
    if cfg!(debug_assertions) {
        let subscriber = tracing_subscriber::FmtSubscriber::builder()
            .pretty()
            .with_max_level(log_level)
            .finish();
        tracing::subscriber::set_global_default(subscriber)
            .expect("setting default tracing subscriber failed");
    } else {
        let subscriber = tracing_subscriber::FmtSubscriber::builder()
            .with_max_level(log_level)
            .finish();
        tracing::subscriber::set_global_default(subscriber)
            .expect("setting default tracing subscriber failed");
    }

    println!("Log level: {:?}", log_level);

    common::shared::init("emailer").await;

    // Run database migrations
    sqlx::migrate!("./migrations")
        .run(db())
        .await
        .expect("Unable to run migrations");

    println!("Emailer service starting...");

    // Initialize NATS service
    let nats = nats();
    let service = nats
        .service_builder()
        .start("emailer", env!("CARGO_PKG_VERSION"))
        .await
        .expect("Unable to start emailer service");

    // Initialize AWS SES client
    let ses_client = ses::init_ses_client()
        .await
        .expect("Unable to initialize AWS SES client");

    let mut toplevel = JoinSet::<()>::new();

    // Register top-level send handlers
    let emailer_group = service.group("emailer");
    toplevel.spawn(handler!(emailer_group, "send", sends::handle_send_request));
    toplevel.spawn(handler!(
        emailer_group,
        "add_targets",
        sends::handle_add_targets_request
    ));

    // Register event handlers (fire-and-forget)
    let event_group = service.group("emailer.event");
    toplevel.spawn(recv_handler!(event_group, "bounce", events::handle_bounce));
    toplevel.spawn(recv_handler!(
        event_group,
        "complaint",
        events::handle_complaint
    ));
    toplevel.spawn(recv_handler!(
        event_group,
        "unsubscribe",
        events::handle_unsubscribe
    ));
    toplevel.spawn(recv_handler!(event_group, "open", events::handle_open));

    // Register analytics handlers
    let analytics_group = service.group("emailer.analytics");
    toplevel.spawn(handler!(
        analytics_group,
        "queue_status",
        analytics::handle_queue_status
    ));
    toplevel.spawn(handler!(
        analytics_group,
        "list_sendings",
        analytics::handle_list_sendings
    ));
    toplevel.spawn(handler!(
        analytics_group,
        "get_sending",
        analytics::handle_get_sending
    ));
    toplevel.spawn(handler!(
        analytics_group,
        "check_sendability",
        analytics::handle_check_sendability
    ));

    // Register dataset introspection handlers
    let datasets_group = service.group("emailer.datasets");
    toplevel.spawn(handler!(
        datasets_group,
        "list",
        datasets_info::handle_list_datasets
    ));
    toplevel.spawn(handler!(
        datasets_group,
        "variables",
        datasets_info::handle_get_dataset_variables
    ));

    // Register scheduled send handlers
    toplevel.spawn(handler!(
        emailer_group,
        "schedule",
        scheduled::handle_schedule_send
    ));
    toplevel.spawn(handler!(
        emailer_group,
        "cancel_scheduled",
        scheduled::handle_cancel_scheduled_send
    ));

    // Register stored email handlers
    let stored_group = service.group("emailer.stored");
    toplevel.spawn(handler!(
        stored_group,
        "list",
        stored_emails::handle_list_stored_emails
    ));
    toplevel.spawn(handler!(
        stored_group,
        "get",
        stored_emails::handle_get_stored_email
    ));
    toplevel.spawn(handler!(
        stored_group,
        "create",
        stored_emails::handle_create_stored_email
    ));
    toplevel.spawn(handler!(
        stored_group,
        "update",
        stored_emails::handle_update_stored_email
    ));
    toplevel.spawn(handler!(
        stored_group,
        "delete",
        stored_emails::handle_delete_stored_email
    ));

    // Register oneoff email handlers
    let oneoff_group = service.group("emailer.oneoff");
    toplevel.spawn(handler!(
        oneoff_group,
        "list",
        oneoff_emails::handle_list_oneoff_emails
    ));
    toplevel.spawn(handler!(
        oneoff_group,
        "get",
        oneoff_emails::handle_get_oneoff_email
    ));
    toplevel.spawn(handler!(
        oneoff_group,
        "create",
        oneoff_emails::handle_create_oneoff_email
    ));
    toplevel.spawn(handler!(
        oneoff_group,
        "update",
        oneoff_emails::handle_update_oneoff_email
    ));
    toplevel.spawn(handler!(
        oneoff_group,
        "delete",
        oneoff_emails::handle_delete_oneoff_email
    ));
    toplevel.spawn(handler!(
        oneoff_group,
        "archive",
        oneoff_emails::handle_archive_oneoff_email
    ));

    // Register trigger handlers
    let triggers_group = service.group("emailer.triggers");
    toplevel.spawn(handler!(
        triggers_group,
        "list",
        triggers::handle_list_triggers
    ));
    toplevel.spawn(handler!(
        triggers_group,
        "get",
        triggers::handle_get_trigger
    ));
    toplevel.spawn(handler!(
        triggers_group,
        "create",
        triggers::handle_create_trigger
    ));
    toplevel.spawn(handler!(
        triggers_group,
        "update",
        triggers::handle_update_trigger
    ));
    toplevel.spawn(handler!(
        triggers_group,
        "delete",
        triggers::handle_delete_trigger
    ));

    // Register trigger event handler (fire-and-forget)
    toplevel.spawn(recv_handler!(
        emailer_group,
        "trigger",
        triggers::handle_trigger_event
    ));

    // Register health check handler
    let health_group = service.group("emailer.health");
    toplevel.spawn(handler!(health_group, "check", health::health_check));

    // Ad-hoc read-only SQL over this service's database, for the staff
    // query tool in the frontend's MCP endpoint.
    toplevel.spawn(common::query::serve(service.group("emailer.query")));

    // Spawn queue processor background task
    toplevel.spawn(async move {
        processor::run_queue_processor(ses_client).await;
    });

    // Spawn scheduled send processor background task
    toplevel.spawn(async move {
        processor::run_scheduled_processor().await;
    });

    // Spawn unsubscribe sync background task
    toplevel.spawn(sync::synchronize_unsubscribes());

    // Spawn message template sync background task
    toplevel.spawn(sync::synchronize_message_templates());

    println!("Emailer service ready");

    // Wait for all handlers to complete (they shouldn't unless there's an error)
    while let Some(result) = toplevel.join_next().await {
        if let Err(e) = result {
            eprintln!("Handler task failed: {:?}", e);
        }
    }

    println!("Emailer service shutting down");
}
