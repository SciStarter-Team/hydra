use common::message::{
    location::LocationInfo,
    user::BasicInfo,
    user::CommunityInfo,
};
use poem::{Response, handler, http::HeaderMap, session::Session, web::RealIp};
use std::sync::Arc;

use crate::{
    component::{Component, Head, StandardLayout},
    middleware::authorization::Authenticated,
};

#[derive(sailfish::TemplateOnce)]
#[template(path = "pages/index.stpl.html")]
struct IndexPage {
    head: <Head as Component>::Prepared,
    layout: <StandardLayout<IndexPageContent> as Component>::Prepared,
}

#[derive(sailfish::TemplateOnce)]
#[template(path = "pages/index_content.stpl.html")]
struct IndexPageContent {
    #[allow(dead_code)]
    user_info: Option<Arc<BasicInfo>>,
    featured_items: Vec<crate::schedule::FeaturedItem>,
    news_items: Vec<crate::news::RenderedNewsItem>,
    featured_communities: Vec<CommunityInfo>,
    location_lat: String,
    location_lng: String,
    location_display: String,
    geosearch_key: String,
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

#[handler]
#[tracing::instrument]
pub async fn index_page(
    _session: &Session,
    _headers: &HeaderMap,
    RealIp(client_ip): RealIp,
    Authenticated(user_info): Authenticated,
) -> Result<Response, anyhow::Error> {
    let location = if let Some(addr) = client_ip {
        common::service::request("location.ip", addr.to_string())
            .await
            .unwrap_or_default()
    } else {
        LocationInfo::default()
    };

    let location_display = location_display_from_ip(&location);
    let location_lat = location.latitude.clone();
    let location_lng = location.longitude.clone();

    let featured_items = crate::schedule::Schedule::get_current_featured_items()
        .await
        .unwrap_or_default();

    let news_items = crate::news::NewsItem::get_rendered_active_items().await;

    let featured_communities = crate::communities::list_communities(true, true)
        .await
        .unwrap_or_default();

    let layout = StandardLayout::new(
        "/",
        IndexPageContent {
            user_info: user_info.clone(),
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

    IndexPage {
        head: Head::default().with_slick().with_geosearch().prepare(None).await?,
        layout,
    }
    .into_response(None)
    .await
}

// Temporarily needed until https://github.com/poem-web/poem/issues/1098 is fixed
#[tracing::instrument]
pub async fn index_page_pseudo_handler(
    client_ip: Option<std::net::IpAddr>,
    user_info: Option<Arc<BasicInfo>>,
) -> Result<Response, anyhow::Error> {
    let location = if let Some(addr) = client_ip {
        common::service::request("location.ip", addr.to_string())
            .await
            .unwrap_or_default()
    } else {
        LocationInfo::default()
    };

    let location_display = location_display_from_ip(&location);
    let location_lat = location.latitude.clone();
    let location_lng = location.longitude.clone();

    let featured_items = crate::schedule::Schedule::get_current_featured_items()
        .await
        .unwrap_or_default();

    let news_items = crate::news::NewsItem::get_rendered_active_items().await;

    let featured_communities = crate::communities::list_communities(true, true)
        .await
        .unwrap_or_default();

    let layout = StandardLayout::new(
        "/",
        IndexPageContent {
            user_info: user_info.clone(),
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

    IndexPage {
        head: Head::default().with_slick().with_geosearch().prepare(None).await?,
        layout,
    }
    .into_response(None)
    .await
}
