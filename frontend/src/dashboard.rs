pub mod bookmarks;
pub mod entity_summary;
pub mod interests;
pub mod lists;
pub mod main;
pub mod messages;
pub mod owned;
pub mod places;
pub mod profile;
pub mod settings;

use poem::{Endpoint, Route, get, post};

pub fn routes() -> impl Endpoint {
    Route::new()
        .at("/", get(main::page))
        .at("/owned", get(owned::page))
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
        .at("/messages/send", post(messages::send))
        .at("/messages/:topic_id", get(messages::thread))
        .at("/messages/:topic_id/reply", post(messages::reply))
        .at("/messages/:topic_id/discard", post(messages::discard))
        .at("/profile", get(profile::page))
        .at("/interests", get(interests::page).post(interests::save))
        .at("/places", get(places::page).post(places::add))
        .at("/places/:place_id/delete", post(places::delete))
        .at("/settings", get(settings::page))
        .at("/settings/general", post(settings::save_general))
        .at("/settings/ambassador", post(settings::save_ambassador))
}
