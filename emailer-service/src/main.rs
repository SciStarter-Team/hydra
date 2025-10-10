use async_nats::service::ServiceExt;
use common::{handler, recv_handler, shared::{db, nats}};
use futures_util::stream::StreamExt;
use tokio::task::JoinSet;

mod analytics;
mod datasets;
mod datasets_info;
mod events;
mod format;
mod processor;
mod ratelimit;
mod scheduled;
mod sends;
mod ses;
mod template;

#[tokio::main]
async fn main() {
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

    // Register send email handler
    toplevel.spawn(handler!(service, "send", sends::handle_send_request));

    // Register event handlers (fire-and-forget)
    let event_group = service.group("event");
    toplevel.spawn(recv_handler!(event_group, "bounce", events::handle_bounce));
    toplevel.spawn(recv_handler!(event_group, "complaint", events::handle_complaint));
    toplevel.spawn(recv_handler!(event_group, "unsubscribe", events::handle_unsubscribe));
    toplevel.spawn(recv_handler!(event_group, "open", events::handle_open));

    // Register analytics handlers
    let analytics_group = service.group("analytics");
    toplevel.spawn(handler!(analytics_group, "queue_status", analytics::handle_queue_status));
    toplevel.spawn(handler!(analytics_group, "list_sendings", analytics::handle_list_sendings));
    toplevel.spawn(handler!(analytics_group, "get_sending", analytics::handle_get_sending));
    toplevel.spawn(handler!(analytics_group, "check_sendability", analytics::handle_check_sendability));

    // Register dataset introspection handlers
    let datasets_group = service.group("datasets");
    toplevel.spawn(handler!(datasets_group, "list", datasets_info::handle_list_datasets));
    toplevel.spawn(handler!(datasets_group, "variables", datasets_info::handle_get_dataset_variables));

    // Register scheduled send handlers
    toplevel.spawn(handler!(service, "schedule", scheduled::handle_schedule_send));
    toplevel.spawn(handler!(service, "cancel_scheduled", scheduled::handle_cancel_scheduled_send));

    // Spawn queue processor background task
    toplevel.spawn(async move {
        processor::run_queue_processor(ses_client).await;
    });

    // Spawn scheduled send processor background task
    toplevel.spawn(async move {
        processor::run_scheduled_processor().await;
    });

    println!("Emailer service ready");

    // Wait for all handlers to complete (they shouldn't unless there's an error)
    while let Some(result) = toplevel.join_next().await {
        if let Err(e) = result {
            eprintln!("Handler task failed: {:?}", e);
        }
    }

    println!("Emailer service shutting down");
}
