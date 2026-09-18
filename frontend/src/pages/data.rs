//! The public statistics page (`/data`).
//!
//! Ported from orrery's `ss-data` Vue component and the `data_overview` API
//! behind it. Three sources feed it: the project figures are counted here from
//! this instance's own tables, the account count comes from the user service,
//! and the year's engagement totals from the log service. The demographics are
//! survey findings rather than anything either system records, so they are
//! written into the template.

use anyhow::Error;
use common::{
    message::{
        log::{AnnualEngagementRequest, AnnualEngagementResponse},
        user::{UserCountAsOfRequest, UserCountResponse},
    },
    service::request,
    shared::db,
    timestring::TimeString,
};
use poem::{Response, handler};
use sailfish::TemplateOnce;
use std::time::Duration;

use crate::{
    component::{Component, Head, StandardLayout},
    lazy_cache::{LazyCached, get_cached},
    middleware::authorization::Authenticated,
    number::thousands,
};

/// How long a frontend instance holds a figure gathered from another service
/// before refreshing it in the background. These are site-wide totals for a
/// finished year and an account count in the hundreds of thousands; neither
/// moves visibly over a few hours, and the page is public, so the refresh
/// should not ride on a visitor's request.
const STATS_CACHE_MAX_AGE: Duration = Duration::from_secs(6 * 60 * 60);

/// GET /data
#[handler]
pub async fn data_page(Authenticated(user_info): Authenticated) -> Result<Response, Error> {
    let engagement = load_engagement().await?;

    let content = DataPageContent {
        accounts: account_count().await?,
        engagement,
        projects: ProjectStats::load().await?,
    };

    let layout = StandardLayout::new("/data", content)
        .prepare(user_info.clone())
        .await?;

    let page = DataPage {
        head: Head::default().prepare(None).await?,
        layout,
    };

    Ok(page.into_response(None).await?)
}

#[derive(TemplateOnce)]
#[template(path = "pages/data.stpl.html")]
struct DataPage {
    head: <Head as Component>::Prepared,
    layout: <StandardLayout<DataPageContent> as Component>::Prepared,
}

#[derive(TemplateOnce)]
#[template(path = "pages/data_content.stpl.html")]
struct DataPageContent {
    /// Accounts ever created.
    accounts: i64,
    engagement: Engagement,
    projects: ProjectStats,
}

// ---------------------------------------------------------------------------
// A year's engagement
// ---------------------------------------------------------------------------

/// The most recent finished year's engagement, with the two per-account
/// averages the page reports alongside the totals.
struct Engagement {
    year: i32,
    views: i64,
    saves: i64,
    referrals: i64,
    contributions: i64,
    /// Accounts that existed by the end of the year: the denominator both
    /// averages are taken over.
    accounts: i64,
}

impl Engagement {
    fn per_account(&self, total: i64) -> f64 {
        if self.accounts == 0 {
            0.0
        } else {
            total as f64 / self.accounts as f64
        }
    }

    fn avg_referrals(&self) -> f64 {
        self.per_account(self.referrals)
    }

    fn avg_contributions(&self) -> f64 {
        self.per_account(self.contributions)
    }
}

/// The year's totals, plus the account population to divide them by.
///
/// The population is asked for after the totals because only the answer says
/// which year they cover.
async fn load_engagement() -> Result<Engagement, Error> {
    let totals = annual_engagement().await?;
    let accounts = accounts_through(totals.year).await?;

    Ok(Engagement {
        year: totals.year,
        views: totals.views,
        saves: totals.saves,
        referrals: totals.referrals,
        contributions: totals.contributions,
        accounts,
    })
}

#[derive(Clone)]
struct AnnualEngagement;

impl LazyCached for AnnualEngagement {
    fn cache_key(&self) -> String {
        "annual_engagement".to_string()
    }

    fn max_age(&self) -> Duration {
        STATS_CACHE_MAX_AGE
    }

    async fn fetch_fresh(&self) -> Result<String, Error> {
        let response: AnnualEngagementResponse = request(
            "log.stats.annual_engagement",
            AnnualEngagementRequest { year: None },
        )
        .await?;
        Ok(serde_json::to_string(&response)?)
    }
}

async fn annual_engagement() -> Result<AnnualEngagementResponse, Error> {
    Ok(serde_json::from_str(&get_cached(&AnnualEngagement).await?)?)
}

#[derive(Clone)]
struct AccountsThrough {
    year: i32,
}

impl LazyCached for AccountsThrough {
    fn cache_key(&self) -> String {
        format!("accounts_through:{}", self.year)
    }

    fn max_age(&self) -> Duration {
        STATS_CACHE_MAX_AGE
    }

    async fn fetch_fresh(&self) -> Result<String, Error> {
        let before = TimeString::try_from(format!("{}-01-01T00:00:00Z", self.year + 1))?;
        let response: UserCountResponse =
            request("user.stats.count_as_of", UserCountAsOfRequest { before }).await?;
        Ok(response.count.to_string())
    }
}

/// How many accounts existed at the end of `year`.
async fn accounts_through(year: i32) -> Result<i64, Error> {
    Ok(get_cached(&AccountsThrough { year })
        .await?
        .parse()
        .unwrap_or(0))
}

#[derive(Clone)]
struct AccountCount;

impl LazyCached for AccountCount {
    fn cache_key(&self) -> String {
        "account_count".to_string()
    }

    fn max_age(&self) -> Duration {
        STATS_CACHE_MAX_AGE
    }

    async fn fetch_fresh(&self) -> Result<String, Error> {
        let response: UserCountResponse = request("user.stats.total_count", ()).await?;
        Ok(response.count.to_string())
    }
}

/// How many accounts exist now.
async fn account_count() -> Result<i64, Error> {
    Ok(get_cached(&AccountCount).await?.parse().unwrap_or(0))
}

// ---------------------------------------------------------------------------
// The project catalogue
// ---------------------------------------------------------------------------

/// A labelled count, drawn as one bar of a chart.
struct Bar {
    label: String,
    value: i64,
}

/// What the catalogue of published projects looks like.
///
/// Every figure counts the same population — published, unflagged, real
/// projects — so that the parts are comparable with the whole. orrery counted
/// its topic, audience and status bars over slightly different populations,
/// which is why its bars did not add up.
struct ProjectStats {
    alltime: i64,
    active: i64,
    expired: i64,
    hiatus: i64,
    inperson: i64,
    online: i64,
    global: i64,
    region: i64,
    point: i64,
    indoors: i64,
    outdoors: i64,
    indoors_and_outdoors: i64,
    topics: Vec<Bar>,
    audiences: Vec<Bar>,
}

impl ProjectStats {
    async fn load() -> Result<Self, Error> {
        let counts = sqlx::query!(
            r#"
            WITH published AS (
                SELECT p.*
                FROM projects p
                JOIN entities e
                  ON e.target = p.project_id AND e.kind = 'Project' AND e.publish = 1
                WHERE p.flagged = 0 AND p.duplicate = 0 AND p.is_test = 0
            )
            SELECT
                (SELECT COUNT(*) FROM published) AS "alltime!: i64",
                (SELECT COUNT(*) FROM published WHERE status = 'Active') AS "active!: i64",
                (SELECT COUNT(*) FROM published WHERE status IN ('Complete', 'Expired')) AS "expired!: i64",
                (SELECT COUNT(*) FROM published WHERE status IN ('Hiatus', 'Hidden')) AS "hiatus!: i64",
                (SELECT COUNT(*) FROM published WHERE location_type IN ('At a specific address', 'In a geographic location or range')) AS "inperson!: i64",
                (SELECT COUNT(*) FROM published WHERE entirely_online = 1) AS "online!: i64",
                (SELECT COUNT(*) FROM published WHERE location_type = 'Global, anywhere on the planet') AS "global!: i64",
                (SELECT COUNT(*) FROM published WHERE location_type = 'In a geographic location or range') AS "region!: i64",
                (SELECT COUNT(*) FROM published WHERE location_type = 'At a specific address') AS "point!: i64",
                (SELECT COUNT(*) FROM published WHERE indoors = 1) AS "indoors!: i64",
                (SELECT COUNT(*) FROM published WHERE outdoors = 1) AS "outdoors!: i64",
                (SELECT COUNT(*) FROM published WHERE indoors = 1 AND outdoors = 1) AS "indoors_and_outdoors!: i64"
            "#
        )
        .fetch_one(db())
        .await?;

        let topics = sqlx::query!(
            r#"
            SELECT t.topic AS "label!: String", COUNT(*) AS "value!: i64"
            FROM project_topics t
            JOIN projects p ON p.project_id = t.project_id
            JOIN entities e
              ON e.target = p.project_id AND e.kind = 'Project' AND e.publish = 1
            WHERE p.flagged = 0 AND p.duplicate = 0 AND p.is_test = 0
            GROUP BY t.topic
            ORDER BY COUNT(*) DESC, t.topic
            "#
        )
        .fetch_all(db())
        .await?
        .into_iter()
        .map(|row| Bar {
            label: row.label,
            value: row.value,
        })
        .collect();

        let audiences = sqlx::query!(
            r#"
            SELECT a.audience AS "label!: String", COUNT(*) AS "value!: i64"
            FROM project_audiences a
            JOIN projects p ON p.project_id = a.project_id
            JOIN entities e
              ON e.target = p.project_id AND e.kind = 'Project' AND e.publish = 1
            WHERE p.flagged = 0 AND p.duplicate = 0 AND p.is_test = 0
            GROUP BY a.audience
            ORDER BY COUNT(*) DESC, a.audience
            "#
        )
        .fetch_all(db())
        .await?
        .into_iter()
        .map(|row| Bar {
            label: row.label,
            value: row.value,
        })
        .collect();

        Ok(ProjectStats {
            alltime: counts.alltime,
            active: counts.active,
            expired: counts.expired,
            hiatus: counts.hiatus,
            inperson: counts.inperson,
            online: counts.online,
            global: counts.global,
            region: counts.region,
            point: counts.point,
            indoors: counts.indoors,
            outdoors: counts.outdoors,
            indoors_and_outdoors: counts.indoors_and_outdoors,
            topics,
            audiences,
        })
    }

    /// Indoor / outdoor / both, as the bars the page draws them as. A project
    /// that can be done either way is counted in all three, which is why the
    /// chart says so.
    fn settings(&self) -> Vec<Bar> {
        vec![
            Bar {
                label: "Indoor".to_string(),
                value: self.indoors,
            },
            Bar {
                label: "Outdoor".to_string(),
                value: self.outdoors,
            },
            Bar {
                label: "Indoors and Outdoors".to_string(),
                value: self.indoors_and_outdoors,
            },
        ]
    }
}

// ---------------------------------------------------------------------------
// Template helpers
// ---------------------------------------------------------------------------

/// `num` as a whole percentage of `den`, for a bar's width.
fn percent(num: i64, den: i64) -> i64 {
    if den <= 0 { 0 } else { (num * 100) / den }
}

/// The largest value in a chart, which its bars are drawn relative to.
fn largest(bars: &[Bar]) -> i64 {
    bars.iter().map(|bar| bar.value).max().unwrap_or(0)
}

/// A per-account average. Two decimal places, because one of the two is
/// below 1 and a single place would round it to a flat "1.0".
fn average(value: f64) -> String {
    format!("{value:.2}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_of_nothing_is_zero_rather_than_a_division_by_zero() {
        assert_eq!(percent(0, 0), 0);
        assert_eq!(percent(5, 0), 0);
    }

    #[test]
    fn percent_rounds_down() {
        assert_eq!(percent(1, 3), 33);
        assert_eq!(percent(2871, 2871), 100);
    }

    #[test]
    fn an_empty_chart_has_no_largest_bar() {
        assert_eq!(largest(&[]), 0);
    }

    /// With no accounts there is nothing to average over, and the page says
    /// zero rather than NaN.
    #[test]
    fn averages_over_an_empty_population_are_zero() {
        let engagement = Engagement {
            year: 2025,
            views: 1,
            saves: 1,
            referrals: 1,
            contributions: 1,
            accounts: 0,
        };

        assert_eq!(average(engagement.avg_referrals()), "0.00");
        assert_eq!(average(engagement.avg_contributions()), "0.00");
    }
}
