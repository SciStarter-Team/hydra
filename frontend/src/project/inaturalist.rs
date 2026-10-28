//! Projects on iNaturalist: which numbered iNaturalist project each one is,
//! which is what log-service credits observations by.

use std::time::Duration;

use anyhow::{Error, anyhow};
use async_nats::service::Request;
use common::{
    elect,
    message::user::{InatProject, InatProjectMap},
    primary::PRIMARY,
    shared::db,
    ulid::AsUlid,
};
use futures::future::BoxFuture;
use jiff::Timestamp;
use tokio::time::sleep;
use ulid::Ulid;

use crate::peering::LoadStorePeered;
use crate::project::field::{ProjectField, ProjectFieldValue};

/// The project an observation in no mapped iNaturalist project is credited
/// to.
pub const INATURALIST_SLUG: &str = "inaturalist";

/// NATS `frontend.inaturalist.projects` endpoint. Every project that names an
/// iNaturalist project, and the iNaturalist project itself, for log-service's
/// observation importer: the projects live only in the frontend's database.
/// Any frontend instance can answer, since the project table is replicated to
/// all of them.
///
/// Refuses rather than answering without a fallback. Every observation in no
/// mapped project is credited to it, so an importer handed none would have
/// nowhere true to put most of what it reads.
pub async fn projects(_req: &Request) -> Result<InatProjectMap, Error> {
    let rows = sqlx::query!(
        r#"SELECT "project_id", "inat_project_id" AS "inat_project_id!"
           FROM "projects"
           WHERE "inat_project_id" IS NOT NULL AND "inat_project_id" != 0"#
    )
    .fetch_all(db())
    .await?;

    let mut projects = Vec::with_capacity(rows.len());

    for row in rows {
        projects.push(InatProject {
            inat_project_id: row.inat_project_id,
            project_id: row.project_id.as_ulid()?,
        });
    }

    let fallback = inaturalist_itself().await?;

    Ok(InatProjectMap { projects, fallback })
}

/// The iNaturalist project itself.
async fn inaturalist_itself() -> Result<Ulid, Error> {
    sqlx::query_scalar!(
        r#"SELECT e.target FROM entities e
           JOIN projects p ON p.project_id = e.target
           WHERE e.slug = ? AND e.kind = 'Project'
           LIMIT 1"#,
        INATURALIST_SLUG
    )
    .fetch_optional(db())
    .await?
    .ok_or_else(|| anyhow!("No project carries the slug `{INATURALIST_SLUG}`"))?
    .as_ulid()
}

/// iNaturalist's network sites whose hosts don't say "inaturalist".
const NETWORK_HOSTS: [&str; 3] = ["naturalista.mx", "argentinat.org", "biodiversity4all.org"];

/// How often every project still waiting on a lookup is tried again.
const SWEEP_INTERVAL: Duration = Duration::from_secs(60 * 60 * 24);

/// iNaturalist asks API clients to keep to about one request a second.
const REQUEST_INTERVAL: Duration = Duration::from_secs(1);

/// Whether `url` is a page on iNaturalist or one of its network sites.
pub fn is_network_url(url: &str) -> bool {
    let Some(host) = url::Url::parse(url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_lowercase))
    else {
        return false;
    };
    let host = host.strip_prefix("www.").unwrap_or(&host);

    host.split('.').any(|label| label == "inaturalist") || NETWORK_HOSTS.contains(&host)
}

/// The slug (or number) of the iNaturalist project `url` is a page of.
///
/// None for any other page, iNaturalist's own included: the site's front
/// page, an app page and a place page all say the project is on iNaturalist
/// without saying which project it is.
pub fn project_slug(url: &str) -> Option<String> {
    if !is_network_url(url) {
        return None;
    }

    let parsed = url::Url::parse(url).ok()?;
    let mut path = parsed.path_segments()?.filter(|segment| !segment.is_empty());

    if path.next()? != "projects" {
        return None;
    }

    Some(urlencoding::decode(path.next()?).ok()?.into_owned())
}

#[derive(serde::Deserialize)]
struct ApiProjects {
    #[serde(default)]
    results: Vec<ApiProject>,
}

#[derive(serde::Deserialize)]
struct ApiProject {
    id: i64,
    slug: Option<String>,
}

/// The id of the iNaturalist project `url` is a page of, or None.
///
/// None when the URL names no project, or names one iNaturalist doesn't have.
/// A failure to get an answer is an error instead, so that the caller knows
/// to try again later.
///
/// The answer is only accepted if it is the project that was asked about:
/// `/projects/new` or `/projects/search` look like project pages, and the API
/// is under no obligation to read them the way the site does.
pub async fn lookup_project_id(url: &str) -> Result<Option<i64>, Error> {
    let Some(slug) = project_slug(url) else {
        return Ok(None);
    };

    let response = reqwest::Client::new()
        .get(format!(
            "https://api.inaturalist.org/v1/projects/{}",
            urlencoding::encode(&slug)
        ))
        .header("User-Agent", "SciStarter/1.0 (+https://scistarter.org)")
        .timeout(Duration::from_secs(30))
        .send()
        .await?;

    if matches!(
        response.status(),
        reqwest::StatusCode::NOT_FOUND | reqwest::StatusCode::UNPROCESSABLE_ENTITY
    ) {
        return Ok(None);
    }

    let found: ApiProjects = response.error_for_status()?.json().await?;

    Ok(matching_project(found, &slug))
}

/// The project among `found` that `slug` names, by number or by slug.
fn matching_project(found: ApiProjects, slug: &str) -> Option<i64> {
    let slug_lower = slug.to_lowercase();

    found
        .results
        .into_iter()
        .find(|project| {
            project.id.to_string() == slug
                || project
                    .slug
                    .as_deref()
                    .is_some_and(|s| s.to_lowercase() == slug_lower)
        })
        .map(|project| project.id)
}

async fn store_field(project_id: Ulid, field: ProjectFieldValue) -> Result<(), Error> {
    ProjectField {
        project_id,
        field,
        source: None,
    }
    .store()
    .await
}

/// Keep a project's iNaturalist fields in step with its URL, as orrery's
/// `derive_inaturalist_link` does.
///
/// A project whose page is on the iNaturalist network is an iNaturalist
/// project. Once it is tied to a numbered iNaturalist project, observations
/// filed under that project are credited to it, which is what makes it an
/// affiliate, part of iNaturalist. Finding the number means asking
/// iNaturalist, which only the primary does; storing the number brings the
/// project back through here to be made an affiliate.
///
/// Called whenever a project's URL, `is_inaturalist` or iNaturalist project
/// id is stored, and when a project is created.
///
/// Nothing here is ever unset: staff mark projects hosted elsewhere as
/// iNaturalist projects by hand, and a URL edited away from iNaturalist
/// doesn't undo that.
///
/// Boxed because it recurses: what it stores comes back through here.
pub fn derive_link(project_id: Ulid) -> BoxFuture<'static, Result<(), Error>> {
    Box::pin(derive_link_unboxed(project_id))
}

async fn derive_link_unboxed(project_id: Ulid) -> Result<(), Error> {
    let id_str = project_id.to_string();

    let Some(project) = sqlx::query!(
        r#"SELECT url, is_inaturalist AS "is_inaturalist: bool",
                  inat_project_id, is_affiliate AS "is_affiliate: bool", part_of
           FROM projects WHERE project_id = ?"#,
        id_str
    )
    .fetch_optional(db())
    .await?
    else {
        return Ok(());
    };

    if !project.is_inaturalist && is_network_url(&project.url) {
        // Storing it brings the project back through here for the rest.
        return store_field(project_id, ProjectFieldValue::IsInaturalist(true)).await;
    }

    let linked = project.inat_project_id.is_some_and(|id| id != 0);

    if project.is_inaturalist && linked {
        if !project.is_affiliate {
            store_field(project_id, ProjectFieldValue::IsAffiliate(true)).await?;
        }

        let inaturalist = inaturalist_itself().await?;

        if inaturalist != project_id && project.part_of != Some(inaturalist.to_string()) {
            store_field(project_id, ProjectFieldValue::PartOf(Some(inaturalist))).await?;
        }
    } else if !linked && PRIMARY && project_slug(&project.url).is_some() {
        tokio::spawn(async move {
            if let Err(err) = link(project_id).await {
                tracing::warn!(
                    "iNaturalist lookup for {project_id} failed; the daily sweep will retry: {err:#}"
                );
            }
        });
    }

    Ok(())
}

/// Tie a project to the iNaturalist project its URL is a page of, if it isn't
/// tied to one already and iNaturalist has it.
async fn link(project_id: Ulid) -> Result<(), Error> {
    let id_str = project_id.to_string();

    let Some(project) = sqlx::query!(
        r#"SELECT url, inat_project_id FROM projects WHERE project_id = ?"#,
        id_str
    )
    .fetch_optional(db())
    .await?
    else {
        return Ok(());
    };

    if project.inat_project_id.is_some_and(|id| id != 0) {
        return Ok(());
    }

    let Some(inat_project_id) = lookup_project_id(&project.url).await? else {
        tracing::info!("No iNaturalist project for {project_id} at {}", project.url);
        return Ok(());
    };

    tracing::info!("Linking {project_id} to iNaturalist project {inat_project_id}");

    store_field(
        project_id,
        ProjectFieldValue::InatProjectId(Some(i32::try_from(inat_project_id)?)),
    )
    .await
}

/// Each day, try again to link every project whose URL is an iNaturalist
/// project page but which has no iNaturalist project id: those whose lookup
/// failed when they were saved, and those saved before saving looked them
/// up. One instance is elected to do it, and only while this build is the
/// primary; until then orrery looks the projects up and its sync brings the
/// ids here.
pub async fn link_periodically() {
    if !PRIMARY {
        tracing::info!("iNaturalist linking: orrery is still the primary, so not looking up ids.");
        return;
    }

    loop {
        let interval = SWEEP_INTERVAL.as_secs() as i64;
        let slot = Timestamp::now().as_second() / interval + 1;
        let due = Timestamp::from_second(slot * interval).unwrap_or_else(|_| Timestamp::now());
        let wait = (due - Timestamp::now())
            .total(jiff::Unit::Second)
            .unwrap_or(0.0)
            .max(0.0);
        sleep(Duration::from_secs_f64(wait)).await;

        let elected = elect::elect_instance(
            "INAT_LINK_CLAIMS",
            &slot.to_string(),
            &Ulid::new().to_string(),
            SWEEP_INTERVAL,
        )
        .await;

        match elected {
            Ok(true) => {
                if let Err(err) = link_unlinked().await {
                    tracing::error!("iNaturalist linking: {err:#}");
                }
            }
            Ok(false) => {}
            Err(err) => tracing::error!("iNaturalist linking: the election failed: {err:#}"),
        }
    }
}

/// Link every project whose URL is an iNaturalist project page but which has
/// no iNaturalist project id. One that fails is reported and the rest are
/// still tried.
async fn link_unlinked() -> Result<(), Error> {
    let rows = sqlx::query!(
        r#"SELECT project_id, url FROM projects
           WHERE inat_project_id IS NULL OR inat_project_id = 0"#
    )
    .fetch_all(db())
    .await?;

    for row in rows {
        if project_slug(&row.url).is_none() {
            continue;
        }

        let project_id = row.project_id.as_ulid()?;

        sleep(REQUEST_INTERVAL).await;

        if let Err(err) = link(project_id).await {
            tracing::warn!("iNaturalist linking: {project_id}: {err:#}");
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_sites_are_recognised() {
        assert!(is_network_url("https://www.inaturalist.org/projects/echidnacsi"));
        assert!(is_network_url("https://inaturalist.ala.org.au/projects/x"));
        assert!(is_network_url("https://inaturalist.nz/projects/x"));
        assert!(is_network_url("https://www.naturalista.mx/projects/x"));
        assert!(!is_network_url("https://www.zooniverse.org/projects/x"));
        assert!(!is_network_url("https://notinaturalist.org/projects/x"));
        assert!(!is_network_url("inaturalist.org/projects/x"));
    }

    #[test]
    fn only_a_project_page_names_a_project() {
        assert_eq!(
            project_slug("https://www.inaturalist.org/projects/echidnacsi?tab=about").as_deref(),
            Some("echidnacsi")
        );
        assert_eq!(
            project_slug("https://www.inaturalist.org//projects/12345/").as_deref(),
            Some("12345")
        );
        assert_eq!(
            project_slug("https://www.inaturalist.org/projects/caf%C3%A9").as_deref(),
            Some("café")
        );
        assert_eq!(project_slug("https://www.inaturalist.org/"), None);
        assert_eq!(project_slug("https://www.inaturalist.org/projects"), None);
        assert_eq!(project_slug("https://www.inaturalist.org/places/x"), None);
        assert_eq!(project_slug("https://example.org/projects/x"), None);
    }

    fn found(projects: &[(i64, &str)]) -> ApiProjects {
        ApiProjects {
            results: projects
                .iter()
                .map(|&(id, slug)| ApiProject {
                    id,
                    slug: Some(slug.to_string()),
                })
                .collect(),
        }
    }

    #[test]
    fn the_answer_must_be_the_project_asked_about() {
        assert_eq!(matching_project(found(&[(7, "EchidnaCSI")]), "echidnacsi"), Some(7));
        assert_eq!(matching_project(found(&[(7, "echidnacsi")]), "7"), Some(7));
        assert_eq!(matching_project(found(&[(7, "something-else")]), "new"), None);
        assert_eq!(matching_project(found(&[]), "echidnacsi"), None);
    }
}
