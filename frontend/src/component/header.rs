use std::sync::Arc;

use common::message::user::BasicInfo;
use sailfish::TemplateOnce;

use super::Component;

#[derive(Debug, Default)]
pub struct Header;

impl Component for Header {
    type Param = Option<Arc<BasicInfo>>;
    type Prepared = HeaderTpl;

    #[tracing::instrument]
    async fn prepare(self, param: Self::Param) -> Result<Self::Prepared, anyhow::Error> {
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
