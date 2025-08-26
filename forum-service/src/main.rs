use async_nats::service::ServiceExt;
use common::{
    handler, recv_handler,
    shared::{db, nats},
};
use futures::StreamExt;
use tokio::task::JoinSet;

pub mod categories;
pub mod content_filter;
pub mod forums;
pub mod moderation;
pub mod posts;
pub mod topics;

#[tokio::main]
async fn main() {
    common::shared::init("forum").await;

    sqlx::migrate!("./migrations")
        .run(db())
        .await
        .expect("Unable to run migrations");

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

    // Forum category management endpoints
    let categories = service.group("forum.categories");
    toplevel.spawn(handler!(categories, "create", categories::create_forum_category));
    toplevel.spawn(handler!(categories, "list", categories::get_forum_categories));
    toplevel.spawn(handler!(categories, "update", categories::update_forum_category));
    toplevel.spawn(handler!(categories, "delete", categories::delete_forum_category));

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

    toplevel.join_all().await;
}
