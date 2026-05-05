use std::sync::Arc;

use common::message::forum::{UnreadCountRequest, UnreadCountResponse};
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
        let unread_count = match param.as_deref() {
            Some(info) => {
                let result: Result<UnreadCountResponse, _> = common::service::request(
                    "forum.messaging.unread_count",
                    UnreadCountRequest {
                        user_id: info.user_id,
                    },
                )
                .await;
                result.map(|resp| resp.count).unwrap_or(0)
            }
            None => 0,
        };

        Ok(SidebarPrepared {
            basic_info: param,
            path: self.path,
            unread_count,
        })
    }
}

#[derive(sailfish::TemplateOnce)]
#[template(path = "site/sidebar.stpl.html")]
pub struct SidebarPrepared {
    basic_info: Option<Arc<BasicInfo>>,
    path: String,
    unread_count: i64,
}
