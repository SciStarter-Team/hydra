//! The dashboard's Link Tokens, which the account holder gives an affiliate
//! project so that it can report their contributions. Listed and deleted here,
//! and further ones added; the tokens themselves are user-service's, which
//! gives every account its first. Until cutover, a token copied from orrery
//! is deleted on orrery, where affiliates report.

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
    common::service::request("user.link_tokens.ensure_by_user", user_id.to_string())
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
            orrery_owns: false,
        }
    }

    #[test]
    fn a_token_orrery_keeps_is_deleted_there_rather_than_here() {
        let html = LinkTokensSection {
            tokens: vec![
                LinkToken {
                    orrery_owns: true,
                    ..token("default", "01HQ3Z8Y6V2N4J5K7M8P9R0S1T")
                },
                token("Stall Catchers", "01HQ3Z8Y6V2N4J5K7M8P9R0S2V"),
            ],
            label: String::new(),
            refusal: None,
        }
        .render_once()
        .unwrap();

        check!(html.contains(r#"name="token" value="01HQ3Z8Y6V2N4J5K7M8P9R0S2V""#));
        check!(!html.contains(r#"name="token" value="01HQ3Z8Y6V2N4J5K7M8P9R0S1T""#));
        check!(html.contains("dashboard on scistarter.org"));
        check!(html.contains(r#"data-copies="link-token-01HQ3Z8Y6V2N4J5K7M8P9R0S1T""#));
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

        check!(html.contains("01HQ3Z8Y6V2N4J5K7M8P9R0S1T"));
        check!(html.contains(r#"name="token" value="01HQ3Z8Y6V2N4J5K7M8P9R0S1T""#));
        check!(html.contains(r#"hx-post="/dashboard/link-tokens/delete""#));
        check!(html.contains("hx-confirm="));
        check!(!html.contains(r#"role="alert""#));
    }

    #[test]
    fn a_lone_token_is_shown_without_a_name_and_with_the_form_folded_away() {
        let html = LinkTokensSection {
            tokens: vec![token("default", "01HQ3Z8Y6V2N4J5K7M8P9R0S1T")],
            label: String::new(),
            refusal: None,
        }
        .render_once()
        .unwrap();

        check!(!html.contains(">default<"));
        check!(html.contains("<details>"));
        check!(html.contains("+ Add another token"));
    }

    #[test]
    fn several_tokens_are_told_apart_by_name() {
        let html = LinkTokensSection {
            tokens: vec![
                token("default", "01HQ3Z8Y6V2N4J5K7M8P9R0S1T"),
                token("Stall Catchers", "01HQ3Z8Y6V2N4J5K7M8P9R0S2V"),
            ],
            label: String::new(),
            refusal: None,
        }
        .render_once()
        .unwrap();

        check!(html.contains(">default<"));
        check!(html.contains(">Stall Catchers<"));
    }

    #[test]
    fn a_refusal_leaves_the_form_open() {
        let html = LinkTokensSection {
            tokens: vec![token("default", "01HQ3Z8Y6V2N4J5K7M8P9R0S1T")],
            label: "default".to_string(),
            refusal: Some("Could not create a token named \"default\".".to_string()),
        }
        .render_once()
        .unwrap();

        check!(html.contains("<details open>"));
    }

    #[test]
    fn each_copy_button_names_the_element_holding_its_own_token() {
        let html = LinkTokensSection {
            tokens: vec![
                token("Stall Catchers", "01HQ3Z8Y6V2N4J5K7M8P9R0S1T"),
                token("Anecdata", "01HQ3Z8Y6V2N4J5K7M8P9R0S2V"),
            ],
            label: String::new(),
            refusal: None,
        }
        .render_once()
        .unwrap();

        for value in ["01HQ3Z8Y6V2N4J5K7M8P9R0S1T", "01HQ3Z8Y6V2N4J5K7M8P9R0S2V"] {
            check!(html.contains(&format!(r#"data-copies="link-token-{value}""#)));
            check!(html.contains(&format!(
                r#"id="link-token-{value}" class="font-mono text-sm break-all select-all">{value}<"#
            )));
        }
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
