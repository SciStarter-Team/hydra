use std::sync::Arc;

use common::message::user::BasicInfo;
use sailfish::TemplateOnce;

use super::Component;

pub struct Header;

impl Component for Header {
    type Param<'l> = Option<Arc<BasicInfo>>;
    type Prepared = HeaderTpl;

    async fn prepare<'l>(self, param: Self::Param<'l>) -> Result<Self::Prepared, anyhow::Error> {
        Ok(HeaderTpl {
            basic_info: param.clone(),
        })
    }
}

#[derive(TemplateOnce)]
#[template(path = "site/header.stpl.html")]
pub struct HeaderTpl {
    basic_info: Option<Arc<BasicInfo>>,
}
