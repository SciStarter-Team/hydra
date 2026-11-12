use anyhow::Error;
use async_nats::service::Request;
use common::{
    message::user::{
        EntityCountsRequest, JoinCount, JoinCountsByProjectsResponse, JoinCountsByUsersRequest,
        JoinCountsByUsersResponse, JoinSource, ProjectJoinCheckRequest, ProjectJoinCheckResponse,
        ProjectJoinRequest, ProjectJoinResponse, UserJoinEntry, UserJoinsListRequest,
        UserJoinsListResponse,
    },
    service::{decode_structured_message, log_activity, send},
    shared::db,
    timestring::TimeString,
    transaction::Immediate,
};
use sqlx::SqlitePool;
use ulid::Ulid;

pub async fn create_join(req: &Request) -> Result<ProjectJoinResponse, Error> {
    let message: ProjectJoinRequest = decode_structured_message(req.message.clone())?;

    if message.source == JoinSource::LegacyOrrery {
        anyhow::bail!("only the join sync records orrery's joins");
    }

    record_join(message.user_id, &message.project_id, message.source).await?;

    Ok(ProjectJoinResponse {
        success: true,
        message: "Project joined successfully".to_string(),
    })
}

/// Join `user_id` to `project_id` now, on the evidence `source` names, and
/// if that makes them a participant they were not before, say so to the
/// activity log and to the log service's record of joins.
pub async fn record_join(user_id: Ulid, project_id: &str, source: JoinSource) -> Result<(), Error> {
    let user_id_str = user_id.to_string();
    let now = TimeString::now();

    let new = store_join(db(), &user_id_str, project_id, &now, source).await?;

    if !new {
        return Ok(());
    }

    log_activity(
        Some(now.clone()),
        "project_join",
        Some(user_id),
        None::<Ulid>,
        Some(project_id.to_string()),
    )
    .await?;

    // `log.join`'s fields, in order: when, project, person, active,
    // consented, contacted, notified, activity, reason.
    send(
        "log.join",
        (
            now.to_string(),
            project_id.to_string(),
            user_id_str,
            true,
            None::<String>,
            None::<String>,
            false,
            now.to_string(),
            String::new(),
        ),
    )
    .await?;

    Ok(())
}

/// Record that `user_id` joined `project_id` at `when`, on the evidence
/// `source` names, and return whether they had not joined it before.
///
/// This is the one place the rule for a second join of the same project
/// lives, for live joins and the sync's copies alike. The earlier of the two
/// times is kept, since that is when they joined. Anything hydra recorded
/// replaces the source of orrery's copy, because it says more than orrery's
/// rules did; otherwise the first evidence stands.
pub(crate) async fn store_join(
    pool: &SqlitePool,
    user_id: &str,
    project_id: &str,
    when: &TimeString,
    source: JoinSource,
) -> Result<bool, Error> {
    let mut tx = Immediate::begin_with_retry(pool, None, None).await?;

    let existed = sqlx::query_scalar!(
        r#"SELECT EXISTS (
             SELECT 1 FROM "project_joins" WHERE "user_id" = ? AND "project_id" = ?
           ) AS "existed: bool""#,
        user_id,
        project_id,
    )
    .fetch_one(&mut tx)
    .await?;

    sqlx::query!(
        r#"INSERT INTO "project_joins" ("user_id", "project_id", "when", "source")
           VALUES (?, ?, ?, ?)
           ON CONFLICT ("user_id", "project_id") DO UPDATE SET
               "when" = CASE
                   WHEN unixepoch(excluded."when", 'subsec') < unixepoch("when", 'subsec')
                   THEN excluded."when" ELSE "when" END,
               "source" = CASE
                   WHEN "source" = 'legacy_orrery' THEN excluded."source" ELSE "source" END"#,
        user_id,
        project_id,
        when,
        source,
    )
    .execute(&mut tx)
    .await?;

    tx.commit().await?;

    Ok(!existed)
}

/// Remove orrery's copy of `user_id`'s join to `project_id`, now that orrery
/// no longer counts it, leaving any join hydra recorded itself.
pub(crate) async fn retract_legacy_join(
    pool: &SqlitePool,
    user_id: &str,
    project_id: &str,
) -> Result<(), Error> {
    sqlx::query!(
        r#"DELETE FROM "project_joins"
           WHERE "user_id" = ? AND "project_id" = ? AND "source" = 'legacy_orrery'"#,
        user_id,
        project_id,
    )
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn check_join(req: &Request) -> Result<ProjectJoinCheckResponse, Error> {
    let message: ProjectJoinCheckRequest = decode_structured_message(req.message.clone())?;

    let user_id_str = message.user_id.to_string();

    let result = sqlx::query_scalar!(
        r#"
        SELECT 1 as "result: i32"
        FROM project_joins
        WHERE user_id = ? AND project_id = ?
        LIMIT 1
        "#,
        user_id_str,
        message.project_id
    )
    .fetch_optional(db())
    .await?;

    Ok(ProjectJoinCheckResponse {
        has_joined: result.is_some(),
    })
}

/// How many users have joined each of the requested projects. Serves
/// the dashboard's owned-entity "joins" figures.
pub async fn count_by_projects(req: &Request) -> Result<JoinCountsByProjectsResponse, Error> {
    let message: EntityCountsRequest = decode_structured_message(req.message.clone())?;

    let primary_since = crate::primary_since::read(db()).await?;

    Ok(JoinCountsByProjectsResponse {
        counts: counts_by_projects(db(), &message.entity_ids, primary_since.as_ref()).await?,
        primary_since,
    })
}

/// The counts for each of `project_ids`. `since` is the instant
/// [`JoinCount::since_primary`] counts from; where there is none, there is no
/// such count.
async fn counts_by_projects(
    pool: &SqlitePool,
    project_ids: &[String],
    since: Option<&TimeString>,
) -> Result<Vec<(String, JoinCount)>, Error> {
    let mut counts = Vec::new();

    for project_id in project_ids {
        let row = sqlx::query!(
            r#"SELECT COUNT(*) AS "total!: i64",
                      COALESCE(SUM("source" <> 'legacy_orrery'
                          AND unixepoch("when", 'subsec') >= unixepoch(?, 'subsec')), 0)
                          AS "since_primary!: i64"
               FROM "project_joins"
               WHERE "project_id" = ?"#,
            since,
            project_id,
        )
        .fetch_one(pool)
        .await?;

        if row.total > 0 {
            counts.push((
                project_id.clone(),
                JoinCount {
                    total: row.total,
                    since_primary: since.map(|_| row.since_primary),
                },
            ));
        }
    }

    Ok(counts)
}

/// How many projects each of these people has joined
/// (`user.joins.count_by_users`).
///
/// One grouped query rather than a count per person: the curated-page staff
/// report asks about everyone on a page at once, which can be thousands of
/// people.
pub async fn count_by_users(req: &Request) -> Result<JoinCountsByUsersResponse, Error> {
    let message: JoinCountsByUsersRequest = decode_structured_message(req.message.clone())?;

    let primary_since = crate::primary_since::read(db()).await?;

    Ok(JoinCountsByUsersResponse {
        counts: counts_by_users(db(), &message.user_ids, primary_since.as_ref()).await?,
        primary_since,
    })
}

/// The counts for each of `user_ids`, with `since` as in
/// [`counts_by_projects`].
async fn counts_by_users(
    pool: &SqlitePool,
    user_ids: &[String],
    since: Option<&TimeString>,
) -> Result<Vec<(String, JoinCount)>, Error> {
    use sqlx::{QueryBuilder, Row, Sqlite};

    let mut counts = Vec::new();

    // SQLite caps a statement at 999 bound parameters by default.
    for chunk in user_ids.chunks(900) {
        let mut qb: QueryBuilder<Sqlite> = QueryBuilder::new(
            r#"SELECT "user_id", COUNT(*) AS "total",
                      COALESCE(SUM("source" <> 'legacy_orrery'
                          AND unixepoch("when", 'subsec') >= unixepoch("#,
        );
        qb.push_bind(since.cloned());
        qb.push(
            r#", 'subsec')), 0) AS "since_primary"
               FROM "project_joins" WHERE "user_id" IN ("#,
        );
        let mut separated = qb.separated(", ");
        for user_id in chunk {
            separated.push_bind(user_id.clone());
        }
        qb.push(r#") GROUP BY "user_id""#);

        for row in qb.build().fetch_all(pool).await? {
            counts.push((
                row.get::<String, _>("user_id"),
                JoinCount {
                    total: row.get("total"),
                    since_primary: since.map(|_| row.get("since_primary")),
                },
            ));
        }
    }

    Ok(counts)
}

pub async fn list_by_user_id(req: &Request) -> Result<Vec<String>, Error> {
    let user_id: String = flexbuffers::from_slice(&req.message.payload)?;

    let rows = sqlx::query_scalar!(
        "SELECT project_id FROM project_joins WHERE user_id = ?",
        user_id
    )
    .fetch_all(db())
    .await?;

    Ok(rows)
}

/// The user_ids of the project's participants, as far as hydra knows them
/// itself. Serves the entity broadcast composer, which messages all of a
/// project's participants at once. The request payload is the project_id
/// string (mirrors `list_by_user_id`).
///
/// Orrery's copies are left out: orrery counted following some projects out
/// to their websites as joining them, so a project's owner messaging its
/// participants would reach people who only looked.
pub async fn list_by_project_id(req: &Request) -> Result<Vec<String>, Error> {
    let project_id: String = flexbuffers::from_slice(&req.message.payload)?;

    let rows = sqlx::query_scalar!(
        r#"SELECT "user_id" FROM "project_joins"
           WHERE "project_id" = ? AND "source" <> 'legacy_orrery'"#,
        project_id
    )
    .fetch_all(db())
    .await?;

    Ok(rows)
}

pub async fn list_user_joins(req: &Request) -> Result<UserJoinsListResponse, Error> {
    let message: UserJoinsListRequest = decode_structured_message(req.message.clone())?;

    // Look up user_id from opaque_id
    let user_id = sqlx::query_scalar!(
        "SELECT user_id FROM users WHERE opaque_id = ? LIMIT 1",
        message.opaque_id
    )
    .fetch_optional(db())
    .await?;

    let Some(user_id) = user_id else {
        return Ok(UserJoinsListResponse {
            found: false,
            projects: vec![],
        });
    };

    let rows = sqlx::query_scalar!(
        "SELECT project_id FROM project_joins WHERE user_id = ?",
        user_id
    )
    .fetch_all(db())
    .await?;

    Ok(UserJoinsListResponse {
        found: true,
        projects: rows
            .into_iter()
            .map(|project_id| UserJoinEntry { project_id })
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert2::check;

    const ADA: &str = "01JADA0000000000000000000A";
    const GRACE: &str = "01JGRACE000000000000000000";
    const SQUIRREL: &str = "01JSQRL0000000000000000000";

    /// The joins table as its migrations make it, beside the accounts it
    /// refers to.
    async fn fixture() -> SqlitePool {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("pool");

        sqlx::raw_sql(
            r#"create table "users" ("user_id" text primary key not null);
               create table "misc" ("key" text primary key not null, "value");"#,
        )
        .execute(&pool)
        .await
        .expect("accounts");

        for migration in [
            include_str!("../migrations/20251112172628_project_joins.up.sql"),
            include_str!("../migrations/20261111123511_project_join_when_and_source.up.sql"),
        ] {
            sqlx::raw_sql(migration)
                .execute(&pool)
                .await
                .expect("schema");
        }

        for user_id in [ADA, GRACE] {
            sqlx::query(r#"insert into "users" ("user_id") values (?)"#)
                .bind(user_id)
                .execute(&pool)
                .await
                .expect("user");
        }

        pool
    }

    fn at(when: &str) -> TimeString {
        TimeString::try_from(when.to_string()).unwrap()
    }

    async fn stored(pool: &SqlitePool, user_id: &str) -> Option<(String, JoinSource)> {
        sqlx::query_as(
            r#"select "when", "source" from "project_joins"
               where "user_id" = ? and "project_id" = ?"#,
        )
        .bind(user_id)
        .bind(SQUIRREL)
        .fetch_optional(pool)
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn only_the_first_join_is_new() {
        let pool = fixture().await;
        let when = at("2027-01-02T00:00:00Z");

        check!(
            store_join(&pool, ADA, SQUIRREL, &when, JoinSource::Explicit)
                .await
                .unwrap()
        );
        check!(
            !store_join(&pool, ADA, SQUIRREL, &when, JoinSource::Participation)
                .await
                .unwrap()
        );
        check!(
            stored(&pool, ADA).await == Some((when.to_string(), JoinSource::Explicit)),
            "the first evidence stands"
        );
    }

    #[tokio::test]
    async fn hydra_evidence_replaces_orrery_but_keeps_the_earlier_time() {
        let pool = fixture().await;
        let orrery = at("2019-03-04T05:06:07.123456Z");
        let hydra = at("2027-01-02T00:00:00Z");

        store_join(&pool, ADA, SQUIRREL, &orrery, JoinSource::LegacyOrrery)
            .await
            .unwrap();
        check!(
            !store_join(&pool, ADA, SQUIRREL, &hydra, JoinSource::PlatformConnect)
                .await
                .unwrap()
        );
        check!(stored(&pool, ADA).await == Some((orrery.to_string(), JoinSource::PlatformConnect)));

        // The same two, the other way round: the sync's backfill arriving
        // after hydra recorded the join.
        store_join(&pool, GRACE, SQUIRREL, &hydra, JoinSource::Explicit)
            .await
            .unwrap();
        store_join(&pool, GRACE, SQUIRREL, &orrery, JoinSource::LegacyOrrery)
            .await
            .unwrap();
        check!(stored(&pool, GRACE).await == Some((orrery.to_string(), JoinSource::Explicit)));
    }

    #[tokio::test]
    async fn retracting_orrerys_copy_leaves_hydras_joins() {
        let pool = fixture().await;
        let when = at("2019-03-04T05:06:07Z");

        store_join(&pool, ADA, SQUIRREL, &when, JoinSource::LegacyOrrery)
            .await
            .unwrap();
        store_join(&pool, GRACE, SQUIRREL, &when, JoinSource::LegacyOrrery)
            .await
            .unwrap();
        store_join(
            &pool,
            GRACE,
            SQUIRREL,
            &at("2027-01-02T00:00:00Z"),
            JoinSource::Explicit,
        )
        .await
        .unwrap();

        retract_legacy_join(&pool, ADA, SQUIRREL).await.unwrap();
        retract_legacy_join(&pool, GRACE, SQUIRREL).await.unwrap();

        check!(stored(&pool, ADA).await == None);
        check!(stored(&pool, GRACE).await.map(|(_, source)| source) == Some(JoinSource::Explicit));
    }

    #[tokio::test]
    async fn since_primary_counts_only_what_hydra_recorded_after_cutover() {
        let pool = fixture().await;
        let cutover = at("2027-01-01T00:00:00Z");

        // Orrery's copy dated after cutover still is not hydra's; hydra's
        // own join in the first fraction of a second after cutover is.
        store_join(
            &pool,
            ADA,
            SQUIRREL,
            &at("2027-01-05T00:00:00Z"),
            JoinSource::LegacyOrrery,
        )
        .await
        .unwrap();
        store_join(
            &pool,
            GRACE,
            SQUIRREL,
            &at("2027-01-01T00:00:00.25Z"),
            JoinSource::Explicit,
        )
        .await
        .unwrap();

        let squirrel = vec![SQUIRREL.to_string()];

        check!(
            counts_by_projects(&pool, &squirrel, Some(&cutover))
                .await
                .unwrap()
                == vec![(
                    SQUIRREL.to_string(),
                    JoinCount {
                        total: 2,
                        since_primary: Some(1),
                    }
                )]
        );
        check!(
            counts_by_projects(&pool, &squirrel, None).await.unwrap()
                == vec![(
                    SQUIRREL.to_string(),
                    JoinCount {
                        total: 2,
                        since_primary: None,
                    }
                )]
        );

        let mut by_user =
            counts_by_users(&pool, &[ADA.to_string(), GRACE.to_string()], Some(&cutover))
                .await
                .unwrap();
        by_user.sort_by(|a, b| a.0.cmp(&b.0));

        check!(
            by_user
                == vec![
                    (
                        ADA.to_string(),
                        JoinCount {
                            total: 1,
                            since_primary: Some(0),
                        }
                    ),
                    (
                        GRACE.to_string(),
                        JoinCount {
                            total: 1,
                            since_primary: Some(1),
                        }
                    ),
                ]
        );
    }
}
