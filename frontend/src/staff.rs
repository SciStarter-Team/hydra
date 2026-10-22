//! Staff-only reports ported from orrery's `/staff/` endpoints.
//!
//! orrery serves this tree from two places at once — Django views under
//! `orrery/staff/` and an Axum router under `src/staff/` — but they share one
//! URL prefix and one audience, so hydra gathers them into a single module and
//! keeps orrery's paths, which is what staff bookmarks and internal links use.

mod acts_connected;
mod affiliates;
mod ambassadors;
mod claim_links;
mod compare_emails;
mod curated_report;
mod email_report;
mod events;
mod foundations;
mod google;
pub mod import_review;
mod interested;
mod masquerade;
mod moore;
mod project_locations;
mod projectmap;
mod referrals;
mod report_range;
mod reports;
mod solar_system;
mod usermap;

/// All `/staff/` endpoints, gated as a whole by `StaffOnly` (mirrors how the
/// `/-admin/` tree is protected) — so every staff route, current and future, is
/// covered by one middleware rather than per-route guards.
pub fn routes() -> impl poem::Endpoint {
    use crate::middleware::authorization::StaffOnly;
    use poem::{EndpointExt, Route, get};

    Route::new()
        .at(
            "/acts-of-science-connected-signups",
            get(acts_connected::signups),
        )
        .at("/affiliate-activity", get(affiliates::affiliate_activity))
        .at(
            "/ax4gr27",
            get(masquerade::masquerade_form).post(masquerade::masquerade),
        )
        .at(
            "/published-affiliates",
            get(affiliates::published_affiliates),
        )
        .at("/verizon-activity", get(affiliates::verizon_activity))
        .at(
            "/claim-links",
            get(claim_links::pages_form).post(claim_links::look_up),
        )
        .at("/claim-links/generate", poem::post(claim_links::generate))
        .at(
            "/compare-emails",
            get(compare_emails::form).post(compare_emails::compare_emails),
        )
        .at("/events/events.html", get(events::form))
        .at("/events/events.csv", get(events::export_csv))
        .at(
            "/ambassadors/ambassador_tutorial.csv",
            get(ambassadors::tutorial),
        )
        .at(
            "/ambassadors/ambassador_application.csv",
            get(ambassadors::application),
        )
        .at(
            "/ambassadors/ambassador_verified.csv",
            get(ambassadors::verified),
        )
        .at(
            "/ambassadors/ambassador_certified.csv",
            get(ambassadors::certified),
        )
        .at(
            "/ambassadors/champions_not_yet_ambassadors.csv",
            get(ambassadors::champions),
        )
        .at("/curated-report/:slug", get(curated_report::report))
        .at("/email-report", get(email_report::email_report))
        .at(
            "/google-authorize",
            get(google::authorize_form).post(google::authorize),
        )
        .at("/google-authorized", get(google::authorized))
        .at("/import-review", get(import_review::index))
        .at(
            "/import-review/origin",
            get(import_review::review_origin).post(import_review::approve),
        )
        .at(
            "/interested",
            get(interested::slug_form).post(interested::interested),
        )
        .at("/moore", get(moore::index))
        .at("/moore/data", get(moore::data))
        .at("/moore/export.csv", get(moore::export_csv))
        .at("/signup-referrals", get(referrals::index))
        .at("/signup-referrals/data", get(referrals::data))
        .at("/foundations-completions", get(foundations::index))
        .at("/foundations-completions/data", get(foundations::data))
        .at(
            "/project-locations",
            get(project_locations::project_locations),
        )
        .at("/projectmap/map.html", get(projectmap::map_html))
        .at(
            "/projectmap/projects.geojson",
            get(projectmap::projects_geojson),
        )
        .at(
            "/solar-system-ambassadors",
            get(solar_system::solar_system_ambassadors),
        )
        .at("/usermap/map.html", get(usermap::map_html))
        .at("/usermap/users.geojson", get(usermap::users_geojson))
        .at("/site-map", get(reports::site_map))
        .at("/training-analytics", get(reports::training_analytics))
        .with(StaffOnly)
}

/// Quote one CSV field, doubling any embedded quotes, per RFC 4180.
pub(crate) fn csv_field(value: &str) -> String {
    if value.contains(['"', ',', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}
