//! Who a People Finder audience reaches (`user.people_finder.*`). The whole
//! tool is described in `common::message::people_finder`.
//!
//! The groups the staff filters narrow to are each defined once, elsewhere,
//! and only referred to here:
//!
//! - Library Network accounts are [`crate::library::network_accounts`];
//! - Champions have `ambassador_progress.tutorial_completed_at`, which the
//!   Champion tutorial stamps and every later stage of the funnel keeps (see
//!   `apply_ambassador_state` in main.rs);
//! - Ambassadors are the `ambassador_directory_members` view.

use anyhow::Error;
use async_nats::service::Request;
use common::{
    message::people_finder::{
        PeopleFinderArea, PeopleFinderAudience, PeopleFinderCountRequest, PeopleFinderCounts,
        PeopleFinderFilter, PeopleFinderReach, PeopleFinderRecipientsPage,
        PeopleFinderRecipientsRequest,
    },
    service::decode_structured_message,
    shared::db,
};
use sqlx::SqlitePool;
use ulid::Ulid;

/// The most recipients one page may carry.
const MAX_PAGE: i64 = 5000;

/// One person in an audience, and which side of the operator divide they are
/// on.
#[derive(Debug, PartialEq, Eq)]
struct Member {
    user_id: String,
    operator: bool,
}

/// A longitude/latitude box that contains the whole of `area`, for the
/// spatial index to narrow on before the exact distance is measured.
///
/// A degree of latitude is never shorter than 110,574 m, so dividing by that
/// can only make the box taller than it needs to be. A degree of longitude
/// shrinks with the cosine of the latitude, and the box is widened by the
/// cosine at whichever edge is nearer a pole. Where the box would cross a
/// pole or the antimeridian, it simply takes in every longitude.
fn bounding_box(area: &PeopleFinderArea) -> (f64, f64, f64, f64) {
    const MIN_METERS_PER_DEGREE: f64 = 110_574.0;

    let lat_span = area.radius_meters() / MIN_METERS_PER_DEGREE;
    let min_lat = area.latitude - lat_span;
    let max_lat = area.latitude + lat_span;

    let poleward = min_lat.abs().max(max_lat.abs());
    let lon_span = if poleward >= 90.0 {
        None
    } else {
        Some(lat_span / poleward.to_radians().cos())
    };

    let (min_lon, max_lon) = match lon_span {
        Some(span) if area.longitude - span >= -180.0 && area.longitude + span <= 180.0 => {
            (area.longitude - span, area.longitude + span)
        }
        _ => (-180.0, 180.0),
    };

    (min_lon, min_lat.max(-90.0), max_lon, max_lat.min(90.0))
}

/// The people `audience` holds whom `reach` selects, in `user_id` order,
/// starting after `after`, at most `limit` of them (`-1` for no limit).
/// `operators` are the people who operate a published project or event.
///
/// Counting and delivery both read the audience through here, so the number
/// staff are shown is the number of people the message reaches.
async fn members(
    pool: &SqlitePool,
    audience: &PeopleFinderAudience,
    operators: &[Ulid],
    reach: PeopleFinderReach,
    after: Option<Ulid>,
    limit: i64,
) -> Result<Vec<Member>, Error> {
    let area = &audience.area;
    let (min_lon, min_lat, max_lon, max_lat) = bounding_box(area);
    let radius = area.radius_meters();

    let operators = serde_json::to_string(operators)?;

    let library_network = audience
        .filters
        .contains(&PeopleFinderFilter::LibraryNetwork);
    let library_accounts = if library_network {
        crate::library::network_accounts(pool).await?
    } else {
        Default::default()
    };
    let library_accounts = serde_json::to_string(&library_accounts)?;

    let champions = audience.filters.contains(&PeopleFinderFilter::Champions);
    let ambassadors = audience.filters.contains(&PeopleFinderFilter::Ambassadors);

    let want_participants = reach.includes_participants();
    let want_operators = reach.includes_operators();

    // ULIDs sort the same as their strings, so an empty string comes before
    // every one of them.
    let after = after.map(|id| id.to_string()).unwrap_or_default();

    let rows = sqlx::query!(
        r#"SELECT "user_id" AS "user_id!", "operator" AS "operator!: bool"
             FROM (
               SELECT u."user_id",
                      EXISTS (SELECT 1 FROM json_each(?) o WHERE o."value" = u."user_id") AS "operator"
                 FROM "users" u
                WHERE u.rowid IN (
                        SELECT "pkid" FROM "idx_users_point"
                         WHERE "xmin" <= ? AND "xmax" >= ? AND "ymin" <= ? AND "ymax" >= ?
                      )
                  AND ST_Distance(u."point", MakePoint(?, ?, 4326), 1) < ?
                  AND (NOT ? OR EXISTS (SELECT 1 FROM json_each(?) l WHERE l."value" = u."user_id"))
                  AND (NOT ? OR EXISTS (SELECT 1 FROM "ambassador_progress" p
                                         WHERE p."user_id" = u."user_id"
                                           AND p."tutorial_completed_at" IS NOT NULL))
                  AND (NOT ? OR EXISTS (SELECT 1 FROM "ambassador_directory_members" m
                                         WHERE m."user_id" = u."user_id"))
                  AND u."user_id" > ?
             )
            WHERE (? OR NOT "operator") AND (? OR "operator")
            ORDER BY "user_id"
            LIMIT ?"#,
        operators,
        max_lon,
        min_lon,
        max_lat,
        min_lat,
        area.longitude,
        area.latitude,
        radius,
        library_network,
        library_accounts,
        champions,
        ambassadors,
        after,
        want_operators,
        want_participants,
        limit,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| Member {
            user_id: row.user_id,
            operator: row.operator,
        })
        .collect())
}

async fn counts(
    pool: &SqlitePool,
    audience: &PeopleFinderAudience,
    operators: &[Ulid],
) -> Result<PeopleFinderCounts, Error> {
    let everyone = members(
        pool,
        audience,
        operators,
        PeopleFinderReach::Everyone,
        None,
        -1,
    )
    .await?;

    let operators = everyone.iter().filter(|m| m.operator).count() as i64;

    Ok(PeopleFinderCounts {
        participants: everyone.len() as i64 - operators,
        operators,
    })
}

/// `user.people_finder.count`
pub async fn count(req: &Request) -> Result<PeopleFinderCounts, Error> {
    let request: PeopleFinderCountRequest = decode_structured_message(req.message.clone())?;

    counts(db(), &request.audience, &request.operators).await
}

/// `user.people_finder.recipients`
pub async fn recipients(req: &Request) -> Result<PeopleFinderRecipientsPage, Error> {
    let request: PeopleFinderRecipientsRequest = decode_structured_message(req.message.clone())?;

    let page = members(
        db(),
        &request.audience,
        &request.operators,
        request.reach,
        request.after,
        request.limit.clamp(1, MAX_PAGE),
    )
    .await?;

    Ok(PeopleFinderRecipientsPage {
        user_ids: page
            .into_iter()
            .filter_map(|m| m.user_id.parse().ok())
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::shared::hash_identifier_lower;
    use std::collections::BTreeSet;

    const HERE: (f64, f64) = (-98.0, 39.0);
    const FAR_AWAY: (f64, f64) = (-70.0, 42.0);

    /// Just enough of the schema for an audience: located accounts with their
    /// addresses, the three groups' sources, and the real directory view.
    async fn fixture() -> SqlitePool {
        // SAFETY: mod_spatialite is the same trusted extension every service
        // database loads.
        let options = unsafe {
            sqlx::sqlite::SqliteConnectOptions::new()
                .filename(":memory:")
                .extension("mod_spatialite")
        };
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .expect("pool");

        sqlx::raw_sql(
            r#"SELECT InitSpatialMetaData(1);
               CREATE TABLE "users" (
                 "user_id" text NOT NULL,
                 "primary_email" text NOT NULL,
                 "ambassador" text
               );
               SELECT AddGeometryColumn('users', 'point', 4326, 'POINT', 'XY');
               SELECT CreateSpatialIndex('users', 'point');
               CREATE TABLE "emails" (
                 "user_id" text NOT NULL,
                 "email" text NOT NULL,
                 "hash_lower" text NOT NULL
               );
               CREATE TABLE "library_network_signups" (
                 "user_id" text,
                 "email" text NOT NULL DEFAULT ''
               );
               CREATE TABLE "ambassador_progress" (
                 "user_id" text PRIMARY KEY,
                 "tutorial_completed_at" text
               );
               CREATE TABLE "ambassador_applications" (
                 "user_id" text PRIMARY KEY,
                 "app_status" integer NOT NULL DEFAULT 0,
                 "city" text, "state" text, "bio" text, "youtube_video_url" text,
                 "position" real, "hidden" integer NOT NULL DEFAULT 0
               );
               -- The migration replaces these.
               CREATE VIEW "ambassador_directory_listing" AS SELECT 1;
               CREATE VIEW "ambassador_directory_members" AS SELECT 1;"#,
        )
        .execute(&pool)
        .await
        .expect("schema");

        sqlx::raw_sql(include_str!(
            "../migrations/20260928225648_ambassadors_have_applications.up.sql"
        ))
        .execute(&pool)
        .await
        .expect("directory view");

        pool
    }

    async fn person(pool: &SqlitePool, at: (f64, f64)) -> Ulid {
        let id = Ulid::new();
        let email = format!("{}@example.invalid", id.to_string().to_lowercase());

        sqlx::query(
            r#"INSERT INTO "users" ("user_id", "primary_email", "point")
               VALUES (?, ?, MakePoint(?, ?, 4326))"#,
        )
        .bind(id.to_string())
        .bind(&email)
        .bind(at.0)
        .bind(at.1)
        .execute(pool)
        .await
        .expect("user");

        sqlx::query(r#"INSERT INTO "emails" ("user_id", "email", "hash_lower") VALUES (?, ?, ?)"#)
            .bind(id.to_string())
            .bind(&email)
            .bind(hash_identifier_lower(&email))
            .execute(pool)
            .await
            .expect("email");

        id
    }

    async fn exec(pool: &SqlitePool, sql: &'static str, id: Ulid) {
        sqlx::query(sql)
            .bind(id.to_string())
            .execute(pool)
            .await
            .expect(sql);
    }

    async fn champion(pool: &SqlitePool, id: Ulid) {
        exec(
            pool,
            r#"INSERT INTO "ambassador_progress" VALUES (?, '2026-01-01T00:00:00Z')"#,
            id,
        )
        .await;
    }

    async fn ambassador(pool: &SqlitePool, id: Ulid, app_status: i64) {
        champion(pool, id).await;
        exec(
            pool,
            r#"UPDATE "users" SET "ambassador" = '2026-01-01T00:00:00Z' WHERE "user_id" = ?"#,
            id,
        )
        .await;
        sqlx::query(
            r#"INSERT INTO "ambassador_applications" ("user_id", "app_status") VALUES (?, ?)"#,
        )
        .bind(id.to_string())
        .bind(app_status)
        .execute(pool)
        .await
        .expect("application");
    }

    fn audience(filters: &[PeopleFinderFilter]) -> PeopleFinderAudience {
        PeopleFinderAudience {
            area: PeopleFinderArea::new(HERE.1, HERE.0, 50.0).unwrap(),
            filters: filters.iter().copied().collect(),
        }
    }

    async fn reached(
        pool: &SqlitePool,
        filters: &[PeopleFinderFilter],
        operators: &[Ulid],
        reach: PeopleFinderReach,
    ) -> BTreeSet<String> {
        members(pool, &audience(filters), operators, reach, None, -1)
            .await
            .expect("members")
            .into_iter()
            .map(|m| m.user_id)
            .collect()
    }

    fn ids(people: &[Ulid]) -> BTreeSet<String> {
        people.iter().map(Ulid::to_string).collect()
    }

    #[tokio::test]
    async fn the_filters_narrow_to_their_groups_and_combine_as_and() {
        let pool = fixture().await;

        let nobody = person(&pool, HERE).await;

        let linked = person(&pool, HERE).await;
        exec(
            &pool,
            r#"INSERT INTO "library_network_signups" ("user_id") VALUES (?)"#,
            linked,
        )
        .await;

        // Signed out, and in other capitals than the account's own address.
        let emailed = person(&pool, HERE).await;
        sqlx::query(r#"INSERT INTO "library_network_signups" ("email") VALUES (?)"#)
            .bind(format!(
                "{}@EXAMPLE.invalid",
                emailed.to_string().to_uppercase()
            ))
            .execute(&pool)
            .await
            .unwrap();

        let trained = person(&pool, HERE).await;
        champion(&pool, trained).await;

        let current = person(&pool, HERE).await;
        ambassador(&pool, current, 6).await;

        // Demoted to Champion: still dated, but no longer an Ambassador.
        let demoted = person(&pool, HERE).await;
        ambassador(&pool, demoted, 5).await;

        let librarian_ambassador = person(&pool, HERE).await;
        ambassador(&pool, librarian_ambassador, 6).await;
        exec(
            &pool,
            r#"INSERT INTO "library_network_signups" ("user_id") VALUES (?)"#,
            librarian_ambassador,
        )
        .await;

        let distant = person(&pool, FAR_AWAY).await;
        ambassador(&pool, distant, 6).await;

        use PeopleFinderFilter::*;
        let everyone = PeopleFinderReach::Everyone;

        assert_eq!(
            reached(&pool, &[], &[], everyone).await,
            ids(&[
                nobody,
                linked,
                emailed,
                trained,
                current,
                demoted,
                librarian_ambassador
            ])
        );
        assert_eq!(
            reached(&pool, &[LibraryNetwork], &[], everyone).await,
            ids(&[linked, emailed, librarian_ambassador])
        );
        assert_eq!(
            reached(&pool, &[Champions], &[], everyone).await,
            ids(&[trained, current, demoted, librarian_ambassador])
        );
        assert_eq!(
            reached(&pool, &[Ambassadors], &[], everyone).await,
            ids(&[current, librarian_ambassador])
        );
        assert_eq!(
            reached(&pool, &[LibraryNetwork, Ambassadors], &[], everyone).await,
            ids(&[librarian_ambassador])
        );
    }

    #[tokio::test]
    async fn reach_and_counts_split_on_the_operators() {
        let pool = fixture().await;

        let operator = person(&pool, HERE).await;
        let participant = person(&pool, HERE).await;
        let elsewhere = person(&pool, FAR_AWAY).await;
        let operators = [operator, elsewhere];

        assert_eq!(
            reached(&pool, &[], &operators, PeopleFinderReach::Operators).await,
            ids(&[operator])
        );
        assert_eq!(
            reached(&pool, &[], &operators, PeopleFinderReach::Participants).await,
            ids(&[participant])
        );
        assert_eq!(
            counts(&pool, &audience(&[]), &operators).await.unwrap(),
            PeopleFinderCounts {
                participants: 1,
                operators: 1
            }
        );
    }

    #[tokio::test]
    async fn pages_follow_on_from_one_another() {
        let pool = fixture().await;

        let mut everyone = Vec::new();
        for _ in 0..5 {
            everyone.push(person(&pool, HERE).await);
        }
        everyone.sort();

        let mut seen = Vec::new();
        let mut after = None;
        loop {
            let page = members(
                &pool,
                &audience(&[]),
                &[],
                PeopleFinderReach::Everyone,
                after,
                2,
            )
            .await
            .unwrap();
            let last = page.len() < 2;
            for m in page {
                after = Some(m.user_id.parse().unwrap());
                seen.push(after.unwrap());
            }
            if last {
                break;
            }
        }

        assert_eq!(seen, everyone);
    }

    #[test]
    fn the_box_holds_the_circle() {
        let area = PeopleFinderArea::new(39.0, -98.0, 100.0).unwrap();
        let (min_lon, min_lat, max_lon, max_lat) = bounding_box(&area);

        // 100 miles is about 1.45 degrees of latitude, and more of longitude.
        assert!(min_lat < 39.0 - 1.44 && max_lat > 39.0 + 1.44);
        assert!(max_lon - (-98.0) > max_lat - 39.0);
        assert!(min_lon > -180.0 && max_lon < 180.0);
    }

    #[test]
    fn a_box_over_the_antimeridian_takes_every_longitude() {
        let area = PeopleFinderArea::new(60.0, 179.5, 100.0).unwrap();
        let (min_lon, _, max_lon, _) = bounding_box(&area);

        assert_eq!((min_lon, max_lon), (-180.0, 180.0));
    }
}
