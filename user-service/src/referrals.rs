//! Monthly sign-up referral report (`user.referrals.monthly`).
//!
//! The report — classification, tallying, and everything the page shows — is
//! `common::referrals`, shared byte-for-byte with orrery. What is decided here
//! is only hydra's side of it: which accounts count, read from `users`.
//!
//! **Which accounts count.** The same as orrery's: sign-ups that never chose
//! SciStarter themselves are left out. EvolveMe (an external rewards
//! platform), whether tagged or recognizable by a referrer on one of its task
//! pages; and the Canvas and Clever school integrations, by the `origin` they
//! stamp on the accounts they create. orrery also checks its Canvas and Clever
//! roster tables, which hydra does not carry; an account in a roster but
//! created some other way is therefore counted here and not there. That
//! difference is nil from 2025 on and a handful of accounts a year before.

use anyhow::Error;
use async_nats::service::Request;
use common::{
    message::dashboard::{ReferralsRequest, ReferralsResponse},
    referrals::{report, sources::Attribution},
    report_kit::month::{DefaultSpan, Month, Range},
    shared::db,
};

/// One counted account: its join month (`YYYY-MM`) and attribution columns.
struct SignupRow {
    month: String,
    referer: String,
    origin: String,
    utm_source: String,
    utm_medium: String,
}

/// The first instant of a month, in the form `date_joined` is written, so
/// that the range can be bounded by plain string comparison.
fn bound(month: Month) -> String {
    format!("{month}-01")
}

pub async fn monthly(req: &Request) -> Result<ReferralsResponse, Error> {
    let request: ReferralsRequest = flexbuffers::from_slice(&req.message.payload)?;
    // Both ends are given, so the default span never applies.
    let range = Range::resolve(
        Some(request.first),
        Some(request.last),
        request.last,
        DefaultSpan::SinceJanuaryLastYear,
    );

    let start = bound(range.first);
    let end = bound(range.end());

    // `date_joined` is an ISO timestamp in UTC, so its first seven characters
    // are the month, and whole-month bounds compare correctly as strings.
    // SQLite's LIKE is case-insensitive for ASCII, as orrery's ILIKE is.
    let rows = sqlx::query_as!(
        SignupRow,
        r#"SELECT substr(u."date_joined", 1, 7) AS "month!: String",
                  u."referer" AS "referer!",
                  u."origin" AS "origin!",
                  u."utm_source" AS "utm_source!",
                  u."utm_medium" AS "utm_medium!"
             FROM "users" u
            WHERE u."date_joined" >= ?1
              AND u."date_joined" < ?2
              -- EvolveMe, by the one rule every report shares.
              AND NOT EXISTS (
                  SELECT 1 FROM "evolveme_users" e WHERE e."user_id" = u."user_id"
              )
              -- Canvas and Clever, by the origin they stamp.
              AND u."origin" NOT LIKE 'canvaslti%'
              AND u."origin" NOT LIKE '%clever%'"#,
        start,
        end,
    )
    .fetch_all(db())
    .await?;

    let months = report::tally(
        range,
        rows.iter().filter_map(|row| {
            let month = Month::parse(&row.month)?;
            let attribution = Attribution {
                referer: &row.referer,
                origin: &row.origin,
                utm_source: &row.utm_source,
                utm_medium: &row.utm_medium,
            };
            Some((month, attribution))
        }),
    );

    Ok(ReferralsResponse { months })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounds_are_the_first_of_the_month_as_written_in_date_joined() {
        let month = Month::new(2025, 9).unwrap();
        assert_eq!(bound(month), "2025-09-01");
        assert!(bound(month).as_str() <= "2025-09-01T00:00:00Z");
        assert!(bound(month.next()).as_str() > "2025-09-30T23:59:59.999999Z");
    }
}
