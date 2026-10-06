//! Monthly arrivals at each milestone the Moore Foundation grant reports on
//! (`user.moore.buckets`).
//!
//! The ledger the frontend draws is cumulative, but what is counted here is
//! arrivals: how many people first reached each milestone in each month. One
//! pass over each table produces that; running totals are a fold over the
//! result, and a month nobody arrived in simply has no bucket.
//!
//! **Dating a milestone.** orrery has to work around `state_changed` being a
//! Django `auto_now` field, which holds only the *most recent* transition — so
//! it dates people by the earliest immutable evidence it can find (a badge, a
//! certificate, a recorded training event) and falls back to the mutable field.
//! hydra's progress tables stamp each step as it completes and never rewrite
//! those columns, so the immutable evidence is in the progress row itself —
//! for the ambassador funnel too, whose every stage is dated by a milestone
//! rather than by a state machine's last move.

use anyhow::Error;
use async_nats::service::Request;
use std::collections::BTreeMap;

use common::{
    badge::Badge,
    message::dashboard::{MooreBucket, MooreBucketsResponse},
    organizations,
    shared::db,
};

/// The badge awarded for finishing Foundations.
fn foundations_badgeclass() -> &'static str {
    Badge::FoundationsOfCitizenScience.badgeclass()
}

/// The badge awarded to a certified Ambassador.
fn ambassador_badgeclass() -> &'static str {
    Badge::SciStarterAmbassador.badgeclass()
}

/// Every metric's monthly arrivals, in one query per shape.
///
/// The metric tags are the ledger's own, and the frontend maps them back to
/// columns — a tag it does not know is ignored rather than misplaced.
pub async fn buckets(_req: &Request) -> Result<MooreBucketsResponse, Error> {
    let foundations_badge = foundations_badgeclass();
    let ambassador_badge = ambassador_badgeclass();

    struct Row {
        metric: String,
        month: Option<String>,
        count: i64,
    }

    // `substr(t, 1, 7)` is the month: every timestamp column here is an ISO
    // string in UTC, so the first seven characters are `YYYY-MM` without a
    // conversion. A row whose timestamp is empty or malformed yields a month
    // that does not parse, and is dropped below rather than dated to the epoch.
    let rows = sqlx::query_as!(
        Row,
        r#"
        WITH librarians AS (
            -- One row per person, not per form: a librarian who submits the
            -- form twice is one member of the network, dated by the first.
            SELECT min("recorded") AS t
              FROM "library_network_signups"
             GROUP BY lower(trim("email"))
        ),
        foundations AS (
            SELECT min(t) AS t FROM (
                -- Reaching the first step is what "completed the tutorial"
                -- counts as, matching orrery's `state <> 'initial'`.
                SELECT "user_id", "hour1_completed_at" AS t
                  FROM "foundations_progress"
                 WHERE "hour1_completed_at" IS NOT NULL
                UNION ALL
                SELECT "user_id", "when"
                  FROM "user_badges"
                 WHERE "badgeclass" = ?1
            )
            GROUP BY "user_id"
        ),
        champions AS (
            SELECT min(t) AS t FROM (
                SELECT "user_id", "tutorial_completed_at" AS t
                  FROM "ambassador_progress"
                 WHERE "tutorial_completed_at" IS NOT NULL
                UNION ALL
                -- Reaching the end of the route means having walked it, so a
                -- closure dates the training even where no step was stamped.
                SELECT "user_id", COALESCE("rejected_at", "international_at", "screening_failed_at")
                  FROM "ambassador_progress"
                 WHERE "rejected_at" IS NOT NULL
                    OR "international_at" IS NOT NULL
                    OR "screening_failed_at" IS NOT NULL
                UNION ALL
                SELECT "user_id", "created_at"
                  FROM "certificates"
                 WHERE "mode" = 'champion'
                UNION ALL
                -- The application form is only reachable once the training is
                -- done, so a submitted application dates the training too.
                SELECT "user_id", "submitted"
                  FROM "ambassador_applications"
                 WHERE "draft" = 0 AND "submitted" IS NOT NULL
                UNION ALL
                SELECT "user_id", "ambassador"
                  FROM "users"
                 WHERE "ambassador" IS NOT NULL
            )
            GROUP BY "user_id"
        )
        SELECT 'librarians' AS "metric!", substr(t, 1, 7) AS "month", count(*) AS "count!: i64"
          FROM librarians GROUP BY substr(t, 1, 7)
        UNION ALL
        SELECT 'foundations', substr(t, 1, 7), count(*)
          FROM foundations GROUP BY substr(t, 1, 7)
        UNION ALL
        SELECT 'foundations_badge', substr("when", 1, 7), count(*)
          FROM "user_badges" WHERE "badgeclass" = ?1
         GROUP BY substr("when", 1, 7)
        UNION ALL
        SELECT 'champions', substr(t, 1, 7), count(*)
          FROM champions GROUP BY substr(t, 1, 7)
        UNION ALL
        SELECT 'certified', substr("when", 1, 7), count(*)
          FROM "user_badges" WHERE "badgeclass" = ?2
         GROUP BY substr("when", 1, 7)
        UNION ALL
        SELECT 'approved', substr("ambassador", 1, 7), count(*)
          FROM "users" WHERE "ambassador" IS NOT NULL
         GROUP BY substr("ambassador", 1, 7)
        UNION ALL
        SELECT 'verified', substr("date_joined", 1, 7), count(*)
          FROM "users" WHERE "verified" = 1
         GROUP BY substr("date_joined", 1, 7)
        UNION ALL
        SELECT 'accounts', substr("date_joined", 1, 7), count(*)
          FROM "users"
         GROUP BY substr("date_joined", 1, 7)
        "#,
        foundations_badge,
        ambassador_badge,
    )
    .fetch_all(db())
    .await?;

    let buckets = rows
        .into_iter()
        .map(|row| (row.metric, row.month, row.count))
        .chain(library_buckets().await?)
        .filter_map(|(metric, month, count)| {
            // A month has to look like `YYYY-MM` to be placed on the ledger.
            let month = month?;
            let (year, rest) = month.split_once('-')?;

            (year.len() == 4
                && rest.len() == 2
                && year.chars().all(|c| c.is_ascii_digit())
                && rest.chars().all(|c| c.is_ascii_digit()))
            .then_some(MooreBucket {
                metric,
                month,
                count,
            })
        })
        .collect();

    Ok(MooreBucketsResponse { buckets })
}

/// Monthly arrivals of libraries that signed up through the form, as
/// `(metric, month, count)`. The ledger's headline library figure is the
/// program's tracker, which the frontend reads; this is the form's share.
///
/// Sign-ups are one row per person, so this is not the librarian count: the
/// rows are collapsed to organizations by `common::organizations` — the rule
/// orrery uses, and the Library Network map — and each organization is dated
/// by its earliest sign-up. That rule joins rows that agree on a normalized
/// name *or* a normalized address, transitively, which SQL can't express.
async fn library_buckets() -> Result<Vec<(String, Option<String>, i64)>, Error> {
    struct Row {
        recorded: String,
        name: String,
        address: String,
        zip: String,
    }

    // Oldest first, so the first row of each organization is the moment it
    // entered the network.
    let rows = sqlx::query_as!(
        Row,
        r#"SELECT "recorded" AS "recorded!",
                  "organization_name" AS "name!",
                  "organization_street_address" AS "address!",
                  "organization_zip_code" AS "zip!"
             FROM "library_network_signups"
            ORDER BY "recorded" ASC, "orrery_id" ASC, "signup_id" ASC"#
    )
    .fetch_all(db())
    .await?;

    let firsts = organizations::first_of_each(
        &rows
            .iter()
            .map(|row| organizations::Identity {
                name: &row.name,
                address: &row.address,
                zip: &row.zip,
            })
            .collect::<Vec<_>>(),
    );

    let mut months: BTreeMap<String, i64> = BTreeMap::new();

    for index in firsts {
        let month = rows[index].recorded.get(0..7).unwrap_or_default();
        *months.entry(month.to_string()).or_default() += 1;
    }

    Ok(months
        .into_iter()
        .map(|(month, count)| (String::from("form_libraries"), Some(month), count))
        .collect())
}
