//! Forms SciStarter hosts for projects (common's `message::form`): the page
//! where people fill one in, and what the people running it see of what was
//! sent.
//!
//! A form's definition and its submissions are kept by user-service, which
//! holds every submission to its form ([`FormDefinition::read_submission`]);
//! this module only draws them and passes on what people send.
//!
//! - [`page`]: the form's page, served as an [`EntityKind::Form`] entity at
//!   `/form/<slug>`, and the submission it sends to `/-form/<form id>`.
//! - [`photo`]: photos uploaded for a form's photo fields as it is filled in.
//! - [`map`]: the public map of where a form's submissions were made.
//! - [`submissions`]: the submissions, and their CSV, for staff and for the
//!   people the form belongs to.
//! - [`entities`]: the pages' entities, kept in step with the forms.
//! - [`admin`]: staff's form builder.
//!
//! [`EntityKind::Form`]: crate::entity::EntityKind::Form

use std::sync::Arc;

use anyhow::Error;
use common::{
    message::{
        form::{FormDefinition, FormLookup, FormProject, FormSummary},
        user::BasicInfo,
    },
    service::request,
};
use poem::{Endpoint, Route, get, post};
use sailfish::TemplateOnce;
use ulid::Ulid;

use crate::{
    edit_access::{Editor, Page, may_edit},
    entity::{Entity, EntityKind},
};

pub mod admin;
mod answers;
pub mod entities;
mod map;
pub mod page;
mod photo;
mod submissions;

/// The routes under `/-form`.
pub fn routes() -> impl Endpoint {
    Route::new()
        .at("/:form_id", post(page::submit))
        .at("/:form_id/photo", post(photo::upload))
        .at("/:form_id/map.json", get(map::markers))
        .at("/:form_id/submissions", get(submissions::table))
        .at("/:form_id/submissions.csv", get(submissions::csv))
        .at(
            "/:form_id/submissions/:submission_id/evaluation",
            post(submissions::evaluate),
        )
        .at(
            "/:form_id/submissions/:submission_id",
            poem::delete(submissions::delete),
        )
}

/// The form `lookup` names, if there is one.
pub async fn load(lookup: FormLookup) -> Result<Option<FormDefinition>, Error> {
    request("user.forms.get", lookup).await
}

/// A form, and where its page is.
#[derive(Clone)]
pub struct FormLink {
    pub form: FormSummary,
    /// The path of the form's page, while it is published.
    pub page_path: Option<String>,
}

/// `forms`, each with where its page is.
pub async fn with_pages(forms: Vec<FormSummary>) -> Result<Vec<FormLink>, Error> {
    let mut links = Vec::with_capacity(forms.len());

    for form in forms {
        let page_path = Entity::page_of(EntityKind::Form, form.form_id)
            .await?
            .filter(Entity::publish)
            .map(|page| page.canonical_path());

        links.push(FormLink { form, page_path });
    }

    Ok(links)
}

/// Forms, for someone who may see their submissions: each with its page,
/// its submissions and their CSV. `None` if they could not be listed.
#[derive(Clone, TemplateOnce)]
#[template(path = "hosted_form/form_links.stpl.html")]
pub struct FormLinks {
    pub forms: Option<Vec<FormLink>>,
}

impl FormLinks {
    /// Whether there are forms.
    pub fn has_forms(&self) -> bool {
        self.forms.as_ref().is_some_and(|forms| !forms.is_empty())
    }

    /// Whether there is anything to show: forms, or that they could not be
    /// listed.
    pub fn is_worth_showing(&self) -> bool {
        self.forms.as_ref().is_none_or(|forms| !forms.is_empty())
    }

    async fn listed(what: &str, forms: Result<Vec<FormSummary>, Error>) -> FormLinks {
        let links = match forms {
            Ok(forms) => with_pages(forms).await,
            Err(e) => Err(e),
        };

        FormLinks {
            forms: links
                .inspect_err(|e| tracing::warn!("Couldn't list {what}: {e:#}"))
                .ok(),
        }
    }

    /// The forms of the project `project_id`.
    pub async fn of_project(project_id: Ulid) -> FormLinks {
        let forms = request("user.forms.for_project", project_id.to_string()).await;
        FormLinks::listed(&format!("the forms of the project {project_id}"), forms).await
    }

    /// The forms `user_id` owns.
    pub async fn owned_by(user_id: Ulid) -> FormLinks {
        let forms = request("user.forms.owned_by", user_id).await;
        FormLinks::listed(&format!("the forms {user_id} owns"), forms).await
    }
}

/// Whether `user` may see the submissions of `form`, as orrery decided who
/// may download a form's data: staff, the people the form belongs to, and
/// whoever may edit its project's page.
pub async fn may_see_submissions(
    form: &FormDefinition,
    user: Option<&Arc<BasicInfo>>,
) -> Result<bool, Error> {
    let Some(user) = user else {
        return Ok(false);
    };

    if user.is_staff || user.is_superuser || form.owners.contains(&user.user_id) {
        return Ok(true);
    }

    match &form.project {
        Some(FormProject { project_id, .. }) => match project_id.parse::<Ulid>() {
            Ok(project_id) => may_edit(Page::Project(project_id), Editor::from(&**user)).await,
            Err(_) => Ok(false),
        },
        None => Ok(false),
    }
}

/// `next` as somewhere to send someone on this site, if it is a path here:
/// one slash, not the two (or the slash and backslash) that name another
/// host. Anything else is no place to send them.
pub fn site_path(next: &str) -> Option<&str> {
    let next = next.trim();

    (next.starts_with('/') && !next.starts_with("//") && !next.starts_with("/\\")).then_some(next)
}

/// `url` as a link a page may offer, if it is one: a path on this site or
/// an address on the web. A `javascript:` address, say, is no link.
pub fn link(url: &str) -> Option<&str> {
    let url = url.trim();
    let lower = url.to_ascii_lowercase();

    (site_path(url).is_some() || lower.starts_with("https://") || lower.starts_with("http://"))
        .then_some(url)
}

#[cfg(test)]
mod tests;
