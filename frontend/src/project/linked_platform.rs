//! Which outside platform credits a project's participants, once they name
//! their username there.
//!
//! An iNaturalist project says so itself, in `projects.is_inaturalist`.
//! Membership of the Zooniverse affiliate program is known only to
//! log-service, whose bridge sweeps those projects for classifications, so
//! the frontend asks it and caches the answer.

use std::collections::HashSet;
use std::time::Duration;

use anyhow::Error;
use common::linked_platform::LinkedPlatform;
use common::service::request;
use ulid::Ulid;

use crate::lazy_cache::{LazyCached, get_cached, push_cached_value};

/// The projects log-service's Zooniverse bridge credits, as a JSON array of
/// project ids, cached on each frontend instance.
#[derive(Clone)]
struct ZooniverseAffiliates;

impl LazyCached for ZooniverseAffiliates {
    fn cache_key(&self) -> String {
        "zooniverse_affiliates".to_string()
    }

    fn max_age(&self) -> Duration {
        Duration::from_secs(600)
    }

    async fn fetch_fresh(&self) -> Result<String, Error> {
        let projects: Vec<Ulid> = request("log.zooniverse.projects", ()).await?;
        Ok(serde_json::to_string(&projects)?)
    }
}

/// The projects in the Zooniverse affiliate program.
pub async fn zooniverse_affiliates() -> Result<HashSet<Ulid>, Error> {
    Ok(serde_json::from_str(
        &get_cached(&ZooniverseAffiliates).await?,
    )?)
}

/// Narrow a query over `projects p` to the Zooniverse affiliates.
///
/// Reads the list from the cache in the same statement, so that a filter
/// builder with no way to await can apply it. The caller must have awaited
/// [`zooniverse_affiliates`] first; with nothing cached, nothing matches.
pub(crate) fn push_zooniverse_affiliate_filter(qb: &mut sqlx::QueryBuilder<sqlx::Sqlite>) {
    qb.push(" AND p.project_id IN (SELECT value FROM json_each(");
    push_cached_value(qb, &ZooniverseAffiliates);
    qb.push("))");
}

/// How a project page talks about a platform, beside what
/// `common::linked_platform` knows of it.
pub trait PlatformCopy {
    /// Where the platform's projects that credit participation are listed,
    /// when only some of them do; None when every project on the platform
    /// does.
    fn eligible_projects_url(self) -> Option<String>;

    /// "a" or "an", before the platform's name.
    fn article(self) -> &'static str;

    /// Where a visitor signs in to the platform to find their username.
    fn sign_in_url(self) -> &'static str;

    /// One unit of what the platform credits, and what a participant does to
    /// make one: an observation is uploaded, a classification made.
    fn contribution(self) -> &'static str;
    fn contributed(self) -> &'static str;
}

impl PlatformCopy for LinkedPlatform {
    fn eligible_projects_url(self) -> Option<String> {
        match self {
            LinkedPlatform::INaturalist => None,
            LinkedPlatform::Zooniverse => Some(
                crate::search::FinderQuery::projects()
                    .with_misc_filter(crate::search::MiscFilter::ZooniverseAffiliate)
                    .url(),
            ),
        }
    }

    fn article(self) -> &'static str {
        match self {
            LinkedPlatform::INaturalist => "an",
            LinkedPlatform::Zooniverse => "a",
        }
    }

    fn sign_in_url(self) -> &'static str {
        match self {
            LinkedPlatform::INaturalist => "https://www.inaturalist.org/login",
            LinkedPlatform::Zooniverse => "https://www.zooniverse.org/",
        }
    }

    fn contribution(self) -> &'static str {
        match self {
            LinkedPlatform::INaturalist => "observation",
            LinkedPlatform::Zooniverse => "classification",
        }
    }

    fn contributed(self) -> &'static str {
        match self {
            LinkedPlatform::INaturalist => "uploaded",
            LinkedPlatform::Zooniverse => "made",
        }
    }
}

/// The platform whose activity on a project is credited to anybody who
/// connects their account there, if there is one.
///
/// Only an affiliate credits anything. A project in the Zooniverse program is
/// a Zooniverse project whatever its URL says: several of them are hosted on
/// their own domains.
pub async fn for_project(
    project_id: Ulid,
    is_affiliate: bool,
    is_inaturalist: bool,
) -> Result<Option<LinkedPlatform>, Error> {
    if !is_affiliate {
        return Ok(None);
    }

    if zooniverse_affiliates().await?.contains(&project_id) {
        return Ok(Some(LinkedPlatform::Zooniverse));
    }

    Ok(is_inaturalist.then_some(LinkedPlatform::INaturalist))
}

#[cfg(test)]
mod tests {
    use sqlx::SqlitePool;
    use sqlx::sqlite::SqlitePoolOptions;

    use super::*;

    /// A swept and an unswept project, and the cache holding `affiliates`
    /// the way `fetch_fresh` stores it, or nothing.
    async fn projects(affiliates: Option<fn(Ulid, Ulid) -> Vec<Ulid>>) -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect(":memory:")
            .await
            .unwrap();

        sqlx::query("CREATE TABLE projects (project_id TEXT, name TEXT)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("CREATE TABLE lazy_cache (cache_key TEXT, value TEXT, updated_at TEXT)")
            .execute(&pool)
            .await
            .unwrap();

        let (swept, unswept) = (Ulid::new(), Ulid::new());

        for (project_id, name) in [(swept, "swept"), (unswept, "unswept")] {
            sqlx::query("INSERT INTO projects VALUES (?, ?)")
                .bind(project_id.to_string())
                .bind(name)
                .execute(&pool)
                .await
                .unwrap();
        }

        if let Some(affiliates) = affiliates {
            sqlx::query("INSERT INTO lazy_cache VALUES (?, ?, '2026-10-21T00:00:00Z')")
                .bind(ZooniverseAffiliates.cache_key())
                .bind(serde_json::to_string(&affiliates(swept, unswept)).unwrap())
                .execute(&pool)
                .await
                .unwrap();
        }

        pool
    }

    async fn listed(pool: &SqlitePool) -> Vec<String> {
        let mut qb = sqlx::QueryBuilder::new("SELECT p.name FROM projects p WHERE 1 = 1");
        push_zooniverse_affiliate_filter(&mut qb);
        qb.push(" ORDER BY p.name");

        qb.build_query_scalar::<String>()
            .fetch_all(pool)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn the_filter_lists_the_projects_the_bridge_credits() {
        let pool = projects(Some(|swept, _| vec![swept])).await;

        assert_eq!(listed(&pool).await, ["swept"]);
    }

    #[tokio::test]
    async fn with_nothing_cached_the_filter_lists_nothing() {
        assert!(listed(&projects(None).await).await.is_empty());
    }
}
