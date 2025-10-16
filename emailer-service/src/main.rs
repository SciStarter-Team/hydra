use async_nats::service::ServiceExt;
use common::{handler, recv_handler, shared::{db, nats}};
use futures_util::stream::StreamExt;
use tokio::task::JoinSet;

mod analytics;
mod datasets;
mod datasets_info;
mod events;
mod format;
mod oneoff_emails;
mod processor;
mod ratelimit;
mod scheduled;
mod sends;
mod ses;
mod stored_emails;
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
    let event_group = service.group("emailer.event");
    toplevel.spawn(recv_handler!(event_group, "bounce", events::handle_bounce));
    toplevel.spawn(recv_handler!(event_group, "complaint", events::handle_complaint));
    toplevel.spawn(recv_handler!(event_group, "unsubscribe", events::handle_unsubscribe));
    toplevel.spawn(recv_handler!(event_group, "open", events::handle_open));

    // Register analytics handlers
    let analytics_group = service.group("emailer.analytics");
    toplevel.spawn(handler!(analytics_group, "queue_status", analytics::handle_queue_status));
    toplevel.spawn(handler!(analytics_group, "list_sendings", analytics::handle_list_sendings));
    toplevel.spawn(handler!(analytics_group, "get_sending", analytics::handle_get_sending));
    toplevel.spawn(handler!(analytics_group, "check_sendability", analytics::handle_check_sendability));

    // Register dataset introspection handlers
    let datasets_group = service.group("emailer.datasets");
    toplevel.spawn(handler!(datasets_group, "list", datasets_info::handle_list_datasets));
    toplevel.spawn(handler!(datasets_group, "variables", datasets_info::handle_get_dataset_variables));

    // Register scheduled send handlers
    toplevel.spawn(handler!(service, "schedule", scheduled::handle_schedule_send));
    toplevel.spawn(handler!(service, "cancel_scheduled", scheduled::handle_cancel_scheduled_send));

    // Register stored email handlers
    let stored_group = service.group("emailer.stored");
    toplevel.spawn(handler!(stored_group, "list", stored_emails::handle_list_stored_emails));
    toplevel.spawn(handler!(stored_group, "get", stored_emails::handle_get_stored_email));
    toplevel.spawn(handler!(stored_group, "create", stored_emails::handle_create_stored_email));
    toplevel.spawn(handler!(stored_group, "update", stored_emails::handle_update_stored_email));
    toplevel.spawn(handler!(stored_group, "delete", stored_emails::handle_delete_stored_email));

    // Register oneoff email handlers
    let oneoff_group = service.group("emailer.oneoff");
    toplevel.spawn(handler!(oneoff_group, "list", oneoff_emails::handle_list_oneoff_emails));
    toplevel.spawn(handler!(oneoff_group, "get", oneoff_emails::handle_get_oneoff_email));
    toplevel.spawn(handler!(oneoff_group, "create", oneoff_emails::handle_create_oneoff_email));
    toplevel.spawn(handler!(oneoff_group, "update", oneoff_emails::handle_update_oneoff_email));
    toplevel.spawn(handler!(oneoff_group, "delete", oneoff_emails::handle_delete_oneoff_email));
    toplevel.spawn(handler!(oneoff_group, "archive", oneoff_emails::handle_archive_oneoff_email));

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
