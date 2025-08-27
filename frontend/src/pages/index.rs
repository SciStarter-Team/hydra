use common::message::location::LocationInfo;
use poem::{Response, handler, http::HeaderMap, session::Session, web::RealIp};

use crate::{
    component::{Component, Footer, Head, Header},
    middleware::authorization::Authenticated,
};

#[derive(sailfish::TemplateOnce)]
#[template(path = "pages/index.stpl.html")]
struct IndexPage {
    head: <Head as Component>::Prepared,
    header: <Header as Component>::Prepared,
    footer: <Footer as Component>::Prepared,
}

#[handler]
pub async fn index_page(
    _session: &Session,
    _headers: &HeaderMap,
    RealIp(client_ip): RealIp,
    Authenticated(user_info): Authenticated,
) -> Result<Response, anyhow::Error> {
    let _location = if let Some(addr) = client_ip {
        common::service::request("location.ip", addr.to_string())
            .await
            .unwrap_or_default()
    } else {
        LocationInfo::default()
    };

    IndexPage {
        head: Head::default().prepare(None).await?,
        header: Header::default().prepare(user_info.clone()).await?,
        footer: Footer::default().prepare(user_info).await?,
    }
    .render(None)
    .await
}
