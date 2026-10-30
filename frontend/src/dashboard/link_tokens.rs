//! The dashboard's Link Tokens, which the account holder gives an affiliate
//! project so that it can report their contributions. Made, listed and
//! deleted here; the tokens themselves are user-service's.

use anyhow::{Error, anyhow};
use common::message::dashboard::{
    LINK_TOKEN_LABEL_LENGTH, LinkToken, LinkTokenAdd, LinkTokenAdded, LinkTokenDelete,
};
use poem::web::{Form, Html};
use poem::{IntoResponse, Response, handler};
use sailfish::TemplateOnce;
use serde::Deserialize;
use ulid::Ulid;

use crate::local_time::{TimeFormat, local_time};
use crate::middleware::authorization::Authenticated;

/// The Link Tokens section, which replaces itself when a token is made or
/// deleted.
#[derive(TemplateOnce)]
#[template(path = "dashboard/_partials/link_tokens_section.stpl.html")]
pub struct LinkTokensSection {
    tokens: Vec<LinkToken>,

    /// What the name box holds: empty, or the name of a token just refused.
    label: String,

    /// Why the token asked for was not made.
    refusal: Option<String>,
}

impl LinkTokensSection {
    /// The section as the account's tokens stand.
    pub async fn load(user_id: Ulid) -> Self {
        Self {
            tokens: list(user_id).await,
            label: String::new(),
            refusal: None,
        }
    }
}

async fn list(user_id: Ulid) -> Vec<LinkToken> {
    common::service::request("user.link_tokens.list_by_user", user_id.to_string())
        .await
        .unwrap_or_default()
}

#[derive(Debug, Deserialize)]
pub struct AddForm {
    #[serde(default)]
    label: String,
}

#[handler]
pub async fn add(
    Authenticated(user_info): Authenticated,
    Form(form): Form<AddForm>,
) -> Result<Response, Error> {
    let user_info = user_info.ok_or_else(|| anyhow!("Login required"))?;
    let user_id = user_info.user_id;

    let added: LinkTokenAdded = common::service::request(
        "user.link_tokens.add",
        LinkTokenAdd {
            user_id,
            label: form.label.clone(),
        },
    )
    .await?;

    let refusal = match added {
        LinkTokenAdded::Added(_) => None,
        LinkTokenAdded::LabelTaken(label) => Some(format!(
            "Could not create a token named \"{label}\". Maybe you already have one?"
        )),
        LinkTokenAdded::LabelTooLong => Some(format!(
            "A token name can be at most {LINK_TOKEN_LABEL_LENGTH} characters long."
        )),
    };

    let section = LinkTokensSection {
        tokens: list(user_id).await,
        label: if refusal.is_some() {
            form.label
        } else {
            String::new()
        },
        refusal,
    };

    Ok(Html(section.render_once()?).into_response())
}

#[derive(Debug, Deserialize)]
pub struct DeleteForm {
    token: String,
}

#[handler]
pub async fn delete(
    Authenticated(user_info): Authenticated,
    Form(form): Form<DeleteForm>,
) -> Result<Response, Error> {
    let user_info = user_info.ok_or_else(|| anyhow!("Login required"))?;
    let user_id = user_info.user_id;

    let () = common::service::request(
        "user.link_tokens.delete",
        LinkTokenDelete {
            user_id,
            token: form.token.trim().to_string(),
        },
    )
    .await?;

    Ok(Html(LinkTokensSection::load(user_id).await.render_once()?).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert2::check;

    fn token(label: &str, token: &str) -> LinkToken {
        LinkToken {
            token: token.to_string(),
            label: label.to_string(),
            created: "2024-02-20T15:04:05Z".to_string(),
        }
    }

    #[test]
    fn each_token_is_shown_with_its_string_and_a_delete_button() {
        let html = LinkTokensSection {
            tokens: vec![token("Stall Catchers", "01HQ3Z8Y6V2N4J5K7M8P9R0S1T")],
            label: String::new(),
            refusal: None,
        }
        .render_once()
        .unwrap();

        check!(html.contains("Stall Catchers"));
        check!(html.contains("01HQ3Z8Y6V2N4J5K7M8P9R0S1T"));
        check!(html.contains(r#"name="token" value="01HQ3Z8Y6V2N4J5K7M8P9R0S1T""#));
        check!(html.contains(r#"hx-post="/dashboard/link-tokens/delete""#));
        check!(!html.contains(r#"role="alert""#));
    }

    #[test]
    fn a_refused_name_stays_in_the_box_beside_the_reason() {
        let html = LinkTokensSection {
            tokens: vec![],
            label: "<Stall Catchers>".to_string(),
            refusal: Some("Could not create a token named \"<Stall Catchers>\".".to_string()),
        }
        .render_once()
        .unwrap();

        check!(html.contains(r#"value="&lt;Stall Catchers&gt;""#));
        check!(html.contains(r#"role="alert""#));
        check!(!html.contains("<Stall Catchers>"));
    }
}
