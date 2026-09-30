//! Girl Scouts Take Action logging (`/girlscouts/action`).
//!
//! Ported from orrery's `ss-girlscouts-take-action` Vue component and its
//! `girlscouts_action_save` endpoint. The last step of the Think Like a Citizen
//! Scientist Journey asks a troop to document a sustainable Take Action
//! project; the write-up is for SciStarter staff to read, not for publication,
//! so nothing here surfaces on the public site.
//!
//! Submissions live in user-service (`user.girlscouts.*`) next to the user
//! records they belong to. The frontend only renders the form and posts it: it
//! never reads a submission back on a page request, so there is nothing here
//! worth keeping in the frontend database or peering between instances.
//!
//! One deliberate departure from orrery: it took these anonymously, so staff
//! were left with write-ups they could not follow up on. A signed-in submitter
//! is required here, which the Journey's account-based flow already assumes.

use anyhow::Error;
use common::message::dashboard::TakeActionSubmission;
use common::message::user::BasicInfo;
use common::service::request;
use poem::{
    IntoResponse, Response, handler,
    web::{Form, Query, Redirect},
};
use sailfish::TemplateOnce;
use serde::Deserialize;
use std::sync::Arc;
use ulid::Ulid;

use crate::component::{
    Component, Head, StandardLayout,
    image_field::{ImageField, ImageLayout},
};
use crate::middleware::authorization::Authenticated;

pub const PATH: &str = "/girlscouts/action";

/// The Take Action form body, rendered inside `StandardLayout`.
#[derive(TemplateOnce)]
#[template(path = "site/girlscouts_take_action.stpl.html")]
pub struct TakeActionContent {
    pub logged_in: bool,
    /// Set once a submission has been recorded, replacing the form with the
    /// congratulations message.
    pub recorded: bool,
    pub error: Option<String>,
    pub troop: String,
    pub problem: String,
    pub solution: String,
    pub sustainable: String,
    pub photo: ImageField,
}

impl TakeActionContent {
    fn new(logged_in: bool, form: TakeActionForm, error: Option<String>) -> Self {
        TakeActionContent {
            logged_in,
            recorded: false,
            error,
            troop: form.troop,
            problem: form.problem,
            solution: form.solution,
            sustainable: form.sustainable,
            photo: ImageField {
                label: "Add a picture of your troop doing the Take Action project".to_string(),
                value: form.image,
                credit: None,
                layout: ImageLayout::Inline,
            },
        }
    }
}

#[derive(TemplateOnce)]
#[template(path = "site/girlscouts_take_action_page.stpl.html")]
struct TakeActionPage {
    head: <Head as Component>::Prepared,
    layout: <StandardLayout<TakeActionContent> as Component>::Prepared,
}

/// The posted form. `image` is the URL the shared upload widget writes back;
/// every field defaults to empty so a partly-filled form re-renders rather
/// than failing to parse.
#[derive(Debug, Default, Deserialize)]
struct TakeActionForm {
    #[serde(default)]
    troop: String,
    #[serde(default)]
    problem: String,
    #[serde(default)]
    solution: String,
    #[serde(default)]
    sustainable: String,
    #[serde(default)]
    image: String,
}

async fn render(
    user_info: Option<Arc<BasicInfo>>,
    content: TakeActionContent,
) -> Result<Response, Error> {
    let layout = StandardLayout::new(PATH.to_string(), content)
        .prepare(user_info)
        .await?;
    TakeActionPage {
        head: Head::default().prepare(None).await?,
        layout,
    }
    .into_response(None)
    .await
}

/// `?recorded=true` is what a successful submission redirects back to, so that
/// refreshing the confirmation cannot post the write-up a second time.
#[derive(Debug, Deserialize)]
pub struct IndexQuery {
    #[serde(default)]
    recorded: bool,
}

#[handler]
pub async fn index(
    Authenticated(user_info): Authenticated,
    Query(query): Query<IndexQuery>,
) -> Result<Response, Error> {
    let mut content = TakeActionContent::new(user_info.is_some(), TakeActionForm::default(), None);
    content.recorded = query.recorded;
    render(user_info, content).await
}

#[handler]
pub async fn submit(
    Authenticated(user_info): Authenticated,
    Form(form): Form<TakeActionForm>,
) -> Result<Response, Error> {
    let Some(user) = user_info.clone() else {
        return Ok(Redirect::see_other(format!("/login?next={PATH}")).into_response());
    };

    // Orrery checked the same four fields client-side before saving.
    let missing = [
        &form.troop,
        &form.problem,
        &form.solution,
        &form.sustainable,
    ]
    .iter()
    .any(|value| value.trim().is_empty());

    if missing {
        let content = TakeActionContent::new(
            true,
            form,
            Some("Please fill in all of the required fields.".to_string()),
        );
        return render(user_info, content).await;
    }

    let _: Ulid = request(
        "user.girlscouts.take_action_submit",
        TakeActionSubmission {
            user_id: user.user_id,
            troop: form.troop.trim().to_string(),
            problem: form.problem.trim().to_string(),
            solution: form.solution.trim().to_string(),
            sustainable: form.sustainable.trim().to_string(),
            photo: form.image.trim().to_string(),
        },
    )
    .await?;

    Ok(Redirect::see_other(format!("{PATH}?recorded=true")).into_response())
}
