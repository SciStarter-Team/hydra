use common::message::{location::LocationInfo, user::BasicInfo, user::CommunityInfo};
use poem::{Response, handler, http::HeaderMap, session::Session, web::RealIp};
use std::sync::Arc;

use crate::{
    component::{Component, Head, StandardLayout},
    middleware::authorization::Authenticated,
};

#[derive(sailfish::TemplateOnce)]
#[template(path = "pages/index.stpl.html")]
struct IndexPageLoggedIn {
    head: <Head as Component>::Prepared,
    layout: <StandardLayout<IndexPageContentLoggedIn> as Component>::Prepared,
}

#[derive(sailfish::TemplateOnce)]
#[template(path = "pages/index_content_logged_in.stpl.html")]
struct IndexPageContentLoggedIn {
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

    if user_info.is_some() {
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

        let news_items = crate::news::NewsItem::get_rendered_active_items().await;

        let layout = StandardLayout::new(
            "/",
            IndexPageContentLoggedIn {
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
