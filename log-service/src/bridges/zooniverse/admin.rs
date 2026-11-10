//! Connecting Zooniverse projects to the bridge, which is what hydra's admin
//! page for them asks of log-service.
//!
//! While orrery is the primary, staff keep most connections in orrery's
//! Django admin, and the project sync copies them here: each copy carries
//! orrery's row id, and is listed here but not changed here, since the next
//! copy would undo it. Staff may connect projects of their own here as well.
//! Once hydra is the primary, every connection is kept here.
//!
//! A connection is one Panoptes project, so the same project can't be
//! connected twice: the bridge's progress through its classifications is
//! the project's, and reading it twice would credit it twice. Connecting a
//! project, or moving it to the other account, needs Panoptes to confirm
//! that the account can read it, because a project read as an account that
//! can't stalls without a word.

use std::collections::HashSet;

use anyhow::Result;
use async_nats::service::Request;
use common::message::log::{
    ZooniverseAccess, ZooniverseProject, ZooniverseProjectAdd, ZooniverseProjectAdded,
    ZooniverseProjectDelete, ZooniverseProjectUpdate, ZooniverseProjectUpdated, ZooniverseReader,
};
use common::primary::CopyKeeper;
use common::service::decode_structured_message;
use common::shared::db;
use common::transaction::Immediate;
use serde::Deserialize;
use sqlx::SqlitePool;
use ulid::Ulid;

use super::{access, announce_projects};

/// NATS `log.zooniverse.admin.list`: every connected project, in Panoptes id
/// order.
pub async fn handle_list(_req: &Request) -> Result<Vec<ZooniverseProject>> {
    connected(db(), None, CopyKeeper::NOW).await
}

/// NATS `log.zooniverse.admin.add`.
pub async fn handle_add(req: &Request) -> Result<ZooniverseProjectAdded> {
    let add: ZooniverseProjectAdd = decode_structured_message(req.message.clone())?;
    let client = access::client()?;

    let added = add_connection(db(), &add, CopyKeeper::NOW, async |reader, project_id| {
        access::check(&client, reader, project_id).await
    })
    .await?;

    if let ZooniverseProjectAdded::Added(_) = added {
        announce_projects().await;
    }

    Ok(added)
}

/// NATS `log.zooniverse.admin.update`.
pub async fn handle_update(req: &Request) -> Result<ZooniverseProjectUpdated> {
    let update: ZooniverseProjectUpdate = decode_structured_message(req.message.clone())?;
    let client = access::client()?;

    let updated = update_connection(
        db(),
        &update,
        CopyKeeper::NOW,
        async |reader, project_id| access::check(&client, reader, project_id).await,
    )
    .await?;

    if let ZooniverseProjectUpdated::Updated(_) = updated {
        announce_projects().await;
    }

    Ok(updated)
}

/// NATS `log.zooniverse.admin.delete`.
pub async fn handle_delete(req: &Request) -> Result<()> {
    let delete: ZooniverseProjectDelete = decode_structured_message(req.message.clone())?;

    delete_connection(db(), delete.zooniverse_id, CopyKeeper::NOW).await?;
    announce_projects().await;

    Ok(())
}

/// The connected projects, or only the one connected for `zooniverse_id`.
async fn connected(
    pool: &SqlitePool,
    zooniverse_id: Option<i64>,
    keeper: CopyKeeper,
) -> Result<Vec<ZooniverseProject>> {
    let rows = sqlx::query!(
        r#"SELECT p.zooniverse_id AS "zooniverse_id!", p.entity_id, p.orrery_id,
                  p.reader AS "reader: ZooniverseReader",
                  COALESCE(g.last_classification_id, 0) AS "last_classification_id!: i64"
           FROM zooniverse_projects p
           LEFT JOIN zooniverse_progress g ON g.zooniverse_id = p.zooniverse_id
           WHERE ?1 IS NULL OR p.zooniverse_id = ?1
           ORDER BY p.zooniverse_id"#,
        zooniverse_id,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .filter_map(|row| {
            let entity_id = row
                .entity_id
                .parse::<Ulid>()
                .inspect_err(|e| {
                    eprintln!(
                        "Unreadable entity {:?} for Zooniverse project {}: {e}",
                        row.entity_id, row.zooniverse_id
                    )
                })
                .ok()?;

            Some(ZooniverseProject {
                zooniverse_id: row.zooniverse_id,
                entity_id,
                reader: row.reader,
                orrery_owns: keeper.orrery_owns(row.orrery_id),
                last_classification_id: row.last_classification_id,
            })
        })
        .collect())
}

async fn connected_one(
    pool: &SqlitePool,
    zooniverse_id: i64,
    keeper: CopyKeeper,
) -> Result<Option<ZooniverseProject>> {
    Ok(connected(pool, Some(zooniverse_id), keeper)
        .await?
        .into_iter()
        .next())
}

/// Connect `add`'s Panoptes project, unless it is connected already, or
/// `check` -- which asks Panoptes, outside any transaction -- doesn't
/// confirm that its reader can read it.
///
/// A project connected before and since disconnected carries on from the
/// classification the bridge had reached; one new here starts from the
/// first, which the connection floors keep from crediting anybody's back
/// catalogue.
async fn add_connection(
    pool: &SqlitePool,
    add: &ZooniverseProjectAdd,
    keeper: CopyKeeper,
    check: impl AsyncFnOnce(ZooniverseReader, i64) -> ZooniverseAccess,
) -> Result<ZooniverseProjectAdded> {
    if add.zooniverse_id <= 0 {
        return Ok(ZooniverseProjectAdded::InvalidProjectId);
    }

    if let Some(existing) = connected_one(pool, add.zooniverse_id, keeper).await? {
        return Ok(ZooniverseProjectAdded::AlreadyConnected(existing));
    }

    let access = check(add.reader, add.zooniverse_id).await;

    if !access.connectable() {
        return Ok(ZooniverseProjectAdded::NotGranted(access));
    }

    let entity_id = add.entity_id.to_string();

    // Connected meanwhile, perhaps by the sync, if nothing is inserted.
    let inserted = sqlx::query!(
        r#"INSERT INTO zooniverse_projects (zooniverse_id, entity_id, reader, orrery_id)
           VALUES (?, ?, ?, NULL)
           ON CONFLICT (zooniverse_id) DO NOTHING"#,
        add.zooniverse_id,
        entity_id,
        add.reader,
    )
    .execute(pool)
    .await?
    .rows_affected()
        == 1;

    let Some(project) = connected_one(pool, add.zooniverse_id, keeper).await? else {
        anyhow::bail!(
            "Zooniverse project {} vanished as it was connected",
            add.zooniverse_id
        );
    };

    Ok(if inserted {
        ZooniverseProjectAdded::Added(project)
    } else {
        ZooniverseProjectAdded::AlreadyConnected(project)
    })
}

/// Credit a connected project to `update`'s SciStarter project and read it
/// as `update`'s reader. A new reader needs `check` to confirm that it can
/// read the project; the bridge's progress through it is the project's, so
/// the new reader carries on where the old one left off.
async fn update_connection(
    pool: &SqlitePool,
    update: &ZooniverseProjectUpdate,
    keeper: CopyKeeper,
    check: impl AsyncFnOnce(ZooniverseReader, i64) -> ZooniverseAccess,
) -> Result<ZooniverseProjectUpdated> {
    let Some(current) = connected_one(pool, update.zooniverse_id, keeper).await? else {
        return Ok(ZooniverseProjectUpdated::NotFound);
    };

    if current.orrery_owns {
        return Ok(ZooniverseProjectUpdated::OrreryOwns(current));
    }

    if update.reader != current.reader {
        let access = check(update.reader, update.zooniverse_id).await;

        if !access.connectable() {
            return Ok(ZooniverseProjectUpdated::NotGranted(access));
        }
    }

    let entity_id = update.entity_id.to_string();

    // Nothing is updated if the project was disconnected meanwhile, or the
    // sync found it in orrery's set.
    sqlx::query!(
        r#"UPDATE zooniverse_projects SET entity_id = ?, reader = ?
           WHERE zooniverse_id = ? AND (orrery_id IS NULL OR NOT ?)"#,
        entity_id,
        update.reader,
        update.zooniverse_id,
        keeper.orrery_keeps_copies(),
    )
    .execute(pool)
    .await?;

    Ok(
        match connected_one(pool, update.zooniverse_id, keeper).await? {
            None => ZooniverseProjectUpdated::NotFound,
            Some(project) if project.orrery_owns => ZooniverseProjectUpdated::OrreryOwns(project),
            Some(project) => ZooniverseProjectUpdated::Updated(project),
        },
    )
}

/// Disconnect a project, unless orrery keeps it. Its progress is kept, for
/// if it is connected again.
async fn delete_connection(
    pool: &SqlitePool,
    zooniverse_id: i64,
    keeper: CopyKeeper,
) -> Result<()> {
    sqlx::query!(
        r#"DELETE FROM zooniverse_projects
           WHERE zooniverse_id = ? AND (orrery_id IS NULL OR NOT ?)"#,
        zooniverse_id,
        keeper.orrery_keeps_copies(),
    )
    .execute(pool)
    .await?;

    Ok(())
}

/// One of orrery's Zooniverse project connections, as
/// `/staff/sync/zooniverse_projects` lists it.
#[derive(Debug, Clone, Deserialize)]
pub struct OrreryProject {
    pub id: i64,
    pub zooniverse_id: i64,
    pub entity_ulid: String,
    pub reader: ZooniverseReader,
}

/// Make the connections copied from orrery match `projects`, the whole of
/// orrery's set; see [`mirror`].
pub async fn mirror_orrery(projects: &[OrreryProject]) -> Result<()> {
    mirror(db(), projects).await
}

/// Make the connections copied from orrery match `projects`, the whole of
/// orrery's set, in one transaction. Each is copied, or its copy brought up
/// to date, and a copy whose original is missing from the set is deleted;
/// connections made here are left alone.
///
/// A copy is matched to its original by Panoptes id, because a connection
/// is one Panoptes project and the bridge's progress is that project's. So
/// an original also adopts a connection here for the same project: the one
/// made here, which then becomes orrery's, or one copied before copies
/// carried orrery's id. Either keeps its progress.
///
/// orrery doesn't refuse a second connection for one Panoptes project, as
/// this does. Of several, the earliest is copied and the rest are reported.
/// One that can't be read -- an id that can't be orrery's, or a SciStarter
/// project that isn't a ULID -- leaves its copy as it was.
async fn mirror(pool: &SqlitePool, projects: &[OrreryProject]) -> Result<()> {
    let mut projects: Vec<&OrreryProject> = projects.iter().collect();
    projects.sort_by_key(|project| project.id);

    let mut tx = Immediate::begin_with_retry(pool, None, None).await?;
    let mut kept: Vec<i64> = Vec::with_capacity(projects.len());
    let mut copied: HashSet<i64> = HashSet::new();

    for project in projects {
        if project.id <= 0 || project.zooniverse_id <= 0 {
            eprintln!(
                "Skipping orrery's Zooniverse project {} (Panoptes {}): its ids can't be right",
                project.id, project.zooniverse_id
            );
            continue;
        }

        if project.entity_ulid.parse::<Ulid>().is_err() {
            eprintln!(
                "Leaving orrery's Zooniverse project {} as it was: {:?} is no SciStarter project",
                project.id, project.entity_ulid
            );
            kept.push(project.id);
            continue;
        }

        if !copied.insert(project.zooniverse_id) {
            eprintln!(
                "Skipping orrery's Zooniverse project {}: Panoptes project {} is connected by an \
                 earlier one",
                project.id, project.zooniverse_id
            );
            continue;
        }

        // The original now names another Panoptes project, so its copy is
        // a different connection from the one this was.
        sqlx::query!(
            r#"DELETE FROM zooniverse_projects WHERE orrery_id = ? AND zooniverse_id != ?"#,
            project.id,
            project.zooniverse_id,
        )
        .execute(&mut tx)
        .await?;

        sqlx::query!(
            r#"INSERT INTO zooniverse_projects (zooniverse_id, entity_id, reader, orrery_id)
               VALUES (?, ?, ?, ?)
               ON CONFLICT (zooniverse_id) DO UPDATE SET
                 entity_id = excluded.entity_id,
                 reader = excluded.reader,
                 orrery_id = excluded.orrery_id"#,
            project.zooniverse_id,
            project.entity_ulid,
            project.reader,
            project.id,
        )
        .execute(&mut tx)
        .await?;

        kept.push(project.id);
    }

    let kept = serde_json::to_string(&kept)?;

    sqlx::query!(
        r#"DELETE FROM zooniverse_projects
           WHERE orrery_id IS NOT NULL
             AND orrery_id NOT IN (SELECT value FROM json_each(?))"#,
        kept,
    )
    .execute(&mut tx)
    .await?;

    tx.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use assert2::{check, let_assert};

    use super::*;

    const ENTITY: &str = "01JKXMG5WNN9CJJ6VAWF62645Z";
    const OTHER_ENTITY: &str = "01JKXMG32Q9C83WDYD4GP9337R";

    async fn fixture() -> SqlitePool {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("pool");

        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("schema");

        pool
    }

    /// A connection as the migration left one copied before copies carried
    /// orrery's id, with the bridge `last` classifications in.
    async fn copied_before(pool: &SqlitePool, zooniverse_id: i64, last: i64) {
        sqlx::query(
            r#"INSERT INTO zooniverse_projects (zooniverse_id, entity_id, reader, orrery_id)
               VALUES (?, ?, 'legacy', -?)"#,
        )
        .bind(zooniverse_id)
        .bind(ENTITY)
        .bind(zooniverse_id)
        .execute(pool)
        .await
        .unwrap();

        progress(pool, zooniverse_id, last).await;
    }

    async fn progress(pool: &SqlitePool, zooniverse_id: i64, last: i64) {
        sqlx::query(
            r#"INSERT INTO zooniverse_progress (zooniverse_id, last_classification_id)
               VALUES (?, ?)"#,
        )
        .bind(zooniverse_id)
        .bind(last)
        .execute(pool)
        .await
        .unwrap();
    }

    fn orrery(id: i64, zooniverse_id: i64, reader: ZooniverseReader) -> OrreryProject {
        OrreryProject {
            id,
            zooniverse_id,
            entity_ulid: ENTITY.to_string(),
            reader,
        }
    }

    fn add(zooniverse_id: i64, reader: ZooniverseReader) -> ZooniverseProjectAdd {
        ZooniverseProjectAdd {
            entity_id: OTHER_ENTITY.parse().unwrap(),
            zooniverse_id,
            reader,
        }
    }

    fn access(reader: ZooniverseReader, roles: &[&str]) -> ZooniverseAccess {
        ZooniverseAccess {
            reader,
            account: None,
            roles: roles.iter().map(|role| role.to_string()).collect(),
            readable: true,
            trouble: None,
        }
    }

    async fn granted(reader: ZooniverseReader, _project_id: i64) -> ZooniverseAccess {
        access(reader, &["collaborator"])
    }

    async fn refused(reader: ZooniverseReader, _project_id: i64) -> ZooniverseAccess {
        access(reader, &[])
    }

    async fn never_asked(_reader: ZooniverseReader, _project_id: i64) -> ZooniverseAccess {
        panic!("Panoptes was asked when it needn't have been")
    }

    /// Each connection as (Panoptes id, reader, orrery's, progress).
    async fn listed(pool: &SqlitePool) -> Vec<(i64, ZooniverseReader, bool, i64)> {
        connected(pool, None, CopyKeeper::Orrery)
            .await
            .unwrap()
            .into_iter()
            .map(|p| {
                (
                    p.zooniverse_id,
                    p.reader,
                    p.orrery_owns,
                    p.last_classification_id,
                )
            })
            .collect()
    }

    use ZooniverseReader::{Legacy, Scistarter};

    #[tokio::test]
    async fn orrerys_set_adopts_the_copies_made_before_it_was_marked_and_keeps_their_progress() {
        let pool = fixture().await;
        copied_before(&pool, 6250, 790276182).await;
        copied_before(&pool, 19023, 658838757).await;

        mirror(&pool, &[orrery(1, 6250, Legacy)]).await.unwrap();

        // 19023 was deleted on orrery since it was copied.
        check!(listed(&pool).await == vec![(6250, Legacy, true, 790276182)]);

        let orrery_id: Option<i64> =
            sqlx::query_scalar("SELECT orrery_id FROM zooniverse_projects")
                .fetch_one(&pool)
                .await
                .unwrap();
        check!(orrery_id == Some(1));
    }

    #[tokio::test]
    async fn the_mirror_follows_orrerys_set_and_leaves_connections_made_here_alone() {
        let pool = fixture().await;
        add_connection(&pool, &add(30240, Scistarter), CopyKeeper::Orrery, granted)
            .await
            .unwrap();

        mirror(&pool, &[orrery(1, 6250, Legacy), orrery(2, 8149, Legacy)])
            .await
            .unwrap();
        check!(
            listed(&pool).await
                == vec![
                    (6250, Legacy, true, 0),
                    (8149, Legacy, true, 0),
                    (30240, Scistarter, false, 0),
                ]
        );

        // A reader changed on orrery is changed here, and a connection
        // deleted there is deleted here.
        mirror(&pool, &[orrery(2, 8149, Scistarter)]).await.unwrap();
        check!(
            listed(&pool).await == vec![(8149, Scistarter, true, 0), (30240, Scistarter, false, 0)]
        );

        mirror(&pool, &[]).await.unwrap();
        check!(listed(&pool).await == vec![(30240, Scistarter, false, 0)]);
    }

    #[tokio::test]
    async fn an_original_repointed_at_another_panoptes_project_is_a_new_connection() {
        let pool = fixture().await;
        mirror(&pool, &[orrery(1, 6250, Legacy)]).await.unwrap();
        progress(&pool, 6250, 500).await;

        mirror(&pool, &[orrery(1, 8149, Legacy)]).await.unwrap();
        check!(listed(&pool).await == vec![(8149, Legacy, true, 0)]);

        // The progress through 6250 is still 6250's.
        mirror(&pool, &[orrery(1, 6250, Legacy)]).await.unwrap();
        check!(listed(&pool).await == vec![(6250, Legacy, true, 500)]);
    }

    #[tokio::test]
    async fn orrerys_connection_for_a_project_connected_here_takes_it_over() {
        let pool = fixture().await;
        add_connection(&pool, &add(30240, Scistarter), CopyKeeper::Orrery, granted)
            .await
            .unwrap();
        progress(&pool, 30240, 77).await;

        mirror(&pool, &[orrery(9, 30240, Legacy)]).await.unwrap();

        let projects = connected(&pool, None, CopyKeeper::Orrery).await.unwrap();
        let_assert!([project] = projects.as_slice());
        check!(project.orrery_owns);
        check!(project.reader == Legacy);
        check!(project.entity_id.to_string() == ENTITY);
        check!(project.last_classification_id == 77);
    }

    #[tokio::test]
    async fn of_two_originals_for_one_panoptes_project_the_earlier_is_copied() {
        let pool = fixture().await;
        let mut later = orrery(5, 6250, Scistarter);
        later.entity_ulid = OTHER_ENTITY.to_string();

        mirror(&pool, &[later, orrery(3, 6250, Legacy)])
            .await
            .unwrap();

        let projects = connected(&pool, None, CopyKeeper::Orrery).await.unwrap();
        let_assert!([project] = projects.as_slice());
        check!(project.reader == Legacy);
        check!(project.entity_id.to_string() == ENTITY);
    }

    #[tokio::test]
    async fn an_unreadable_original_leaves_its_copy_as_it_was() {
        let pool = fixture().await;
        mirror(&pool, &[orrery(1, 6250, Legacy)]).await.unwrap();

        let mut unreadable = orrery(1, 6250, Scistarter);
        unreadable.entity_ulid = "not a ulid".to_string();
        mirror(&pool, &[unreadable]).await.unwrap();

        check!(listed(&pool).await == vec![(6250, Legacy, true, 0)]);
    }

    #[tokio::test]
    async fn a_project_is_connected_only_once_panoptes_confirms_its_reader_can_read_it() {
        let pool = fixture().await;

        let_assert!(
            ZooniverseProjectAdded::NotGranted(refusal) =
                add_connection(&pool, &add(30240, Scistarter), CopyKeeper::Orrery, refused)
                    .await
                    .unwrap()
        );
        check!(refusal.reader == Scistarter);
        check!(listed(&pool).await.is_empty());

        let unanswered = async |reader, _| ZooniverseAccess {
            trouble: Some("Panoptes didn't answer in time".to_string()),
            ..access(reader, &["collaborator"])
        };
        let_assert!(
            ZooniverseProjectAdded::NotGranted(_) = add_connection(
                &pool,
                &add(30240, Scistarter),
                CopyKeeper::Orrery,
                unanswered
            )
            .await
            .unwrap()
        );

        let_assert!(
            ZooniverseProjectAdded::Added(project) =
                add_connection(&pool, &add(30240, Scistarter), CopyKeeper::Orrery, granted)
                    .await
                    .unwrap()
        );
        check!(!project.orrery_owns);
        check!(project.entity_id.to_string() == OTHER_ENTITY);
    }

    #[tokio::test]
    async fn a_project_already_connected_either_way_is_refused_without_asking_panoptes() {
        let pool = fixture().await;
        mirror(&pool, &[orrery(1, 6250, Legacy)]).await.unwrap();
        add_connection(&pool, &add(30240, Scistarter), CopyKeeper::Orrery, granted)
            .await
            .unwrap();

        for zooniverse_id in [6250, 30240] {
            let_assert!(
                ZooniverseProjectAdded::AlreadyConnected(existing) = add_connection(
                    &pool,
                    &add(zooniverse_id, Scistarter),
                    CopyKeeper::Orrery,
                    never_asked
                )
                .await
                .unwrap()
            );
            check!(existing.zooniverse_id == zooniverse_id);
        }

        for zooniverse_id in [0, -6250] {
            let_assert!(
                ZooniverseProjectAdded::InvalidProjectId = add_connection(
                    &pool,
                    &add(zooniverse_id, Scistarter),
                    CopyKeeper::Orrery,
                    never_asked
                )
                .await
                .unwrap()
            );
        }
    }

    #[tokio::test]
    async fn a_reconnected_project_carries_on_where_the_bridge_left_it() {
        let pool = fixture().await;
        add_connection(&pool, &add(30240, Scistarter), CopyKeeper::Orrery, granted)
            .await
            .unwrap();
        progress(&pool, 30240, 123).await;

        delete_connection(&pool, 30240, CopyKeeper::Orrery)
            .await
            .unwrap();
        check!(listed(&pool).await.is_empty());

        let_assert!(
            ZooniverseProjectAdded::Added(project) =
                add_connection(&pool, &add(30240, Scistarter), CopyKeeper::Orrery, granted)
                    .await
                    .unwrap()
        );
        check!(project.last_classification_id == 123);
    }

    #[tokio::test]
    async fn only_a_new_reader_needs_panoptes_to_confirm_it() {
        let pool = fixture().await;
        add_connection(&pool, &add(30240, Scistarter), CopyKeeper::Orrery, granted)
            .await
            .unwrap();

        let moved = |entity: &str, reader| ZooniverseProjectUpdate {
            zooniverse_id: 30240,
            entity_id: entity.parse().unwrap(),
            reader,
        };

        let_assert!(
            ZooniverseProjectUpdated::Updated(project) = update_connection(
                &pool,
                &moved(ENTITY, Scistarter),
                CopyKeeper::Orrery,
                never_asked
            )
            .await
            .unwrap()
        );
        check!(project.entity_id.to_string() == ENTITY);

        let_assert!(
            ZooniverseProjectUpdated::NotGranted(_) =
                update_connection(&pool, &moved(ENTITY, Legacy), CopyKeeper::Orrery, refused)
                    .await
                    .unwrap()
        );
        check!(listed(&pool).await == vec![(30240, Scistarter, false, 0)]);

        let_assert!(
            ZooniverseProjectUpdated::Updated(project) =
                update_connection(&pool, &moved(ENTITY, Legacy), CopyKeeper::Orrery, granted)
                    .await
                    .unwrap()
        );
        check!(project.reader == Legacy);

        let mut gone = moved(ENTITY, Legacy);
        gone.zooniverse_id = 1;
        let_assert!(
            ZooniverseProjectUpdated::NotFound =
                update_connection(&pool, &gone, CopyKeeper::Orrery, never_asked)
                    .await
                    .unwrap()
        );
    }

    #[tokio::test]
    async fn orrerys_connections_are_changed_here_only_once_hydra_keeps_them() {
        let pool = fixture().await;
        mirror(&pool, &[orrery(1, 6250, Legacy)]).await.unwrap();
        let update = ZooniverseProjectUpdate {
            zooniverse_id: 6250,
            entity_id: OTHER_ENTITY.parse().unwrap(),
            reader: Scistarter,
        };

        let_assert!(
            ZooniverseProjectUpdated::OrreryOwns(_) =
                update_connection(&pool, &update, CopyKeeper::Orrery, never_asked)
                    .await
                    .unwrap()
        );
        delete_connection(&pool, 6250, CopyKeeper::Orrery)
            .await
            .unwrap();
        check!(listed(&pool).await == vec![(6250, Legacy, true, 0)]);

        let_assert!(
            ZooniverseProjectUpdated::Updated(project) =
                update_connection(&pool, &update, CopyKeeper::Hydra, granted)
                    .await
                    .unwrap()
        );
        check!(!project.orrery_owns);
        check!(project.reader == Scistarter);

        delete_connection(&pool, 6250, CopyKeeper::Hydra)
            .await
            .unwrap();
        check!(listed(&pool).await.is_empty());
    }

    #[tokio::test]
    async fn the_schema_allows_no_negative_orrery_id_but_a_copys_mark() {
        let pool = fixture().await;
        let insert = |zooniverse_id: i64, orrery_id: i64| {
            sqlx::query(
                r#"INSERT INTO zooniverse_projects (zooniverse_id, entity_id, reader, orrery_id)
                   VALUES (?, ?, 'legacy', ?)"#,
            )
            .bind(zooniverse_id)
            .bind(ENTITY)
            .bind(orrery_id)
            .execute(&pool)
        };

        check!(insert(6250, -6250).await.is_ok());
        check!(insert(8149, -6250).await.is_err());
        check!(insert(8149, -1).await.is_err());
        check!(insert(8149, 0).await.is_err());
        check!(insert(-8149, 8149).await.is_err());
    }
}
