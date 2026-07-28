//! Fetching WordPress pages so they can be embedded in a hydra page.
//!
//! The embedding is deliberately *not* an iframe and *not* a shadow root: the
//! content is server-rendered into the ordinary document so that it is indexed,
//! and so that the block plugins' own scripts --- which bind on
//! `DOMContentLoaded` via `document.querySelectorAll` --- can find their markup.
//! Tabs, accordions and sliders therefore work as they do on WordPress.
//!
//! Style isolation is what the shadow root used to provide, so it is rebuilt
//! from two CSS features instead, and ends up stronger than the shadow root was:
//!
//! - Outward, every carried rule is wrapped in `@scope (.wordpress-content)`,
//!   so it cannot match anything outside the container.
//! - Inward, the whole block sits in the `wordpress` cascade layer, which is
//!   declared after the `hydra` layer holding all of this site's own CSS. Layer
//!   order beats specificity, so no rule of ours can reach in however specific
//!   it is. `all: initial` on the container then cuts off inheritance, which a
//!   shadow boundary does not stop.
//!
//! External stylesheets are fetched and inlined rather than linked, because a
//! `<link>` pulls in a sheet that no `@scope` block can contain.

use crate::lru_cache::LruCache;
use anyhow::Result;
use std::sync::{LazyLock, Mutex};
use std::time::Duration;
use url::Url;

/// The class name the embedded markup, its CSS and its scripts are confined to.
pub const CONTAINER_CLASS: &str = "wordpress-content";

/// A script carried over from the WordPress page, kept in document order.
#[derive(Debug, Clone)]
pub struct Script {
    /// Absolute URL for an external script, or `None` for an inline one.
    pub src: Option<String>,
    /// Source text of an inline script.
    pub body: String,
    pub is_module: bool,
    pub is_async: bool,
    pub is_defer: bool,
}

#[derive(Debug, Clone)]
pub struct WordPressPage {
    pub title: String,
    pub content: String,
    /// Every stylesheet the page pulled in, inlined and confined to the
    /// container. Ready to drop into a single `<style>` element.
    pub css: String,
    pub scripts: Vec<Script>,
}

/// Scripts deliberately left behind.
///
/// These fall into three groups: analytics, which would double-count against
/// this site's own; WordPress chrome, which has no markup inside the content
/// element we extract; and the site's own embed script, which would draw a
/// second copy of our header inside our own page. Matched against each script's
/// `src` and `id`.
const EXCLUDED_SCRIPTS: &[&str] = &[
    "jetpack",
    "stats.wp.com",
    "googletagmanager",
    "gtm.js",
    "wp-emoji",
    "heateor",
    "sassy-social-share",
    "skip-link",
    "/-embed/",
];

const CACHE_SIZE: usize = 100;
const CACHE_DURATION: Duration = Duration::from_secs(600); // 10 minutes

static WORDPRESS_CACHE: LazyLock<Mutex<LruCache<WordPressPage>>> =
    LazyLock::new(|| Mutex::new(LruCache::new(CACHE_SIZE, CACHE_DURATION)));

/// Access the WordPress cache for maintenance operations
pub fn cache() -> &'static Mutex<LruCache<WordPressPage>> {
    &WORDPRESS_CACHE
}

/// Fetch a WordPress page or post, with the styles and scripts its content needs
///
/// The URL should be the full page/post URL (e.g., https://wordpress.example.com/my-page/)
/// Results are cached in a 100-entry LRU cache with 10-minute expiration (refreshed on each access).
pub async fn fetch_page(page_url: &str) -> Result<WordPressPage> {
    if let Some(cached) = WORDPRESS_CACHE.lock().unwrap().get(page_url) {
        tracing::debug!("WordPress page found in cache: {}", page_url);
        return Ok(cached);
    }

    tracing::debug!("Fetching WordPress page: {}", page_url);

    // Fetch through the SSRF-guarded client so a staff-supplied URL cannot be
    // pointed at internal/link-local addresses.
    let response = crate::ssrf::guarded_get(page_url).await?;

    if !response.status().is_success() {
        return Err(anyhow::anyhow!(
            "Failed to fetch WordPress page: status {}",
            response.status()
        ));
    }

    let html = response.text().await?;
    let page = Url::parse(page_url)?;

    let extracted = extract(&html, &page)?;

    // Linked sheets come first so their rules keep their original order relative
    // to the inline ones that follow them in the head.
    let mut sheets = Vec::new();
    for sheet_url in &extracted.sheet_urls {
        match fetch_stylesheet(sheet_url).await {
            Ok(css) => sheets.push(css),
            Err(error) => {
                tracing::warn!("Skipping WordPress stylesheet {}: {}", sheet_url, error);
            }
        }
    }
    sheets.extend(extracted.inline_styles);

    tracing::debug!(
        "Carried {} stylesheet(s) and {} script(s) from {}",
        sheets.len(),
        extracted.scripts.len(),
        page_url
    );

    let result = WordPressPage {
        title: extracted.title,
        content: extracted.content,
        css: confine_css(&sheets.join("\n")),
        scripts: extracted.scripts,
    };

    WORDPRESS_CACHE
        .lock()
        .unwrap()
        .insert(page_url.to_string(), result.clone());

    Ok(result)
}

/// Everything taken from the fetched markup, before any further fetching
struct Extracted {
    title: String,
    content: String,
    sheet_urls: Vec<String>,
    inline_styles: Vec<String>,
    scripts: Vec<Script>,
}

/// Pull the content, styles and scripts out of a fetched WordPress page
///
/// Split out from [`fetch_page`] for two reasons: `scraper`'s document is
/// neither `Send` nor `Sync`, so it must not be held across the stylesheet
/// fetches, and separating parsing from I/O is what makes the rules about what
/// gets carried testable.
fn extract(html: &str, page: &Url) -> Result<Extracted> {
    use scraper::{Html, Selector};

    fn selector(spec: &str) -> Result<scraper::Selector> {
        Selector::parse(spec).map_err(|e| anyhow::anyhow!("Invalid CSS selector: {:?}", e))
    }

    let document = Html::parse_document(html);

    let title = document
        .select(&selector("h1.wp-block-heading, h1.entry-title, title")?)
        .next()
        .map(|el| el.text().collect::<String>())
        .unwrap_or_default();

    let content = document
        .select(&selector(
            ".entry-content, .wp-block-post-content, article",
        )?)
        .next()
        .map(|el| el.html())
        .unwrap_or_default();

    let sheet_urls = document
        .select(&selector("link[rel='stylesheet']")?)
        .filter_map(|el| el.value().attr("href"))
        .filter_map(|href| page.join(href).ok())
        .map(String::from)
        .collect();

    let inline_styles = document
        .select(&selector("head style")?)
        .map(|el| el.text().collect::<String>())
        .filter(|css| !css.trim().is_empty())
        .map(|css| rewrite_relative_urls(&css, page))
        .collect();

    let scripts = document
        .select(&selector("script")?)
        .filter_map(|el| carried_script(&el, page))
        .collect();

    Ok(Extracted {
        title,
        content: rewrite_relative_urls(&content, page),
        sheet_urls,
        inline_styles,
        scripts,
    })
}

/// Decide whether a `<script>` comes along, and in what form
fn carried_script(element: &scraper::ElementRef, page: &Url) -> Option<Script> {
    let value = element.value();

    // Anything that is not executable JavaScript is metadata for WordPress
    // itself (JSON-LD, speculation rules, emoji settings) and has no bearing on
    // the content.
    let script_type = value.attr("type").unwrap_or("");
    let is_module = script_type == "module";
    if !(script_type.is_empty() || is_module || script_type == "text/javascript") {
        return None;
    }

    let src = value.attr("src");
    let haystack = format!("{} {}", src.unwrap_or(""), value.attr("id").unwrap_or(""));
    if EXCLUDED_SCRIPTS
        .iter()
        .any(|excluded| haystack.contains(excluded))
    {
        return None;
    }

    let body = element.text().collect::<String>();

    // `document.write` replaces the whole document when it runs after parsing,
    // which is exactly when a carried script runs.
    if src.is_none() && body.contains("document.write") {
        return None;
    }

    Some(Script {
        src: src.and_then(|src| page.join(src).ok()).map(String::from),
        body,
        is_module,
        is_async: value.attr("async").is_some(),
        is_defer: value.attr("defer").is_some(),
    })
}

/// Fetch one stylesheet and make its relative URLs absolute
///
/// Inlining is what lets the rules be wrapped in `@scope`; the cost is that
/// `url()` references no longer resolve against the WordPress origin, so they
/// are rewritten here while that origin is still known.
async fn fetch_stylesheet(sheet_url: &str) -> Result<String> {
    let response = crate::ssrf::guarded_get(sheet_url).await?;
    if !response.status().is_success() {
        return Err(anyhow::anyhow!("status {}", response.status()));
    }
    let css = response.text().await?;
    let base = Url::parse(sheet_url)?;

    // A nested `@import` cannot survive being wrapped in `@scope`, and would
    // pull in an unconfined sheet if it did.
    let imports = regex::Regex::new(r"(?m)^\s*@import[^;]*;").unwrap();
    if imports.is_match(&css) {
        tracing::warn!("Dropping @import rule(s) from WordPress stylesheet {sheet_url}");
    }

    Ok(rewrite_relative_urls(&imports.replace_all(&css, ""), &base))
}

/// Resolve `url(...)`, `src=` and `href=` references against the document they
/// came from, since the markup and CSS are about to be served from this site
fn rewrite_relative_urls(text: &str, base: &Url) -> String {
    use regex::{Captures, Regex};
    use std::sync::LazyLock;

    static CSS_URL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\burl\(([^)]*)\)").unwrap());
    static ATTRIBUTE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r#"\b(src|href)="([^"]*)""#).unwrap());

    fn resolve(target: &str, base: &Url) -> String {
        // Data and fragment references are already self-contained.
        if target.is_empty() || target.starts_with("data:") || target.starts_with('#') {
            return target.to_string();
        }
        base.join(target)
            .map(String::from)
            .unwrap_or_else(|_| target.to_string())
    }

    let css_rewritten = CSS_URL.replace_all(text, |caps: &Captures| {
        let inner = caps[1].trim();
        let quote = inner.chars().next().filter(|c| *c == '"' || *c == '\'');
        let target = match quote {
            Some(q) => inner.trim_matches(q),
            None => inner,
        };
        let quote = quote.map(String::from).unwrap_or_default();
        format!("url({quote}{}{quote})", resolve(target, base))
    });

    ATTRIBUTE
        .replace_all(&css_rewritten, |caps: &Captures| {
            format!("{}=\"{}\"", &caps[1], resolve(&caps[2], base))
        })
        .to_string()
}

/// Wrap a stylesheet so it can only apply inside the container, and so that none
/// of this site's own rules can apply within it
fn confine_css(css: &str) -> String {
    // The result is emitted inside a `<style>` element. A page's own inline
    // styles cannot contain `</style` and still have been valid, but a fetched
    // `.css` file is under no such constraint, and one closing tag inside a
    // string or comment would end the element early and spill the rest as
    // markup. `\/` is just `/` to a CSS string parser, so this keeps the
    // stylesheet's meaning while denying the HTML tokenizer its match.
    let css = css.replace("</style", "<\\/style");

    format!(
        "@layer wordpress {{\n@scope (.{CONTAINER_CLASS}) {{\n\
         :scope {{ all: initial; display: block; }}\n{}\n}}\n}}\n",
        retarget_root_selectors(&css)
    )
}

/// Point a stylesheet's document-root selectors at the container instead
///
/// `:root`, `html` and `body` match nothing inside the container, so their
/// declarations --- which is where WordPress puts its custom properties and page
/// defaults --- would otherwise be lost. Only selector preludes are rewritten,
/// so the same words appearing in declarations, strings and `url()` values are
/// left alone.
fn retarget_root_selectors(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut prelude = String::new();
    let mut chars = css.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '{' => {
                out.push_str(&retarget_prelude(&prelude));
                prelude.clear();
                out.push('{');
            }
            '}' | ';' => {
                out.push_str(&prelude);
                prelude.clear();
                out.push(c);
            }
            '"' | '\'' => {
                prelude.push(c);
                for quoted in chars.by_ref() {
                    prelude.push(quoted);
                    if quoted == c {
                        break;
                    }
                }
            }
            '/' if chars.peek() == Some(&'*') => {
                prelude.push(c);
                let mut previous = '\0';
                for commented in chars.by_ref() {
                    prelude.push(commented);
                    if previous == '*' && commented == '/' {
                        break;
                    }
                    previous = commented;
                }
            }
            _ => prelude.push(c),
        }
    }
    out.push_str(&prelude);
    out
}

/// Rewrite the root selectors within one selector prelude
fn retarget_prelude(prelude: &str) -> String {
    // An at-rule prelude (`@media (max-width: 600px)`) holds no selectors.
    if prelude.trim_start().starts_with('@') {
        return prelude.to_string();
    }

    let mut out = String::with_capacity(prelude.len());
    let mut compound = String::new();
    let mut depth = 0usize;

    for c in prelude.chars() {
        match c {
            '[' | '(' => {
                depth += 1;
                compound.push(c);
            }
            ']' | ')' => {
                depth = depth.saturating_sub(1);
                compound.push(c);
            }
            _ if depth == 0 && (c.is_whitespace() || matches!(c, ',' | '>' | '+' | '~')) => {
                out.push_str(&retarget_compound(&compound));
                compound.clear();
                out.push(c);
            }
            _ => compound.push(c),
        }
    }
    out.push_str(&retarget_compound(&compound));
    out
}

/// Replace a leading `:root`, `html` or `body` type selector with `:scope`
fn retarget_compound(compound: &str) -> String {
    for root in [":root", "html", "body"] {
        if let Some(rest) = compound.strip_prefix(root) {
            // `.somebody` and `htmlish` keep their own meaning; only a complete
            // type selector, optionally qualified, is a document-root selector.
            if rest.is_empty() || rest.starts_with(['.', '#', '[', ':']) {
                return format!(":scope{rest}");
            }
        }
    }
    compound.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_selectors_move_onto_the_container() {
        assert_eq!(
            retarget_root_selectors("html, body { margin: 0 }"),
            ":scope, :scope { margin: 0 }"
        );
        assert_eq!(
            retarget_root_selectors(":root { --wp--x: 1px }"),
            ":scope { --wp--x: 1px }"
        );
        assert_eq!(
            retarget_root_selectors("body.home > .x { color: red }"),
            ":scope.home > .x { color: red }"
        );
    }

    #[test]
    fn other_uses_of_those_words_are_left_alone() {
        // The previous implementation rewrote every one of these.
        assert_eq!(
            retarget_root_selectors(".some-body { color: red }"),
            ".some-body { color: red }"
        );
        assert_eq!(
            retarget_root_selectors(".x { background: url(body.png) }"),
            ".x { background: url(body.png) }"
        );
        assert_eq!(
            retarget_root_selectors(".x::after { content: \"body\" }"),
            ".x::after { content: \"body\" }"
        );
        assert_eq!(
            retarget_root_selectors("@media (min-width: 1px) { .x { color: red } }"),
            "@media (min-width: 1px) { .x { color: red } }"
        );
    }

    #[test]
    fn confined_css_is_scoped_and_layered() {
        let css = confine_css("body { color: red }");
        assert!(css.starts_with("@layer wordpress {"));
        assert!(css.contains("@scope (.wordpress-content) {"));
        assert!(css.contains(":scope { all: initial; display: block; }"));
        assert!(css.contains(":scope { color: red }"));
    }

    #[test]
    fn a_stylesheet_cannot_close_the_style_element_it_is_emitted_into() {
        let css = confine_css(r#".x::after { content: "</style><script>alert(1)</script>" }"#);
        assert!(!css.contains("</style>"));
        assert!(css.contains(r#"content: "<\/style>"#));
    }

    /// Shaped after a real page: WordPress emits the block runtimes among a
    /// crowd of analytics, chrome and metadata scripts, and some of the ones
    /// worth keeping carry no `id` at all.
    const PAGE: &str = r#"
<html><head>
  <title>Education</title>
  <link rel='stylesheet' href='/wp-content/themes/t/style.css' />
  <style>body { color: red }</style>
  <script type="application/ld+json">{"@context":"https://schema.org"}</script>
  <script src="/wp-includes/js/jquery/jquery.min.js" id="jquery-core-js"></script>
  <script>document.addEventListener("DOMContentLoaded", function() { let stk_tab = 1; })</script>
  <script src="https://www.googletagmanager.com/gtm.js?id=X"></script>
  <script src="https://scistarter.org/-embed/header.js"></script>
</head><body>
  <div class="entry-content"><p>Body text</p><img src="/wp-content/uploads/x.png"></div>
  <script>document.write(new Date().getFullYear())</script>
  <script type="speculationrules">{"prefetch":[]}</script>
  <script src="/wp-content/plugins/stackable/dist/frontend_block_tabs.js" id="stk-frontend-tabs-js"></script>
  <script src="/wp-content/plugins/smart-slider-3/n2.min.js"></script>
  <script src="https://stats.wp.com/e-1.js" id="jetpack-stats-js" defer></script>
  <script id="wp-emoji-settings" type="application/json">{}</script>
</body></html>
"#;

    fn extract_page() -> Extracted {
        extract(
            PAGE,
            &Url::parse("https://pages.example.org/education/").unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn the_scripts_the_content_behaves_by_are_carried() {
        let carried = extract_page().scripts;
        let sources = carried
            .iter()
            .map(|s| s.src.clone().unwrap_or_else(|| s.body.clone()))
            .collect::<Vec<_>>();

        assert_eq!(sources.len(), 4, "carried: {sources:#?}");
        assert!(sources[0].ends_with("/wp-includes/js/jquery/jquery.min.js"));
        assert!(
            sources[1].contains("stk_tab"),
            "the tab initialiser is inline and has no id"
        );
        assert!(sources[2].ends_with("frontend_block_tabs.js"));
        assert!(
            sources[3].ends_with("n2.min.js"),
            "Smart Slider's scripts have no id"
        );
    }

    #[test]
    fn analytics_chrome_and_metadata_are_left_behind() {
        let carried = extract_page().scripts;
        let all = carried
            .iter()
            .map(|s| format!("{} {}", s.src.clone().unwrap_or_default(), s.body))
            .collect::<Vec<_>>()
            .join("\n");

        for unwanted in [
            "googletagmanager", // would double-count against our own analytics
            "stats.wp.com",     // ditto
            "/-embed/",         // would draw a second copy of our header
            "document.write",   // would destroy the document it runs in
            "schema.org",       // metadata, not behaviour
            "speculationrules",
            "wp-emoji-settings",
        ] {
            assert!(
                !all.contains(unwanted),
                "{unwanted} should not be carried:\n{all}"
            );
        }
    }

    #[test]
    fn content_and_styles_come_across_with_absolute_urls() {
        let extracted = extract_page();

        assert_eq!(extracted.title, "Education");
        assert!(extracted.content.contains("<p>Body text</p>"));
        assert!(
            extracted
                .content
                .contains(r#"src="https://pages.example.org/wp-content/uploads/x.png""#)
        );
        assert_eq!(
            extracted.sheet_urls,
            vec!["https://pages.example.org/wp-content/themes/t/style.css"]
        );
        assert_eq!(extracted.inline_styles, vec!["body { color: red }"]);
    }

    #[test]
    fn relative_urls_resolve_against_their_source() {
        let base = Url::parse("https://pages.example.org/wp-content/themes/t/style.css").unwrap();
        assert_eq!(
            rewrite_relative_urls("a { background: url(../img/x.png) }", &base),
            "a { background: url(https://pages.example.org/wp-content/themes/img/x.png) }"
        );
        assert_eq!(
            rewrite_relative_urls(r#"<img src="/wp-content/x.png">"#, &base),
            r#"<img src="https://pages.example.org/wp-content/x.png">"#
        );
        assert_eq!(
            rewrite_relative_urls("a { background: url(data:image/png;base64,AAA) }", &base),
            "a { background: url(data:image/png;base64,AAA) }"
        );
    }
}
