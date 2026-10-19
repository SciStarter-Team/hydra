use anyhow::{Error, anyhow};
use async_nats::service::Request;
use common::{
    message::user::{InatProject, InatProjectMap},
    shared::db,
    ulid::AsUlid,
};

use crate::dashboard::entity_summary::lookup_batch_by_slug;

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

    let fallback = lookup_batch_by_slug(&[INATURALIST_SLUG.to_string()])
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("No project carries the slug `{INATURALIST_SLUG}`"))?
        .id;

    Ok(InatProjectMap { projects, fallback })
}
