//! SciStarter's own social media posts, as collected by Curator.io and shown
//! on the Citizen Science Month pages.
//!
//! Curator's embeddable widget is a script that runs in the page, loads its
//! code, styles and every post's pictures from Curator's and the social
//! networks' servers, and is free to set what cookies it likes while there.
//! Instead, this server reads the feed from Curator's public API and renders
//! the posts itself, with their pictures served through
//! [`crate::remote_image`]. A visitor's browser never contacts Curator or any
//! network; following a post's link to the network is the visitor's choice.
//!
//! The posts are SciStarter's own, gathered by a feed staff moderate at
//! Curator, but their text is still HTML from elsewhere: it is reduced to
//! plain text here and escaped when shown, never interpreted.

use std::sync::{LazyLock, Mutex};
use std::time::Duration;

use anyhow::{Error, bail};
use poem::http::StatusCode;
use poem::web::{Html, Query};
use poem::{Endpoint, Response, Route, get, handler};
use sailfish::TemplateOnce;
use serde::Deserialize;

use crate::lru_cache::LruCache;

/// The feed, as Curator knows it.
const FEED_ID: &str = "9acd2fa2-8cb5-4bb8-aa94-c4e46af90e9f";

/// Posts shown at a time, as the widget showed them.
const PAGE_SIZE: usize = 12;

/// How long a page of the feed is kept before it is read again.
const FRESHNESS: Duration = Duration::from_secs(15 * 60);

/// Hosts the feed's pictures come from. A picture anywhere else is not shown,
/// since serving it would mean fetching from wherever the feed said.
const IMAGE_HOSTS: &[&str] = &[
    "curator-assets.b-cdn.net",
    "cdn-be.curator.io",
    "pbs.twimg.com",
];

/// Where [`remote_image`] serves a feed picture.
pub const IMAGE_PATH: &str = "/-social-image";

/// A post, ready to show.
#[derive(Debug, Clone)]
pub struct Post {
    /// The post on its network.
    pub url: String,
    /// Which network it was posted to: "Instagram", "Facebook" and so on.
    pub network: String,
    /// Its words, as plain text with the line breaks it was written with.
    pub text: String,
    /// Its picture as this site serves it, if it has one that can be.
    pub image: Option<String>,
    /// When it was posted, as an RFC 3339 timestamp.
    pub posted_at: String,
}

/// One page of the feed.
#[derive(Debug, Clone)]
pub struct FeedPage {
    pub posts: Vec<Post>,
    /// Where the next page starts, if there is one.
    pub next: Option<String>,
}

#[derive(Deserialize)]
struct ApiResponse {
    posts: Vec<ApiPost>,
    /// An object with an `after` cursor while there are more posts, and an
    /// empty array once there are not.
    #[serde(default)]
    pagination: serde_json::Value,
}

#[derive(Deserialize)]
struct ApiPost {
    #[serde(default)]
    url: String,
    #[serde(default)]
    network_name: String,
    #[serde(default)]
    text: String,
    #[serde(default)]
    is_html: i64,
    #[serde(default)]
    image: String,
    #[serde(default)]
    source_created_at: String,
}

impl From<ApiPost> for Post {
    fn from(post: ApiPost) -> Post {
        let text = if post.is_html != 0 {
            plain_text(&post.text)
        } else {
            post.text.trim().to_string()
        };

        Post {
            url: post.url,
            network: post.network_name,
            text,
            image: image_path(&post.image),
            posted_at: post.source_created_at,
        }
    }
}

/// Pages already read, by the cursor they start at ("" for the first).
static PAGES: LazyLock<Mutex<LruCache<FeedPage>>> =
    LazyLock::new(|| Mutex::new(LruCache::new(50, FRESHNESS)));

/// Whether `cursor` has the shape of one of Curator's: base64 of a small
/// JSON object. Anything else is refused rather than passed to Curator.
fn valid_cursor(cursor: &str) -> bool {
    !cursor.is_empty()
        && cursor.len() <= 256
        && cursor
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '=' | '-' | '_'))
}

/// The page of the feed starting at `after`, or the first page.
pub async fn page(after: Option<&str>) -> Result<FeedPage, Error> {
    let key = after.unwrap_or_default();

    if let Some(after) = after
        && !valid_cursor(after)
    {
        bail!("not a feed cursor: {after:?}");
    }

    if let Some(page) = PAGES.lock().expect("feed cache lock poisoned").get(key) {
        return Ok(page);
    }

    let mut url = format!("https://api.curator.io/v1/feeds/{FEED_ID}/posts?limit={PAGE_SIZE}");
    if let Some(after) = after {
        url.push_str("&after=");
        url.push_str(&urlencoding::encode(after));
    }

    let response: ApiResponse = reqwest::get(&url).await?.error_for_status()?.json().await?;

    let next = (!response.posts.is_empty())
        .then(|| {
            response
                .pagination
                .get("after")?
                .as_str()
                .map(str::to_string)
        })
        .flatten()
        .filter(|cursor| valid_cursor(cursor));

    let page = FeedPage {
        posts: response.posts.into_iter().map(Post::from).collect(),
        next,
    };

    PAGES
        .lock()
        .expect("feed cache lock poisoned")
        .insert(key.to_string(), page.clone());

    Ok(page)
}

/// Where this site serves the picture at `url`, or `None` if it is not one
/// of the feed's to serve.
fn image_path(url: &str) -> Option<String> {
    allowed_image(url).then(|| format!("{IMAGE_PATH}?src={}", urlencoding::encode(url)))
}

fn allowed_image(url: &str) -> bool {
    url::Url::parse(url).is_ok_and(|parsed| {
        parsed.scheme() == "https"
            && parsed.port().is_none()
            && parsed
                .host_str()
                .is_some_and(|host| IMAGE_HOSTS.contains(&host))
    })
}

/// The words of a post's HTML as plain text: tags dropped, entities decoded,
/// and the line breaks its `<br>`s and blocks made kept as newlines.
fn plain_text(html: &str) -> String {
    use scraper::{Html, Node};

    let fragment = Html::parse_fragment(html);
    let mut text = String::new();

    for node in fragment.root_element().descendants() {
        match node.value() {
            Node::Text(words) => {
                let hidden = node.ancestors().any(|ancestor| {
                    ancestor
                        .value()
                        .as_element()
                        .is_some_and(|element| matches!(element.name(), "script" | "style"))
                });
                if !hidden {
                    text.push_str(words);
                }
            }
            Node::Element(element) => match element.name() {
                "br" => text.push('\n'),
                "p" | "div" | "blockquote" | "li" | "h1" | "h2" | "h3" | "h4" => {
                    if !text.is_empty() && !text.ends_with('\n') {
                        text.push('\n');
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }

    // Each line trimmed, and runs of blank lines kept to one.
    let mut tidy = String::new();
    let mut blank_run = 0;
    for line in text.lines().map(str::trim) {
        if line.is_empty() {
            blank_run += 1;
            if blank_run > 1 {
                continue;
            }
        } else {
            blank_run = 0;
        }
        tidy.push_str(line);
        tidy.push('\n');
    }

    tidy.trim().to_string()
}

/// The feed's posts as cards in a grid, with a control at the end that
/// replaces itself with the next page's cards.
#[derive(TemplateOnce)]
#[template(path = "component/social_feed.stpl.html")]
pub struct SocialFeed<'a> {
    pub page: &'a FeedPage,
}

/// One page's cards and the control for the next, without the grid around
/// them: what the control is replaced with.
#[derive(TemplateOnce)]
#[template(path = "component/social_feed_page.stpl.html")]
pub struct SocialFeedPage<'a> {
    pub page: &'a FeedPage,
}

#[derive(Deserialize)]
struct MoreQuery {
    after: String,
}

/// The next page of cards, for the "show more" control.
#[handler]
async fn more_posts(Query(query): Query<MoreQuery>) -> poem::Result<Html<String>> {
    if !valid_cursor(&query.after) {
        return Err(StatusCode::BAD_REQUEST.into());
    }

    let page = page(Some(&query.after)).await.map_err(|error| {
        tracing::warn!("Reading the social feed failed: {error:#}");
        StatusCode::BAD_GATEWAY
    })?;

    let html = SocialFeedPage { page: &page }
        .render_once()
        .map_err(|error| {
            poem::Error::from_string(error.to_string(), StatusCode::INTERNAL_SERVER_ERROR)
        })?;

    Ok(Html(html))
}

#[derive(Deserialize)]
struct ImageQuery {
    src: String,
}

/// A feed picture, served from this site.
#[handler]
async fn serve_image(Query(query): Query<ImageQuery>) -> poem::Result<Response> {
    if !allowed_image(&query.src) {
        return Err(StatusCode::NOT_FOUND.into());
    }

    crate::remote_image::serve(&query.src).await
}

/// The route for the feed's pictures, mounted at [`IMAGE_PATH`] outside the
/// session middleware: a picture needs no session, and must not start one,
/// and unlike the pages around it may be cached.
pub fn image_route() -> impl Endpoint {
    get(serve_image)
}

/// Routes under `/-social-feed`.
pub fn routes() -> impl Endpoint {
    Route::new().at("/more", get(more_posts))
}

#[cfg(test)]
mod tests {
    use assert2::check;

    use super::*;

    #[test]
    fn html_posts_become_their_words() {
        check!(
            plain_text(
                "On April 29, you can help.<br />\n<br />\nJoin us &amp; <a href=\"https://x\">astronomers</a>!"
            ) == "On April 29, you can help.\n\nJoin us & astronomers!"
        );
        check!(plain_text("<p>One</p><p>Two</p><script>alert(1)</script>") == "One\nTwo");
    }

    #[test]
    fn only_the_feeds_own_image_hosts_are_served() {
        check!(allowed_image(
            "https://curator-assets.b-cdn.net/9acd2fa2-8cb5-4bb8-aa94-c4e46af90e9f/388224975.jpg?width=480"
        ));
        check!(allowed_image(
            "https://pbs.twimg.com/media/HHCA2VxXEAA-Azk.jpg:medium"
        ));

        check!(!allowed_image("http://curator-assets.b-cdn.net/a.jpg"));
        check!(!allowed_image(
            "https://curator-assets.b-cdn.net:8443/a.jpg"
        ));
        check!(!allowed_image(
            "https://scontent-iad3-2.xx.fbcdn.net/v/a.jpg"
        ));
        check!(!allowed_image("https://169.254.169.254/latest/meta-data"));
        check!(!allowed_image("not a url"));
    }

    #[test]
    fn cursors_are_only_curators_shape() {
        check!(valid_cursor(
            "eyJwaW5uZWQiOjAsInNvdXJjZV9jcmVhdGVkX2F0IjoiMjAyNi0wNC0yOFQxNzowMzoyMS4wMDAwMDBaIn0="
        ));
        check!(!valid_cursor(""));
        check!(!valid_cursor("abc&limit=1000"));
    }
}
