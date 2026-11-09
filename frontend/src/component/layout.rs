use std::sync::Arc;

use common::message::user::BasicInfo;

use crate::component::{Component, Footer, Header, Sidebar};

pub struct StandardLayout<C> {
    path: String,
    collapsed: bool,
    content: C,
}

impl<T> StandardLayout<T>
where
    T: sailfish::TemplateOnce,
{
    pub fn new(path: impl Into<String>, content: T) -> Self {
        StandardLayout {
            path: path.into(),
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
        // Both may ask the user service about a signed-in viewer, so they
        // wait together rather than one after the other.
        let (header, concierge) = tokio::join!(
            Header::default()
                .with_current_uri(&self.path)
                .prepare(user.clone()),
            crate::concierge::launcher_for(user.as_deref()),
        );
        let header = header?;
        let sidebar = Sidebar::new(&self.path).prepare(user.clone()).await?;
        let footer = Footer::default()
            .config(&self.path)
            .prepare(user.clone())
            .await?;

        Ok(StandardLayoutPrepared {
            user,
            header,
            sidebar,
            footer,
            concierge,
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
    /// Flower's floating launcher, when the concierge is presented that way
    /// and available to this viewer.
    concierge: Option<crate::concierge::ConciergeLauncher>,
    content: T,
    collapsed: bool,
}
