use std::sync::Arc;

use common::message::user::BasicInfo;
use sailfish::TemplateOnce;

use super::Component;

#[derive(Default)]
pub struct Footer;

impl Component for Footer {
    type Param = Option<Arc<BasicInfo>>;
    type Prepared = FooterTpl;

    async fn prepare(self, param: Self::Param) -> Result<Self::Prepared, anyhow::Error> {
        Ok(FooterTpl {
            basic_info: param.clone(),
        })
    }
}

#[derive(TemplateOnce)]
#[template(path = "site/footer.stpl.html")]
pub struct FooterTpl {
    basic_info: Option<Arc<BasicInfo>>,
}
