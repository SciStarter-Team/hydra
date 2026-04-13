use std::{collections::HashSet, net::IpAddr, sync::Arc};

use aho_corasick::AhoCorasick;
use common::{
    message::{
        log::{ExternalLinksRequest, ExternalLinksResponse},
        user::UserInfo,
    },
    service::{log_pageview, request},
};
use jiff::{ToSpan, Zoned};
use poem::{
    Endpoint, FromRequest, IntoResponse, Middleware, Response, session::Session, web::RealIp,
};
use regex::Regex;
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

        let uri = req.uri();
        let path = uri.path().to_string();
        let query = uri.query().map(|q| q.to_string());
        let user_info = req
            .extensions()
            .get::<Authenticated>()
            .cloned()
            .unwrap_or_default();

        let ip_address = RealIp::from_request_without_body(&req)
            .await
            .map(|RealIp(ip)| ip)
            .unwrap_or_default()
            .unwrap_or_else(|| IpAddr::from([0, 0, 0, 0]))
            .to_string();

        // Extract referrer header
        let referrer = req
            .headers()
            .get("referer")
            .and_then(|h| h.to_str().ok())
            .map(|s| s.to_string());

        let extensions = req.extensions_mut();
        extensions.insert(Device(Arc::new(device.clone())));
        extensions.insert(Interaction(Arc::new(interaction.clone())));

        // Call the endpoint first to get the response
        let output = self.wrapped.call(req).await?;
        let mut response = output.into_response();
        let status = response.status();

        // Skip logging for 404 Not Found responses
        if status != poem::http::StatusCode::NOT_FOUND {
            if let Err(e) = log_pageview(
                None, // when - let it use current time
                ip_address,
                &device,
                &interaction,
                user_info.user_id(),
                &path,
                referrer,
                query,
                None, // geolocation - let it resolve from IP
            )
            .await
            {
                tracing::warn!("Failed to log page view: {}", e);
            }
        }

        // Rewrite external links in HTML responses
        let is_html = response
            .headers()
            .get("content-type")
            .and_then(|ct| ct.to_str().ok())
            .map(|ct_str| ct_str.contains("text/html"))
            .unwrap_or(false);

        if is_html {
            if let Ok(body) = response.take_body().into_string().await {
                // Extract slug from path (remove leading slash)
                let mut slug = path.strip_prefix('/').unwrap_or(&path).to_string();

                // Handle index page specially
                if slug.is_empty() || slug == "index.html" {
                    slug = "index".to_string();
                }

                match rewrite_external_links(&body, &slug).await {
                    Ok(rewritten) => {
                        response = Response::builder()
                            .status(status)
                            .content_type("text/html; charset=utf-8")
                            .body(rewritten);
                    }
                    Err(e) => {
                        tracing::warn!("Failed to rewrite external links: {}", e);
                        // Return original body on error
                        response = Response::builder()
                            .status(status)
                            .content_type("text/html; charset=utf-8")
                            .body(body);
                    }
                }
            }
        }

        Ok(response)
    }
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

/// Rewrites external links in HTML to use tracked /-link/<ULID> URLs
async fn rewrite_external_links(html: &str, slug: &str) -> Result<String, anyhow::Error> {
    const DOMAIN: &str = env!("DOMAIN");

    // Find all URLs in <a> tag href attributes, excluding <link> tags
    // This regex matches: <a ...href="..."...> but not <link ...href="..."...>
    let url_regex = Regex::new(r#"<a\s[^>]*href=["']([^"']+)["'][^>]*>"#)?;

    // Collect unique external URLs
    let mut external_urls = HashSet::new();
    let domain_url = format!("https://{}", DOMAIN);

    for cap in url_regex.captures_iter(html) {
        if let Some(url_match) = cap.get(1) {
            let url = url_match.as_str();

            // Check if it's an external link:
            // - Must start with http:// or https://
            // - Must not start with https://<DOMAIN>
            if (url.starts_with("http://") || url.starts_with("https://"))
                && !url.starts_with(&domain_url)
            {
                external_urls.insert(url.to_string());
            }
        }
    }

    // If no external links found, return original HTML
    if external_urls.is_empty() {
        return Ok(html.to_string());
    }

    // Batch request ULIDs for all external URLs
    let urls: Vec<String> = external_urls.into_iter().collect();
    let req_data = ExternalLinksRequest {
        slug: slug.to_string(),
        urls,
    };

    let response: ExternalLinksResponse = request("log.links.get_or_create", &req_data).await?;

    // Build patterns and replacements for aho-corasick, constrained to href
    // attributes so that URL text content is not rewritten
    let mut patterns = Vec::new();
    let mut replacements = Vec::new();
    for (url, ulid) in &response.links {
        let tracked = format!("/-link/{}", ulid);
        for quote in ['"', '\''] {
            patterns.push(format!("href={quote}{url}{quote}"));
            replacements.push(format!("href={quote}{tracked}{quote}"));
        }
    }

    // Use aho-corasick for efficient parallel replacement
    let ac = AhoCorasick::new(&patterns)?;
    let result = ac.replace_all(html, &replacements);

    Ok(result)
}
