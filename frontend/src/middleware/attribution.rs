//! First-touch attribution: where a visitor came from, captured on the first
//! page they load and recorded on the account they go on to create.
//!
//! The capture rules match orrery's (`src/attribution.rs` there), so an
//! account means the same thing whichever codebase created it:
//!
//! - only a browser loading a page into a tab is a landing — not an asset,
//!   an XHR, an embedded frame, or anything but a GET;
//! - `referer` is the landing request's `Referer` header;
//! - `origin` is the landing page's `?origin=` parameter when it has one,
//!   otherwise its decoded path;
//! - `utm_*` are the landing page's query parameters of those names, the
//!   last one winning when a name repeats.
//!
//! The capture lives in the session, so it lasts as long as the session
//! does, and is read back by [`first_touch`] when a signup is made.

use common::message::user::Attribution;
use poem::{Endpoint, IntoResponse, Middleware, Request, Response, http::Method, session::Session};

const SESSION_KEY: &str = "first_touch";

/// The most bytes each value may keep, as in orrery. The session is stored
/// server side, but the values are whatever a client sent.
const REFERER_LIMIT: usize = 1500;
const ORIGIN_LIMIT: usize = 600;
const UTM_LIMIT: usize = 300;

/// The attribution captured for this session, or empty values if none was
/// (a client that keeps no session cookie, say).
pub fn first_touch(session: &Session) -> Attribution {
    session.get::<Attribution>(SESSION_KEY).unwrap_or_default()
}

/// The longest prefix of `value`, cut at a character boundary, of at most
/// `limit` bytes.
fn truncate(value: &str, limit: usize) -> String {
    let mut end = value.len().min(limit);

    while !value.is_char_boundary(end) {
        end -= 1;
    }

    value[..end].to_string()
}

/// Whether a request is a browser loading a page into a tab. Browsers say so
/// with `Sec-Fetch-Dest`; for clients that don't send it, a GET that accepts
/// HTML is the closest available stand-in.
fn is_navigation(req: &Request) -> bool {
    if req.method() != Method::GET {
        return false;
    }

    match req.headers().get("sec-fetch-dest") {
        Some(dest) => dest.as_bytes() == b"document",
        None => req
            .headers()
            .get(poem::http::header::ACCEPT)
            .and_then(|accept| accept.to_str().ok())
            .is_some_and(|accept| accept.contains("text/html")),
    }
}

/// The attribution of a landing request.
fn from_landing(req: &Request) -> Attribution {
    let query: Vec<(String, String)> = req
        .uri()
        .query()
        .map(|query| {
            url::form_urlencoded::parse(query.as_bytes())
                .into_owned()
                .collect()
        })
        .unwrap_or_default();

    let param = |name: &str| {
        query
            .iter()
            .rev()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    };

    let referer = req
        .headers()
        .get(poem::http::header::REFERER)
        .map(|value| String::from_utf8_lossy(value.as_bytes()).into_owned())
        .unwrap_or_default();

    let path = urlencoding::decode_binary(req.uri().path().as_bytes());
    let path = String::from_utf8_lossy(&path);

    let utm = |name: &str| truncate(param(name).unwrap_or_default(), UTM_LIMIT);

    Attribution {
        referer: truncate(&referer, REFERER_LIMIT),
        origin: truncate(param("origin").unwrap_or(&path), ORIGIN_LIMIT),
        utm_source: utm("utm_source"),
        utm_medium: utm("utm_medium"),
        utm_campaign: utm("utm_campaign"),
        utm_term: utm("utm_term"),
        utm_content: utm("utm_content"),
    }
}

/// Captures a visitor's first touch into their session on their first page
/// load. Must be installed inside the session middleware.
pub struct FirstTouchMiddleware;

pub struct FirstTouchMiddlewareEndpoint<E> {
    wrapped: E,
}

impl<E: Endpoint> Middleware<E> for FirstTouchMiddleware {
    type Output = FirstTouchMiddlewareEndpoint<E>;

    fn transform(&self, ep: E) -> Self::Output {
        FirstTouchMiddlewareEndpoint { wrapped: ep }
    }
}

impl<E: Endpoint> Endpoint for FirstTouchMiddlewareEndpoint<E> {
    type Output = Response;

    async fn call(&self, req: Request) -> poem::Result<Self::Output> {
        if let Some(session) = req.extensions().get::<Session>() {
            if session.get::<Attribution>(SESSION_KEY).is_none() && is_navigation(&req) {
                session.set(SESSION_KEY, from_landing(&req));
            }
        } else {
            tracing::error!("FirstTouchMiddleware: Session not found in request");
        }

        Ok(self.wrapped.call(req).await?.into_response())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(uri: &str, headers: &[(&str, &str)]) -> Request {
        let mut builder = Request::builder().uri(uri.parse().unwrap());

        for (name, value) in headers {
            builder = builder.header(*name, *value);
        }

        builder.finish()
    }

    #[test]
    fn a_document_load_is_a_navigation() {
        assert!(is_navigation(&request(
            "/",
            &[("sec-fetch-dest", "document")]
        )));
    }

    #[test]
    fn frames_assets_and_xhrs_are_not_navigations() {
        for dest in ["iframe", "image", "script", "style", "empty", "font"] {
            let req = request("/", &[("sec-fetch-dest", dest), ("accept", "text/html")]);
            assert!(!is_navigation(&req), "{dest}");
        }
    }

    #[test]
    fn without_fetch_metadata_a_get_accepting_html_is_a_navigation() {
        assert!(is_navigation(&request("/", &[("accept", "text/html,*/*")])));
        assert!(!is_navigation(&request(
            "/",
            &[("accept", "application/json")]
        )));
        assert!(!is_navigation(&request("/", &[])));
    }

    #[test]
    fn only_a_get_is_a_navigation() {
        let req = Request::builder()
            .method(Method::POST)
            .uri("/".parse().unwrap())
            .header("sec-fetch-dest", "document")
            .finish();
        assert!(!is_navigation(&req));
    }

    #[test]
    fn a_landing_records_referer_path_and_utm_tags() {
        let touch = from_landing(&request(
            "/project/123?utm_source=newsletter&utm_medium=email&utm_campaign=csm&utm_term=t&utm_content=c&x=1",
            &[("referer", "https://www.google.com/")],
        ));

        assert_eq!(
            touch,
            Attribution {
                referer: "https://www.google.com/".to_string(),
                origin: "/project/123".to_string(),
                utm_source: "newsletter".to_string(),
                utm_medium: "email".to_string(),
                utm_campaign: "csm".to_string(),
                utm_term: "t".to_string(),
                utm_content: "c".to_string(),
            }
        );
    }

    #[test]
    fn an_origin_parameter_overrides_the_path_even_when_empty() {
        assert_eq!(
            from_landing(&request("/login?origin=evolveme", &[])).origin,
            "evolveme"
        );
        assert_eq!(from_landing(&request("/login?origin=", &[])).origin, "");
    }

    #[test]
    fn a_repeated_parameter_keeps_its_last_value() {
        let touch = from_landing(&request("/?utm_source=first&utm_source=last", &[]));
        assert_eq!(touch.utm_source, "last");
    }

    #[test]
    fn the_path_is_recorded_decoded() {
        let touch = from_landing(&request("/citizen%20science/caf%C3%A9", &[]));
        assert_eq!(touch.origin, "/citizen science/café");
    }

    #[test]
    fn values_are_cut_at_a_character_boundary() {
        let kept = truncate(&"é".repeat(400), UTM_LIMIT);
        assert_eq!(kept.len(), UTM_LIMIT);
        assert!(kept.chars().all(|ch| ch == 'é'));

        assert_eq!(
            truncate(&format!("a{}", "é".repeat(400)), UTM_LIMIT).len(),
            UTM_LIMIT - 1
        );
    }
}
