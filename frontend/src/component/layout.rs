use std::sync::Arc;

use common::message::user::BasicInfo;

use crate::component::{Component, Footer, Header, Sidebar};

pub struct StandardLayout<C> {
    collapsed: bool,
    content: C,
}

impl<T> StandardLayout<T>
where
    T: sailfish::TemplateOnce,
{
    pub fn new(content: T) -> Self {
        StandardLayout {
            collapsed: false,
            content,
        }
    }

    pub fn collapse(mut self) -> Self {
        self.collapsed = true;
        self
    }
}

impl<T> Component for StandardLayout<T>
where
    T: sailfish::TemplateOnce,
{
    type Param = Option<Arc<BasicInfo>>;
    type Prepared = StandardLayoutPrepared<T>;

    async fn prepare(self, user: Self::Param) -> Result<Self::Prepared, anyhow::Error> {
        let header = Header::default().prepare(user.clone()).await?;
        let sidebar = Sidebar::default().prepare(user.clone()).await?;
        let footer = Footer::default().prepare(user.clone()).await?;

        Ok(StandardLayoutPrepared {
            user,
            header,
            sidebar,
            footer,
            content: self.content,
            collapsed: self.collapsed,
        })
    }
}

#[derive(sailfish::TemplateOnce)]
#[template(path = "site/standard_layout.stpl.html")]
pub struct StandardLayoutPrepared<T: sailfish::TemplateOnce> {
    user: Option<Arc<BasicInfo>>,
    header: <Header as Component>::Prepared,
    sidebar: <Sidebar as Component>::Prepared,
    footer: <Footer as Component>::Prepared,
    content: T,
    collapsed: bool,
}
