use common::message::user::BasicInfo;
use sailfish::TemplateOnce;
use std::sync::Arc;

use crate::wordpress::Script;

pub struct WordPressContent {
    pub page_url: String,
}

#[derive(TemplateOnce)]
#[template(path = "site/wordpress.stpl.html")]
pub struct WordPressContentPrepared {
    pub title: String,
    pub content: String,
    pub excerpt: String,
    /// The page's stylesheets, inlined and confined to the container class.
    pub css: String,
    /// The scripts the content's behaviour depends on, in document order.
    pub scripts: Vec<Script>,
    /// The class the markup, CSS and scripts are confined to.
    pub container_class: &'static str,
}

impl super::Component for WordPressContent {
    type Param = Option<Arc<BasicInfo>>;
    type Prepared = WordPressContentPrepared;

    async fn prepare(self, _param: Self::Param) -> Result<Self::Prepared, anyhow::Error> {
        let page = crate::wordpress::fetch_page(&self.page_url).await?;

        Ok(WordPressContentPrepared {
            title: page.title,
            content: page.content,
            excerpt: String::new(),
            css: page.css,
            scripts: page.scripts,
            container_class: crate::wordpress::CONTAINER_CLASS,
        })
    }
}
