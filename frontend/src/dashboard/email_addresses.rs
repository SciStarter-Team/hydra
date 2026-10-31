//! The account's email addresses on the settings page. Projects that report
//! participation by email address credit it to whichever account has the
//! address, so an account holder adds the addresses they use elsewhere here.
//! An address joins the account at once, trusted as far as the account is,
//! and is shown unconfirmed only once that trust is lost (a bounce, say); an
//! unconfirmed address can be mailed the link that confirms it. One
//! its owner asked to add through the stricter `user.emails.request_add`,
//! which waits for a mailed link to be followed, is listed as waiting and can
//! be added now instead. The addresses and the links are user-service's.

use anyhow::{Error, anyhow};
use common::message::user::{
    AccountEmails, EmailAdd, EmailAdded, EmailDrop, EmailDropped, EmailVerificationResend,
    ResendEmailVerification,
};
use poem::web::{Form, Html};
use poem::{IntoResponse, Response, handler};
use sailfish::TemplateOnce;
use serde::Deserialize;
use ulid::Ulid;

use crate::middleware::authorization::Authenticated;

/// The Email Addresses section, which replaces itself when an address is
/// added, dropped, or sent its confirmation link.
#[derive(TemplateOnce)]
#[template(path = "dashboard/_partials/email_addresses_section.stpl.html")]
pub struct EmailAddressesSection {
    emails: AccountEmails,

    /// What the address box holds: empty, or an address just refused.
    entered: String,

    notice: Option<Notice>,
}

/// What the section says about the last thing asked of it.
enum Notice {
    Done(String),
    Refused(String),
}

impl EmailAddressesSection {
    /// The section as the account's addresses stand.
    pub async fn load(user_id: Ulid) -> Self {
        Self {
            emails: list(user_id).await,
            entered: String::new(),
            notice: None,
        }
    }
}

async fn list(user_id: Ulid) -> AccountEmails {
    common::service::request("user.emails.account", user_id.to_string())
        .await
        .unwrap_or_default()
}

fn added_notice(added: EmailAdded, email: &str) -> Notice {
    match added {
        EmailAdded::Added => Notice::Done(format!("Added {email}.")),
        EmailAdded::Invalid => {
            Notice::Refused("That doesn't look like an email address.".to_string())
        }
        EmailAdded::AlreadyYours => {
            Notice::Refused("That address is already on your account.".to_string())
        }
        EmailAdded::Taken => Notice::Refused(
            "That address belongs to another SciStarter account. If it's yours, write to us at info@scistarter.org."
                .to_string(),
        ),
    }
}

fn confirmation_notice(resent: EmailVerificationResend, email: &str) -> Option<Notice> {
    match resent {
        EmailVerificationResend::Sent => Some(Notice::Done(format!(
            "We sent a link to {email}. Follow it to confirm the address."
        ))),
        EmailVerificationResend::TooSoon => Some(Notice::Refused(
            "We sent a link to that address less than a minute ago. Check its inbox (and its spam folder)."
                .to_string(),
        )),
        EmailVerificationResend::LimitReached => Some(Notice::Refused(
            "We've sent several links to that address today. Please check its spam folder, or write to us at info@scistarter.org."
                .to_string(),
        )),
        EmailVerificationResend::Undeliverable => Some(Notice::Refused(
            "Our email to that address is not getting through. Please write to us at info@scistarter.org."
                .to_string(),
        )),
        // Gone from the account meanwhile; the section as it now stands says so.
        EmailVerificationResend::NotOnAccount => None,
    }
}

#[derive(Debug, Deserialize)]
pub struct EmailForm {
    #[serde(default)]
    email: String,
}

/// POST `/dashboard/emails` -- add an address to the account.
#[handler]
pub async fn add(
    Authenticated(user_info): Authenticated,
    Form(form): Form<EmailForm>,
) -> Result<Response, Error> {
    let user_info = user_info.ok_or_else(|| anyhow!("Login required"))?;
    let user_id = user_info.user_id;
    let email = form.email.trim().to_string();

    let added: EmailAdded = common::service::request(
        "user.emails.add_own",
        EmailAdd {
            user_id,
            email: email.clone(),
        },
    )
    .await?;

    let notice = added_notice(added, &email);

    let section = EmailAddressesSection {
        emails: list(user_id).await,
        entered: match notice {
            Notice::Refused(_) => email,
            Notice::Done(_) => String::new(),
        },
        notice: Some(notice),
    };

    Ok(Html(section.render_once()?).into_response())
}

/// POST `/dashboard/emails/confirm` -- mail one of the account's addresses
/// the link that confirms it.
#[handler]
pub async fn confirm(
    Authenticated(user_info): Authenticated,
    Form(form): Form<EmailForm>,
) -> Result<Response, Error> {
    let user_info = user_info.ok_or_else(|| anyhow!("Login required"))?;
    let user_id = user_info.user_id;

    let resent: EmailVerificationResend = common::service::request(
        "user.auth.resend_email_verification",
        ResendEmailVerification {
            user_id,
            email: Some(form.email.clone()),
        },
    )
    .await?;

    let section = EmailAddressesSection {
        emails: list(user_id).await,
        entered: String::new(),
        notice: confirmation_notice(resent, &form.email),
    };

    Ok(Html(section.render_once()?).into_response())
}

/// POST `/dashboard/emails/remove` -- take an address off the account, or
/// stop waiting for one.
#[handler]
pub async fn remove(
    Authenticated(user_info): Authenticated,
    Form(form): Form<EmailForm>,
) -> Result<Response, Error> {
    let user_info = user_info.ok_or_else(|| anyhow!("Login required"))?;
    let user_id = user_info.user_id;

    let dropped: EmailDropped = common::service::request(
        "user.emails.drop",
        EmailDrop {
            user_id,
            email: form.email,
        },
    )
    .await?;

    let notice = match dropped {
        EmailDropped::Dropped | EmailDropped::NotFound => None,
        EmailDropped::Primary => Some(Notice::Refused(
            "Your primary address stays on your account.".to_string(),
        )),
    };

    let section = EmailAddressesSection {
        emails: list(user_id).await,
        entered: String::new(),
        notice,
    };

    Ok(Html(section.render_once()?).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert2::check;
    use common::message::user::AccountEmail;

    fn section(notice: Option<Notice>) -> String {
        EmailAddressesSection {
            emails: AccountEmails {
                addresses: vec![
                    AccountEmail {
                        email: "ada@example.invalid".to_string(),
                        verified: true,
                        primary: true,
                    },
                    AccountEmail {
                        email: "countess@example.invalid".to_string(),
                        verified: false,
                        primary: false,
                    },
                ],
                pending: vec!["lovelace@example.invalid".to_string()],
            },
            entered: String::new(),
            notice,
        }
        .render_once()
        .unwrap()
    }

    #[test]
    fn only_addresses_other_than_the_primary_can_be_removed() {
        let html = section(None);

        check!(html.contains("ada@example.invalid"));
        check!(!html.contains(r#"name="email" value="ada@example.invalid""#));
        check!(html.contains(r#"name="email" value="countess@example.invalid""#));
        check!(html.contains("Primary"));
        check!(html.contains("Unconfirmed"));
    }

    #[test]
    fn only_unconfirmed_addresses_offer_a_confirmation_link() {
        let html = section(None);

        check!(
            html.matches(r#"hx-post="/dashboard/emails/confirm""#)
                .count()
                == 1
        );
        check!(html.contains("Send confirmation"));
    }

    #[test]
    fn an_address_waiting_on_its_link_can_be_added_now_or_cancelled() {
        let html = section(None);

        check!(html.contains("lovelace@example.invalid"));
        check!(
            html.matches(r#"name="email" value="lovelace@example.invalid""#)
                .count()
                == 2
        );
        check!(html.contains(r#"hx-post="/dashboard/emails""#));
        check!(html.contains(r#"hx-post="/dashboard/emails/remove""#));
        check!(html.contains("Add now"));
    }

    #[test]
    fn a_refusal_is_an_alert_and_an_addition_is_not() {
        check!(section(Some(Notice::Refused("No.".to_string()))).contains(r#"role="alert""#));

        let added = section(Some(Notice::Done("Added.".to_string())));
        check!(added.contains(r#"role="status""#));
        check!(!added.contains(r#"role="alert""#));
    }
}
