use sailfish::TemplateOnce;
use std::sync::Arc;
use common::message::user::BasicInfo;

pub struct WordPressContent {
    pub page_url: String,
}

#[derive(TemplateOnce)]
#[template(path = "site/wordpress.stpl.html")]
pub struct WordPressContentPrepared {
    pub title: String,
    pub content: String,
    pub excerpt: String,
    pub stylesheets: Vec<String>,
    pub inline_styles: Vec<String>,
}

impl super::Component for WordPressContent {
    type Param = Option<Arc<BasicInfo>>;
    type Prepared = WordPressContentPrepared;

    async fn prepare(self, _param: Self::Param) -> Result<Self::Prepared, anyhow::Error> {
        // Fetch and parse the WordPress page HTML
        let page_with_styles = crate::wordpress::fetch_page_with_styles(&self.page_url).await?;

        // Extract base URL from page_url
        let url = url::Url::parse(&self.page_url)?;
        let base_url = format!("{}://{}", url.scheme(), url.host_str().unwrap_or(""));

        // Rewrite relative URLs in content to absolute URLs
        let content = rewrite_urls(&page_with_styles.content, &base_url);

        Ok(WordPressContentPrepared {
            title: page_with_styles.title,
            content,
            excerpt: String::new(),
            stylesheets: page_with_styles.stylesheets,
            inline_styles: page_with_styles.inline_styles,
        })
    }
}

/// Rewrite relative URLs in HTML content to absolute URLs
fn rewrite_urls(html: &str, base_url: &str) -> String {
    use regex::Regex;

    let mut result = html.to_string();

    // Rewrite src="/wp-content/..." to src="https://base.url/wp-content/..."
    let src_pattern = Regex::new(r#"src="(/wp-content/[^"]+)""#).unwrap();
    result = src_pattern.replace_all(&result, |caps: &regex::Captures| {
        format!(r#"src="{}{}""#, base_url, &caps[1])
    }).to_string();

    // Rewrite href="/wp-content/..." to href="https://base.url/wp-content/..."
    let href_pattern = Regex::new(r#"href="(/wp-content/[^"]+)""#).unwrap();
    result = href_pattern.replace_all(&result, |caps: &regex::Captures| {
        format!(r#"href="{}{}""#, base_url, &caps[1])
    }).to_string();

    result
}