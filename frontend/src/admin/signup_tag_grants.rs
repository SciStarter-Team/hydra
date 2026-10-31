//! Admin management of signup tag grants (Engine Room): the signup tags whose
//! cohort is credited with a training outright, badge and all.
//!
//! The grants are user-service's. Most are copied from orrery's admin, which
//! owns them while both systems are in use, so those are listed here but not
//! deleted here; a grant made here can be.

use std::sync::Arc;

use crate::component::{AdminLayout, Component};
use crate::middleware::authorization::Authenticated;
use common::message::user::{
    BasicInfo, SIGNUP_TAG_GRANT_NEXT_PATH_LENGTH, SIGNUP_TAG_GRANT_TAG_LENGTH, SignupTagBadge,
    SignupTagGrant, SignupTagGrantAdd, SignupTagGrantAdded, SignupTagGrantDelete,
};
use poem::{
    Endpoint, Route, delete, get, handler, post,
    web::{Form, Path},
};
use sailfish::TemplateOnce;
use serde::Deserialize;

// ==================== Templates ====================

#[derive(TemplateOnce)]
#[template(path = "admin/signup_tag_grants.stpl.html")]
struct SignupTagGrantsPage {
    grants: Vec<SignupTagGrant>,

    /// What the tag box holds: empty, or the tag of a grant just refused.
    tag: String,

    /// What the path box holds, likewise.
    next_path: String,

    /// The grant just made, to show the link that hands it out.
    added: Option<SignupTagGrant>,

    /// Why the grant asked for was not made.
    refusal: Option<String>,
}

impl SignupTagGrantsPage {
    /// The link a partner hands its cohort, which carries the tag into the
    /// session for signup and login alike, and sends whoever follows it on to
    /// the grant's path afterwards when it has one.
    fn link(grant: &SignupTagGrant) -> String {
        let mut link = format!(
            "https://{}/login?tag={}",
            env!("DOMAIN"),
            urlencoding::encode(&grant.tag)
        );

        // The path's slashes are left as they are, as orrery leaves them, so
        // the link reads the same on both and is legible to whoever pastes it.
        if !grant.next_path.is_empty() {
            link.push_str("&next=");
            link.push_str(&urlencoding::encode(&grant.next_path).replace("%2F", "/"));
        }

        link
    }
}

#[derive(Debug, Deserialize)]
struct CreateGrantForm {
    #[serde(default)]
    tag: String,

    #[serde(default)]
    next_path: String,
}

// ==================== Handlers ====================

async fn list() -> Vec<SignupTagGrant> {
    common::service::request("user.signup_tag_grants.list", ())
        .await
        .unwrap_or_default()
}

async fn render_page(
    user_info: Option<Arc<BasicInfo>>,
    page: SignupTagGrantsPage,
) -> Result<poem::Response, anyhow::Error> {
    let prepared = AdminLayout::new("Signup Tag Grants", page)
        .with_sidebar_active("engine-room-signup-tag-grants")
        .with_breadcrumb("Engine Room", None)
        .with_breadcrumb("Signup Tag Grants", None)
        .prepare(user_info)
        .await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(prepared.render_once()?))
}

#[handler]
async fn grants_list(
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let page = SignupTagGrantsPage {
        grants: list().await,
        tag: String::new(),
        next_path: String::new(),
        added: None,
        refusal: None,
    };

    render_page(user_info, page).await
}

#[handler]
async fn grant_create(
    Form(form): Form<CreateGrantForm>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    // The one badge a cohort can be credited with so far, so the form does not
    // ask; a second member of `SignupTagBadge` brings a choice with it.
    let badge = SignupTagBadge::Foundations;

    let added: SignupTagGrantAdded = common::service::request(
        "user.signup_tag_grants.add",
        SignupTagGrantAdd {
            tag: form.tag.clone(),
            badge,
            next_path: form.next_path.clone(),
        },
    )
    .await?;

    let (added, refusal) = match added {
        SignupTagGrantAdded::Added(grant) => (Some(grant), None),
        SignupTagGrantAdded::AlreadyGranted(tag) => (
            None,
            Some(format!(
                "\"{tag}\" already grants the {} badge.",
                badge.badge().name()
            )),
        ),
        SignupTagGrantAdded::EmptyTag => (
            None,
            Some("A tag needs more than punctuation and spaces.".to_string()),
        ),
        SignupTagGrantAdded::TagTooLong => (
            None,
            Some(format!(
                "A tag can be at most {SIGNUP_TAG_GRANT_TAG_LENGTH} characters long."
            )),
        ),
        SignupTagGrantAdded::NextPathNotLocal => (
            None,
            Some(
                "\"After signing up, go to\" needs to be a page on this site, starting with \
                 a single /, such as /l/library-network-signup."
                    .to_string(),
            ),
        ),
        SignupTagGrantAdded::NextPathTooLong => (
            None,
            Some(format!(
                "\"After signing up, go to\" can be at most \
                 {SIGNUP_TAG_GRANT_NEXT_PATH_LENGTH} characters long."
            )),
        ),
    };

    if let (Some(grant), Some(user_info)) = (&added, &user_info) {
        common::service::log_activity(
            None,
            "signup-tag-grant-create",
            user_info.user_id,
            None,
            Some(format!("{} {}", grant.tag, grant.badge.as_str())),
        )
        .await?;
    }

    let refused = refusal.is_some();

    let page = SignupTagGrantsPage {
        grants: list().await,
        tag: if refused { form.tag } else { String::new() },
        next_path: if refused {
            form.next_path
        } else {
            String::new()
        },
        added,
        refusal,
    };

    render_page(user_info, page).await
}

#[handler]
async fn grant_delete(
    Path(id): Path<i64>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let () = common::service::request("user.signup_tag_grants.delete", SignupTagGrantDelete { id })
        .await?;

    if let Some(user_info) = &user_info {
        common::service::log_activity(
            None,
            "signup-tag-grant-delete",
            user_info.user_id,
            None,
            Some(id.to_string()),
        )
        .await?;
    }

    // The row removes itself via an empty outerHTML swap.
    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(String::new()))
}

// ==================== Routes ====================

pub fn routes() -> impl Endpoint {
    Route::new()
        .at("/", get(grants_list))
        .at("/create", post(grant_create))
        .at("/:id", delete(grant_delete))
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert2::check;

    fn grant(id: i64, tag: &str, from_orrery: bool) -> SignupTagGrant {
        SignupTagGrant {
            id,
            tag: tag.to_string(),
            badge: SignupTagBadge::Foundations,
            next_path: String::new(),
            created: "2026-10-30T23:46:00Z".to_string(),
            from_orrery,
        }
    }

    #[test]
    fn only_a_grant_made_here_offers_to_be_deleted() {
        let html = SignupTagGrantsPage {
            grants: vec![grant(1, "azla", true), grant(2, "webinar", false)],
            tag: String::new(),
            next_path: String::new(),
            added: None,
            refusal: None,
        }
        .render_once()
        .unwrap();

        check!(html.contains("/login?tag=azla"));
        check!(html.contains("/login?tag=webinar"));
        check!(html.contains(r#"hx-delete="/-admin/signup-tag-grants/2""#));
        check!(!html.contains(r#"hx-delete="/-admin/signup-tag-grants/1""#));
        check!(html.contains("Foundations of Citizen Science"));
        check!(!html.contains(r#"role="alert""#));
    }

    #[test]
    fn a_refused_tag_stays_in_the_box_beside_the_reason() {
        let html = SignupTagGrantsPage {
            grants: vec![],
            tag: "<AZLA>".to_string(),
            next_path: "/l/<form>".to_string(),
            added: None,
            refusal: Some("\"<azla>\" already grants the badge.".to_string()),
        }
        .render_once()
        .unwrap();

        check!(html.contains(r#"value="&lt;AZLA&gt;""#));
        check!(html.contains(r#"value="/l/&lt;form&gt;""#));
        check!(html.contains(r#"role="alert""#));
        check!(!html.contains("<AZLA>"));
    }

    #[test]
    fn a_grant_with_a_path_hands_out_a_link_that_goes_there() {
        let mut call = grant(1, "call", true);
        call.next_path = "/l/library-network-signup".to_string();

        let link = SignupTagGrantsPage::link(&call);

        check!(link.ends_with("/login?tag=call&next=/l/library-network-signup"));
        check!(
            SignupTagGrantsPage::link(&grant(2, "webinar cohort", false))
                .ends_with("/login?tag=webinar%20cohort")
        );
    }
}
