use std::sync::Arc;

use common::message::user::BasicInfo;
use sailfish::TemplateOnce;

use super::Component;

#[derive(Debug, Default)]
pub struct Footer;

impl Component for Footer {
    type Param = Option<Arc<BasicInfo>>;
    type Prepared = FooterTpl;

    #[tracing::instrument]
    async fn prepare(self, param: Self::Param) -> Result<Self::Prepared, anyhow::Error> {
        Ok(FooterTpl {
            basic_info: param.clone(),
        })
    }
}

#[derive(TemplateOnce)]
#[template(path = "site/footer.stpl.html")]
#[allow(dead_code)]
pub struct FooterTpl {
    basic_info: Option<Arc<BasicInfo>>,
}
