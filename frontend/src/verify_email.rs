//! Confirming an account's email address: the banner an unconfirmed account
//! is shown under the site header, the endpoint behind its "send it again"
//! link, and the link in the confirmation email itself.
//!
//! The codes, the limits on asking for another, and what being unverified
//! costs an account all belong to the user service
//! (`user-service/src/email_verification.rs`); this is only its face.

use std::sync::Arc;

use common::{
    message::user::{
        BasicInfo, EmailVerificationResend, ResendEmailVerification, UserEmailInfo,
        UserEmailInfoRequest, VerifiedEmail, VerifyEmail,
    },
    service::request,
};
use poem::{
    IntoResponse, Response, handler,
    http::StatusCode,
    web::{Html, Query, Redirect},
};
use sailfish::TemplateOnce;

use crate::{
    component::{Component, Head, StandardLayout},
    i18n::tr,
    middleware::authorization::Authenticated,
};

/// The notice under the site header for a signed-in account whose email
/// address is not yet confirmed.
///
/// What confirming unlocks is explained in the confirmation email itself, so
/// the banner only asks, offers to send the email again, and says what to do
/// when it does not arrive.
#[derive(Debug, TemplateOnce)]
#[template(path = "site/verify_banner.stpl.html")]
pub struct VerifyBanner {
    /// The address the confirmation email goes to.
    email: String,
    status: ResendStatus,
}

impl VerifyBanner {
    /// The banner to show this visitor, if they are owed one.
    ///
    /// Asked of the user service on every page rather than remembered in the
    /// session, because the link is often followed on another device, and
    /// the banner should be gone from this one the next time it loads a page.
    pub async fn for_user(user: Option<&Arc<BasicInfo>>) -> Option<VerifyBanner> {
        let user_id = user?.user_id;

        let info: UserEmailInfo = match request(
            "user.info.email_info",
            UserEmailInfoRequest { user_id },
        )
        .await
        {
            Ok(info) => info,
            Err(e) => {
                // A page without the banner is better than no page.
                tracing::warn!(%user_id, "Could not learn whether the account is verified: {e}");
                return None;
            }
        };

        if info.verified {
            return None;
        }

        Some(VerifyBanner {
            email: info.primary_email,
            status: ResendStatus::new(ResendState::Idle),
        })
    }
}

/// Where the banner's offer to send the confirmation email again stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResendState {
    /// Nothing has been asked for yet.
    Idle,
    Sent,
    TooSoon,
    LimitReached,
    Undeliverable,
    NotLoggedIn,
    Failed,
}

impl From<EmailVerificationResend> for ResendState {
    fn from(outcome: EmailVerificationResend) -> Self {
        match outcome {
            EmailVerificationResend::Sent => ResendState::Sent,
            EmailVerificationResend::TooSoon => ResendState::TooSoon,
            EmailVerificationResend::LimitReached => ResendState::LimitReached,
            EmailVerificationResend::Undeliverable => ResendState::Undeliverable,
            // The banner names no address, so its own is the account's.
            EmailVerificationResend::NotOnAccount => ResendState::Failed,
        }
    }
}

impl ResendState {
    /// What the banner says in this state.
    fn message(self) -> Option<String> {
        match self {
            ResendState::Idle => None,
            ResendState::Sent => Some(tr("Sent! Check your inbox (and your spam folder).")),
            ResendState::TooSoon => Some(tr(
                "We sent one less than a minute ago, so check your inbox (and your spam folder).",
            )),
            ResendState::LimitReached => Some(tr(
                "We've sent several today. Please check your spam folder, or write to us at info@scistarter.org.",
            )),
            ResendState::Undeliverable => Some(tr(
                "Our email to this address is not getting through. Please write to us at info@scistarter.org.",
            )),
            ResendState::NotLoggedIn => Some(tr("Your session has ended. Please log in again.")),
            ResendState::Failed => Some(tr("We couldn't send it just now.")),
        }
    }

    /// The wording of the link that asks for the email again, in the states
    /// where asking again is any use.
    fn link(self) -> Option<String> {
        match self {
            ResendState::Idle => Some(tr("Click here to resend a confirmation email.")),
            ResendState::TooSoon => Some(tr("Click here to send another.")),
            ResendState::Failed => Some(tr("Click here to try again.")),
            ResendState::Sent
            | ResendState::LimitReached
            | ResendState::Undeliverable
            | ResendState::NotLoggedIn => None,
        }
    }
}

/// The part of the banner that reports on, and offers, sending the
/// confirmation email again. Asking swaps a fresh one in over the last.
#[derive(Debug, TemplateOnce)]
#[template(path = "site/verify_banner_status.stpl.html")]
struct ResendStatus {
    message: Option<String>,
    link: Option<String>,
}

impl ResendStatus {
    fn new(state: ResendState) -> Self {
        ResendStatus {
            message: state.message(),
            link: state.link(),
        }
    }
}

/// POST `/resend-verification` -- send the signed-in account's confirmation
/// email again, and answer with what the banner should now say.
///
/// Always answers 200: the outcome is the markup, and a refusal is as much
/// something to show as a success is.
#[handler]
#[tracing::instrument]
pub async fn resend_verification_action(
    Authenticated(user_info): Authenticated,
) -> Result<Response, anyhow::Error> {
    let state = match user_info.as_deref() {
        None => ResendState::NotLoggedIn,
        Some(user) => {
            match request::<EmailVerificationResend>(
                "user.auth.resend_email_verification",
                ResendEmailVerification {
                    user_id: user.user_id,
                    email: None,
                },
            )
            .await
            {
                Ok(outcome) => outcome.into(),
                Err(e) => {
                    tracing::error!(user_id = %user.user_id, "Resending the confirmation email failed: {e}");
                    ResendState::Failed
                }
            }
        }
    };

    Ok(Html(ResendStatus::new(state).render_once()?).into_response())
}

#[derive(TemplateOnce)]
#[template(path = "auth/email_verified.stpl.html")]
struct EmailVerifiedPage {
    head: <Head as Component>::Prepared,
    layout: <StandardLayout<EmailVerifiedContent> as Component>::Prepared,
}

#[derive(TemplateOnce)]
#[template(path = "auth/email_verified_content.stpl.html")]
struct EmailVerifiedContent {
    logged_in: bool,
}

#[derive(TemplateOnce)]
#[template(path = "auth/verify_link_expired.stpl.html")]
struct VerifyLinkExpiredPage {
    head: <Head as Component>::Prepared,
    layout: <StandardLayout<VerifyLinkExpiredContent> as Component>::Prepared,
}

#[derive(TemplateOnce)]
#[template(path = "auth/verify_link_expired_content.stpl.html")]
struct VerifyLinkExpiredContent {
    logged_in: bool,
}

#[derive(Debug, serde::Deserialize)]
struct VerifyParameters {
    code: Option<String>,
}

/// GET `/verify?code=` -- the link in the confirmation email. A code that is
/// still good confirms the address it was mailed to and lands on
/// `/verified`; any other gets the page that says the link is spent.
///
/// The visitor need not be signed in: whoever holds the code reads that
/// address's mail, which is all the link is there to establish.
#[handler]
#[tracing::instrument]
pub async fn verify_email_action(
    Authenticated(user_info): Authenticated,
    Query(params): Query<VerifyParameters>,
) -> Result<Response, anyhow::Error> {
    let code = params.code.unwrap_or_default();

    if !code.is_empty() {
        let verified: Option<VerifiedEmail> =
            request("user.auth.verify_email", VerifyEmail { code }).await?;

        if verified.is_some() {
            // Redirected, so that reloading the page does not present a code
            // that is spent by then.
            return Ok(Redirect::see_other("/verified").into_response());
        }
    }

    let logged_in = user_info.is_some();

    let layout = StandardLayout::new("/verify", VerifyLinkExpiredContent { logged_in })
        .prepare(user_info.clone())
        .await?;

    let mut response = VerifyLinkExpiredPage {
        head: Head::default().prepare(None).await?,
        layout,
    }
    .into_response(None)
    .await?;

    response.set_status(StatusCode::GONE);

    Ok(response)
}

/// GET `/verified` -- where a confirmation link that worked ends up.
#[handler]
#[tracing::instrument]
pub async fn email_verified_page(
    Authenticated(user_info): Authenticated,
) -> Result<Response, anyhow::Error> {
    let logged_in = user_info.is_some();

    let layout = StandardLayout::new("/verified", EmailVerifiedContent { logged_in })
        .prepare(user_info.clone())
        .await?;

    EmailVerifiedPage {
        head: Head::default().prepare(None).await?,
        layout,
    }
    .into_response(None)
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    const STATES: [ResendState; 7] = [
        ResendState::Idle,
        ResendState::Sent,
        ResendState::TooSoon,
        ResendState::LimitReached,
        ResendState::Undeliverable,
        ResendState::NotLoggedIn,
        ResendState::Failed,
    ];

    /// The status is all the banner has to say for itself after a click, so
    /// no state may leave it blank.
    #[test]
    fn every_state_has_something_to_show() {
        for state in STATES {
            assert!(
                state.message().is_some() || state.link().is_some(),
                "{state:?} shows nothing"
            );
        }
    }

    /// Offering to send again is only honest where sending again could work.
    #[test]
    fn the_link_is_offered_only_where_asking_again_can_help() {
        let offered: Vec<ResendState> = STATES
            .into_iter()
            .filter(|state| state.link().is_some())
            .collect();

        assert_eq!(
            offered,
            vec![ResendState::Idle, ResendState::TooSoon, ResendState::Failed]
        );
    }

    #[test]
    fn the_status_renders_its_link_as_a_request_to_resend() {
        let html = ResendStatus::new(ResendState::Idle).render_once().unwrap();

        assert!(html.contains(r#"hx-post="/resend-verification""#));
        assert!(html.contains("Click here to resend a confirmation email."));

        let html = ResendStatus::new(ResendState::Sent).render_once().unwrap();

        assert!(!html.contains("hx-post"));
        assert!(html.contains("Sent!"));
    }

    #[test]
    fn the_banner_names_the_address_and_escapes_it() {
        let html = VerifyBanner {
            email: "ada+<b>@example.invalid".to_string(),
            status: ResendStatus::new(ResendState::Idle),
        }
        .render_once()
        .unwrap();

        assert!(html.contains("ada+&lt;b&gt;@example.invalid"));
        assert!(!html.contains("<b>@example.invalid"));
    }
}
