use std::sync::Arc;

use poem::{Endpoint, EndpointExt, Route, handler};
use sailfish::TemplateOnce;

use crate::middleware::authorization::StaffOnly;
use crate::{
    component::{Component, Footer, Head, Header},
    middleware::authorization::Authenticated,
};
use common::message::user::BasicInfo;

pub fn routes() -> impl Endpoint {
    Route::new()
        .at("/background-processing", background_processing)
        .with(StaffOnly)
}

#[handler]
#[tracing::instrument]
pub async fn background_processing(
    Authenticated(basic_info): Authenticated,
) -> poem::Result<poem::Response> {
    Ok(BackgroundProcessing.into_response(basic_info).await?)
}

#[derive(Debug)]
pub struct BackgroundProcessing;

impl Component for BackgroundProcessing {
    type Param = Option<Arc<BasicInfo>>;
    type Prepared = BackgroundProcessingTpl;

    #[tracing::instrument]
    async fn prepare(self, param: Self::Param) -> Result<Self::Prepared, anyhow::Error> {
        let header = Header::default().prepare(param.clone()).await?;
        let footer = Footer::default().prepare(param).await?;
        Ok(BackgroundProcessingTpl { header, footer })
    }
}

#[derive(TemplateOnce)]
#[template(path = "dev/background_processing.stpl.html")]
pub struct BackgroundProcessingTpl {
    header: <Header as Component>::Prepared,
    footer: <Footer as Component>::Prepared,
}
