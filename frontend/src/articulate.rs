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
