use anyhow::Result;
use url::Url;
use std::sync::{LazyLock, Mutex};
use std::time::Duration;
use crate::lru_cache::LruCache;

#[derive(Debug, Clone)]
pub struct WordPressPageWithStyles {
    pub title: String,
    pub content: String,
    pub stylesheets: Vec<String>,
    pub inline_styles: Vec<String>,
}

const CACHE_SIZE: usize = 100;
const CACHE_DURATION: Duration = Duration::from_secs(600); // 10 minutes

static WORDPRESS_CACHE: LazyLock<Mutex<LruCache<WordPressPageWithStyles>>> =
    LazyLock::new(|| Mutex::new(LruCache::new(CACHE_SIZE, CACHE_DURATION)));

/// Access the WordPress cache for maintenance operations
pub fn cache() -> &'static Mutex<LruCache<WordPressPageWithStyles>> {
    &WORDPRESS_CACHE
}

/// Fetch a WordPress page or post by its URL, including content, stylesheets and inline styles
///
/// The URL should be the full page/post URL (e.g., https://wordpress.example.com/my-page/)
/// This fetches and parses the rendered HTML page to extract everything we need.
/// Results are cached in a 100-entry LRU cache with 10-minute expiration (refreshed on each access).
pub async fn fetch_page_with_styles(page_url: &str) -> Result<WordPressPageWithStyles> {
    use scraper::{Html, Selector};

    // Check cache first
    if let Some(cached) = WORDPRESS_CACHE.lock().unwrap().get(page_url) {
        tracing::debug!("WordPress page found in cache: {}", page_url);
        return Ok(cached);
    }

    tracing::debug!("Fetching WordPress page: {}", page_url);

    let response = reqwest::get(page_url).await?;

    if !response.status().is_success() {
        return Err(anyhow::anyhow!(
            "Failed to fetch WordPress page: status {}",
            response.status()
        ));
    }

    let html = response.text().await?;
    let document = Html::parse_document(&html);

    let url = Url::parse(page_url)?;
    let base_url = format!("{}://{}", url.scheme(), url.host_str().unwrap_or(""));

    // Extract title
    let title_selector = Selector::parse("h1.wp-block-heading, h1.entry-title, title")
        .map_err(|e| anyhow::anyhow!("Invalid CSS selector: {:?}", e))?;
    let title = document.select(&title_selector)
        .next()
        .map(|el| el.text().collect::<String>())
        .unwrap_or_default();

    // Extract main content
    let content_selector = Selector::parse(".entry-content, .wp-block-post-content, article")
        .map_err(|e| anyhow::anyhow!("Invalid CSS selector: {:?}", e))?;
    let content = document.select(&content_selector)
        .next()
        .map(|el| el.html())
        .unwrap_or_default();

    // Extract stylesheet links
    let link_selector = Selector::parse("link[rel='stylesheet']")
        .map_err(|e| anyhow::anyhow!("Invalid CSS selector: {:?}", e))?;

    let mut stylesheets = Vec::new();
    for element in document.select(&link_selector) {
        if let Some(href) = element.value().attr("href") {
            let stylesheet_url = if href.starts_with("http://") || href.starts_with("https://") {
                href.to_string()
            } else if href.starts_with("//") {
                format!("https:{}", href)
            } else if href.starts_with('/') {
                format!("{}{}", base_url, href)
            } else {
                format!("{}/{}", base_url, href)
            };
            stylesheets.push(stylesheet_url);
        }
    }

    // Extract inline styles from <style> tags in the <head>
    let style_selector = Selector::parse("head style")
        .map_err(|e| anyhow::anyhow!("Invalid CSS selector: {:?}", e))?;

    let mut inline_styles = Vec::new();
    for element in document.select(&style_selector) {
        let style_content = element.text().collect::<String>();
        if !style_content.trim().is_empty() {
            // Transform selectors for Shadow DOM compatibility
            let transformed = transform_css_for_shadow_dom(&style_content);
            inline_styles.push(transformed);
        }
    }

    tracing::debug!("Found {} stylesheets and {} inline styles", stylesheets.len(), inline_styles.len());

    let result = WordPressPageWithStyles {
        title,
        content,
        stylesheets,
        inline_styles,
    };

    // Store in cache
    WORDPRESS_CACHE.lock().unwrap().insert(page_url.to_string(), result.clone());

    Ok(result)
}

/// Transform CSS selectors for Shadow DOM compatibility
fn transform_css_for_shadow_dom(css: &str) -> String {
    use regex::Regex;

    let mut result = css.to_string();

    // First, replace :root with :host (for CSS custom properties)
    result = result.replace(":root", ":host");

    // Replace "html" selector with ":host"
    // Using word boundaries to avoid matching "html5", etc.
    let html_selector = Regex::new(r"\bhtml\b").unwrap();
    result = html_selector.replace_all(&result, ":host").to_string();

    // Then replace all remaining instances of "body" as a selector with ":host"
    // Using word boundaries to avoid matching "somebody", "bodywork", etc.
    let body_selector = Regex::new(r"\bbody\b").unwrap();
    result = body_selector.replace_all(&result, ":host").to_string();

    result
}

