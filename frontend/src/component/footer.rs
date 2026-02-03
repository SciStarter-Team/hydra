use std::sync::Arc;

use common::message::user::BasicInfo;
use sailfish::TemplateOnce;

use super::Component;

#[derive(Debug)]
pub struct Footer {
    enable_donate: bool,
    year: i16,
}

impl Default for Footer {
    fn default() -> Self {
        Self {
            enable_donate: true,
            year: jiff::Zoned::now().year(),
        }
    }
}

impl Footer {
    pub fn without_donate(mut self) -> Self {
        self.enable_donate = false;
        self
    }

    pub fn config(self, path: &str) -> Self {
        if path.to_ascii_lowercase().contains("nasa") {
            self.without_donate()
        } else {
            self
        }
    }
}

impl Component for Footer {
    type Param = Option<Arc<BasicInfo>>;
    type Prepared = FooterTpl;

    #[tracing::instrument]
    async fn prepare(self, param: Self::Param) -> Result<Self::Prepared, anyhow::Error> {
        Ok(FooterTpl {
            basic_info: param.clone(),
            enable_donate: self.enable_donate,
            year: self.year,
        })
    }
}

#[derive(Debug, TemplateOnce)]
#[template(path = "site/footer.stpl.html")]
#[allow(dead_code)]
pub struct FooterTpl {
    basic_info: Option<Arc<BasicInfo>>,
    enable_donate: bool,
    year: i16,
}
