use crate::component::{Component, footer::Footer, head::Head, header::Header};
use common::message::user::BasicInfo;
use sailfish::TemplateOnce;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct Breadcrumb {
    pub label: String,
    pub url: Option<String>,
}

#[derive(Debug)]
pub struct AdminLayout<C: TemplateOnce> {
    pub title: String,
    pub breadcrumbs: Vec<Breadcrumb>,
    pub sidebar_active: String,
    pub content: C,
    pub enable_quill: bool,
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/layout.stpl.html")]
pub struct AdminLayoutPrepared<C: TemplateOnce> {
    pub head: Head,
    pub header: <Header as Component>::Prepared,
    pub footer: <Footer as Component>::Prepared,
    pub title: String,
    pub breadcrumbs: Vec<Breadcrumb>,
    pub sidebar_active: String,
    pub content: C,
}

impl<C: TemplateOnce> AdminLayout<C> {
    pub fn new(title: &str, content: C) -> Self {
        Self {
            title: title.to_string(),
            breadcrumbs: vec![Breadcrumb {
                label: "Admin".to_string(),
                url: Some("/-admin".to_string()),
            }],
            sidebar_active: String::new(),
            content,
            enable_quill: false,
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

    pub fn with_quill(mut self) -> Self {
        self.enable_quill = true;
        self
    }
}

impl<C: TemplateOnce> Component for AdminLayout<C> {
    type Param = Option<Arc<BasicInfo>>;
    type Prepared = AdminLayoutPrepared<C>;

    async fn prepare(self, param: Self::Param) -> Result<Self::Prepared, anyhow::Error> {
        let user_info = param.clone();

        let mut head = Head::default().with_alpine().with_webawesome();
        if self.enable_quill {
            head = head.with_quill();
        }

        Ok(AdminLayoutPrepared {
            head,
            header: Header::default().prepare(user_info.clone()).await?,
            footer: Footer::default().prepare(user_info).await?,
            title: self.title,
            breadcrumbs: self.breadcrumbs,
            sidebar_active: self.sidebar_active,
            content: self.content,
        })
    }
}
