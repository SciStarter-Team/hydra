//! The account's email addresses on the settings page. Projects that report
//! participation by email address credit it to whichever account has the
//! address, so an account holder adds the addresses they use elsewhere here.
//! An address joins the account when the link mailed to it is followed; the
//! addresses and the links are user-service's.

use anyhow::{Error, anyhow};
use common::message::user::{
    AccountEmails, EmailAddRequest, EmailAddRequested, EmailDrop, EmailDropped,
};
use poem::web::{Form, Html};
use poem::{IntoResponse, Response, handler};
use sailfish::TemplateOnce;
use serde::Deserialize;
use ulid::Ulid;

use crate::middleware::authorization::Authenticated;

/// The Email Addresses section, which replaces itself when an address is
/// asked for or dropped.
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

fn requested_notice(requested: EmailAddRequested, email: &str) -> Notice {
    match requested {
        EmailAddRequested::Sent => Notice::Done(format!(
            "We sent a link to {email}. Follow it to add the address to your account."
        )),
        EmailAddRequested::Invalid => {
            Notice::Refused("That doesn't look like an email address.".to_string())
        }
        EmailAddRequested::AlreadyYours => {
            Notice::Refused("That address is already on your account.".to_string())
        }
        EmailAddRequested::Taken => Notice::Refused(
            "That address belongs to another SciStarter account. If it's yours, write to us at info@scistarter.org."
                .to_string(),
        ),
        EmailAddRequested::TooSoon => Notice::Refused(
            "We sent a link to that address less than a minute ago. Check its inbox (and its spam folder)."
                .to_string(),
        ),
        EmailAddRequested::LimitReached => Notice::Refused(
            "We've sent several links to that address today. Please check its spam folder, or write to us at info@scistarter.org."
                .to_string(),
        ),
        EmailAddRequested::Undeliverable => Notice::Refused(
            "Our email to that address is not getting through. Please write to us at info@scistarter.org."
                .to_string(),
        ),
    }
}

#[derive(Debug, Deserialize)]
pub struct EmailForm {
    #[serde(default)]
    email: String,
}

/// POST `/dashboard/emails` -- mail an address the link that adds it.
#[handler]
pub async fn add(
    Authenticated(user_info): Authenticated,
    Form(form): Form<EmailForm>,
) -> Result<Response, Error> {
    let user_info = user_info.ok_or_else(|| anyhow!("Login required"))?;
    let user_id = user_info.user_id;
    let email = form.email.trim().to_string();

    let requested: EmailAddRequested = common::service::request(
        "user.emails.request_add",
        EmailAddRequest {
            user_id,
            email: email.clone(),
        },
    )
    .await?;

    let notice = requested_notice(requested, &email);

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
    fn an_address_asked_for_can_be_sent_again_or_cancelled() {
        let html = section(None);

        check!(html.contains("lovelace@example.invalid"));
        check!(
            html.matches(r#"name="email" value="lovelace@example.invalid""#)
                .count()
                == 2
        );
        check!(html.contains(r#"hx-post="/dashboard/emails""#));
        check!(html.contains(r#"hx-post="/dashboard/emails/remove""#));
    }

    #[test]
    fn a_refusal_is_an_alert_and_a_sent_link_is_not() {
        check!(section(Some(Notice::Refused("No.".to_string()))).contains(r#"role="alert""#));

        let sent = section(Some(Notice::Done("Sent.".to_string())));
        check!(sent.contains(r#"role="status""#));
        check!(!sent.contains(r#"role="alert""#));
    }
}
