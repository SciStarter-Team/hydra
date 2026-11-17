//! The feeds the log service builds its daily project stats snapshot from,
//! for the parts of a project's stats this service holds: who has joined
//! it, when its current saves were made, and what is known about the
//! accounts that view it. See `common::message::project_stats`.

use anyhow::Error;
use async_nats::service::Request;
use common::{
    message::project_stats::{Gender, JoinPair, PageRequest, Person, Save},
    service::decode_structured_message,
    shared::db,
    timestring::TimeString,
};
use sqlx::SqlitePool;
use ulid::Ulid;

/// `user.stats.primary_since`: when hydra became the primary site, from
/// which on the views, visits and shares it logs are real traffic rather
/// than testing. `None` until it has.
pub async fn primary_since(_req: &Request) -> Result<Option<TimeString>, Error> {
    crate::primary_since::read(db()).await
}

/// The most rows any page holds, whatever the caller asks for, so that a
/// page fits in one NATS message.
const MOST: u32 = 5_000;

/// `user.stats.joins`: every join, by account and then project.
pub async fn joins(req: &Request) -> Result<Vec<JoinPair>, Error> {
    let page: PageRequest<(Ulid, Ulid)> = decode_structured_message(req.message.clone())?;
    joins_page(db(), page).await
}

async fn joins_page(
    pool: &SqlitePool,
    page: PageRequest<(Ulid, Ulid)>,
) -> Result<Vec<JoinPair>, Error> {
    let (user, project) = page
        .after
        .map(|(user, project)| (user.to_string(), project.to_string()))
        .unwrap_or_default();
    let limit = page.limit.min(MOST);

    let rows = sqlx::query!(
        r#"SELECT "user_id", "project_id", "when"
           FROM "project_joins"
           WHERE ("user_id", "project_id") > (?, ?)
           ORDER BY "user_id", "project_id"
           LIMIT ?"#,
        user,
        project,
        limit,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .filter_map(|row| {
            Some(JoinPair {
                project_id: row.project_id.parse().ok()?,
                user_id: row.user_id.parse().ok()?,
                when: row.when,
            })
        })
        .collect())
}

/// `user.stats.saves`: every save, by account and then project.
pub async fn saves(req: &Request) -> Result<Vec<Save>, Error> {
    let page: PageRequest<(Ulid, Ulid)> = decode_structured_message(req.message.clone())?;
    saves_page(db(), page).await
}

async fn saves_page(
    pool: &SqlitePool,
    page: PageRequest<(Ulid, Ulid)>,
) -> Result<Vec<Save>, Error> {
    let (user, project) = page
        .after
        .map(|(user, project)| (user.to_string(), project.to_string()))
        .unwrap_or_default();
    let limit = page.limit.min(MOST);

    let rows = sqlx::query!(
        r#"SELECT "user_id", "project_id", date("created") AS "day: String"
           FROM "bookmarks"
           WHERE ("user_id", "project_id") > (?, ?)
           ORDER BY "user_id", "project_id"
           LIMIT ?"#,
        user,
        project,
        limit,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .filter_map(|row| {
            Some(Save {
                user_id: row.user_id.parse().ok()?,
                project_id: row.project_id.parse().ok()?,
                day: row.day?,
            })
        })
        .collect())
}

/// `user.stats.people`: the accounts a project's audience can say something
/// about, by account.
pub async fn people(req: &Request) -> Result<Vec<Person>, Error> {
    let page: PageRequest<Ulid> = decode_structured_message(req.message.clone())?;
    people_page(db(), page).await
}

async fn people_page(pool: &SqlitePool, page: PageRequest<Ulid>) -> Result<Vec<Person>, Error> {
    let after = page.after.map(|user| user.to_string()).unwrap_or_default();
    let limit = page.limit.min(MOST);

    let rows = sqlx::query!(
        r#"SELECT "user_id", "gender", "birthday",
                  ("is_staff" OR "is_superuser") AS "is_staff!: bool"
           FROM "users"
           WHERE "user_id" > ?
             AND (COALESCE("gender", '') <> ''
                  OR COALESCE("birthday", '') <> ''
                  OR "is_staff" OR "is_superuser")
           ORDER BY "user_id"
           LIMIT ?"#,
        after,
        limit,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .filter_map(|row| {
            Some(Person {
                user_id: row.user_id.parse().ok()?,
                gender: Gender::from_profile(row.gender.as_deref()),
                birthday: row.birthday.filter(|birthday| !birthday.trim().is_empty()),
                is_staff: row.is_staff,
            })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert2::check;

    const ADA: &str = "01HQ3Z8Y6V2N4J5K7M8P9R0S1A";
    const BEA: &str = "01HQ3Z8Y6V2N4J5K7M8P9R0S1B";
    const CY: &str = "01HQ3Z8Y6V2N4J5K7M8P9R0S1C";
    const PROJECT: &str = "01HQ3Z8Y6V2N4J5K7M8P9R0P01";
    const OTHER: &str = "01HQ3Z8Y6V2N4J5K7M8P9R0P02";

    async fn pool() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();

        for statement in [
            r#"CREATE TABLE "users" ("user_id" TEXT PRIMARY KEY, "gender" TEXT,
                   "birthday" TEXT, "is_staff" INTEGER NOT NULL DEFAULT 0,
                   "is_superuser" INTEGER NOT NULL DEFAULT 0)"#,
            r#"CREATE TABLE "project_joins" ("user_id" TEXT NOT NULL,
                   "project_id" TEXT NOT NULL, "when" TEXT NOT NULL,
                   PRIMARY KEY ("user_id", "project_id"))"#,
            r#"CREATE TABLE "bookmarks" ("user_id" TEXT NOT NULL,
                   "project_id" TEXT NOT NULL, "created" TEXT NOT NULL,
                   "starred" BOOLEAN NOT NULL DEFAULT 1)"#,
        ] {
            sqlx::query(statement).execute(&pool).await.unwrap();
        }

        pool
    }

    #[tokio::test]
    async fn joins_page_through_in_order() {
        let pool = pool().await;
        for (user, project) in [(ADA, PROJECT), (BEA, PROJECT), (CY, OTHER)] {
            sqlx::query(r#"INSERT INTO "project_joins" VALUES (?, ?, '2026-01-01T00:00:00Z')"#)
                .bind(user)
                .bind(project)
                .execute(&pool)
                .await
                .unwrap();
        }

        let first = joins_page(&pool, PageRequest::first(2)).await.unwrap();
        check!(first.len() == 2);

        let last = first.last().unwrap();
        let rest = joins_page(
            &pool,
            PageRequest {
                after: Some((last.user_id, last.project_id)),
                limit: 2,
            },
        )
        .await
        .unwrap();

        check!(rest.len() == 1);
        check!(rest[0].user_id.to_string() == CY);
    }

    #[tokio::test]
    async fn every_save_comes_with_its_day() {
        let pool = pool().await;
        for (user, created, starred) in [
            (ADA, "2026-11-01T10:00:00Z", true),
            (BEA, "2026-11-01 23:00:00", true),
            (CY, "2026-11-02T01:00:00Z", false),
        ] {
            sqlx::query(r#"INSERT INTO "bookmarks" VALUES (?, ?, ?, ?)"#)
                .bind(user)
                .bind(PROJECT)
                .bind(created)
                .bind(starred)
                .execute(&pool)
                .await
                .unwrap();
        }

        let saves = saves_page(&pool, PageRequest::first(10)).await.unwrap();

        check!(saves.len() == 3);
        check!(saves[0].day == "2026-11-01");
        check!(saves[1].day == "2026-11-01");
        check!(saves[2].day == "2026-11-02");
    }

    #[tokio::test]
    async fn people_are_those_with_something_to_say() {
        let pool = pool().await;
        for (user, gender, birthday, staff) in [
            (ADA, "Female", "", false),
            (BEA, "", "", false),
            (CY, "", "", true),
        ] {
            sqlx::query(r#"INSERT INTO "users" VALUES (?, ?, ?, ?, 0)"#)
                .bind(user)
                .bind(gender)
                .bind(birthday)
                .bind(staff)
                .execute(&pool)
                .await
                .unwrap();
        }

        let people = people_page(&pool, PageRequest::first(10)).await.unwrap();

        check!(people.len() == 2);
        check!(people[0].gender == Gender::Female);
        check!(people[0].birthday == None);
        check!(people[1].is_staff);
    }
}
