use std::sync::Arc;
use sailfish::TemplateOnce;
use crate::component::{Component, head::Head, header::Header, footer::Footer};
use common::message::user::BasicInfo;

#[derive(Debug, Clone)]
pub struct Breadcrumb {
    pub label: String,
    pub url: Option<String>,
}

#[derive(Debug)]
pub struct AdminLayout {
    pub title: String,
    pub breadcrumbs: Vec<Breadcrumb>,
    pub sidebar_active: String,
    pub content: String,
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/layout.stpl.html")]
pub struct AdminLayoutPrepared {
    pub head: Head,
    pub header: <Header as Component>::Prepared,
    pub footer: <Footer as Component>::Prepared,
    pub title: String,
    pub breadcrumbs: Vec<Breadcrumb>,
    pub sidebar_active: String,
    pub content: String,
}

impl AdminLayout {
    pub fn new(title: &str) -> Self {
        Self {
            title: title.to_string(),
            breadcrumbs: vec![
                Breadcrumb {
                    label: "Admin".to_string(),
                    url: Some("/-admin".to_string()),
                }
            ],
            sidebar_active: String::new(),
            content: String::new(),
        }
    }

    pub fn with_breadcrumb(mut self, label: &str, url: Option<&str>) -> Self {
        self.breadcrumbs.push(Breadcrumb {
            label: label.to_string(),
            url: url.map(|s| s.to_string()),
        });
        self
    }

    pub fn with_sidebar_active(mut self, section: &str) -> Self {
        self.sidebar_active = section.to_string();
        self
    }

    pub fn with_content(mut self, content: String) -> Self {
        self.content = content;
        self
    }
}

impl Component for AdminLayout {
    type Param = Option<Arc<BasicInfo>>;
    type Prepared = AdminLayoutPrepared;

    async fn prepare(self, param: Self::Param) -> Result<Self::Prepared, anyhow::Error> {
        let user_info = param.clone();
        Ok(AdminLayoutPrepared {
            head: Head::default()
                .with_alpine()
                .with_webawesome(),
            header: Header::default().prepare(user_info.clone()).await?,
            footer: Footer::default().prepare(user_info).await?,
            title: self.title,
            breadcrumbs: self.breadcrumbs,
            sidebar_active: self.sidebar_active,
            content: self.content,
        })
    }
}