use poem::{Endpoint, EndpointExt, Route};

use super::handlers::*;
use crate::middleware::session_cookie::EnablePartitionedCookies;

pub fn routes() -> impl Endpoint {
    Route::new()
        .at("/:exchange", poem::get(exchange_home))
        .at("/:exchange/", poem::get(exchange_home))
        .at("/:exchange/index.html", poem::get(exchange_home))
        .at("/:exchange/results", poem::get(exchange_results))
        .at("/:exchange/projects.csv", poem::get(exchange_csv))
        .at("/:exchange/org.html", poem::get(exchange_admin))
        .at("/:exchange/data.html", poem::get(exchange_data))
        .at("/:exchange/engagement", poem::get(exchange_engagement))
        .at("/:exchange/settings", poem::post(exchange_update_settings))
        .at("/:exchange/predef", poem::get(get_predefined_search_form))
        .at(
            "/:exchange/predefined-search",
            poem::post(create_predefined_search),
        )
        .at(
            "/:exchange/predefined-search/:id",
            poem::delete(delete_predefined_search),
        )
        .at("/:exchange/staff", poem::post(add_exchange_staff))
        .at(
            "/:exchange/staff/:user_id",
            poem::delete(remove_exchange_staff),
        )
        .at(
            "/:exchange/projects/:project_id/approve",
            poem::post(approve_exchange_project),
        )
        .at(
            "/:exchange/projects/:project_id/reject",
            poem::delete(reject_exchange_project),
        )
        .at(
            "/:exchange/:project_slug/project.html",
            poem::get(exchange_project),
        )
        .at("/:exchange/:project_slug/save", poem::post(save_project))
        .at("/:exchange/:project_slug/vote", poem::post(vote_project))
        .at("/:exchange/:project_slug/hide", poem::post(hide_project))
        // Mark these routes as needing partitioned cookies for iframe embedding
        .with(EnablePartitionedCookies)
}
