use std::sync::Arc;

use common::message::user::BasicInfo;

use super::Component;

#[derive(Default)]
pub struct Sidebar;

impl Component for Sidebar {
    type Param = Option<Arc<BasicInfo>>;

    type Prepared = SidebarPrepared;

    async fn prepare(self, _param: Self::Param) -> Result<Self::Prepared, anyhow::Error> {
        Ok(SidebarPrepared {})
    }
}

#[derive(sailfish::TemplateOnce)]
#[template(path = "site/sidebar.stpl.html")]
pub struct SidebarPrepared {}
