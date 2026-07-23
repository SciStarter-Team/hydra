use common::message::{location::LocationInfo, user::BasicInfo, user::CommunityInfo};
use poem::{Response, handler, http::HeaderMap, session::Session, web::RealIp};
use std::sync::Arc;

use crate::{
    component::{Component, Head, StandardLayout},
    concierge::ConciergeSection,
    home::{HomeSettings, UserState, WelcomeMessage},
    middleware::authorization::Authenticated,
    search::FinderResult,
};

/// The "projects & events near you" section shown above the featured list.
struct NearbySection {
    heading: String,
    items: Vec<FinderResult>,
    see_all_url: String,
}

#[derive(sailfish::TemplateOnce)]
#[template(path = "pages/index.stpl.html")]
struct IndexPageLoggedIn {
    head: <Head as Component>::Prepared,
    layout: <StandardLayout<IndexPageContentLoggedIn> as Component>::Prepared,
}

#[derive(sailfish::TemplateOnce)]
#[template(path = "pages/index_content_logged_in.stpl.html")]
struct IndexPageContentLoggedIn {
    welcome: Option<WelcomeMessage>,
    concierge: Option<ConciergeSection>,
    nearby: Option<NearbySection>,
    featured_items: Vec<crate::schedule::FeaturedItem>,
    news_items: Vec<crate::news::RenderedNewsItem>,
    featured_communities: Vec<CommunityInfo>,
    location_lat: String,
    location_lng: String,
    location_display: String,
    geosearch_key: String,
}

#[derive(sailfish::TemplateOnce)]
#[template(path = "pages/index.stpl.html")]
struct IndexPageLoggedOut {
    head: <Head as Component>::Prepared,
    layout: <StandardLayout<IndexPageContentLoggedOut> as Component>::Prepared,
}

#[derive(sailfish::TemplateOnce)]
#[template(path = "pages/index_content_logged_out.stpl.html")]
struct IndexPageContentLoggedOut {
    concierge: Option<ConciergeSection>,
    nearby: Option<NearbySection>,
    featured_items: Vec<crate::schedule::FeaturedItem>,
    featured_communities: Vec<CommunityInfo>,
}

fn location_display_from_ip(location: &LocationInfo) -> String {
    if !location.city_name.is_empty() && !location.region_name.is_empty() {
        format!("{}, {}", location.city_name, location.region_name)
    } else if !location.city_name.is_empty() {
        location.city_name.clone()
    } else {
        location.region_name.clone()
    }
}

/// Resolve the viewer's approximate location from their IP, defaulting to an
/// empty `LocationInfo` when the IP is unknown or the lookup fails.
async fn resolve_location(client_ip: Option<std::net::IpAddr>) -> LocationInfo {
    if let Some(addr) = client_ip {
        common::service::request("location.ip", addr.to_string())
            .await
            .unwrap_or_default()
    } else {
        LocationInfo::default()
    }
}

/// A usable viewer location, or `None` when unknown. `LocationInfo::default()`
/// carries `(0.0, 0.0)` as its "no location" sentinel, so null island is
/// treated as unknown rather than a real position.
fn resolved_point(location: &LocationInfo) -> Option<common::message::location::Point> {
    let point = location.as_point()?;
    if point.latitude == 0.0 && point.longitude == 0.0 {
        None
    } else {
        Some(point)
    }
}

/// Build the "near you" section for the home page, or `None` when it is
/// disabled, the viewer's location is unknown, or nothing nearby was found.
async fn build_nearby_section(
    location: &LocationInfo,
    settings: &HomeSettings,
) -> Option<NearbySection> {
    if !settings.show_nearby {
        return None;
    }

    let point = resolved_point(location)?;
    let items =
        crate::search::nearby_entities(point.latitude, point.longitude, settings.nearby_limit)
            .await
            .ok()?;
    if items.is_empty() {
        return None;
    }

    // Hand the same coordinates to the finder so "see all" continues the search.
    let location_name = location_display_from_ip(location);
    let see_all_url = format!(
        "/finder?lat={}&lng={}&location_name={}",
        point.latitude,
        point.longitude,
        urlencoding::encode(&location_name),
    );

    Some(NearbySection {
        heading: settings.nearby_heading.clone(),
        items,
        see_all_url,
    })
}

#[handler]
#[tracing::instrument]
pub async fn index_page(
    _session: &Session,
    _headers: &HeaderMap,
    RealIp(client_ip): RealIp,
    Authenticated(user_info): Authenticated,
) -> Result<Response, anyhow::Error> {
    render_index_page(client_ip, user_info).await
}

// Temporarily needed until https://github.com/poem-web/poem/issues/1098 is fixed
#[tracing::instrument]
pub async fn index_page_pseudo_handler(
    client_ip: Option<std::net::IpAddr>,
    user_info: Option<Arc<BasicInfo>>,
) -> Result<Response, anyhow::Error> {
    render_index_page(client_ip, user_info).await
}

async fn render_index_page(
    client_ip: Option<std::net::IpAddr>,
    user_info: Option<Arc<BasicInfo>>,
) -> Result<Response, anyhow::Error> {
    let featured_items = crate::schedule::Schedule::get_current_featured_items()
        .await
        .unwrap_or_default();

    let featured_communities = crate::communities::list_communities(true, true)
        .await
        .unwrap_or_default();

    let settings = HomeSettings::current().await;

    // Location (and therefore the "near you" list) is resolved for everyone.
    let location = resolve_location(client_ip).await;
    let nearby = build_nearby_section(&location, &settings).await;
    let concierge = crate::concierge::section_for(&settings, user_info.as_deref()).await;

    if let Some(user) = user_info.clone() {
        let location_display = location_display_from_ip(&location);
        let location_lat = location.latitude.clone();
        let location_lng = location.longitude.clone();

        // Resolve the personalized welcome message from the decision tree.
        let location_known = resolved_point(&location).is_some();
        let state = UserState::gather(&user, location_known, &settings).await;
        let resolved = settings.resolve_welcome(&state).personalized(&user.handle);
        let welcome = (!resolved.is_empty()).then_some(resolved);

        let news_items = crate::news::NewsItem::get_rendered_active_items().await;

        let layout = StandardLayout::new(
            "/",
            IndexPageContentLoggedIn {
                welcome,
                concierge,
                nearby,
                featured_items,
                news_items,
                featured_communities,
                location_lat,
                location_lng,
                location_display,
                geosearch_key: env!("GEOSEARCH_KEY").to_string(),
            },
        )
        .prepare(user_info.clone())
        .await?;

        IndexPageLoggedIn {
            head: Head::default()
                .with_slick()
                .with_geosearch()
                .prepare(None)
                .await?,
            layout,
        }
        .into_response(None)
        .await
    } else {
        let layout = StandardLayout::new(
            "/",
            IndexPageContentLoggedOut {
                concierge,
                nearby,
                featured_items,
                featured_communities,
            },
        )
        .prepare(user_info.clone())
        .await?;

        IndexPageLoggedOut {
            head: Head::default().with_slick().prepare(None).await?,
            layout,
        }
        .into_response(None)
        .await
    }
}
