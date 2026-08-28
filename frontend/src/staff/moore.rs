//! Live progress against the Moore Foundation grant (GBMF #9889.03). Ported
//! from orrery's `src/staff/moore.rs`.
//!
//! Nine program counts, one row per month, cumulative — the shape the grant's
//! own reporting takes, so a monthly refresh adds a row rather than editing
//! one. Everything is computed from the databases on each load; there is no
//! snapshot table to fall out of date.
//!
//! **Dating a milestone.** orrery has to date people by the earliest immutable
//! evidence it can find, because its state machines carry a Django `auto_now`
//! field that holds only the most recent transition — so a member counted in a
//! past month would silently leave it as soon as they moved again. hydra's
//! progress tables stamp each step as it completes and never rewrite it, so the
//! dating is done in the user service against those columns; see
//! `user-service/src/moore.rs`.
//!
//! Two of the grant's indicators are not ledger columns. Kit-building downloads
//! are go-link clicks, counted by the log service. Acts of Science is counted
//! once a year during Citizen Science Month, so it gets its own table of
//! campaigns rather than a cumulative column.

use std::collections::BTreeMap;

use common::{
    message::{
        dashboard::MooreBucketsResponse,
        log::{ActsToDateRequest, ActsToDateResponse, GoUrlTotalsRequest, GoUrlTotalsResponse},
    },
    service::request,
};
use jiff::{Timestamp, tz::TimeZone};
use poem::{Response, handler};

use super::csv_field;
use crate::component::html_escape;

/// First month shown. The Ambassador program opened in July 2024, so a ledger
/// starting a month earlier shows every program metric from zero except the two
/// that predate the grant entirely (Foundations and accounts), whose running
/// totals carry in from before.
const FIRST_MONTH: (i32, u32) = (2024, 6);

/// The go-link slugs whose clicks are the grant's "kit building downloads"
/// indicator. Both the activity guides and the build-a-kit guides count; the
/// Spanish guides are separate links.
const KIT_SLUGS: &[&str] = &[
    "kit-building-guides",
    "library-activity-monitoring-air-quality",
    "library-activity-observing-pollinators-en",
    "library-activity-observing-pollinators-es",
    "pollinators-activity-guide-es",
    "library-activity-zombee-hunting",
    "library-activity-exporing-biodiversity-en",
    "library-activity-exporing-biodiversity-es",
    "library-activity-mapping-mosquito-habitats",
    "library-activity-measuring-light-in-the-night-en",
    "library-activity-measuring-light-in-the-night-es",
    "library-build-exploring-biodiversity",
    "library-build-mapping-mosquito-habitats",
    "library-build-measuring-light-in-the-night",
    "library-build-monitoring-air-quality",
    "library-build-observing-pollinators",
    "library-build-zombee-hunting",
];

/// The first Citizen Science Month whose Acts campaign is shown, and the three
/// inside the grant term that carry a mark.
const FIRST_ACTS_YEAR: i32 = 2024;
const GRANT_ACTS_YEARS: [i32; 3] = [2026, 2027, 2028];

/// The grant's mark for the nth April of the term: an even climb from the
/// agreement's baseline to its target, since the agreement sets only the final
/// figure.
fn acts_mark(nth: i64) -> i64 {
    const BASELINE: i64 = 1_000_000;
    const TARGET: i64 = 5_000_000;

    BASELINE + (TARGET - BASELINE) * nth / GRANT_ACTS_YEARS.len() as i64
}

/// One column of the ledger.
///
/// The variants are the unit of truth for column order: the user service labels
/// its buckets with [`Metric::tag`], the running totals are indexed by
/// [`Metric::column`], and the page walks [`Metric::ALL`], so a metric cannot be
/// added to one of those and forgotten in the others.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Metric {
    Librarians,
    Libraries,
    Foundations,
    FoundationsBadge,
    Champions,
    Certified,
    Approved,
    Verified,
    Accounts,
}

impl Metric {
    const ALL: [Metric; 9] = [
        Metric::Librarians,
        Metric::Libraries,
        Metric::Foundations,
        Metric::FoundationsBadge,
        Metric::Champions,
        Metric::Certified,
        Metric::Approved,
        Metric::Verified,
        Metric::Accounts,
    ];

    /// The label the metric carries in the user service's `metric` column.
    fn tag(self) -> &'static str {
        match self {
            Metric::Librarians => "librarians",
            Metric::Libraries => "libraries",
            Metric::Foundations => "foundations",
            Metric::FoundationsBadge => "foundations_badge",
            Metric::Champions => "champions",
            Metric::Certified => "certified",
            Metric::Approved => "approved",
            Metric::Verified => "verified",
            Metric::Accounts => "accounts",
        }
    }

    fn from_tag(tag: &str) -> Option<Metric> {
        Metric::ALL.into_iter().find(|m| m.tag() == tag)
    }

    fn column(self) -> usize {
        Metric::ALL
            .iter()
            .position(|m| *m == self)
            .expect("every variant is in ALL")
    }

    fn heading(self) -> &'static str {
        match self {
            Metric::Librarians => "Librarians",
            Metric::Libraries => "Libraries",
            Metric::Foundations => "Foundations",
            Metric::FoundationsBadge => "Foundations badge",
            Metric::Champions => "Champions",
            Metric::Certified => "Certified Amb.",
            Metric::Approved => "Approved Amb.",
            Metric::Verified => "Verified accounts",
            Metric::Accounts => "All accounts",
        }
    }

    /// Column header for the CSV, where there is room to be unambiguous.
    fn csv_heading(self) -> &'static str {
        match self {
            Metric::Librarians => "Librarians in the network",
            Metric::Libraries => "Libraries in the network",
            Metric::Foundations => "Completed Foundations tutorial",
            Metric::FoundationsBadge => "Earned Foundations badge",
            Metric::Champions => "Completed Champion training",
            Metric::Certified => "Certified Ambassadors",
            Metric::Approved => "Fully approved Ambassadors",
            Metric::Verified => "Verified account holders",
            Metric::Accounts => "All account holders",
        }
    }

    /// Whether the metric is one of the five headline counts, shown as a tile.
    /// The other four are supporting detail for the metric above them.
    fn headline(self) -> bool {
        matches!(
            self,
            Metric::Librarians
                | Metric::Foundations
                | Metric::Champions
                | Metric::Certified
                | Metric::Verified
        )
    }

    fn tile_label(self) -> &'static str {
        match self {
            Metric::Librarians => "Librarians in the network",
            Metric::Foundations => "Completed Foundations",
            Metric::Champions => "Completed Champion training",
            Metric::Certified => "Certified Ambassadors",
            Metric::Verified => "Verified account holders",
            _ => self.heading(),
        }
    }

    /// Months before which this metric has no records at all, rendered as "not
    /// tracked" instead of a zero that would read as "nobody yet".
    ///
    /// Expressed as `(year, month)` so it compares directly against a ledger
    /// row's month.
    fn tracked_from(self) -> Option<(i32, u32)> {
        match self {
            // The Library Network sign-up form went live in June 2025, and
            // both of these are derived from it.
            Metric::Librarians | Metric::Libraries => Some((2025, 6)),
            _ => None,
        }
    }
}

/// One month's running totals.
struct MonthTotals {
    year: i32,
    month: u32,
    totals: [i64; Metric::ALL.len()],
}

impl MonthTotals {
    fn get(&self, metric: Metric) -> i64 {
        self.totals[metric.column()]
    }

    /// Whether this metric was being recorded at all by this month.
    fn tracked(&self, metric: Metric) -> bool {
        match metric.tracked_from() {
            Some((year, month)) => (self.year, self.month) >= (year, month),
            None => true,
        }
    }

    fn label(&self) -> String {
        const MONTH_NAMES: [&str; 12] = [
            "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
        ];
        format!(
            "{} {}",
            MONTH_NAMES[(self.month as usize - 1).min(11)],
            self.year
        )
    }
}

/// Everything the page needs, already accumulated.
struct Ledger {
    months: Vec<MonthTotals>,
    kit_downloads: i64,
    acts: Vec<ActsYear>,
}

/// One April campaign: what it counted, and what the grant asks of it.
///
/// Acts do not accumulate — every April starts from nothing — so a campaign is
/// judged against its own mark and not against what earlier ones managed.
struct ActsYear {
    year: i32,
    /// `None` for a campaign that has not happened yet.
    total: Option<i64>,
    /// The grant's mark for this April, for the three campaigns inside the
    /// grant term. Earlier campaigns are shown for context and have none.
    mark: Option<i64>,
}

/// Turn per-month arrivals into per-month running totals.
///
/// Accumulation starts at the earliest bucket in the data, not at
/// [`FIRST_MONTH`], so the first row shown already carries everything that
/// happened before it. Months with no arrivals still get a row, holding the
/// totals unchanged.
fn accumulate(buckets: &MooreBucketsResponse) -> Vec<MonthTotals> {
    // (year, month) -> per-metric arrivals.
    let mut arrivals: BTreeMap<(i32, u32), [i64; Metric::ALL.len()]> = BTreeMap::new();

    for bucket in &buckets.buckets {
        let Some(metric) = Metric::from_tag(&bucket.metric) else {
            // A tag this page does not know is ignored rather than misplaced
            // into some other metric's column.
            continue;
        };
        let Some((year, month)) = parse_month(&bucket.month) else {
            continue;
        };

        arrivals.entry((year, month)).or_default()[metric.column()] += bucket.count;
    }

    let today = Timestamp::now().to_zoned(TimeZone::UTC).date();
    let last = (i32::from(today.year()), today.month() as u32);

    let Some(first) = arrivals.keys().next().copied() else {
        return Vec::new();
    };

    let mut running = [0i64; Metric::ALL.len()];
    let mut months = Vec::new();
    let mut cursor = first;

    while cursor <= last {
        if let Some(month_arrivals) = arrivals.get(&cursor) {
            for (total, arrived) in running.iter_mut().zip(month_arrivals) {
                *total += arrived;
            }
        }

        // Everything before FIRST_MONTH is accumulated but not shown, so the
        // first visible row carries the history that precedes the grant.
        if cursor >= FIRST_MONTH {
            months.push(MonthTotals {
                year: cursor.0,
                month: cursor.1,
                totals: running,
            });
        }

        cursor = if cursor.1 == 12 {
            (cursor.0 + 1, 1)
        } else {
            (cursor.0, cursor.1 + 1)
        };
    }

    months
}

/// `YYYY-MM` to `(year, month)`.
fn parse_month(month: &str) -> Option<(i32, u32)> {
    let (year, month) = month.split_once('-')?;
    let year = year.parse().ok()?;
    let month = month.parse().ok()?;

    (1..=12).contains(&month).then_some((year, month))
}

/// One indicator from Attachment 1 of the agreement.
///
/// The agreement states a baseline and a single December 2028 target for each;
/// it sets no year-one or year-two milestone, so the year marks this page draws
/// are an even split of baseline to target across the three years and are
/// labelled as ours.
struct Goal {
    name: &'static str,
    /// How Attachment 1 words the indicator, which is not always what we call
    /// it now — the agreement's "# of ambassadors" is today's Champions.
    indicator: &'static str,
    source: &'static str,
    baseline: i64,
    target: i64,
    /// `None` for an indicator this page does not measure.
    current: Option<i64>,
    /// Replaces the computed pace line. For an indicator whose baseline counts
    /// a different population than anything the platform can see, saying how
    /// far there is to go would be arithmetic on two incomparable numbers.
    pace_override: Option<String>,
}

fn goals(ledger: &Ledger) -> Vec<Goal> {
    let latest = ledger.months.last();
    let at = |metric: Metric| latest.map(|m| m.get(metric));

    vec![
        Goal {
            name: "Champions",
            indicator: "\u{201c}# of ambassadors\u{201d}",
            source: "live",
            baseline: 400,
            target: 4_000,
            current: at(Metric::Champions),
            pace_override: None,
        },
        Goal {
            name: "Certified Ambassadors",
            indicator: "\u{201c}# of Certified ambassadors\u{201d}, by digital badge",
            source: "live",
            baseline: 100,
            target: 500,
            current: at(Metric::Certified),
            pace_override: None,
        },
        Goal {
            name: "Verified SciStarter members",
            indicator: "\u{201c}# of participating users\u{201d}",
            source: "live",
            baseline: 200_000,
            target: 500_000,
            current: at(Metric::Verified),
            pace_override: None,
        },
        Goal {
            name: "Kit-building downloads",
            indicator: "facilitation resources, # downloads",
            source: "live, from go-link clicks",
            baseline: 2_500,
            target: 10_000,
            current: Some(ledger.kit_downloads),
            pace_override: None,
        },
        Goal {
            name: "Libraries in the network",
            indicator: "\u{201c}# libraries\u{201d}, across all 50 states",
            source: "sign-ups only; the baseline predates the form",
            baseline: 1_000,
            target: 2_000,
            // The agreement's baseline counts a network that existed before the
            // sign-up form did, and the platform has no record of it. Showing
            // our count against that baseline would read as the network having
            // shrunk.
            current: None,
            pace_override: Some(match at(Metric::Libraries) {
                Some(n) => format!("{} signed up since Jun 2025", thousands(n)),
                None => String::from("not measured here"),
            }),
        },
    ]
}

impl Goal {
    fn year_mark(&self, year: i64) -> i64 {
        self.baseline + (self.target - self.baseline) * year / 3
    }
}

fn thousands(n: i64) -> String {
    let digits = n.abs().to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3 + 1);

    if n < 0 {
        out.push('-');
    }

    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }

    out
}

/// Gather the ledger from the three services that hold its parts.
async fn fetch() -> Result<Ledger, anyhow::Error> {
    let buckets: MooreBucketsResponse = request("user.moore.buckets", ()).await?;

    let kit: GoUrlTotalsResponse = request(
        "log.gourl_totals",
        GoUrlTotalsRequest {
            slugs: KIT_SLUGS.iter().map(|s| (*s).to_owned()).collect(),
        },
    )
    .await?;

    let this_year = i32::from(Timestamp::now().to_zoned(TimeZone::UTC).date().year());
    let last_acts_year = GRANT_ACTS_YEARS[GRANT_ACTS_YEARS.len() - 1];

    let mut acts = Vec::new();
    for year in FIRST_ACTS_YEAR..=last_acts_year {
        // A campaign that has not happened yet has no total, rather than a
        // total of zero, which would read as a campaign that failed.
        let total = if year <= this_year {
            let response: ActsToDateResponse =
                request("log.acts_to_date", ActsToDateRequest { year: Some(year) }).await?;
            Some(response.number)
        } else {
            None
        };

        acts.push(ActsYear {
            year,
            total,
            mark: GRANT_ACTS_YEARS
                .iter()
                .position(|y| *y == year)
                .map(|nth| acts_mark(nth as i64 + 1)),
        });
    }

    Ok(Ledger {
        months: accumulate(&buckets),
        kit_downloads: kit.total,
        acts,
    })
}

/// GET `/staff/moore` — the ledger, the goals and the Acts campaigns.
#[handler]
#[tracing::instrument]
pub async fn index() -> Result<Response, anyhow::Error> {
    let ledger = fetch().await?;

    let mut html = String::from(
        "<!doctype html><html><head><meta charset=\"utf-8\">\
         <title>Moore Foundation Progress</title>\
         <style>body{font-family:sans-serif;margin:2rem;max-width:1100px;}\
         table{border-collapse:collapse;width:100%;margin-top:1rem;}\
         td,th{padding:4px 8px;border-bottom:1px solid #eee;}\
         th{text-align:left;border-bottom:1px solid #ccc;}\
         td.num,th.num{text-align:right;font-variant-numeric:tabular-nums;}\
         .tiles{display:flex;flex-wrap:wrap;gap:1rem;margin-top:1rem;}\
         .tile{border:1px solid #ddd;border-radius:4px;padding:.75rem 1rem;min-width:9rem;}\
         .tile .n{font-size:1.6rem;font-variant-numeric:tabular-nums;}\
         .tile .l{color:#666;font-size:.85rem;}\
         .untracked{color:#bbb;}\
         .note{color:#666;font-size:.9rem;}\
         .met{color:#0a7d33;}</style>\
         </head><body><h1>Moore Foundation progress</h1>\
         <p class=\"note\">Grant GBMF #9889.03. Every figure is computed from the \
         databases on each load.</p>",
    );

    // Headline tiles, from the most recent month.
    if let Some(latest) = ledger.months.last() {
        html.push_str(&format!(
            "<h2>As of {}</h2><div class=\"tiles\">",
            html_escape(&latest.label())
        ));
        for metric in Metric::ALL {
            if !metric.headline() {
                continue;
            }
            let value = if latest.tracked(metric) {
                thousands(latest.get(metric))
            } else {
                String::from("—")
            };
            html.push_str(&format!(
                "<div class=\"tile\"><div class=\"n\">{value}</div>\
                 <div class=\"l\">{}</div></div>",
                html_escape(metric.tile_label()),
            ));
        }
        html.push_str("</div>");
    } else {
        html.push_str("<p class=\"note\">No data yet.</p>");
    }

    // Goals.
    html.push_str(
        "<h2>Agreement indicators</h2>\
         <p class=\"note\">The agreement sets a baseline and a December 2028 target. \
         The year marks below are an even split of the two across three years, and \
         are ours, not the agreement's.</p>\
         <table><tr><th>Indicator</th><th>As worded in Attachment 1</th>\
         <th class=\"num\">Baseline</th><th class=\"num\">Now</th>\
         <th class=\"num\">Year 1</th><th class=\"num\">Year 2</th>\
         <th class=\"num\">Target</th><th>Source</th></tr>",
    );

    for goal in goals(&ledger) {
        let now = match (&goal.pace_override, goal.current) {
            (Some(text), _) => html_escape(text),
            (None, Some(value)) => thousands(value),
            (None, None) => String::from("—"),
        };

        html.push_str(&format!(
            "<tr><td>{}</td><td>{}</td><td class=\"num\">{}</td><td class=\"num\">{now}</td>\
             <td class=\"num\">{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td>\
             <td>{}</td></tr>",
            html_escape(goal.name),
            html_escape(goal.indicator),
            thousands(goal.baseline),
            thousands(goal.year_mark(1)),
            thousands(goal.year_mark(2)),
            thousands(goal.target),
            html_escape(goal.source),
        ));
    }
    html.push_str("</table>");

    // Acts of Science.
    html.push_str(
        "<h2>Acts of Science</h2>\
         <p class=\"note\">Counted once a year during Citizen Science Month, so each \
         April is judged against its own mark rather than a running total.</p>\
         <table><tr><th>April</th><th class=\"num\">Acts</th>\
         <th class=\"num\">Mark</th><th>Met</th></tr>",
    );

    for acts in &ledger.acts {
        let total = acts
            .total
            .map(thousands)
            .unwrap_or_else(|| String::from("—"));
        let mark = acts
            .mark
            .map(thousands)
            .unwrap_or_else(|| String::from("—"));
        let met = match (acts.total, acts.mark) {
            (Some(total), Some(mark)) if total >= mark => "<span class=\"met\">yes</span>",
            (Some(_), Some(_)) => "no",
            _ => "",
        };
        html.push_str(&format!(
            "<tr><td>{}</td><td class=\"num\">{total}</td>\
             <td class=\"num\">{mark}</td><td>{met}</td></tr>",
            acts.year,
        ));
    }
    html.push_str("</table>");

    // The ledger itself.
    html.push_str(
        "<h2>Monthly ledger</h2><p><a href=\"/staff/moore/export.csv\">Export CSV</a></p>",
    );
    html.push_str(&table(&ledger));

    html.push_str("</body></html>");

    Ok(Response::builder()
        .content_type("text/html; charset=utf-8")
        .body(html))
}

/// GET `/staff/moore/data` — the ledger table on its own, for embedding.
#[handler]
#[tracing::instrument]
pub async fn data() -> Result<Response, anyhow::Error> {
    let ledger = fetch().await?;

    Ok(Response::builder()
        .content_type("text/html; charset=utf-8")
        .body(table(&ledger)))
}

fn table(ledger: &Ledger) -> String {
    let mut html = String::from("<table><tr><th>Month</th>");

    for metric in Metric::ALL {
        html.push_str(&format!(
            "<th class=\"num\">{}</th>",
            html_escape(metric.heading())
        ));
    }
    html.push_str("</tr>");

    // Newest first: the current month is what a monthly refresh is looking for.
    for month in ledger.months.iter().rev() {
        html.push_str(&format!("<tr><td>{}</td>", html_escape(&month.label())));
        for metric in Metric::ALL {
            if month.tracked(metric) {
                html.push_str(&format!(
                    "<td class=\"num\">{}</td>",
                    thousands(month.get(metric))
                ));
            } else {
                html.push_str("<td class=\"num untracked\" title=\"not tracked yet\">—</td>");
            }
        }
        html.push_str("</tr>");
    }

    html.push_str("</table>");
    html
}

/// GET `/staff/moore/export.csv` — the ledger as a spreadsheet.
#[handler]
#[tracing::instrument]
pub async fn export_csv() -> Result<Response, anyhow::Error> {
    let ledger = fetch().await?;

    let mut csv = String::from("Month");
    for metric in Metric::ALL {
        csv.push(',');
        csv.push_str(&csv_field(metric.csv_heading()));
    }
    csv.push('\n');

    for month in &ledger.months {
        csv.push_str(&csv_field(&format!("{}-{:02}", month.year, month.month)));
        for metric in Metric::ALL {
            csv.push(',');
            // An untracked month is left empty rather than zeroed, so a
            // spreadsheet does not average in a count that was never taken.
            if month.tracked(metric) {
                csv.push_str(&month.get(metric).to_string());
            }
        }
        csv.push('\n');
    }

    let today = Timestamp::now().to_zoned(TimeZone::UTC).date();

    Ok(Response::builder()
        .content_type("text/csv")
        .header(
            "Content-Disposition",
            format!("attachment; filename=\"moore_{today}.csv\""),
        )
        .body(csv))
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::message::dashboard::MooreBucket;

    fn bucket(metric: Metric, month: &str, count: i64) -> MooreBucket {
        MooreBucket {
            metric: metric.tag().to_string(),
            month: month.to_string(),
            count,
        }
    }

    #[test]
    fn every_metric_round_trips_through_its_tag() {
        for metric in Metric::ALL {
            assert_eq!(Metric::from_tag(metric.tag()), Some(metric));
        }
    }

    #[test]
    fn every_metric_has_its_own_column() {
        let mut columns: Vec<usize> = Metric::ALL.iter().map(|m| m.column()).collect();
        columns.sort_unstable();
        columns.dedup();
        assert_eq!(columns.len(), Metric::ALL.len());
    }

    #[test]
    fn totals_accumulate_across_months() {
        let buckets = MooreBucketsResponse {
            buckets: vec![
                bucket(Metric::Accounts, "2024-06", 10),
                bucket(Metric::Accounts, "2024-08", 5),
            ],
        };

        let months = accumulate(&buckets);

        assert_eq!(months[0].get(Metric::Accounts), 10, "June");
        assert_eq!(months[1].get(Metric::Accounts), 10, "July, nobody arrived");
        assert_eq!(months[2].get(Metric::Accounts), 15, "August");
    }

    #[test]
    fn history_before_the_first_shown_month_is_carried_in() {
        let buckets = MooreBucketsResponse {
            buckets: vec![
                bucket(Metric::Accounts, "2020-01", 100),
                bucket(Metric::Accounts, "2024-06", 1),
            ],
        };

        let months = accumulate(&buckets);

        assert_eq!(months[0].year, FIRST_MONTH.0);
        assert_eq!(months[0].month, FIRST_MONTH.1);
        assert_eq!(
            months[0].get(Metric::Accounts),
            101,
            "the pre-grant history is in the first visible row"
        );
    }

    #[test]
    fn an_unknown_metric_tag_is_ignored() {
        let buckets = MooreBucketsResponse {
            buckets: vec![
                MooreBucket {
                    metric: String::from("something_new"),
                    month: String::from("2024-06"),
                    count: 99,
                },
                bucket(Metric::Accounts, "2024-06", 1),
            ],
        };

        let months = accumulate(&buckets);

        assert_eq!(months[0].get(Metric::Accounts), 1);
        assert_eq!(
            months[0].totals.iter().sum::<i64>(),
            1,
            "the unknown tag landed in no column"
        );
    }

    #[test]
    fn librarians_are_untracked_before_the_form_existed() {
        let month = MonthTotals {
            year: 2024,
            month: 6,
            totals: [0; Metric::ALL.len()],
        };
        assert!(!month.tracked(Metric::Librarians));
        assert!(month.tracked(Metric::Accounts));

        let later = MonthTotals {
            year: 2025,
            month: 6,
            totals: [0; Metric::ALL.len()],
        };
        assert!(later.tracked(Metric::Librarians));
    }

    #[test]
    fn each_april_gets_its_own_mark_not_a_share_of_a_running_total() {
        assert_eq!(acts_mark(1), 2_333_333);
        assert_eq!(acts_mark(2), 3_666_666);
        assert_eq!(acts_mark(3), 5_000_000);
    }

    #[test]
    fn thousands_separates_groups_of_three() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1_000), "1,000");
        assert_eq!(thousands(1_234_567), "1,234,567");
        assert_eq!(thousands(-4_321), "-4,321");
    }

    #[test]
    fn a_malformed_month_is_dropped_rather_than_dated_to_the_epoch() {
        assert_eq!(parse_month("2024-06"), Some((2024, 6)));
        assert_eq!(parse_month("2024-13"), None);
        assert_eq!(parse_month("nonsense"), None);
        assert_eq!(parse_month(""), None);
    }
}
