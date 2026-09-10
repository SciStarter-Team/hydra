//! Shared Articulate/Storyline xAPI (TinCan) launch-URL construction.
//!
//! Wraps a course's base URL with the query parameters the SciStarter LRS
//! (`~/work/scistarter/scistarter-lrs`) and our `record_event_articulate`
//! callback expect: `endpoint` (the LRS), `actor` (an xAPI Agent identifying
//! the user by ULID), `auth` (a `Bearer` JWT signed with `SCISTARTER_JWT_SECRET`
//! whose `email` claim carries the same ULID), an optional `activity` label,
//! `tincan=true`, and — for newer content that requires it — an optional
//! `activity_id` plus a derived `registration`. This mirrors orrery's
//! `articulate_link()` protocol, adapted to hydra's ULID user ids.
//!
//! Used by both the EvolveMe Foundations flow ([`crate::evolveme`]) and the
//! Citizen Science Champion training ([`crate::training::champion`]).

use std::sync::Arc;

use common::message::user::BasicInfo;

/// The shared SciStarter Learning Record Store the Storyline packages report to.
pub const LRS_URL: &str = "https://li287-138.members.linode.com/lrs/";

/// Where a SCORM Driver package's LMS-connected entry point lives, relative to
/// the package directory. This is the URL a training launches.
const SCORM_DRIVER_ENTRY: &str = "scormdriver/indexAPI.html";

/// Where the same package's LMS-free entry point lives. The exporter always
/// ships both, and the package names this one itself when it warns that
/// nothing will be saved.
const STANDALONE_ENTRY: &str = "scormcontent/index.html";

/// The entry point of `launch_url`'s package that renders without an LMS
/// behind it.
///
/// A SCORM Driver wrapper displays nothing at all until its xAPI handshake
/// succeeds: given no launch context it leaves its content frame on
/// `blank.html` indefinitely -- a blank page, with no error and no spinner.
/// So a visitor with no account, who has no launch context to be given and no
/// badge to earn, must be sent here instead of to a link that cannot work.
///
/// Anything that is not a SCORM Driver launch URL is returned unchanged: the
/// older Rise exports have one entry point that runs either way, and a module
/// URL edited in `/-admin/trainings` can be any shape at all.
pub fn standalone_url(launch_url: &str) -> std::borrow::Cow<'_, str> {
    let (path, rest) = split_path(launch_url);

    match path.strip_suffix(SCORM_DRIVER_ENTRY) {
        Some(package) => std::borrow::Cow::Owned(format!("{package}{STANDALONE_ENTRY}{rest}")),
        None => std::borrow::Cow::Borrowed(launch_url),
    }
}

/// Split a URL into the part before any query or fragment, and the rest.
///
/// A module URL is normally a bare path, but it is editable in
/// `/-admin/trainings`, so it can arrive carrying either.
fn split_path(url: &str) -> (&str, &str) {
    match url.find(['?', '#']) {
        Some(at) => url.split_at(at),
        None => (url, ""),
    }
}

/// Attach `query` to `url` so the result is still the URL it was meant to be.
///
/// The launch parameters have to reach the page as a query string, which means
/// two things a plain `format!("{url}?{query}")` gets wrong. A module URL that
/// already carries a query needs `&`, or the second `?` makes the whole tail a
/// single opaque parameter value and no launch parameter is readable. And a
/// module URL carrying a fragment -- a Rise deep link like `#/lessons/intro` --
/// needs the query inserted *before* it, because everything after `#` is the
/// fragment and never reaches the server or `location.search`.
///
/// Neither shape occurs in the built-in constants. Both are a paste away in
/// the admin, and the failure in each case is a module that launches with no
/// launch context at all: the learner finishes, and nothing is recorded.
pub fn with_query(url: &str, query: &str) -> String {
    if query.is_empty() {
        return url.to_string();
    }

    let (base, fragment) = match url.find('#') {
        Some(at) => url.split_at(at),
        None => (url, ""),
    };

    let separator = if !base.contains('?') {
        "?"
    } else if base.ends_with('?') || base.ends_with('&') {
        ""
    } else {
        "&"
    };

    format!("{base}{separator}{query}{fragment}")
}

/// Parameters for an Articulate Storyline LRS launch.
#[derive(Debug)]
pub struct ArticulateParams {
    lrs_url: String,
    user: Arc<BasicInfo>,
    activity: Option<String>,
    activity_id: Option<String>,
    jwt: String,
}

impl ArticulateParams {
    /// Build launch parameters for `user` reporting to the LRS at `lrs_url`
    /// (usually [`LRS_URL`]).
    pub fn new(lrs_url: &str, user: Arc<BasicInfo>) -> Self {
        use jsonwebtoken::{EncodingKey, Header, encode};
        use serde::Serialize;

        #[derive(Serialize)]
        struct ArticulateJwtClaims {
            email: String,
        }

        // Generate JWT with email field containing ULID@scistarter.org
        let claims = ArticulateJwtClaims {
            email: format!("{}@scistarter.org", user.user_id),
        };

        let jwt_secret = env!("SCISTARTER_JWT_SECRET");
        let jwt = encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(jwt_secret.as_bytes()),
        )
        .unwrap_or_else(|_| String::from(""));

        Self {
            lrs_url: lrs_url.to_string(),
            user,
            activity: None,
            activity_id: None,
            jwt,
        }
    }

    /// Attach an optional `activity` label to the launch.
    pub fn with_activity(mut self, activity: &str) -> Self {
        self.activity = Some(activity.to_string());
        self
    }

    /// Attach the launch `activity_id` (and a derived `registration`) so newer
    /// Articulate content emits its assessment-result (passed/scored)
    /// statements. Pass the content's own baked-in activity IRI.
    pub fn with_activity_id(mut self, activity_id: &str) -> Self {
        self.activity_id = Some(activity_id.to_string());
        self
    }
}

impl std::fmt::Display for ArticulateParams {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Build query string for Articulate Storyline
        let mut params = vec![
            format!("endpoint={}", urlencoding::encode(&self.lrs_url)),
            format!(
                "actor={}",
                urlencoding::encode(
                    &serde_json::json!({
                        "objectType": "Agent",
                        "name": self.user.handle,
                        "mbox": format!("mailto:{}@scistarter.org", self.user.user_id),
                    })
                    .to_string()
                )
            ),
            format!(
                "auth={}",
                urlencoding::encode(&format!("Bearer {}", self.jwt))
            ),
        ];

        if let Some(activity) = &self.activity {
            params.push(format!("activity={}", urlencoding::encode(activity)));
        }

        // Newer Articulate content only emits its assessment-result
        // (passed/scored) statements when launched with the full standard xAPI
        // launch context: an activity_id identifying the activity plus a
        // registration grouping the attempt's statements. Older content reports
        // results without them, so these are attached only when a caller
        // supplies an activity_id, leaving existing launches unchanged. The
        // registration is derived deterministically from the user and activity
        // so re-launches resume the same attempt without persisting anything.
        if let Some(activity_id) = &self.activity_id {
            let registration = uuid::Uuid::new_v5(
                &uuid::Uuid::NAMESPACE_URL,
                format!("{}:{}", self.user.user_id, activity_id).as_bytes(),
            );
            params.push(format!("activity_id={}", urlencoding::encode(activity_id)));
            params.push(format!("registration={}", registration));
        }

        params.push("tincan=true".to_string());

        write!(f, "{}", params.join("&"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The two entry points of one package, which is what makes the
    /// derivation safe: they are fixed paths the exporter always writes, not
    /// two URLs someone has to keep in step.
    #[test]
    fn a_scorm_driver_launch_url_yields_its_packages_standalone_entry() {
        assert_eq!(
            standalone_url("https://media.scistarter.org/dataethics-es/scormdriver/indexAPI.html"),
            "https://media.scistarter.org/dataethics-es/scormcontent/index.html",
        );
    }

    /// A Rise export is one file that runs with or without an LMS, so there
    /// is nothing to swap -- and an unrecognized URL is left alone rather
    /// than rewritten into one that would 404.
    #[test]
    fn anything_else_is_left_alone() {
        for url in [
            "https://media.scistarter.org/foundations/index.html",
            "https://media.scistarter.org/dataliteracy/es/index.html",
            "#",
            "",
        ] {
            assert_eq!(standalone_url(url), url);
        }
    }

    /// An admin-edited URL can carry a query or a deep link, and the entry
    /// point is still the thing being swapped: the tail belongs to whoever
    /// wrote it and comes along.
    #[test]
    fn a_query_or_fragment_survives_the_swap() {
        assert_eq!(
            standalone_url("https://media/pkg/scormdriver/indexAPI.html?trace=1"),
            "https://media/pkg/scormcontent/index.html?trace=1",
        );
        assert_eq!(
            standalone_url("https://media/pkg/scormdriver/indexAPI.html#/lessons/intro"),
            "https://media/pkg/scormcontent/index.html#/lessons/intro",
        );
    }

    /// The ordinary case, and the whole reason the launch parameters are a
    /// query at all.
    #[test]
    fn a_bare_url_takes_the_query_with_a_question_mark() {
        assert_eq!(
            with_query("https://media/pkg/scormdriver/indexAPI.html", "a=1&b=2"),
            "https://media/pkg/scormdriver/indexAPI.html?a=1&b=2",
        );
    }

    /// A second `?` would make the entire tail one opaque parameter value,
    /// and every launch parameter unreadable.
    #[test]
    fn an_existing_query_is_extended_rather_than_restarted() {
        assert_eq!(
            with_query("https://media/pkg/index.html?v=2", "a=1"),
            "https://media/pkg/index.html?v=2&a=1",
        );
        assert_eq!(
            with_query("https://media/pkg/index.html?", "a=1"),
            "https://media/pkg/index.html?a=1",
        );
        assert_eq!(
            with_query("https://media/pkg/index.html?v=2&", "a=1"),
            "https://media/pkg/index.html?v=2&a=1",
        );
    }

    /// Everything after `#` is the fragment: a query appended past it never
    /// reaches the server or `location.search`, so the module would launch
    /// with no launch context and record nothing.
    #[test]
    fn a_fragment_stays_at_the_end() {
        assert_eq!(
            with_query("https://media/pkg/index.html#/lessons/intro", "a=1"),
            "https://media/pkg/index.html?a=1#/lessons/intro",
        );
        assert_eq!(
            with_query("https://media/pkg/index.html?v=2#/lessons/intro", "a=1"),
            "https://media/pkg/index.html?v=2&a=1#/lessons/intro",
        );
    }

    /// Nothing to add, nothing to change -- and no stray separator.
    #[test]
    fn an_empty_query_leaves_the_url_alone() {
        assert_eq!(
            with_query("https://media/pkg/index.html", ""),
            "https://media/pkg/index.html",
        );
    }
}
