//! The pages of hosted forms, as entities: one [`EntityKind::Form`] entity
//! for each form, at `/form/<name>`, kept in step with the forms user-service
//! holds.
//!
//! A form's page is named, where it can be, by the form's slug, as orrery
//! named it, and otherwise by its orrery id, or failing that by its own id
//! ([`names`]). A name is the form's to take unless something other than a
//! form's page holds it: staff may have made `/form/crowd-the-tap` a
//! redirect to a page of its own, say, and that stays theirs. A form whose
//! page is named by its slug also answers at `/form/<orrery id>`, as it did
//! on orrery, by a redirect.
//!
//! Renaming a page leaves a redirect at its old name ([`Entity::store`]), and
//! a form deleted from user-service has its page unpublished, so that its
//! name is still held and can be given back if the form returns.

use std::{collections::HashSet, time::Duration};

use anyhow::{Error, anyhow};
use common::{elect, message::form::FormSummary, service::request, shared::db};
use jiff::Timestamp;
use tokio::time::sleep;
use ulid::Ulid;

use crate::{
    entity::{Entity, EntityKind, EntityTarget},
    peering::LoadStorePeered,
};

/// How often the pages are brought into step with the forms.
const INTERVAL: Duration = Duration::from_secs(15 * 60);

/// The entity slugs a form's page may go by, best first. The last, by the
/// form's own id, is always the form's.
fn names(form: &FormSummary) -> Vec<String> {
    let slug = form
        .slug
        .as_deref()
        .map(str::trim)
        .filter(|slug| !slug.is_empty());

    slug.map(|slug| format!("form/{slug}"))
        .into_iter()
        .chain(form.orrery_id.map(|orrery_id| format!("form/{orrery_id}")))
        .chain(std::iter::once(format!("form/{}", form.form_id)))
        .collect()
}

/// What holds an entity slug, as far as a form's page goes.
#[derive(Debug, PartialEq)]
enum Holder {
    Nobody,
    /// The page of the form `form_id`.
    Page(Ulid),
    /// A redirect to a form's page: a page's old name, or another name of
    /// it. A form's page may take the name.
    FormRedirect {
        target: String,
    },
    /// Anything else, which a form's page leaves alone.
    Other,
}

impl Holder {
    /// Whether the page of the form `form_id` may go by a name held so.
    fn yields_to(&self, form_id: Ulid) -> bool {
        match self {
            Holder::Nobody | Holder::FormRedirect { .. } => true,
            Holder::Page(page_of) => *page_of == form_id,
            Holder::Other => false,
        }
    }
}

/// What holds the entity slug `slug`.
async fn holder(slug: &str) -> Result<Holder, Error> {
    let held = sqlx::query!(
        r#"SELECT "kind" AS "kind: EntityKind", "target"
           FROM "entities"
           WHERE LOWER("slug") = LOWER(?)"#,
        slug
    )
    .fetch_optional(db())
    .await?;

    Ok(match held {
        None => Holder::Nobody,
        Some(held) => classify(held.kind, &held.target),
    })
}

/// What an entity of kind `kind` targeting `target` is, as a holder of its
/// slug.
fn classify(kind: EntityKind, target: &str) -> Holder {
    match kind {
        EntityKind::Form => target.parse().map_or(Holder::Other, Holder::Page),
        EntityKind::Redirect if target.starts_with(&EntityKind::Form.path("form/")) => {
            Holder::FormRedirect {
                target: target.to_string(),
            }
        }
        _ => Holder::Other,
    }
}

/// Give the form `form` its page, published, under the best of its
/// [`names`] it may take, and its redirect from `/form/<orrery id>` where
/// that is not the page's own name.
pub async fn reconcile(form: &FormSummary) -> Result<(), Error> {
    let mut slug = None;
    for name in names(form) {
        if holder(&name).await?.yields_to(form.form_id) {
            slug = Some(name);
            break;
        }
    }

    // The last name, by the form's id, is held by something else only if
    // someone made it so by hand.
    let slug = slug.ok_or_else(|| {
        anyhow!(
            "Every name of the page of the form {} is held by something else",
            form.form_id
        )
    })?;

    let target = EntityTarget::Ulid(form.form_id);
    match Entity::page_of(EntityKind::Form, form.form_id).await? {
        Some(page) if page.slug().eq_ignore_ascii_case(&slug) && page.publish() => {}
        Some(page) => {
            let password = page.password().map(str::to_string);
            Entity::new_with_created(
                slug.clone(),
                EntityKind::Form,
                target,
                true,
                page.created().clone(),
            )
            .with_password(password)
            .store()
            .await?;
        }
        None => {
            Entity::new(slug.clone(), EntityKind::Form, target)
                .store()
                .await?
        }
    }

    if let Some(orrery_id) = form.orrery_id {
        let alias = format!("form/{orrery_id}");
        let path = EntityKind::Form.path(&slug);

        if !alias.eq_ignore_ascii_case(&slug) {
            let current = match holder(&alias).await? {
                Holder::Nobody => true,
                Holder::FormRedirect { target } => target != path,
                Holder::Page(_) | Holder::Other => false,
            };

            if current {
                Entity::new(alias, EntityKind::Redirect, EntityTarget::String(path))
                    .store()
                    .await?;
            }
        }
    }

    Ok(())
}

/// Bring every form's page into step with the forms: each form's page as
/// [`reconcile`] makes it, and the pages of forms there are no longer
/// unpublished. A form whose page cannot be made is reported, and the rest
/// still made.
pub async fn reconcile_all() -> Result<(), Error> {
    let forms: Vec<FormSummary> = request("user.forms.list", ()).await?;

    let mut failed = 0;
    for form in &forms {
        if let Err(e) = reconcile(form).await {
            tracing::error!("Couldn't make the page of the form {}: {e:#}", form.form_id);
            failed += 1;
        }
    }

    let kept: HashSet<Ulid> = forms.iter().map(|form| form.form_id).collect();
    let published = sqlx::query_scalar!(
        r#"SELECT "target" FROM "entities" WHERE "kind" = ? AND "publish""#,
        EntityKind::Form,
    )
    .fetch_all(db())
    .await?;

    for target in published {
        match target.parse::<Ulid>() {
            Ok(form_id) if !kept.contains(&form_id) => {
                Entity::unpublish_page(EntityKind::Form, form_id).await?;
            }
            Ok(_) => {}
            Err(_) => tracing::warn!("A form's page targets {target:?}, which is no form id"),
        }
    }

    if failed > 0 {
        return Err(anyhow!(
            "{failed} of {} forms' pages couldn't be made",
            forms.len()
        ));
    }

    Ok(())
}

/// Every [`INTERVAL`], have one frontend instance [`reconcile_all`]. The
/// pages are entities, which reach every instance as they are stored.
pub async fn reconcile_periodically() {
    loop {
        let interval = INTERVAL.as_secs() as i64;
        let slot = Timestamp::now().as_second() / interval + 1;
        let due = Timestamp::from_second(slot * interval).unwrap_or_else(|_| Timestamp::now());
        let wait = (due - Timestamp::now())
            .total(jiff::Unit::Second)
            .unwrap_or(0.0)
            .max(0.0);
        sleep(Duration::from_secs_f64(wait)).await;

        let elected = elect::elect_instance(
            "FORM_PAGE_CLAIMS",
            &slot.to_string(),
            &Ulid::new().to_string(),
            INTERVAL,
        )
        .await;

        match elected {
            Ok(true) => {
                if let Err(e) = reconcile_all().await {
                    tracing::error!("Couldn't bring the forms' pages into step: {e:#}");
                }
            }
            Ok(false) => {}
            Err(e) => tracing::error!("Forms' pages: the election failed: {e:#}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use assert2::check;
    use common::message::form::FormSummary;

    use super::*;

    fn summary(slug: Option<&str>, orrery_id: Option<i64>) -> FormSummary {
        FormSummary {
            form_id: Ulid::new(),
            orrery_id,
            slug: slug.map(str::to_string),
            name: "Squirrels".to_string(),
            closed: false,
            project: None,
        }
    }

    #[test]
    fn a_page_is_named_by_slug_then_orrery_id_then_id() {
        let form = summary(Some("squirrels"), Some(5));
        check!(
            names(&form)
                == [
                    "form/squirrels".to_string(),
                    "form/5".to_string(),
                    format!("form/{}", form.form_id)
                ]
        );

        let form = summary(Some("  "), None);
        check!(names(&form) == [format!("form/{}", form.form_id)]);
    }

    #[test]
    fn a_page_takes_a_name_from_nobody_and_from_a_redirect_to_a_form() {
        let form_id = Ulid::new();

        check!(Holder::Nobody.yields_to(form_id));
        check!(classify(EntityKind::Redirect, "/form/squirrels").yields_to(form_id));
        check!(classify(EntityKind::Form, &form_id.to_string()).yields_to(form_id));
    }

    #[test]
    fn a_page_leaves_a_name_held_otherwise_alone() {
        let form_id = Ulid::new();

        check!(!classify(EntityKind::Form, &Ulid::new().to_string()).yields_to(form_id));
        check!(!classify(EntityKind::Redirect, "/crowd-the-tap-home").yields_to(form_id));
        check!(!classify(EntityKind::Redirect, "/formulas").yields_to(form_id));
        check!(!classify(EntityKind::PageBuilder, &Ulid::new().to_string()).yields_to(form_id));
    }
}
