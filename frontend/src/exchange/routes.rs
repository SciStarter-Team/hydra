use poem::{Endpoint, EndpointExt, Route};

use super::handlers::*;
use crate::middleware::partitioned_session::PartitionedSessionMiddleware;

pub fn routes() -> impl Endpoint {
    Route::new()
        .at("/:exchange", poem::get(exchange_home))
        .at("/:exchange/", poem::get(exchange_home))
        .at("/:exchange/index.html", poem::get(exchange_home))
        .at("/:exchange/results", poem::get(exchange_results))
        .at("/:exchange/projects.csv", poem::get(exchange_csv))
        .at("/:exchange/org.html", poem::get(exchange_admin))
        .at("/:exchange/settings", poem::post(exchange_update_settings))
        .at(
            "/:exchange/predefined-search",
            poem::post(create_predefined_search),
        )
        .at(
            "/:exchange/predefined-search/:id",
            poem::delete(delete_predefined_search),
        )
        .at(
            "/:exchange/:project_slug/project.html",
            poem::get(exchange_project),
        )
        .at("/:exchange/:project_slug/save", poem::post(save_project))
        .at("/:exchange/:project_slug/vote", poem::post(vote_project))
        .at("/:exchange/:project_slug/hide", poem::post(hide_project))
        // Apply partitioned session middleware to enable iframe embedding
        // with partitioned cookies (works in Chrome/Edge/Firefox, fallback for Safari)
        .with(PartitionedSessionMiddleware)
}
