use async_nats::service::ServiceExt;
use common::{
    handler, recv_handler,
    shared::{db, nats},
};
use futures::StreamExt;
use tokio::task::JoinSet;
use tracing::Level;

pub mod categories;
pub mod content_filter;
pub mod forums;
pub mod health;
pub mod messaging;
pub mod moderation;
pub mod posts;
pub mod sync;
pub mod topics;

#[tokio::main]
async fn main() {
    // Set up tracing
    if cfg!(debug_assertions) {
        let subscriber = tracing_subscriber::FmtSubscriber::builder()
            .pretty()
            .with_max_level(Level::INFO)
            .finish();
        tracing::subscriber::set_global_default(subscriber)
            .expect("setting default tracing subscriber failed");
    } else {
        let subscriber = tracing_subscriber::FmtSubscriber::builder()
            .with_max_level(Level::WARN)
            .finish();
        tracing::subscriber::set_global_default(subscriber)
            .expect("setting default tracing subscriber failed");
    }

    common::shared::init("forum").await;

    sqlx::migrate!("./migrations")
        .run(db())
        .await
        .expect("Unable to run migrations");

    // Initialize the bad words filter
    content_filter::initialize_filter()
        .await
        .expect("Failed to initialize content filter");

    let nats = nats();

    let service = nats
        .service_builder()
        .start("forum", env!("CARGO_PKG_VERSION"))
        .await
        .expect("Unable to start forum service");

    let mut toplevel = JoinSet::new();

    // Topic management endpoints
    let topics = service.group("forum.topics");
    toplevel.spawn(handler!(topics, "create", topics::create_topic));
    toplevel.spawn(handler!(topics, "get", topics::get_topic));
    toplevel.spawn(handler!(topics, "list", topics::list_topics));
    toplevel.spawn(handler!(topics, "update", topics::update_topic));
    toplevel.spawn(handler!(
        topics,
        "suggest_similar",
        topics::suggest_similar_topics
    ));
    toplevel.spawn(handler!(topics, "user_topics", topics::get_user_topics));

    // Post management endpoints
    let posts = service.group("forum.posts");
    toplevel.spawn(handler!(posts, "create", posts::create_post));
    toplevel.spawn(handler!(posts, "get", posts::get_post));
    toplevel.spawn(handler!(posts, "list", posts::list_posts));
    toplevel.spawn(handler!(posts, "update", posts::update_post));

    // Moderation endpoints
    let moderation = service.group("forum.moderation");
    toplevel.spawn(handler!(moderation, "action", moderation::moderate_content));
    toplevel.spawn(handler!(moderation, "log", moderation::get_moderation_log));
    toplevel.spawn(handler!(
        moderation,
        "add_moderator",
        moderation::add_forum_moderator
    ));
    toplevel.spawn(handler!(
        moderation,
        "remove_moderator",
        moderation::remove_forum_moderator
    ));
    toplevel.spawn(handler!(
        moderation,
        "list_moderators",
        moderation::list_forum_moderators
    ));

    // Forum management endpoints
    let forum_mgmt = service.group("forum.management");
    toplevel.spawn(handler!(forum_mgmt, "create", forums::create_forum));
    toplevel.spawn(handler!(
        forum_mgmt,
        "get_by_slug",
        forums::get_forum_by_slug
    ));
    toplevel.spawn(handler!(forum_mgmt, "get_by_id", forums::get_forum_by_id));
    toplevel.spawn(handler!(forum_mgmt, "list", forums::list_forums));
    toplevel.spawn(handler!(forum_mgmt, "update", forums::update_forum));
    toplevel.spawn(handler!(
        forum_mgmt,
        "get_personal",
        forums::get_personal_forum
    ));
    toplevel.spawn(handler!(
        forum_mgmt,
        "get_for_community",
        forums::get_community_forum
    ));

    // Forum category management endpoints
    let categories = service.group("forum.categories");
    toplevel.spawn(handler!(
        categories,
        "create",
        categories::create_forum_category
    ));
    toplevel.spawn(handler!(
        categories,
        "list",
        categories::get_forum_categories
    ));
    toplevel.spawn(handler!(
        categories,
        "update",
        categories::update_forum_category
    ));
    toplevel.spawn(handler!(
        categories,
        "delete",
        categories::delete_forum_category
    ));

    // Messaging endpoints (personal forum-based)
    let messaging = service.group("forum.messaging");
    toplevel.spawn(handler!(messaging, "send", messaging::send_message));
    toplevel.spawn(handler!(messaging, "reply", messaging::reply_to_message));
    toplevel.spawn(handler!(messaging, "inbox", messaging::inbox));
    toplevel.spawn(recv_handler!(messaging, "read", messaging::mark_read));
    toplevel.spawn(recv_handler!(messaging, "discard", messaging::discard));
    toplevel.spawn(handler!(messaging, "unread_count", messaging::unread_count));
    toplevel.spawn(handler!(
        messaging,
        "recipients_since",
        messaging::recipients_since
    ));
    toplevel.spawn(handler!(messaging, "pending_list", messaging::pending_list));
    toplevel.spawn(handler!(messaging, "review", messaging::review_message));

    // Content filtering endpoints
    let filter = service.group("forum.filter");
    toplevel.spawn(handler!(filter, "check", content_filter::check_content));
    toplevel.spawn(recv_handler!(
        filter,
        "add_bad_word",
        content_filter::add_bad_word
    ));
    toplevel.spawn(recv_handler!(
        filter,
        "remove_bad_word",
        content_filter::remove_bad_word
    ));
    toplevel.spawn(handler!(filter, "reload", content_filter::reload_filter));

    // Health check endpoint
    let health = service.group("forum.health");
    toplevel.spawn(handler!(health, "check", health::health_check));

    // Background sync task
    toplevel.spawn(sync::synchronize_messages());

    // Set up graceful shutdown
    use tokio::signal::unix::{SignalKind, signal};

    let mut sigint = signal(SignalKind::interrupt()).expect("Failed to create SIGINT handler");
    let mut sigterm = signal(SignalKind::terminate()).expect("Failed to create SIGTERM handler");

    tokio::select! {
        _ = toplevel.join_all() => {
            println!("All service handlers completed");
        }
        _ = sigint.recv() => {
            println!("Received SIGINT, shutting down forum service gracefully...");
        }
        _ = sigterm.recv() => {
            println!("Received SIGTERM, shutting down forum service gracefully...");
        }
    }

    println!("Forum service shutdown complete");
}
