//! Which regions a project falls in.
//!
//! `regions` holds the vocabulary of named areas that region-scoped search is
//! expressed in, mirrored from orrery. `project_regions` caches, for each
//! project, the ones its footprint reaches, so a search joins a table instead
//! of testing polygons per query.
//!
//! The cache is derived, never authored: everything that writes it goes
//! through [`refresh_project`], and [`rebuild_all`] redoes the lot when the
//! rule or a region polygon changes.

use anyhow::Error;
use common::{shared::db, transaction::Immediate};

/// How much of a footprint must lie inside a region to count as being in it.
///
/// Both SpatiaLite and PostGIS treat two shapes that share a single boundary
/// point as intersecting, and neither these polygons nor the ones project
/// managers draw are precise enough for that to carry meaning. On the
/// Australian state boundaries it put South Australian projects into
/// Queensland and Victorian results, because the state polygons themselves
/// overlap each other by a few hundred square kilometres along their shared
/// borders.
///
/// The share is measured against the smaller of the two shapes, so a small
/// project sitting inside a large state and a nationwide project covering a
/// small territory both read as a full match without either needing a special
/// case, while a polygon that merely smears across a border does not.
///
/// Kept deliberately equal to orrery's `Region.MIN_OVERLAP`. The two run the
/// same rule against the same polygons and are expected to agree; if one
/// changes, the other has to change with it, and both caches need rebuilding.
pub const MIN_OVERLAP: f64 = 0.005;

/// Recompute which regions one project falls in.
///
/// Cheap enough to run on every save that could have moved a footprint: it
/// measures one footprint against the region list rather than the region
/// list against the catalogue.
///
/// Three places write a project's geometry, and all three call this:
/// `ProjectAll::save_to_database` for a whole-project save, the `Point` and
/// `Regions` field savers for a single edited field, and
/// `geocode::geocode_and_update_project_point` for a point derived from an
/// address. A fourth writer would need to call it too.
pub async fn refresh_project(project_id: &str) -> Result<(), Error> {
    let mut tx = Immediate::begin_with_retry(db(), None, None).await?;

    refresh_project_in(&mut tx, project_id).await?;

    tx.commit().await?;

    Ok(())
}

/// The matching rule itself, against an open transaction.
///
/// Split out so a whole-catalogue rebuild can hold one transaction across
/// every project rather than paying a commit each, while both paths run the
/// same statement.
async fn refresh_project_in(tx: &mut Immediate, project_id: &str) -> Result<(), Error> {
    sqlx::query!(
        r#"delete from project_regions where "project_id" = ?"#,
        project_id
    )
    .execute(&mut *tx)
    .await?;

    // The one statement of the matching rule. A project earns a region when
    // its point falls inside it, or when its coverage shares a real part of
    // the smaller of the two shapes with it.
    //
    // Either shape wholly inside the other short-circuits the measurement,
    // which is also the common case: most coverage polygons were picked from
    // this very list, or drawn around the whole world.
    sqlx::query!(
        r#"
        with footprint as (
          select
            p."project_id" as "pid",
            MakeValid(p."regions") as "coverage",
            p."point" as "pt"
          from projects p
          where p."project_id" = ?
        )
        insert into project_regions ("project_id", "region_id")
        select footprint."pid", r."region_id"
        from footprint, regions r
        where
          (footprint."pt" is not null and ST_Intersects(footprint."pt", r."value"))
          or (
            footprint."coverage" is not null
            and ST_Intersects(footprint."coverage", r."value")
            and (
              ST_CoveredBy(footprint."coverage", r."value")
              or ST_CoveredBy(r."value", footprint."coverage")
              or ST_Area(ST_Intersection(footprint."coverage", r."value"))
                 >= ? * min(
                      ST_Area(footprint."coverage"),
                      ST_Area(r."value")
                    )
            )
          )
        "#,
        project_id,
        MIN_OVERLAP,
    )
    .execute(&mut *tx)
    .await?;

    Ok(())
}

/// Recompute the whole cache, and report how many projects were examined.
///
/// Wanted after a region polygon moves or after [`MIN_OVERLAP`] changes,
/// neither of which any single project's save would notice.
pub async fn rebuild_all() -> Result<usize, Error> {
    let projects: Vec<String> = sqlx::query_scalar!(
        r#"
        select "project_id" as "project_id!"
        from projects
        where "regions" is not null or "point" is not null
        order by "project_id"
        "#
    )
    .fetch_all(db())
    .await?;

    let total = projects.len();
    let mut failed = 0;

    // One transaction for the lot: a commit per project would cost more than
    // the geometry does.
    let mut tx = Immediate::begin_with_retry(db(), None, None).await?;

    for project_id in projects.iter() {
        // One unusable polygon should not cost the whole rebuild.
        if let Err(err) = refresh_project_in(&mut tx, project_id).await {
            failed += 1;
            tracing::error!("Region refresh failed for project {project_id}: {err}");
        }
    }

    tx.commit().await?;

    // Projects that lost their footprint entirely are not in the list above,
    // so their rows would otherwise linger.
    sqlx::query!(
        r#"
        delete from project_regions
        where "project_id" not in (
          select "project_id" from projects
          where "regions" is not null or "point" is not null
        )
        "#
    )
    .execute(db())
    .await?;

    tracing::info!("Rebuilt region matches for {total} projects, {failed} failed");

    Ok(total)
}

/// The digest held for a region, or `None` if we have never seen it.
///
/// Read before fetching a shape, so an unchanged polygon costs an index
/// lookup rather than a megabyte of transfer.
pub async fn digest_of(region_id: i64) -> Result<Option<String>, Error> {
    Ok(sqlx::query_scalar!(
        r#"select "digest" from regions where "region_id" = ?"#,
        region_id
    )
    .fetch_optional(db())
    .await?)
}

/// Record a region's shape, reporting whether it is new or has moved.
///
/// `digest` is orrery's md5 of the EWKT, carried across so a sync pass can
/// tell an unchanged polygon from one worth ten megabytes of transfer.
pub async fn store(region_id: i64, name: &str, digest: &str, ewkt: &str) -> Result<bool, Error> {
    let held: Option<String> = sqlx::query_scalar!(
        r#"select "digest" from regions where "region_id" = ?"#,
        region_id
    )
    .fetch_optional(db())
    .await?;

    if held.as_deref() == Some(digest) {
        return Ok(false);
    }

    sqlx::query!(
        r#"
        insert or replace into regions ("region_id", "name", "digest", "value")
        values (?, ?, ?, GeomFromEWKT(?))
        "#,
        region_id,
        name,
        digest,
        ewkt,
    )
    .execute(db())
    .await?;

    Ok(true)
}

/// Drop regions orrery no longer has, and say how many went.
///
/// A region that disappears upstream has to disappear here too, or a search
/// keeps offering an area nobody can be in any more.
pub async fn retain_only(region_ids: &[i64]) -> Result<usize, Error> {
    let held: Vec<i64> = sqlx::query_scalar!(r#"select "region_id" from regions"#)
        .fetch_all(db())
        .await?;

    let mut dropped = 0;

    for region_id in held.into_iter().filter(|id| !region_ids.contains(id)) {
        sqlx::query!(
            r#"delete from project_regions where "region_id" = ?"#,
            region_id
        )
        .execute(db())
        .await?;

        sqlx::query!(r#"delete from regions where "region_id" = ?"#, region_id)
            .execute(db())
            .await?;

        dropped += 1;
    }

    Ok(dropped)
}

/// Every region, as (id, name), for offering a choice of them.
pub async fn all() -> Result<Vec<(i64, String)>, Error> {
    Ok(
        sqlx::query!(r#"select "region_id", "name" from regions order by "name""#)
            .fetch_all(db())
            .await?
            .into_iter()
            .map(|row| (row.region_id, row.name))
            .collect(),
    )
}

/// The regions named by these search phrases, as ids.
///
/// The exchange offers regions as tick-boxes whose value is the region's
/// name, so a search arrives carrying names rather than ids. Phrases that
/// name no region are left for the text search to deal with.
pub async fn ids_for_names(names: &[String]) -> Result<Vec<(i64, String)>, Error> {
    let mut found = Vec::new();

    for name in names.iter().filter(|name| !name.trim().is_empty()) {
        let region: Option<(i64, String)> =
            sqlx::query_as(r#"select "region_id", "name" from regions where "name" = ?"#)
                .bind(name)
                .fetch_optional(db())
                .await?;

        if let Some(region) = region {
            found.push(region);
        }
    }

    Ok(found)
}
