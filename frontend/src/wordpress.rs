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
//!   order beats specificity, so no rule of ours can outrank one of theirs.
//!   That settles conflicts, but where WordPress declares nothing our rules
//!   would still apply, so an `all: revert` at zero specificity heads the block
//!   and denies them the subtree outright --- including the inherited
//!   properties a shadow boundary cannot stop. Being zero-specificity, every
//!   WordPress rule still outranks it; being in the later layer is what puts it
//!   ahead of ours. Inline `style` attributes are a different source and are
//!   untouched, which matters because block markup is full of them.
//!
//! External stylesheets are fetched and inlined rather than linked, because a
//! `<link>` pulls in a sheet that no `@scope` block can contain.

use crate::lru_cache::LruCache;
use anyhow::Result;
use std::future::Future;
use std::pin::Pin;
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
    /// The classes WordPress had on `<html>` and `<body>`, to be put on the
    /// container so rules keyed to them keep matching.
    pub root_classes: String,
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
        root_classes: extracted.root_classes,
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
    /// The classes WordPress put on `<html>` and `<body>`.
    root_classes: String,
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

    // WordPress emits per-block styles in the body as well as the head, so this
    // takes every `<style>` on the page rather than only the head's.
    let inline_styles = document
        .select(&selector("style")?)
        .map(|el| el.text().collect::<String>())
        .filter(|css| !css.trim().is_empty())
        .map(|css| rewrite_css_urls(&css, page))
        .collect();

    // WordPress puts page state on the root elements --- `page-id-32677`, theme
    // and plugin feature flags --- and its stylesheets key off them. Lifting
    // only `.entry-content` leaves those ancestors behind, so every rule
    // written as `body.something .block` quietly stops matching. Carrying the
    // classes onto the container puts them back within reach, since `body` and
    // `:root` are retargeted onto that same element.
    let root_classes = ["html", "body"]
        .into_iter()
        .filter_map(|name| selector(name).ok())
        .filter_map(|s| {
            document
                .select(&s)
                .next()
                .and_then(|el| el.value().attr("class"))
                .map(String::from)
        })
        .collect::<Vec<_>>()
        .join(" ");

    let scripts = document
        .select(&selector("script")?)
        .filter_map(|el| carried_script(&el, page))
        .collect();

    Ok(Extracted {
        title,
        content: rewrite_markup_urls(&content, page),
        root_classes,
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
/// How deep a chain of `@import`s to follow before giving up.
const MAX_IMPORT_DEPTH: usize = 3;

async fn fetch_stylesheet(sheet_url: &str) -> Result<String> {
    fetch_stylesheet_at_depth(sheet_url.to_string(), 0).await
}

fn fetch_stylesheet_at_depth(
    sheet_url: String,
    depth: usize,
) -> Pin<Box<dyn Future<Output = Result<String>> + Send>> {
    Box::pin(async move {
        let response = crate::ssrf::guarded_get(&sheet_url).await?;
        if !response.status().is_success() {
            return Err(anyhow::anyhow!("status {}", response.status()));
        }
        let base = Url::parse(&sheet_url)?;
        let css = rewrite_css_urls(&response.text().await?, &base);

        // An `@import` cannot survive being wrapped in `@scope`, and would pull
        // in an unconfined sheet if it did, so each one is fetched and spliced
        // in where it stood. This is how the theme loads its fonts.
        let imports = find_imports(&css);
        if imports.is_empty() {
            return Ok(css);
        }
        if depth >= MAX_IMPORT_DEPTH {
            tracing::warn!("Stopping at @import depth {depth} in {sheet_url}");
            return Ok(splice(&css, &imports, &vec![String::new(); imports.len()]));
        }

        let mut resolved = Vec::with_capacity(imports.len());
        for import in &imports {
            let inlined = match fetch_stylesheet_at_depth(import.url.clone(), depth + 1).await {
                Ok(css) => css,
                Err(error) => {
                    tracing::warn!("Skipping @import {}: {}", import.url, error);
                    String::new()
                }
            };
            resolved.push(match &import.condition {
                // `@import url(x) screen;` only applies under that condition.
                condition if !condition.is_empty() => {
                    format!("@media {condition} {{\n{inlined}\n}}\n")
                }
                _ => inlined,
            });
        }

        Ok(splice(&css, &imports, &resolved))
    })
}

/// An `@import` rule found at the top level of a stylesheet
struct Import {
    start: usize,
    end: usize,
    url: String,
    /// Any media query trailing the URL.
    condition: String,
}

/// Locate the top-level `@import` rules in a stylesheet
///
/// The rule cannot be found by pattern alone: a URL may contain semicolons ---
/// Google Fonts writes weight lists as `wght@0,300;400;700` --- so the end of
/// the rule has to be found by scanning past strings and parentheses.
fn find_imports(css: &str) -> Vec<Import> {
    let mut imports = Vec::new();
    let mut depth = 0usize;
    let mut i = 0;

    while i < css.len() {
        if css[i..].starts_with("/*") {
            i = css[i..].find("*/").map(|e| i + e + 2).unwrap_or(css.len());
            continue;
        }
        let (c, width) = char_at(css, i);
        if c == '"' || c == '\'' {
            i = skip_string(css, i);
            continue;
        }
        if depth == 0 && c == '@' && begins_with_ignoring_case(&css[i..], "@import") {
            let end = at_rule_end(css, i);
            let prelude = css[i + "@import".len()..end].trim_end_matches(';').trim();
            if let Some((url, condition)) = parse_import_prelude(prelude) {
                imports.push(Import {
                    start: i,
                    end,
                    url,
                    condition,
                });
            }
            i = end;
            continue;
        }
        match c {
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            _ => {}
        }
        i += width;
    }

    imports
}

/// Split an `@import` prelude into its URL and any trailing media condition
fn parse_import_prelude(prelude: &str) -> Option<(String, String)> {
    let (url, rest) = if begins_with_ignoring_case(prelude, "url(") {
        let close = prelude.find(')')?;
        (&prelude["url(".len()..close], &prelude[close + 1..])
    } else {
        // A bare string form: `@import "theme.css" screen;`
        let quote = prelude.chars().next().filter(|c| *c == '"' || *c == '\'')?;
        let close = prelude[1..].find(quote)? + 1;
        (&prelude[1..close], &prelude[close + 1..])
    };

    let url = url.trim().trim_matches(['"', '\'']).trim();
    (!url.is_empty()).then(|| (url.to_string(), rest.trim().to_string()))
}

/// Replace each located rule with its resolved replacement
fn splice(css: &str, imports: &[Import], replacements: &[String]) -> String {
    let mut out = String::with_capacity(css.len());
    let mut cursor = 0;
    for (import, replacement) in imports.iter().zip(replacements) {
        out.push_str(&css[cursor..import.start]);
        out.push_str(replacement);
        cursor = import.end;
    }
    out.push_str(&css[cursor..]);
    out
}

/// Make a reference absolute against the document it came from
///
/// The markup and CSS are about to be served from this site, so anything
/// relative would otherwise resolve against the wrong origin.
fn resolve_against(target: &str, base: &Url) -> String {
    // Data and fragment references are already self-contained.
    if target.is_empty() || target.starts_with("data:") || target.starts_with('#') {
        return target.to_string();
    }
    base.join(target)
        .map(String::from)
        .unwrap_or_else(|_| target.to_string())
}

/// Resolve the `url()` references in a stylesheet
///
/// Scanned rather than matched: a `url()` may hold a quoted string containing a
/// closing parenthesis, and a pattern that stops at the first `)` would truncate
/// it --- the same mistake that cut the theme's `@import` in half.
fn rewrite_css_urls(css: &str, base: &Url) -> String {
    let mut out = String::with_capacity(css.len());
    let mut i = 0;

    while i < css.len() {
        if css[i..].starts_with("/*") {
            let end = css[i..].find("*/").map(|e| i + e + 2).unwrap_or(css.len());
            out.push_str(&css[i..end]);
            i = end;
            continue;
        }
        let (c, width) = char_at(css, i);
        if c == '"' || c == '\'' {
            let end = skip_string(css, i);
            out.push_str(&css[i..end]);
            i = end;
            continue;
        }
        if begins_with_ignoring_case(&css[i..], "url(") {
            let open = i + "url(".len();
            let mut j = open;
            while j < css.len() {
                let (c, width) = char_at(css, j);
                match c {
                    '"' | '\'' => j = skip_string(css, j),
                    ')' => break,
                    _ => j += width,
                }
            }
            let inner = css[open..j.min(css.len())].trim();
            let quote = inner.chars().next().filter(|c| *c == '"' || *c == '\'');
            let target = match quote {
                Some(q) => inner.trim_matches(q),
                None => inner,
            };
            let quote = quote.map(String::from).unwrap_or_default();
            out.push_str(&format!(
                "url({quote}{}{quote})",
                resolve_against(target, base)
            ));
            i = (j + 1).min(css.len());
            continue;
        }
        out.push(c);
        i += width;
    }

    out
}

/// Resolve the `src` and `href` attributes in a fragment of markup
///
/// Scanned rather than matched so that only real attributes are touched: the
/// same text inside a script body, a comment or ordinary prose is left alone.
fn rewrite_markup_urls(html: &str, base: &Url) -> String {
    let mut out = String::with_capacity(html.len());
    let bytes = html.as_bytes();
    let mut i = 0;

    while i < html.len() {
        if html[i..].starts_with("<!--") {
            let end = html[i..]
                .find("-->")
                .map(|e| i + e + 3)
                .unwrap_or(html.len());
            out.push_str(&html[i..end]);
            i = end;
            continue;
        }
        if bytes[i] != b'<' {
            let (c, width) = char_at(html, i);
            out.push(c);
            i += width;
            continue;
        }

        // Inside a tag: rewrite the attributes we care about, and copy the rest.
        let tag_end = tag_end(html, i);
        let tag = &html[i..tag_end];
        out.push_str(&rewrite_tag_attributes(tag, base));

        // A raw-text element's body is not markup, so skip over it untouched.
        if let Some(name) = ["script", "style"]
            .into_iter()
            .find(|name| begins_with_ignoring_case(&tag[1..], name))
        {
            let close = format!("</{name}");
            let end = html[tag_end..]
                .to_ascii_lowercase()
                .find(&close)
                .map(|e| tag_end + e)
                .unwrap_or(html.len());
            out.push_str(&html[tag_end..end]);
            i = end;
            continue;
        }
        i = tag_end;
    }

    out
}

/// Index just past the `>` closing the tag starting at `start`
fn tag_end(html: &str, start: usize) -> usize {
    let bytes = html.as_bytes();
    let mut i = start + 1;
    while i < html.len() {
        match bytes[i] {
            b'"' | b'\'' => i = skip_string(html, i),
            b'>' => return i + 1,
            _ => i += 1,
        }
    }
    html.len()
}

/// Rewrite the `src` and `href` attribute values of a single tag
fn rewrite_tag_attributes(tag: &str, base: &Url) -> String {
    let mut out = String::with_capacity(tag.len());
    let bytes = tag.as_bytes();
    let mut i = 0;

    while i < tag.len() {
        let name_start = i;
        while i < tag.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'-') {
            i += 1;
        }
        let name = &tag[name_start..i];

        // An attribute value follows `=` and an optional quote.
        let is_url_attribute =
            name.eq_ignore_ascii_case("src") || name.eq_ignore_ascii_case("href");
        if is_url_attribute && bytes.get(i) == Some(&b'=') {
            let quote = bytes
                .get(i + 1)
                .copied()
                .filter(|q| *q == b'"' || *q == b'\'');
            if let Some(quote) = quote {
                let value_start = i + 2;
                let value_end = skip_string(tag, i + 1) - 1;
                let quote = quote as char;
                out.push_str(name);
                out.push_str(&format!(
                    "={quote}{}{quote}",
                    resolve_against(&tag[value_start..value_end], base)
                ));
                i = value_end + 1;
                continue;
            }
        }

        out.push_str(name);
        if i < tag.len() {
            // Copy one character of separator, or a whole quoted value.
            if bytes[i] == b'"' || bytes[i] == b'\'' {
                let end = skip_string(tag, i);
                out.push_str(&tag[i..end]);
                i = end;
            } else {
                let (c, width) = char_at(tag, i);
                out.push(c);
                i += width;
            }
        }
    }

    out
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

    // `@scope` takes style rules and nested group rules. Rules that register a
    // *name* rather than styling an element --- keyframes, font faces, custom
    // property registrations --- are not valid inside it, and a browser that
    // rejects one drops the animation, font or property it defined. They are
    // global by nature and nothing is gained by scoping them, so they are
    // hoisted out ahead of the block.
    let (global, scoped) = hoist_global_at_rules(&css);

    // Layer order settles which rule wins when both sides style the same thing,
    // but where WordPress declares nothing, this site's rules would still apply
    // --- its heading, list and button styling reaching into content that never
    // asked for it. This reset denies them the whole subtree by rolling every
    // property back past the author origin. `:where()` gives it zero
    // specificity, so every WordPress rule outranks it and the sheet's own
    // baseline survives; being in the later layer is what makes it beat ours.
    // The pseudo-elements are written outside the `:where()` because a
    // pseudo-element is not valid inside one. `:where()` takes a *forgiving*
    // selector list, so putting them in would not fail loudly --- the entries
    // would simply be dropped and the reset would quietly skip every `::before`
    // and `::after`, which is where a colliding class does its worst.
    format!(
        "{global}@layer wordpress {{\n@scope (.{CONTAINER_CLASS}) {{\n\
         :where(:scope, :scope *) {{ all: revert; }}\n\
         :where(:scope, :scope *)::before, :where(:scope, :scope *)::after {{ all: revert; }}\n\
         {}\n}}\n}}\n",
        retarget_root_selectors(&scoped)
    )
}

/// At-rules that define a name for the whole document rather than styling
/// anything, and so belong outside the scoping block
const GLOBAL_AT_RULES: &[&str] = &[
    "@font-face",
    "@keyframes",
    "@-webkit-keyframes",
    "@property",
    "@counter-style",
    "@font-feature-values",
    "@page",
];

/// At-rules whose condition still applies to anything hoisted out from inside
/// them, and so has to be rebuilt around it
const CONDITIONAL_AT_RULES: &[&str] = &["@media", "@supports", "@container"];

/// Split a stylesheet into the at-rules that must stay at the top level and
/// everything else
///
/// A rule nested inside a media query is hoisted too, wrapped in the conditions
/// it was written under, so that leaving the scoping block does not also make it
/// unconditional.
fn hoist_global_at_rules(css: &str) -> (String, String) {
    let mut global = String::new();
    let mut rest = String::with_capacity(css.len());
    // The prelude of each open block, so a hoisted rule can be re-wrapped in the
    // conditions enclosing it. `None` for a block whose condition cannot be
    // reproduced --- a style rule, which nothing hoistable belongs in anyway.
    let mut open: Vec<Option<String>> = Vec::new();
    let mut prelude_start = 0;
    let mut i = 0;

    while i < css.len() {
        if css[i..].starts_with("/*") {
            let end = css[i..].find("*/").map(|e| i + e + 2).unwrap_or(css.len());
            rest.push_str(&css[i..end]);
            i = end;
            continue;
        }
        let (c, width) = char_at(css, i);
        if c == '"' || c == '\'' {
            let end = skip_string(css, i);
            rest.push_str(&css[i..end]);
            i = end;
            continue;
        }

        if c == '@' {
            if GLOBAL_AT_RULES
                .iter()
                .any(|name| begins_with_ignoring_case(&css[i..], name))
            {
                let end = at_rule_end(css, i);
                let conditions: Vec<&str> = open
                    .iter()
                    .filter_map(|prelude| prelude.as_deref())
                    .collect();
                for condition in &conditions {
                    global.push_str(condition);
                    global.push_str(" {\n");
                }
                global.push_str(css[i..end].trim());
                global.push('\n');
                for _ in &conditions {
                    global.push_str("}\n");
                }
                i = end;
                continue;
            }
            // `@charset` is only meaningful as the very first bytes of a
            // stylesheet, which this no longer is.
            if begins_with_ignoring_case(&css[i..], "@charset") {
                i = at_rule_end(css, i);
                continue;
            }
        }

        match c {
            '{' => {
                let prelude = css[prelude_start..i].trim();
                open.push(
                    CONDITIONAL_AT_RULES
                        .iter()
                        .any(|name| begins_with_ignoring_case(prelude, name))
                        .then(|| prelude.to_string()),
                );
            }
            '}' => {
                open.pop();
            }
            _ => {}
        }
        rest.push(c);
        i += width;
        if matches!(c, '{' | '}' | ';') {
            prelude_start = i;
        }
    }

    (global, rest)
}

/// Case-insensitive prefix test that does not allocate
///
/// This runs at every `@` in a stylesheet that can be hundreds of kilobytes, so
/// lowercasing the remainder of the string to compare would be quadratic.
fn begins_with_ignoring_case(haystack: &str, needle: &str) -> bool {
    haystack.len() >= needle.len()
        && haystack.as_bytes()[..needle.len()].eq_ignore_ascii_case(needle.as_bytes())
}

/// The character beginning at `at`, and how many bytes it occupies
///
/// Every scanner below walks a byte offset while slicing `&str`, so the offset
/// must always move by whole characters. Reading the byte and calling it a
/// `char` instead both mangles non-ASCII text on the way out and leaves the
/// offset inside a multi-byte character, where the next slice panics.
fn char_at(text: &str, at: usize) -> (char, usize) {
    match text.get(at..).and_then(|rest| rest.chars().next()) {
        Some(c) => (c, c.len_utf8()),
        None => ('\0', 1),
    }
}

/// Index just past the end of the at-rule starting at `start`, whether it is
/// terminated by a block or a semicolon
fn at_rule_end(css: &str, start: usize) -> usize {
    let mut i = start;
    let mut depth = 0usize;

    while i < css.len() {
        if css[i..].starts_with("/*") {
            i = css[i..].find("*/").map(|e| i + e + 2).unwrap_or(css.len());
            continue;
        }
        let (c, width) = char_at(css, i);
        if c == '"' || c == '\'' {
            i = skip_string(css, i);
            continue;
        }
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return i + 1;
                }
            }
            ';' if depth == 0 => return i + 1,
            _ => {}
        }
        i += width;
    }
    css.len()
}

/// Index just past the closing quote of the string starting at `start`
fn skip_string(css: &str, start: usize) -> usize {
    let (quote, quote_width) = char_at(css, start);
    let mut i = start + quote_width;
    while i < css.len() {
        let (c, width) = char_at(css, i);
        match c {
            // A backslash escapes the whole character after it.
            '\\' => i += width + char_at(css, i + width).1,
            c if c == quote => return i + width,
            _ => i += width,
        }
    }
    css.len()
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
        assert!(css.contains("all: revert;"));
        assert!(css.contains(":scope { color: red }"));
    }

    /// The rule that broke the first deploy. A Google Fonts URL carries its
    /// weight list as `wght@0,300;400;700`, so an `@import` cannot be found by
    /// scanning to the first semicolon: doing that cut the URL in half and left
    /// `700;800&…swap');` behind, whose stray quote opened an unterminated
    /// string that swallowed the remaining 80% of the stylesheet.
    #[test]
    fn an_import_url_may_contain_semicolons() {
        let css = "@import url('https://fonts.googleapis.com/css2?family=Inter:wght@0,300;400;700&display=swap');\n\
                   :root { --x: 1px }\n.a { color: red }";

        let imports = find_imports(css);
        assert_eq!(imports.len(), 1);
        assert_eq!(
            imports[0].url,
            "https://fonts.googleapis.com/css2?family=Inter:wght@0,300;400;700&display=swap"
        );
        assert_eq!(imports[0].condition, "");

        // Removing it must leave the rest of the sheet intact and balanced.
        let without = splice(css, &imports, &[String::new()]);
        assert_eq!(without.trim(), ":root { --x: 1px }\n.a { color: red }");
        assert_eq!(
            without.matches('{').count(),
            without.matches('}').count(),
            "splicing left unbalanced braces:\n{without}"
        );
    }

    #[test]
    fn import_forms_and_conditions_are_understood() {
        assert_eq!(
            parse_import_prelude(r#"url("a.css")"#),
            Some(("a.css".into(), String::new()))
        );
        assert_eq!(
            parse_import_prelude(r#""a.css" screen and (min-width: 1px)"#),
            Some(("a.css".into(), "screen and (min-width: 1px)".into()))
        );
        // An `@import` inside a rule body is not a top-level import.
        assert!(
            find_imports(".a { background: url(x) } @media print { .b { color: red } }").is_empty()
        );
    }

    #[test]
    fn name_defining_at_rules_are_hoisted_out_of_the_scope_block() {
        let css = confine_css(
            "@charset \"UTF-8\";\
             @keyframes spin { from { transform: rotate(0) } to { transform: rotate(1turn) } }\
             @property --x { syntax: \"<length>\"; inherits: false; initial-value: 0px }\
             @font-face { font-family: F; src: url(f.woff2) }\
             .a { animation: spin 1s }",
        );

        let scope_at = css.find("@scope").unwrap();
        for hoisted in ["@keyframes spin", "@property --x", "@font-face"] {
            let at = css
                .find(hoisted)
                .unwrap_or_else(|| panic!("{hoisted} was dropped"));
            assert!(
                at < scope_at,
                "{hoisted} should sit before the @scope block"
            );
        }

        // The rules that actually style elements stay inside.
        assert!(css[scope_at..].contains(".a { animation: spin 1s }"));
        // `@charset` is only meaningful at the very start of a stylesheet.
        assert!(!css.contains("@charset"));
    }

    #[test]
    fn a_conditional_keyframes_rule_is_hoisted_with_its_condition() {
        let css = confine_css("@media (min-width: 1px) { @keyframes k { from { opacity: 0 } } }");
        let scope_at = css.find("@scope").unwrap();
        let hoisted = css.find("@keyframes k").unwrap();

        // It has to leave the scope block, because `@scope` would drop it...
        assert!(
            hoisted < scope_at,
            "conditional @keyframes stayed inside @scope:\n{css}"
        );
        // ...but not become unconditional on the way out.
        let wrapper = css.find("@media (min-width: 1px)").unwrap();
        assert!(wrapper < hoisted, "the condition was lost:\n{css}");
    }

    #[test]
    fn nested_conditions_are_rebuilt_in_order() {
        let css = confine_css(
            "@supports (display: grid) { @media print { @property --x { syntax: \"*\" } } }",
        );
        let hoisted = &css[..css.find("@scope").unwrap()];
        let supports = hoisted.find("@supports (display: grid)").unwrap();
        let media = hoisted.find("@media print").unwrap();
        let property = hoisted.find("@property --x").unwrap();
        assert!(
            supports < media && media < property,
            "wrong nesting:\n{hoisted}"
        );
    }

    /// A stylesheet is a poor place to lose rules, so what stays behind must
    /// still be the rest of the sheet, with balanced braces.
    #[test]
    fn hoisting_leaves_the_remaining_rules_intact() {
        let (global, rest) = hoist_global_at_rules(
            ".a { color: red }\
             @media print { @keyframes k { from { opacity: 0 } } .b { color: blue } }\
             .c { color: green }",
        );
        assert!(global.contains("@keyframes k"));
        assert!(!rest.contains("@keyframes"));
        for kept in [
            ".a { color: red }",
            ".b { color: blue }",
            ".c { color: green }",
        ] {
            assert!(rest.contains(kept), "{kept} was lost:\n{rest}");
        }
        assert_eq!(
            rest.matches('{').count(),
            rest.matches('}').count(),
            "{rest}"
        );
    }

    #[test]
    fn this_sites_own_rules_are_denied_the_subtree() {
        let css = confine_css(".a { color: red }");

        // Elements and pseudo-elements both, with the pseudo-elements written
        // outside the `:where()`: one inside is invalid, and the list is
        // forgiving, so it would be dropped without complaint and every
        // `::before` would go unreset --- which is where a colliding class name
        // does its worst.
        assert!(css.contains(":where(:scope, :scope *) { all: revert; }"));
        assert!(css.contains(":where(:scope, :scope *)::before"));
        assert!(css.contains(":where(:scope, :scope *)::after"));
        assert!(
            !css.contains("::before,") || !css.contains(":where(:scope, :scope *, "),
            "pseudo-elements must not sit inside the :where()"
        );

        // Ahead of the carried rules, so those still win on source order.
        assert!(css.find("all: revert").unwrap() < css.find(".a { color: red }").unwrap());
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

    /// WordPress keys stylesheet rules to classes it puts on the root elements
    /// --- `page-id-32677`, theme and plugin feature flags. Lifting only
    /// `.entry-content` leaves those ancestors behind, so `body.flag .block`
    /// stops matching and the block silently loses whatever it declared.
    #[test]
    fn the_pages_root_classes_come_across() {
        let extracted = extract(
            r#"<html class="no-js"><body class="page-id-7 stk-has-block-style-inheritance">
               <div class="entry-content"><p>x</p></div></body></html>"#,
            &Url::parse("https://pages.example.org/p/").unwrap(),
        )
        .unwrap();

        assert_eq!(
            extracted.root_classes,
            "no-js page-id-7 stk-has-block-style-inheritance"
        );
    }

    /// Per-block styles are emitted in the body, not only the head.
    #[test]
    fn styles_outside_the_head_are_carried_too() {
        let extracted = extract(
            r#"<html><head><style>.a { color: red }</style></head>
               <body><div class="entry-content"><p>x</p></div>
               <style>.b { color: blue }</style></body></html>"#,
            &Url::parse("https://pages.example.org/p/").unwrap(),
        )
        .unwrap();

        assert_eq!(
            extracted.inline_styles,
            vec![".a { color: red }", ".b { color: blue }"]
        );
    }

    #[test]
    fn relative_urls_resolve_against_their_source() {
        let base = Url::parse("https://pages.example.org/wp-content/themes/t/style.css").unwrap();
        assert_eq!(
            rewrite_css_urls("a { background: url(../img/x.png) }", &base),
            "a { background: url(https://pages.example.org/wp-content/themes/img/x.png) }"
        );
        assert_eq!(
            rewrite_markup_urls(r#"<img src="/wp-content/x.png">"#, &base),
            r#"<img src="https://pages.example.org/wp-content/x.png">"#
        );
        assert_eq!(
            rewrite_css_urls("a { background: url(data:image/png;base64,AAA) }", &base),
            "a { background: url(data:image/png;base64,AAA) }"
        );
        // A quoted `url()` may contain the closing parenthesis itself.
        assert_eq!(
            rewrite_css_urls(r#"a { background: url("a)b.png") }"#, &base),
            r#"a { background: url("https://pages.example.org/wp-content/themes/t/a)b.png") }"#
        );
    }

    /// Every scanner here walks a byte offset while slicing `&str`. Stepping a
    /// single byte at a time put the offset inside a multi-byte character and
    /// panicked the next slice --- the page content is full of typographic
    /// quotes, so this crashed the first deploy of the rewrite.
    #[test]
    fn multi_byte_characters_do_not_desync_the_scanners() {
        let base = Url::parse("https://pages.example.org/page/").unwrap();

        let prose = "<p>Girl Scouts’ progress — “quoted”, café, 日本語</p><img src=\"/x.png\">";
        assert_eq!(
            rewrite_markup_urls(prose, &base),
            "<p>Girl Scouts’ progress — “quoted”, café, 日本語</p>\
             <img src=\"https://pages.example.org/x.png\">"
        );

        let css = ".a::after { content: \"’—“café日本語\" } .b { background: url(x.png) }";
        assert_eq!(
            rewrite_css_urls(css, &base),
            ".a::after { content: \"’—“café日本語\" } \
             .b { background: url(https://pages.example.org/page/x.png) }"
        );

        // The same text passing through the confining pipeline, unharmed.
        let confined = confine_css("/* café ’ */ .a { content: \"—\" }");
        assert!(confined.contains("café ’"));
        assert!(confined.contains("content: \"—\""));

        // And a multi-byte character immediately after an escape.
        assert_eq!(
            rewrite_css_urls(r#".a { content: "\’x" }"#, &base),
            r#".a { content: "\’x" }"#
        );
    }

    #[test]
    fn only_real_attributes_are_rewritten() {
        let base = Url::parse("https://pages.example.org/page/").unwrap();

        // The same text inside a script body is not an attribute.
        assert_eq!(
            rewrite_markup_urls(r#"<script>var s = 'src="/x"';</script>"#, &base),
            r#"<script>var s = 'src="/x"';</script>"#
        );
        // Nor is it in prose, or in a comment.
        assert_eq!(
            rewrite_markup_urls(r#"<p>write src="/x" to link</p>"#, &base),
            r#"<p>write src="/x" to link</p>"#
        );
        assert_eq!(
            rewrite_markup_urls(r#"<!-- <img src="/x"> --><img src="/y">"#, &base),
            r#"<!-- <img src="/x"> --><img src="https://pages.example.org/y">"#
        );
        // Attributes whose names merely end in `src`/`href` are left alone.
        assert_eq!(
            rewrite_markup_urls(r#"<img data-src="/x" src="/y">"#, &base),
            r#"<img data-src="/x" src="https://pages.example.org/y">"#
        );
    }
}
