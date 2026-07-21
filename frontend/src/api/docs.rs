use crate::component::{Component, Head, StandardLayout};
use crate::middleware::authorization::Authenticated;
use anyhow::Error;
use poem::{Response, handler};
use sailfish::TemplateOnce;

/// The public API documentation page.
/// GET /api
#[handler]
pub async fn api_docs_page(Authenticated(user_info): Authenticated) -> Result<Response, Error> {
    let layout = StandardLayout::new("/api", ApiDocsContent {})
        .prepare(user_info.clone())
        .await?;

    let page = ApiDocsPage {
        head: Head::default().prepare(None).await?,
        layout,
    };

    Ok(page.into_response(None).await?)
}

#[derive(TemplateOnce)]
#[template(path = "api/api_docs.stpl.html")]
struct ApiDocsPage {
    head: <Head as Component>::Prepared,
    layout: <StandardLayout<ApiDocsContent> as Component>::Prepared,
}

#[derive(TemplateOnce)]
#[template(path = "api/api_docs_content.stpl.html")]
struct ApiDocsContent {}
