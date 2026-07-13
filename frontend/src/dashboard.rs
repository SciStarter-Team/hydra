pub mod bookmarks;
pub mod entity_messages;
pub mod entity_summary;
pub mod interests;
pub mod lists;
pub mod main;
pub mod messages;
pub mod owned;
pub mod places;
pub mod profile;
pub mod settings;

use std::time::Duration;

use poem::{Endpoint, EndpointExt, Route, get, post};

use crate::middleware::rate_limit::{RateLimitConfig, RateLimitMiddleware};

pub fn routes() -> impl Endpoint {
    // Per-IP write limits for direct messaging — the legitimate way to put a
    // topic in someone else's inbox. Conservative to deter mass DM spam.
    let send_limit = RateLimitConfig::new(10, Duration::from_secs(300))
        .with_error_message("Too many messages. Please wait a few minutes before sending again.");
    let reply_limit = RateLimitConfig::new(20, Duration::from_secs(300))
        .with_error_message("Too many replies. Please wait a few minutes before replying again.");

    Route::new()
        .at("/", get(main::page))
        .at("/owned", get(owned::page))
        .at(
            "/owned/:entity_id/messages",
            get(entity_messages::inbox),
        )
        .at(
            "/owned/:entity_id/messages/:topic_id",
            get(entity_messages::thread),
        )
        .at(
            "/owned/:entity_id/messages/:topic_id/reply",
            post(entity_messages::reply),
        )
        .at("/lists", get(lists::page).post(lists::create))
        .at("/lists/:list_id/update", post(lists::update))
        .at("/lists/:list_id/delete", post(lists::delete))
        .at("/lists/:list_id/entries", post(lists::add_entry))
        .at(
            "/lists/:list_id/entries/:entry_id/remove",
            post(lists::remove_entry),
        )
        .at(
            "/lists/:list_id/entries/:entry_id/note",
            post(lists::set_entry_note),
        )
        .at("/lists/:list_id/share", post(lists::share))
        .at("/lists/:list_id/unshare", post(lists::unshare))
        .at("/bookmarks", get(bookmarks::page))
        .at("/messages", get(messages::page))
        .at("/messages/compose", get(messages::compose))
        .at(
            "/messages/send",
            post(messages::send).with(RateLimitMiddleware::new(send_limit)),
        )
        .at("/messages/:topic_id", get(messages::thread))
        .at(
            "/messages/:topic_id/reply",
            post(messages::reply).with(RateLimitMiddleware::new(reply_limit)),
        )
        .at("/messages/:topic_id/discard", post(messages::discard))
        .at("/profile", get(profile::page))
        .at("/interests", get(interests::page).post(interests::save))
        .at("/places", get(places::page).post(places::add))
        .at("/places/:place_id/delete", post(places::delete))
        .at("/settings", get(settings::page))
        .at("/settings/general", post(settings::save_general))
        .at("/settings/ambassador", post(settings::save_ambassador))
}
