//! The site header, packaged for embedding on pages.scistarter.org.
//!
//! That site is WordPress and keeps its own layout, so this is deliberately a
//! separate artifact rather than a projection of [`crate::component::header`].
//! The site header can leave out identity and a way to log out because the
//! logged-in sidebar is always on the page beside it; on the blog it never is,
//! so this one carries them.
//!
//! It ships as a script rather than a fragment so the blog needs nothing but a
//! `<div id="ss-header">` and a `<script src>`: the markup goes into a shadow
//! root, which keeps the blog's stylesheet out and ours in. A classic script
//! tag is not subject to CORS, and the session cookie rides along because
//! pages.scistarter.org and scistarter.org share a registrable domain, which
//! makes the request same-site even though it is cross-origin.

use std::sync::Arc;

use common::message::user::{BasicInfo, UserInfo};
use poem::{Endpoint, Response, Result, Route, get, handler};
use sailfish::TemplateOnce;

use crate::middleware::authorization::Authenticated;

pub fn routes() -> impl Endpoint {
    Route::new().at("/header.js", get(header_js))
}

#[derive(TemplateOnce)]
#[template(path = "embed/header.stpl.html")]
struct EmbedHeader {
    user: Option<Arc<BasicInfo>>,
}

#[derive(TemplateOnce)]
#[template(path = "embed/header_js.stpl.html")]
struct EmbedHeaderJs {
    /// The header markup, already JSON-encoded, ready to be assigned to
    /// `shadow.innerHTML` in the script.
    header_html: String,
}

fn render_error(err: impl std::fmt::Display) -> poem::Error {
    tracing::error!("Failed to render the embedded header: {err}");
    poem::Error::from_string(
        err.to_string(),
        poem::http::StatusCode::INTERNAL_SERVER_ERROR,
    )
}

#[handler]
#[tracing::instrument(skip_all)]
async fn header_js(Authenticated(user): Authenticated) -> Result<Response> {
    let header_html = EmbedHeader { user }.render_once().map_err(render_error)?;

    let body = EmbedHeaderJs {
        header_html: serde_json::to_string(&header_html).map_err(render_error)?,
    }
    .render_once()
    .map_err(render_error)?;

    // The response carries the visitor's identity, so it must not be stored.
    // Everything under `dynamic()` already sets `Cache-Control: no-store`; the
    // `Vary` is here because the body differs by whether a session cookie came
    // with the request, and a cache that ignored `no-store` would otherwise
    // hand one visitor's header to another.
    Ok(Response::builder()
        .content_type("application/javascript; charset=utf-8")
        .header("Vary", "Cookie")
        .body(body))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ulid::Ulid;

    fn someone(handle: &str) -> Option<Arc<BasicInfo>> {
        Some(Arc::new(BasicInfo {
            user_id: Ulid::nil(),
            handle: handle.to_string(),
            is_staff: false,
            is_superuser: false,
            public: true,
            protected: false,
            needs_point: false,
        }))
    }

    fn render(user: Option<Arc<BasicInfo>>) -> String {
        EmbedHeader { user }.render_once().expect("header renders")
    }

    #[test]
    fn every_link_leaves_the_blog_for_the_main_site() {
        // The header is mounted on another origin, so a root-relative href
        // would resolve against the blog and 404.
        for html in [render(None), render(someone("birdwatcher"))] {
            for href in html.split("href=\"").skip(1) {
                let href = href.split('"').next().unwrap();
                assert!(
                    href.starts_with("https://") || href.starts_with('#'),
                    "{href:?} is not absolute"
                );
            }
        }
    }

    #[test]
    fn a_logged_out_visitor_is_offered_a_way_in() {
        let html = render(None);
        assert!(html.contains("https://scistarter.org/login"));
        assert!(html.contains("data-ss-auth"));
        assert!(!html.contains("Log Out"));
    }

    #[test]
    fn a_logged_in_visitor_gets_the_identity_the_sidebar_would_carry() {
        let html = render(someone("birdwatcher"));
        assert!(html.contains("Hi, birdwatcher"));
        assert!(html.contains("https://scistarter.org/logout"));
        assert!(html.contains("Dashboard"));
        // The log out link must be marked so the script can add the return path.
        let logout = html.split("/logout").nth(1).unwrap();
        assert!(logout.contains("data-ss-auth"));
    }

    #[test]
    fn a_handle_that_was_never_set_is_not_greeted_by_name() {
        let html = render(someone(""));
        assert!(!html.contains("Hi, "));
        // Still logged in, though.
        assert!(html.contains("https://scistarter.org/logout"));
    }

    #[test]
    fn the_nav_is_the_agreed_list() {
        let html = render(None);
        for (label, href) in [
            ("Girl Scouts", "https://scistarter.org/girlscouts-info"),
            ("Libraries", "https://scistarter.org/library"),
            ("Trainings", "https://scistarter.org/training"),
            ("Ambassadors", "https://scistarter.org/ambassadors"),
            ("Projects and Events", "https://scistarter.org/finder"),
            ("Communities", "https://scistarter.org/communities"),
            ("Donate", "https://scistarter.org/donate"),
        ] {
            assert!(html.contains(href), "{label} link missing");
            assert!(html.contains(label), "{label} label missing");
        }
    }

    #[test]
    fn print_for_eyeballing() {
        println!("<<<HEADER_LOGGED_OUT\n{}\nHEADER_LOGGED_OUT", render(None));
        println!(
            "<<<HEADER_LOGGED_IN\n{}\nHEADER_LOGGED_IN",
            render(someone("birdwatcher"))
        );
    }
}
