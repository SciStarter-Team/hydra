//! The public Citizen and Community Science Library Network sign-up form.
//!
//! Ported from orrery's `/l/library-network-signup`, at the same path so links
//! already in the wild keep working after the cutover. Deliberately a
//! standalone page rather than one wearing the site layout, matching orrery:
//! it is handed out as a link on its own.
//!
//! Submitting records the signup and, for a signed-in visitor, advances their
//! Libraries-as-Hubs training to `supplemental`.

use anyhow::Error;
use common::message::dashboard::LibrarySignupUpdate;
use common::message::user::{
    WorkflowProgressRequest, WorkflowProgressResponse, WorkflowType,
};
use common::service::request;
use poem::{
    handler,
    web::{Form, Query, Redirect},
};
use sailfish::TemplateOnce;
use serde::Deserialize;

use crate::middleware::authorization::Authenticated;

/// Spellings that count as consent to appear on the map.
///
/// Kept in step with orrery's `frontend.models.affirmative`: the value reaches
/// us from a checkbox here, but from spreadsheet-shaped imports elsewhere, so
/// "yes", "Y" and "TRUE" all count, in any capitalization. Anything else,
/// including nothing at all, is a no — consent has to be given, not merely
/// not-refused.
pub fn affirmative(value: Option<&str>) -> bool {
    matches!(
        value.unwrap_or("").trim().to_ascii_lowercase().as_str(),
        "y" | "yes" | "true"
    )
}

#[derive(Debug, Default, Deserialize)]
pub struct SignupPageQuery {
    /// Where to send the visitor afterwards. Local paths only.
    #[serde(default)]
    pub next: Option<String>,
}

#[derive(TemplateOnce)]
#[template(path = "library_network_signup.stpl.html")]
struct SignupForm {
    email: String,
    next: String,
    organization_types: &'static [&'static str],
    job_types: &'static [&'static str],
}

/// Kept in step with `LibraryNetworkSignup.ORGANIZATION_TYPES` in orrery.
const ORGANIZATION_TYPES: &[&str] = &[
    "Public Library",
    "State Library",
    "Academic Library",
    "Community-based Organization (ex. YMC, 4-H, Girl Scouts)",
    "School (K-12)",
    "University",
    "Museum",
    "Business",
    "Other Non-Profit",
];

/// Kept in step with `LibraryNetworkSignup.JOB_TYPES` in orrery.
const JOB_TYPES: &[&str] = &[
    "Children's/Teen Librarian",
    "Adult Services Librarian",
    "Library Assistant",
    "Library Manager/Administrator",
    "Program Director (Museum, Afterschool Program, Community-Based Organization)",
    "Program Educator (Museum, Afterschool Program, Community-Based Organization)",
    "Outreach Coordinator (Museum, Afterschool Program, Community-Based Organization)",
    "Other",
];

/// Only local paths are accepted, so the form cannot be turned into an open
/// redirect by whoever hands out the link.
///
/// A leading `/` is not sufficient: `//evil.example` also starts with one and
/// a browser reads it as another host entirely. Same rule as
/// [`crate::gourl::GoUrl::is_valid_target`].
fn safe_next(next: Option<String>) -> String {
    next.filter(|n| crate::auth::is_local_next(n))
        .unwrap_or_else(|| String::from("/library"))
}

#[handler]
pub async fn index(
    Authenticated(user_info): Authenticated,
    Query(query): Query<SignupPageQuery>,
) -> Result<poem::Response, Error> {
    let html = SignupForm {
        email: user_info
            .as_ref()
            .map(|u| u.handle.clone())
            .unwrap_or_default(),
        next: safe_next(query.next),
        organization_types: ORGANIZATION_TYPES,
        job_types: JOB_TYPES,
    }
    .render_once()?;

    Ok(poem::Response::builder()
        .content_type("text/html; charset=utf-8")
        .body(html))
}

#[derive(Debug, Deserialize)]
pub struct SignupSubmission {
    #[serde(default)]
    pub next: Option<String>,
    pub email: String,
    pub first_name: String,
    pub last_name: String,
    pub job_title: String,
    #[serde(default)]
    pub have_account: Option<String>,
    pub organization_name: String,
    pub organization_type: String,
    pub primary_job_type: String,
    #[serde(default)]
    pub primary_job_type_other: Option<String>,
    pub organization_street_address: String,
    pub organization_city: String,
    pub organization_state: String,
    pub organization_zip_code: String,
    pub organization_country: String,
    #[serde(default)]
    pub organization_country_other: Option<String>,
    pub organization_web_page: String,
    #[serde(default)]
    pub organization_social_handles: Option<String>,
    #[serde(default)]
    pub add_to_map: Option<String>,
}

/// "Other" on the two radio groups means the free-text box beside it holds the
/// real answer, exactly as orrery's view resolves it.
fn resolve_other(choice: String, other: Option<String>) -> String {
    if choice == "Other" {
        other.unwrap_or_default()
    } else {
        choice
    }
}

#[handler]
pub async fn submit(
    Authenticated(user_info): Authenticated,
    Form(form): Form<SignupSubmission>,
) -> Result<Redirect, Error> {
    let next = safe_next(form.next);
    let user_ulid = user_info.as_ref().map(|u| u.user_id.to_string());

    let _: () = request(
        "user.library.create",
        LibrarySignupUpdate {
            signup_id: String::new(),
            user_ulid: user_ulid.clone(),
            email: form.email,
            first_name: form.first_name,
            last_name: form.last_name,
            job_title: form.job_title,
            // Unparseable or absent reads as "I don't know", the same default
            // orrery's view applies.
            have_account: form
                .have_account
                .and_then(|v| v.trim().parse::<i64>().ok())
                .unwrap_or(2),
            organization_name: form.organization_name,
            organization_type: form.organization_type,
            primary_job_type: resolve_other(form.primary_job_type, form.primary_job_type_other),
            organization_street_address: form.organization_street_address,
            organization_city: form.organization_city,
            organization_state: form.organization_state,
            organization_zip_code: form.organization_zip_code,
            organization_country: resolve_other(
                form.organization_country,
                form.organization_country_other,
            ),
            organization_web_page: form.organization_web_page,
            organization_social_handles: form.organization_social_handles.unwrap_or_default(),
            add_to_map: affirmative(form.add_to_map.as_deref()),
        },
    )
    .await?;

    // Signing up is the supplemental step of the Libraries-as-Hubs training.
    // Best-effort: a training hiccup must not cost the visitor their signup.
    if let Some(user_id) = user_ulid {
        let recorded: Result<WorkflowProgressResponse, _> = request(
            "user.workflow.progress",
            WorkflowProgressRequest {
                user_id,
                workflow: WorkflowType::LibrariesHubs,
                step: "supplemental".to_string(),
            },
        )
        .await;

        if let Err(e) = recorded {
            tracing::error!("Failed to record libraries-hubs supplemental step: {e}");
        }
    }

    Ok(Redirect::see_other(next))
}

#[cfg(test)]
mod tests {
    use super::{affirmative, safe_next};

    #[test]
    fn consent_accepts_every_spelling_orrery_does() {
        for value in ["y", "Y", "yes", "Yes", "YES", "true", "True", "TRUE", " Yes "] {
            assert!(affirmative(Some(value)), "{value:?}");
        }
    }

    #[test]
    fn anything_else_is_a_no() {
        for value in ["n", "no", "false", "0", "1", "maybe", "", "   "] {
            assert!(!affirmative(Some(value)), "{value:?}");
        }
        assert!(!affirmative(None), "an unticked box sends nothing at all");
    }

    #[test]
    fn next_only_accepts_local_paths() {
        assert_eq!(safe_next(Some("/library".into())), "/library");
        assert_eq!(safe_next(Some("/library?joined=1".into())), "/library?joined=1");
        assert_eq!(safe_next(None), "/library");
    }

    /// The link is handed out by third parties, so `next` is attacker-shaped
    /// input. A protocol-relative value is the one that slips past a naive
    /// "starts with a slash" check.
    #[test]
    fn next_rejects_offsite_redirects() {
        for hostile in [
            "https://evil.example",
            "//evil.example",
            "///evil.example",
            "http://evil.example",
            "evil.example",
        ] {
            assert_eq!(safe_next(Some(hostile.into())), "/library", "{hostile:?}");
        }
    }
}
