use std::{net::IpAddr, sync::Arc};

use common::message::user::UserInfo;
use common::service::log_pageview;
use jiff::{ToSpan, Zoned};
use poem::{
    Endpoint, FromRequest, IntoResponse, Middleware, Response, handler,
    http::{Method, StatusCode},
    session::Session,
    web::{Json, RealIp},
};
use serde::Deserialize;
use ulid::Ulid;

use super::authorization::Authenticated;

#[derive(Debug, Clone)]
pub struct Interaction(Arc<String>);

impl<'a> FromRequest<'a> for Interaction {
    async fn from_request(
        req: &'a poem::Request,
        _body: &mut poem::RequestBody,
    ) -> poem::Result<Self> {
        Ok(req
            .extensions()
            .get::<Interaction>()
            .cloned()
            .expect("AnalyticsMiddleware must be installed to use Interaction"))
    }
}

impl AsRef<str> for Interaction {
    fn as_ref(&self) -> &str {
        self.0.as_str()
    }
}

impl From<Interaction> for String {
    fn from(value: Interaction) -> Self {
        value.0.to_string()
    }
}

#[derive(Debug, Clone)]
pub struct Device(Arc<String>);

impl<'a> FromRequest<'a> for Device {
    async fn from_request(
        req: &'a poem::Request,
        _body: &mut poem::RequestBody,
    ) -> poem::Result<Self> {
        Ok(req
            .extensions()
            .get::<Device>()
            .cloned()
            .expect("AnalyticsMiddleware must be installed to use Device"))
    }
}

impl AsRef<str> for Device {
    fn as_ref(&self) -> &str {
        self.0.as_str()
    }
}

impl From<Device> for String {
    fn from(value: Device) -> Self {
        value.0.to_string()
    }
}

/// Records views and interaction sequences
pub struct AnalyticsMiddleware;

pub struct AnalyticsMiddlewareEndpoint<E> {
    wrapped: E,
}

impl<E: Endpoint> Middleware<E> for AnalyticsMiddleware {
    type Output = AnalyticsMiddlewareEndpoint<E>;

    fn transform(&self, ep: E) -> Self::Output {
        AnalyticsMiddlewareEndpoint { wrapped: ep }
    }
}

impl<E: Endpoint> Endpoint for AnalyticsMiddlewareEndpoint<E> {
    type Output = Response;

    async fn call(&self, mut req: poem::Request) -> poem::Result<Self::Output> {
        let Some(session) = req.extensions().get::<Session>() else {
            tracing::error!("AnalyticsMiddleware: Session not found in request");
            let output = self.wrapped.call(req).await?;
            return Ok(output.into_response());
        };

        let device = session_device(session);
        let interaction = session_interaction(session);

        let view = if is_page_request(&req) {
            let uri = req.uri();
            Some(PageView {
                ip_address: RealIp::from_request_without_body(&req)
                    .await
                    .ok()
                    .and_then(|RealIp(ip)| ip),
                device: device.clone(),
                interaction: interaction.clone(),
                user_id: req
                    .extensions()
                    .get::<Authenticated>()
                    .and_then(|user| user.user_id()),
                path: uri.path().to_string(),
                query: uri.query().map(str::to_string),
                referrer: req
                    .headers()
                    .get("referer")
                    .and_then(|h| h.to_str().ok())
                    .map(str::to_string),
            })
        } else {
            None
        };

        let extensions = req.extensions_mut();
        extensions.insert(Device(Arc::new(device)));
        extensions.insert(Interaction(Arc::new(interaction)));

        let response = self.wrapped.call(req).await?.into_response();

        // Only a page that was actually served counts: a redirect is counted
        // at its destination, and an error showed the visitor nothing.
        //
        // External-link clickthroughs are not captured here; a delegated
        // `sendBeacon` click handler (see static/js/pagelife.js) reports
        // them to `/-clickthrough`, which keeps links direct and keeps this
        // middleware off the response body.
        if let Some(view) = view
            && response.status().is_success()
        {
            view.record();
        }

        Ok(response)
    }
}

/// One person looking at one page.
struct PageView {
    ip_address: Option<IpAddr>,
    device: String,
    interaction: String,
    user_id: Option<Ulid>,
    path: String,
    query: Option<String>,
    referrer: Option<String>,
}

impl PageView {
    /// Sends the view to the log service off the response path. Analytics
    /// must never add latency to page rendering, and geolocation resolution
    /// does a NATS round-trip, so this is spawned fire-and-forget.
    fn record(self) {
        tokio::spawn(async move {
            if let Err(e) = log_pageview(
                None, // when - let it use current time
                self.ip_address
                    .unwrap_or_else(|| IpAddr::from([0, 0, 0, 0]))
                    .to_string(),
                self.device,
                self.interaction,
                self.user_id,
                self.path,
                self.referrer,
                self.query,
                None, // geolocation - let it resolve from IP
            )
            .await
            {
                tracing::warn!("Failed to log page view: {}", e);
            }
        });
    }
}

/// Body of the beacon `static/js/pagelife.js` sends when a page the browser
/// fetched speculatively is shown.
#[derive(Debug, Deserialize)]
pub struct ActivationBeacon {
    /// `location.pathname` of the page.
    path: String,
    /// `location.search` without its leading `?`; empty when there is none.
    query: String,
    /// `document.referrer` of the page; empty when there is none.
    referrer: String,
}

/// Counts a view of a page that reached the browser through a prefetch or a
/// prerender.
///
/// [`is_page_request`] refuses to count those requests when they are served,
/// since nobody may ever look at the result, and the browser makes no further
/// request when somebody does. So the page reports it from the client instead,
/// and this records it as the middleware would have. The beacon's own request
/// is a POST, which the middleware never counts.
#[handler]
pub async fn record_activated_view(
    req: &poem::Request,
    user: Authenticated,
    RealIp(ip_address): RealIp,
    device: Device,
    interaction: Interaction,
    Json(beacon): Json<ActivationBeacon>,
) -> StatusCode {
    if !beacon.path.starts_with('/') {
        return StatusCode::BAD_REQUEST;
    }

    let user_agent = req
        .headers()
        .get("User-Agent")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();

    if is_human_browser(user_agent) {
        let non_empty = |s: String| (!s.is_empty()).then_some(s);
        PageView {
            ip_address,
            device: device.into(),
            interaction: interaction.into(),
            user_id: user.user_id(),
            path: beacon.path,
            query: non_empty(beacon.query),
            referrer: non_empty(beacon.referrer),
        }
        .record();
    }

    StatusCode::NO_CONTENT
}

/// Whether `req` is a person's browser asking for a page to look at, as
/// opposed to a crawler, a script, a background fetch from a page that is
/// already open, a form submission or a speculative prefetch. Only such
/// requests are counted as page views.
fn is_page_request(req: &poem::Request) -> bool {
    let headers = req.headers();
    let header = |name: &str| headers.get(name).and_then(|v| v.to_str().ok());

    if req.method() != Method::GET {
        return false;
    }

    // An htmx request swaps a fragment into a page that has already been
    // counted. Browsers that send `Sec-Fetch-Dest` would also mark it `empty`
    // below, but older ones send nothing.
    if headers.contains_key("HX-Request") {
        return false;
    }

    // Navigations are `document` (or a frame, when a page is embedded), while
    // fetch and XHR are `empty`. Its absence says nothing either way, since
    // browsers predating Fetch Metadata never send it.
    if let Some(dest) = header("Sec-Fetch-Dest")
        && !matches!(dest, "document" | "iframe" | "frame")
    {
        return false;
    }

    // A prefetched or prerendered page may never be shown at all. `Purpose`
    // is the older spelling of `Sec-Purpose`.
    if [header("Sec-Purpose"), header("Purpose")]
        .into_iter()
        .flatten()
        .any(|purpose| purpose.contains("prefetch"))
    {
        return false;
    }

    is_human_browser(header("User-Agent").unwrap_or_default())
}

/// Known browser engine tokens (lowercase). If none of these appear in the
/// user-agent the request is almost certainly not from a real browser.
///
/// This list and [`SCRAPER_TOKENS`] are kept identical to orrery's
/// (`is_human_browser` in its `src/common.rs`), so that view counts from the
/// two sites measure the same thing.
const BROWSER_TOKENS: &[&str] = &[
    "chrome/", "firefox/", "safari/", "edg/", "opr/", "opera/", "msie ", "trident/", "crios/",
    "fxios/",
];

/// Known scraper/bot tokens (lowercase). If any of these appear in the
/// user-agent the request is from an automated client, not a human.
const SCRAPER_TOKENS: &[&str] = &[
    "bot",
    "crawl",
    "spider",
    "slurp",
    "googlebot",
    "bingbot",
    "yandex",
    "baiduspider",
    "duckduckbot",
    "facebookexternalhit",
    "facebot",
    "twitterbot",
    "linkedinbot",
    "pinterestbot",
    "whatsapp",
    "telegrambot",
    "discordbot",
    "slackbot",
    "applebot",
    "semrush",
    "ahrefs",
    "mj12bot",
    "dotbot",
    "petalbot",
    "bytespider",
    "gptbot",
    "chatgpt",
    "claudebot",
    "anthropic",
    "ccbot",
    "dataforseo",
    "sogou",
    "exabot",
    "ia_archiver",
    "archive.org_bot",
    "curl",
    "wget",
    "httpx",
    "python-requests",
    "python-urllib",
    "go-http-client",
    "java/",
    "libwww",
    "lwp-trivial",
    "scrapy",
    "mechanize",
    "httpclient",
    "node-fetch",
    "axios",
    "headlesschrome",
    "phantomjs",
    "selenium",
    "uptimerobot",
    "pingdom",
    "statuscake",
    "site24x7",
];

/// Whether `user_agent` names a browser driven by a person: it identifies a
/// browser engine and nothing that marks it as automated.
fn is_human_browser(user_agent: &str) -> bool {
    let ua = user_agent.to_lowercase();
    let is_browser = BROWSER_TOKENS.iter().any(|t| ua.contains(t));
    let is_scraper = SCRAPER_TOKENS.iter().any(|t| ua.contains(t));
    is_browser && !is_scraper
}

/// Ensures that the session contains a unique interaction id, and
/// returns it
#[tracing::instrument]
fn session_interaction(session: &Session) -> String {
    fn new(session: &Session) -> String {
        let interaction = Ulid::new().to_string();
        let expires = (Zoned::now().timestamp() + 1.hour()).to_string();
        session.set("interaction", (&interaction, &expires));
        interaction
    }

    if let Some((interaction, expires)) = session.get::<(String, String)>("interaction") {
        let Ok(expires) = expires.parse() else {
            return new(session);
        };

        let now = Zoned::now().timestamp();

        if now > expires {
            return new(session);
        }

        session.set("interaction", (&interaction, (now + 1.hour()).to_string()));
        interaction
    } else {
        new(session)
    }
}

/// Ensures that the session contains a unique device id, and
/// returns it
#[tracing::instrument]
fn session_device(session: &Session) -> String {
    if let Some(device) = session.get::<String>("device") {
        device
    } else {
        let device = Ulid::new().to_string();
        session.set("device", &device);
        device
    }
}

#[cfg(test)]
mod tests {
    use assert2::check;
    use poem::{Request, http::Method};

    use super::{is_human_browser, is_page_request};

    const DESKTOP_CHROME: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
         (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36";
    const IPHONE_SAFARI: &str = "Mozilla/5.0 (iPhone; CPU iPhone OS 18_1 like Mac OS X) \
         AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.1 Mobile/15E148 Safari/604.1";
    const FIREFOX: &str = "Mozilla/5.0 (X11; Linux x86_64; rv:132.0) Gecko/20100101 Firefox/132.0";
    const GOOGLEBOT_SMARTPHONE: &str = "Mozilla/5.0 (Linux; Android 6.0.1; Nexus 5X Build/MMB29P) \
         AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Mobile Safari/537.36 \
         (compatible; Googlebot/2.1; +http://www.google.com/bot.html)";
    const GPTBOT: &str = "Mozilla/5.0 AppleWebKit/537.36 (KHTML, like Gecko; compatible; \
         GPTBot/1.2; +https://openai.com/gptbot)";
    const CLAUDEBOT: &str = "Mozilla/5.0 AppleWebKit/537.36 (KHTML, like Gecko; compatible; \
         ClaudeBot/1.0; +claudebot@anthropic.com)";
    const HEADLESS_CHROME: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 \
         (KHTML, like Gecko) HeadlessChrome/130.0.0.0 Safari/537.36";

    #[test]
    fn browsers_are_human() {
        check!(is_human_browser(DESKTOP_CHROME));
        check!(is_human_browser(IPHONE_SAFARI));
        check!(is_human_browser(FIREFOX));
    }

    #[test]
    fn crawlers_and_scripts_are_not_human() {
        check!(!is_human_browser(GOOGLEBOT_SMARTPHONE));
        check!(!is_human_browser(GPTBOT));
        check!(!is_human_browser(CLAUDEBOT));
        check!(!is_human_browser(HEADLESS_CHROME));
        check!(!is_human_browser("curl/8.10.1"));
        check!(!is_human_browser("python-requests/2.32.3"));
        check!(!is_human_browser(""));
    }

    fn navigation(user_agent: &str) -> poem::RequestBuilder {
        Request::builder()
            .method(Method::GET)
            .uri_str("/projects/example")
            .header("User-Agent", user_agent)
    }

    #[test]
    fn a_browser_navigation_is_a_page_request() {
        check!(is_page_request(&navigation(DESKTOP_CHROME).finish()));
        check!(is_page_request(
            &navigation(DESKTOP_CHROME)
                .header("Sec-Fetch-Dest", "document")
                .finish()
        ));
        check!(is_page_request(
            &navigation(DESKTOP_CHROME)
                .header("Sec-Fetch-Dest", "iframe")
                .finish()
        ));
    }

    #[test]
    fn a_crawler_navigation_is_not_a_page_request() {
        check!(!is_page_request(&navigation(GOOGLEBOT_SMARTPHONE).finish()));
        check!(!is_page_request(&navigation(GPTBOT).finish()));
    }

    #[test]
    fn a_background_fetch_is_not_a_page_request() {
        check!(!is_page_request(
            &navigation(DESKTOP_CHROME)
                .header("HX-Request", "true")
                .finish()
        ));
        check!(!is_page_request(
            &navigation(DESKTOP_CHROME)
                .header("Sec-Fetch-Dest", "empty")
                .finish()
        ));
    }

    #[test]
    fn a_prefetch_is_not_a_page_request() {
        check!(!is_page_request(
            &navigation(DESKTOP_CHROME)
                .header("Sec-Fetch-Dest", "document")
                .header("Sec-Purpose", "prefetch;prerender")
                .finish()
        ));
        check!(!is_page_request(
            &navigation(DESKTOP_CHROME)
                .header("Purpose", "prefetch")
                .finish()
        ));
    }

    #[test]
    fn only_get_is_a_page_request() {
        for method in [Method::POST, Method::HEAD] {
            let req = Request::builder()
                .method(method)
                .uri_str("/projects/example")
                .header("User-Agent", DESKTOP_CHROME)
                .finish();
            check!(!is_page_request(&req));
        }
    }
}
