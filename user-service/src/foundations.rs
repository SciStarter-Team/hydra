//! Monthly Foundations training completions (`user.foundations.monthly`).
//!
//! The report — the tally, the chart, and the page — is
//! `common::foundations`, shared byte-for-byte with orrery. What is decided
//! here is only hydra's side of it: each completion's month, from the
//! progress records.
//!
//! **Who counts, and when.** The population is the training page's counter
//! (`workflow::tutorial_count`): every account whose Foundations progress has
//! its first step stamped. hydra stamps each step once and never rewrites it,
//! so a completion is dated by that stamp, or by the badge if it came first —
//! a step synced from orrery carries orrery's `state_changed`, which is later
//! than the true completion for anyone who had moved on by then.

use anyhow::Error;
use async_nats::service::Request;
use common::{
    badge::Badge, foundations::report::MonthCount,
    message::dashboard::FoundationsCompletionsResponse, report_kit::month::Month, shared::db,
};

/// One completion's evidence.
struct CompletionRow {
    hour1_completed_at: String,
    first_badge: Option<String>,
    evolveme: bool,
}

/// When a completion happened: the earlier of its first step and its badge.
/// Both are ISO timestamps in UTC, so they compare as strings.
fn completed_at(row: &CompletionRow) -> &str {
    match row.first_badge.as_deref() {
        Some(badge) if badge < row.hour1_completed_at.as_str() => badge,
        _ => &row.hour1_completed_at,
    }
}

pub async fn monthly(_req: &Request) -> Result<FoundationsCompletionsResponse, Error> {
    let badgeclass = Badge::FoundationsOfCitizenScience.badgeclass();

    // The earlier of the two dates is chosen in Rust rather than with a SQL
    // CASE, which sends sqlx's compile-time checking into a very slow rustc.
    let rows = sqlx::query_as!(
        CompletionRow,
        r#"SELECT fp."hour1_completed_at" AS "hour1_completed_at!",
                  (SELECT min(b."when") FROM "user_badges" b
                    WHERE b."user_id" = fp."user_id" AND b."badgeclass" = ?1) AS "first_badge: String",
                  EXISTS (SELECT 1 FROM "evolveme_users" e
                           WHERE e."user_id" = fp."user_id") AS "evolveme!: bool"
             FROM "foundations_progress" fp
            WHERE fp."hour1_completed_at" IS NOT NULL"#,
        badgeclass,
    )
    .fetch_all(db())
    .await?;

    let counts =
        MonthCount::collect(rows.iter().filter_map(|row| {
            Month::parse(completed_at(row)).map(|month| (month, row.evolveme, 1))
        }));

    Ok(FoundationsCompletionsResponse { counts })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(hour1: &str, badge: Option<&str>) -> CompletionRow {
        CompletionRow {
            hour1_completed_at: hour1.to_string(),
            first_badge: badge.map(str::to_string),
            evolveme: false,
        }
    }

    #[test]
    fn a_completion_is_dated_by_its_earliest_evidence() {
        let first_step = "2025-08-14T10:00:00Z";
        let earlier_badge = "2025-07-30T09:00:00Z";
        let later_badge = "2025-09-01T09:00:00Z";

        assert_eq!(completed_at(&row(first_step, None)), first_step);
        assert_eq!(
            completed_at(&row(first_step, Some(earlier_badge))),
            earlier_badge
        );
        assert_eq!(
            completed_at(&row(first_step, Some(later_badge))),
            first_step
        );
    }
}
