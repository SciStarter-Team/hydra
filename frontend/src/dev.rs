use crate::{component::*, middleware::authorization::Authenticated};
use common::message::user::BasicInfo;
use poem::{Endpoint, EndpointExt, Route, handler, web::headers::Header};
use sailfish::TemplateOnce;

use crate::{component::header::HeaderTpl, middleware::authorization::StaffOnly};

pub fn routes() -> impl Endpoint {
    Route::new()
        .at("/background-processing", background_processing)
        .with(StaffOnly)
}

#[handler]
pub async fn background_processing(
    Authenticated(basic_info): Authenticated,
) -> poem::Result<poem::Response> {
    todo!()
}

pub struct BackgroundProcessing {}

impl Component for BackgroundProcessing {
    type Param<'l> = Option<&'l BasicInfo>;
    type Prepared = BackgroundProcessingTpl;

    async fn prepare<'l>(self, param: Self::Param<'l>) -> Result<Self::Prepared, anyhow::Error> {
        let header = Header::default().prepare(param).await?;
        Ok(BackgroundProcessingTpl { header, footer })
    }
}

#[derive(TemplateOnce)]
#[template(path = "dev/background_processing.stpl.html")]
struct BackgroundProcessingTpl {
    header: HeaderTpl,
    footer: FooterTpl,
}
