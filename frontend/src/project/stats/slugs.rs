//! `frontend.entity.project_slugs`: every slug each project has been known
//! by, for the log service to recognize a project's views and clicks by,
//! which it logged under whatever the project's slug was at the time.
//!
//! A project renamed here leaves its old slug as a Redirect to its new path
//! (see `Entity::save_to_database`), and staff add Redirects of their own,
//! so a project's slugs are its current one and every Redirect that leads to
//! it, however many hops away.

use std::collections::{BTreeMap, HashMap};

use anyhow::Error;
use async_nats::service::Request;
use common::{
    message::project_stats::{PageRequest, ProjectSlugs, page_slug},
    service::decode_structured_message,
    shared::db,
};
use ulid::Ulid;

/// The most projects one page holds, so that a page fits in one NATS
/// message.
const MOST: u32 = 1_000;

/// How many Redirects a slug is followed through before it is given up on,
/// which also stops a loop of them.
const HOPS: usize = 8;

pub async fn project_slugs(req: &Request) -> Result<Vec<ProjectSlugs>, Error> {
    let page: PageRequest<Ulid> = decode_structured_message(req.message.clone())?;

    let rows = sqlx::query!(
        r#"SELECT "slug", "kind", "target", "publish" AS "publish: bool"
           FROM "entities"
           WHERE "kind" IN ('Project', 'Redirect')"#
    )
    .fetch_all(db())
    .await?;

    let entities = rows.into_iter().map(|row| EntityRow {
        slug: row.slug,
        redirect: row.kind == "Redirect",
        target: row.target,
        publish: row.publish,
    });

    Ok(slug_map(entities)
        .into_values()
        .filter(|project| page.after.is_none_or(|after| project.project_id > after))
        .take(page.limit.min(MOST) as usize)
        .collect())
}

/// One row of the entity table, as far as the slug map needs it.
struct EntityRow {
    slug: String,
    /// A Redirect rather than a project.
    redirect: bool,
    target: String,
    publish: bool,
}

/// Every project's slugs, by project.
fn slug_map(entities: impl IntoIterator<Item = EntityRow>) -> BTreeMap<Ulid, ProjectSlugs> {
    let mut projects: BTreeMap<Ulid, ProjectSlugs> = BTreeMap::new();
    let mut by_slug: HashMap<String, Ulid> = HashMap::new();
    let mut redirects: HashMap<String, String> = HashMap::new();

    for entity in entities {
        let slug = entity.slug.to_lowercase();

        if entity.redirect {
            redirects.insert(slug, entity.target);
            continue;
        }

        let Ok(project_id) = entity.target.parse::<Ulid>() else {
            continue;
        };

        by_slug.insert(slug.clone(), project_id);
        projects.insert(
            project_id,
            ProjectSlugs {
                project_id,
                published: entity.publish,
                slugs: vec![slug],
            },
        );
    }

    for (slug, target) in &redirects {
        if let Some(project_id) = destination(target, &by_slug, &redirects)
            && let Some(project) = projects.get_mut(&project_id)
        {
            project.slugs.push(slug.clone());
        }
    }

    for project in projects.values_mut() {
        project.slugs.sort_unstable();
        project.slugs.dedup();
    }

    projects
}

/// The project a Redirect to `target` ends at, if it ends at one.
fn destination(
    target: &str,
    by_slug: &HashMap<String, Ulid>,
    redirects: &HashMap<String, String>,
) -> Option<Ulid> {
    let mut target = target;

    for _ in 0..HOPS {
        let slug = page_slug(&site_path(target)?)?;

        if let Some(project_id) = by_slug.get(&slug) {
            return Some(*project_id);
        }

        target = redirects.get(&slug)?;
    }

    None
}

/// The path on this site a Redirect target names: the target itself when it
/// is a path, or the path of an absolute URL to SciStarter. `None` for a URL
/// to anywhere else.
fn site_path(target: &str) -> Option<String> {
    if target.starts_with('/') {
        return Some(target.to_string());
    }

    let url = url::Url::parse(target).ok()?;
    let host = url.host_str()?;
    let ours = ["scistarter.org", "scistarter.com"]
        .iter()
        .any(|domain| host == *domain || host.ends_with(&format!(".{domain}")));

    ours.then(|| url.path().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert2::check;

    const BEES: &str = "01HQ3Z8Y6V2N4J5K7M8P9R0P01";

    fn row(slug: &str, redirect: bool, target: &str) -> EntityRow {
        EntityRow {
            slug: slug.to_string(),
            redirect,
            target: target.to_string(),
            publish: true,
        }
    }

    #[test]
    fn a_project_is_known_by_every_slug_that_leads_to_it() {
        let map = slug_map([
            row("Bee-Count", false, BEES),
            row("bees", true, "/projects/bee-count"),
            row("old-bees", true, "/bees"),
            row("ancient-bees", true, "https://scistarter.org/old-bees"),
            row("elsewhere", true, "https://example.org/bee-count"),
            row("loop-a", true, "/loop-b"),
            row("loop-b", true, "/loop-a"),
        ]);

        let bees = &map[&BEES.parse::<Ulid>().unwrap()];
        check!(bees.slugs == ["ancient-bees", "bee-count", "bees", "old-bees"]);
    }
}
