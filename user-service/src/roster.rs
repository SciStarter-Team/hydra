//! A project's roster, for its Participant Manager: everyone who has joined
//! it, and everyone who has saved it without joining, with the stars its
//! managers put on them. See `common::message::participants`.
//!
//! Protected accounts are left off, as orrery left them off: the roster
//! hands a project's managers a name and a way to write to it, and for the
//! people the flag covers, that they took part is itself what identifies
//! them.

use anyhow::{Error, bail};
use async_nats::service::Request;
use common::{
    message::{
        participants::{
            Participant, ParticipantKind, RosterMembersRequest, RosterRequest, RosterResponse,
            StarRequest,
        },
        user::display_name,
    },
    service::decode_structured_message,
    shared::db,
};
use sqlx::SqlitePool;
use ulid::Ulid;

/// The most people a page of the roster holds.
const MOST: u32 = 200;

/// The most people one message from the roster may go to.
const MOST_RECIPIENTS: usize = 500;

/// `user.roster.list`
pub async fn list(req: &Request) -> Result<RosterResponse, Error> {
    let request: RosterRequest = decode_structured_message(req.message.clone())?;
    list_page(db(), request).await
}

async fn list_page(pool: &SqlitePool, request: RosterRequest) -> Result<RosterResponse, Error> {
    let project_id = request.project_id.to_string();
    let kind = request.kind.map(ParticipantKind::key);
    let search = request.search.trim();
    let pattern = format!("%{}%", like_escaped(search));
    let limit = request.limit.min(MOST);

    let rows = sqlx::query!(
        r#"WITH "roster" AS (
               SELECT j."user_id", 'joined' AS "kind", j."when" AS "since",
                      j."starred", j."consented"
               FROM "project_joins" j
               WHERE j."project_id" = ?1
               UNION ALL
               SELECT b."user_id", 'saved', b."created", b."starred", NULL
               FROM "bookmarks" b
               WHERE b."project_id" = ?1
                 AND NOT EXISTS (
                     SELECT 1 FROM "project_joins" j
                     WHERE j."project_id" = b."project_id" AND j."user_id" = b."user_id")
           )
           SELECT r."user_id" AS "user_id!: String", r."kind" AS "kind!: String",
                  r."since" AS "since!: String", r."starred" AS "starred!: bool",
                  r."consented" AS "consented: String",
                  u."first_name", u."last_name", u."handle",
                  COUNT(*) OVER () AS "total!: i64"
           FROM "roster" r
           JOIN "users" u ON u."user_id" = r."user_id"
           WHERE NOT u."protected"
             AND (?2 IS NULL OR r."kind" = ?2)
             AND (?3 IS NULL OR r."starred" = ?3)
             AND (?4 = '' OR (u."first_name" || ' ' || u."last_name" || ' ' || u."handle")
                             LIKE ?5 ESCAPE '\')
           ORDER BY unixepoch(r."since", 'subsec') DESC, r."user_id"
           LIMIT ?6 OFFSET ?7"#,
        project_id,
        kind,
        request.starred,
        search,
        pattern,
        limit,
        request.offset,
    )
    .fetch_all(pool)
    .await?;

    let total = rows.first().map_or(0, |row| row.total as u32);
    let participants = rows
        .into_iter()
        .filter_map(|row| {
            Some(Participant {
                user_id: row.user_id.parse().ok()?,
                kind: ParticipantKind::from_key(&row.kind)?,
                name: display_name(&row.first_name, &row.last_name, &row.handle),
                since: row.since,
                starred: row.starred,
                consented: row.consented,
            })
        })
        .collect();

    Ok(RosterResponse {
        total,
        participants,
    })
}

/// `text` with LIKE's wildcards and escape taken literally.
fn like_escaped(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(c, '%' | '_' | '\\') {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    escaped
}

/// `user.roster.star`
pub async fn star(req: &Request) -> Result<bool, Error> {
    let request: StarRequest = decode_structured_message(req.message.clone())?;
    set_star(db(), request).await
}

async fn set_star(pool: &SqlitePool, request: StarRequest) -> Result<bool, Error> {
    let project_id = request.project_id.to_string();
    let user_id = request.user_id.to_string();

    let result = match request.kind {
        ParticipantKind::Joined => {
            sqlx::query!(
                r#"UPDATE "project_joins" SET "starred" = ?
                   WHERE "project_id" = ? AND "user_id" = ?"#,
                request.starred,
                project_id,
                user_id,
            )
            .execute(pool)
            .await?
        }
        ParticipantKind::Saved => {
            sqlx::query!(
                r#"UPDATE "bookmarks" SET "starred" = ?
                   WHERE "project_id" = ? AND "user_id" = ?"#,
                request.starred,
                project_id,
                user_id,
            )
            .execute(pool)
            .await?
        }
    };

    Ok(result.rows_affected() > 0)
}

/// `user.roster.members`
pub async fn members(req: &Request) -> Result<Vec<Ulid>, Error> {
    let request: RosterMembersRequest = decode_structured_message(req.message.clone())?;
    roster_members(db(), request).await
}

async fn roster_members(
    pool: &SqlitePool,
    request: RosterMembersRequest,
) -> Result<Vec<Ulid>, Error> {
    if request.user_ids.len() > MOST_RECIPIENTS {
        bail!("one message may go to at most {MOST_RECIPIENTS} people");
    }

    let project_id = request.project_id.to_string();
    let mut members = Vec::new();

    for user_id in request.user_ids {
        let user = user_id.to_string();
        let on_roster = sqlx::query_scalar!(
            r#"SELECT EXISTS (
                   SELECT 1 FROM "users" u
                   WHERE u."user_id" = ?2 AND NOT u."protected"
                     AND (EXISTS (SELECT 1 FROM "project_joins" j
                                  WHERE j."project_id" = ?1 AND j."user_id" = u."user_id")
                          OR EXISTS (SELECT 1 FROM "bookmarks" b
                                     WHERE b."project_id" = ?1 AND b."user_id" = u."user_id"))
               ) AS "on_roster!: bool""#,
            project_id,
            user,
        )
        .fetch_one(pool)
        .await?;

        if on_roster {
            members.push(user_id);
        }
    }

    Ok(members)
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert2::check;

    const ADA: &str = "01HQ3Z8Y6V2N4J5K7M8P9R0S1A";
    const BEA: &str = "01HQ3Z8Y6V2N4J5K7M8P9R0S1B";
    const CY: &str = "01HQ3Z8Y6V2N4J5K7M8P9R0S1C";
    const DEE: &str = "01HQ3Z8Y6V2N4J5K7M8P9R0S1D";
    const PROJECT: &str = "01HQ3Z8Y6V2N4J5K7M8P9R0P01";

    /// Ada joined and saved; Bea saved, unstarred; Cy joined but is
    /// protected; Dee joined most recently.
    async fn pool() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();

        for statement in [
            r#"CREATE TABLE "users" ("user_id" TEXT PRIMARY KEY,
                   "first_name" TEXT NOT NULL DEFAULT '', "last_name" TEXT NOT NULL DEFAULT '',
                   "handle" TEXT NOT NULL DEFAULT '', "protected" INTEGER NOT NULL DEFAULT 0)"#,
            r#"CREATE TABLE "project_joins" ("user_id" TEXT NOT NULL,
                   "project_id" TEXT NOT NULL, "when" TEXT NOT NULL,
                   "starred" BOOLEAN NOT NULL DEFAULT 1, "consented" TEXT,
                   PRIMARY KEY ("user_id", "project_id"))"#,
            r#"CREATE TABLE "bookmarks" ("user_id" TEXT NOT NULL,
                   "project_id" TEXT NOT NULL, "created" TEXT NOT NULL,
                   "starred" BOOLEAN NOT NULL DEFAULT 1,
                   PRIMARY KEY ("user_id", "project_id"))"#,
        ] {
            sqlx::query(statement).execute(&pool).await.unwrap();
        }

        for (user, first, last, handle, protected) in [
            (ADA, "Ada", "Lovelace", "ada", false),
            (BEA, "", "", "bea_100%", false),
            (CY, "Cy", "Young", "cy", true),
            (DEE, "Dee", "", "dee", false),
        ] {
            sqlx::query(r#"INSERT INTO "users" VALUES (?, ?, ?, ?, ?)"#)
                .bind(user)
                .bind(first)
                .bind(last)
                .bind(handle)
                .bind(protected)
                .execute(&pool)
                .await
                .unwrap();
        }

        for (user, when) in [
            (ADA, "2026-01-01T00:00:00Z"),
            (CY, "2026-02-01T00:00:00Z"),
            (DEE, "2026-03-01T00:00:00Z"),
        ] {
            sqlx::query(r#"INSERT INTO "project_joins" VALUES (?, ?, ?, 1, NULL)"#)
                .bind(user)
                .bind(PROJECT)
                .bind(when)
                .execute(&pool)
                .await
                .unwrap();
        }

        for (user, starred) in [(ADA, true), (BEA, false)] {
            sqlx::query(r#"INSERT INTO "bookmarks" VALUES (?, ?, '2026-01-15T00:00:00Z', ?)"#)
                .bind(user)
                .bind(PROJECT)
                .bind(starred)
                .execute(&pool)
                .await
                .unwrap();
        }

        pool
    }

    fn everyone() -> RosterRequest {
        RosterRequest {
            project_id: PROJECT.parse().unwrap(),
            kind: None,
            starred: None,
            search: String::new(),
            offset: 0,
            limit: 10,
        }
    }

    #[tokio::test]
    async fn the_roster_lists_each_person_once_newest_first() {
        let roster = list_page(&pool().await, everyone()).await.unwrap();
        let names: Vec<&str> = roster
            .participants
            .iter()
            .map(|p| p.name.as_str())
            .collect();

        check!(roster.total == 3);
        check!(names == ["Dee", "bea_100%", "Ada Lovelace"]);
        check!(roster.participants[2].kind == ParticipantKind::Joined);
    }

    #[tokio::test]
    async fn the_roster_filters_by_kind_star_and_name() {
        let pool = pool().await;

        let saved = RosterRequest {
            kind: Some(ParticipantKind::Saved),
            ..everyone()
        };
        check!(list_page(&pool, saved).await.unwrap().total == 1);

        let unstarred = RosterRequest {
            starred: Some(false),
            ..everyone()
        };
        let unstarred = list_page(&pool, unstarred).await.unwrap();
        check!(unstarred.participants[0].name == "bea_100%");

        let named = RosterRequest {
            search: "100%".to_string(),
            ..everyone()
        };
        check!(list_page(&pool, named).await.unwrap().total == 1);

        let wildcard = RosterRequest {
            search: "%".to_string(),
            ..everyone()
        };
        check!(list_page(&pool, wildcard).await.unwrap().total == 1);
    }

    #[tokio::test]
    async fn a_star_lands_on_the_row_it_names() {
        let pool = pool().await;
        let star = |user: &str, kind| StarRequest {
            project_id: PROJECT.parse().unwrap(),
            user_id: user.parse().unwrap(),
            kind,
            starred: false,
        };

        check!(
            set_star(&pool, star(DEE, ParticipantKind::Joined))
                .await
                .unwrap()
        );
        check!(
            !set_star(&pool, star(DEE, ParticipantKind::Saved))
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn only_the_listed_may_be_written_to() {
        let request = RosterMembersRequest {
            project_id: PROJECT.parse().unwrap(),
            user_ids: [ADA, BEA, CY].map(|id| id.parse().unwrap()).to_vec(),
        };

        let members = roster_members(&pool().await, request).await.unwrap();

        check!(members.iter().map(Ulid::to_string).collect::<Vec<_>>() == [ADA, BEA]);
    }
}
