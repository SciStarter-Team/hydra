use std::sync::Arc;

use common::message::user::{BasicInfo, UserInfo};

use super::Component;

pub struct Sidebar {
    path: String,
}

impl Sidebar {
    pub fn new(path: impl Into<String>) -> Self {
        Sidebar { path: path.into() }
    }
}

impl Component for Sidebar {
    type Param = Option<Arc<BasicInfo>>;

    type Prepared = SidebarPrepared;

    async fn prepare(self, param: Self::Param) -> Result<Self::Prepared, anyhow::Error> {
        Ok(SidebarPrepared {
            basic_info: param,
            path: self.path,
        })
    }
}

#[derive(sailfish::TemplateOnce)]
#[template(path = "site/sidebar.stpl.html")]
pub struct SidebarPrepared {
    basic_info: Option<Arc<BasicInfo>>,
    path: String,
}
