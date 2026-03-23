use std::sync::Arc;

use common::message::user::BasicInfo;
use sailfish::TemplateOnce;

use super::Component;

#[derive(Debug, Default)]
pub struct Header {
    current_uri: String,
}

impl Header {
    pub fn with_current_uri(mut self, uri: impl Into<String>) -> Self {
        self.current_uri = uri.into();
        self
    }
}

impl Component for Header {
    type Param = Option<Arc<BasicInfo>>;
    type Prepared = HeaderTpl;

    #[tracing::instrument]
    async fn prepare(self, param: Self::Param) -> Result<Self::Prepared, anyhow::Error> {
        let login_url = if self.current_uri.is_empty() {
            "/login".to_string()
        } else {
            format!("/login?next={}", urlencoding::encode(&self.current_uri))
        };

        Ok(HeaderTpl {
            basic_info: param.clone(),
            login_url,
        })
    }
}

#[derive(Debug, TemplateOnce)]
#[template(path = "site/header.stpl.html")]
pub struct HeaderTpl {
    basic_info: Option<Arc<BasicInfo>>,
    login_url: String,
}
