//! The referral report: sign-ups tallied by month and source, and the rows
//! its page shows.
//!
//! **Shared file.** Byte-identical in orrery (`src/staff/referrals/report.rs`)
//! and hydra (`common/src/referrals/report.rs`); see `sources.rs`. Each site
//! supplies only the sign-ups — every counted account's month and attribution
//! — and today's month; everything from there to the rendered rows is here,
//! so the two pages cannot disagree about anything but their data.

use std::collections::{BTreeMap, HashMap};

use super::super::report_kit::format::thousands;
use super::super::report_kit::month::{Month, Range};
use super::chart::{self, ChartMonth};
use super::sources::{Attribution, Group, Source, classify};

/// How many sources the table lists for each month.
pub const TOP_SOURCES: usize = 10;

/// One month of the report.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MonthReport {
    pub month: Month,
    /// Every counted sign-up, whether or not its source is known.
    pub signups: i64,
    /// Sign-ups with a known source, most first; ties by name.
    pub sources: Vec<(Source, i64)>,
}

impl MonthReport {
    pub fn attributed(&self) -> i64 {
        self.sources.iter().map(|(_, n)| n).sum()
    }

    pub fn chart_month(&self) -> ChartMonth {
        let mut groups = [0; Group::ALL.len()];

        for (source, n) in &self.sources {
            groups[source.group().index()] += n;
        }

        ChartMonth {
            month: self.month,
            signups: self.signups,
            groups,
        }
    }
}

/// Classifies and tallies sign-ups by month, giving every month of the range
/// a report, including months with none. Sign-ups outside the range are
/// ignored.
pub fn tally<'a>(
    range: Range,
    signups: impl IntoIterator<Item = (Month, Attribution<'a>)>,
) -> Vec<MonthReport> {
    let mut months: BTreeMap<Month, (i64, HashMap<Source, i64>)> = range
        .months()
        .into_iter()
        .map(|month| (month, (0, HashMap::new())))
        .collect();

    for (month, attribution) in signups {
        let Some((count, sources)) = months.get_mut(&month) else {
            continue;
        };

        *count += 1;

        if let Some(source) = classify(attribution) {
            *sources.entry(source).or_default() += 1;
        }
    }

    months
        .into_iter()
        .map(|(month, (signups, sources))| {
            let mut sources: Vec<(Source, i64)> = sources.into_iter().collect();
            sources.sort_by(|(a, an), (b, bn)| {
                bn.cmp(an).then_with(|| a.to_string().cmp(&b.to_string()))
            });

            MonthReport {
                month,
                signups,
                sources,
            }
        })
        .collect()
}

pub struct SourceCell {
    pub label: String,
    pub count: String,
}

pub struct MonthRow {
    pub label: String,
    pub signups: String,
    pub attributed: String,
    pub top: Vec<SourceCell>,
}

pub struct GroupRow {
    pub label: String,
    pub counts: Vec<String>,
}

/// Everything the report page shows, ready to print.
pub struct ReportView {
    pub chart_svg: String,
    pub months: Vec<MonthRow>,
    pub group_headings: Vec<&'static str>,
    pub group_rows: Vec<GroupRow>,
}

pub fn view(reports: &[MonthReport]) -> ReportView {
    let chart_months: Vec<ChartMonth> = reports.iter().map(MonthReport::chart_month).collect();

    let months = reports
        .iter()
        .map(|report| MonthRow {
            label: report.month.short_label(),
            signups: thousands(report.signups),
            attributed: thousands(report.attributed()),
            top: report
                .sources
                .iter()
                .take(TOP_SOURCES)
                .map(|(source, n)| SourceCell {
                    label: source.to_string(),
                    count: thousands(*n),
                })
                .collect(),
        })
        .collect();

    let group_rows = chart_months
        .iter()
        .map(|m| GroupRow {
            label: m.month.short_label(),
            counts: m.groups.iter().map(|&n| thousands(n)).collect(),
        })
        .collect();

    ReportView {
        chart_svg: chart::render(&chart_months),
        months,
        group_headings: Group::ALL.into_iter().map(Group::label).collect(),
        group_rows,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn month(year: i32, month: u32) -> Month {
        Month::new(year, month).unwrap()
    }

    fn range(first: Month, last: Month) -> Range {
        Range { first, last }
    }

    fn signup(m: Month, referer: &str) -> (Month, Attribution<'_>) {
        (
            m,
            Attribution {
                referer,
                origin: "/",
                utm_source: "",
                utm_medium: "",
            },
        )
    }

    fn tagged<'a>(
        m: Month,
        referer: &'a str,
        utm_source: &'a str,
        utm_medium: &'a str,
    ) -> (Month, Attribution<'a>) {
        (
            m,
            Attribution {
                referer,
                origin: "/",
                utm_source,
                utm_medium,
            },
        )
    }

    #[test]
    fn every_month_in_the_range_gets_a_report() {
        let r = range(month(2025, 1), month(2025, 3));
        let reports = tally(r, [signup(month(2025, 2), "https://www.google.com/")]);

        assert_eq!(reports.len(), 3);
        assert_eq!(reports[0].signups, 0);
        assert_eq!(reports[1].signups, 1);
        assert_eq!(reports[2].signups, 0);
    }

    #[test]
    fn sign_ups_outside_the_range_are_ignored() {
        let r = range(month(2025, 1), month(2025, 1));
        let reports = tally(r, [signup(month(2025, 2), "https://www.google.com/")]);
        assert_eq!(reports[0].signups, 0);
    }

    #[test]
    fn sign_ups_without_a_source_count_in_the_total_only() {
        let m = month(2025, 1);
        let reports = tally(
            range(m, m),
            [
                signup(m, "https://www.google.com/"),
                signup(m, ""),
                signup(m, "https://scistarter.org/"),
            ],
        );

        assert_eq!(reports[0].signups, 3);
        assert_eq!(reports[0].attributed(), 1);
    }

    #[test]
    fn sources_are_ordered_by_count_then_name() {
        let m = month(2025, 1);
        let reports = tally(
            range(m, m),
            [
                signup(m, "https://www.bing.com/"),
                signup(m, "https://www.google.com/"),
                signup(m, "https://www.google.com/"),
                signup(m, "https://www.cocorahs.org/"),
            ],
        );

        let order: Vec<Source> = reports[0].sources.iter().map(|(s, _)| s.clone()).collect();
        assert_eq!(
            order,
            [Source::GoogleSearch, Source::CoCoRaHS, Source::OtherSearch]
        );
    }

    #[test]
    fn ads_and_organic_share_one_chart_series() {
        let m = month(2026, 11);
        let reports = tally(
            range(m, m),
            [
                tagged(m, "https://www.linkedin.com/", "linkedin", "paid"),
                tagged(m, "https://www.linkedin.com/", "", ""),
            ],
        );

        assert_eq!(reports[0].sources.len(), 2);
        assert_eq!(reports[0].chart_month().groups[Group::LinkedIn.index()], 2);
    }

    #[test]
    fn the_view_lists_at_most_ten_sources_a_month() {
        let m = month(2025, 1);
        let hosts: Vec<String> = (0..15).map(|i| format!("https://site{i}.org/")).collect();
        let reports = tally(range(m, m), hosts.iter().map(|h| signup(m, h)));
        let view = view(&reports);

        assert_eq!(view.months[0].top.len(), TOP_SOURCES);
        assert_eq!(view.months[0].attributed, "15");
    }

    #[test]
    fn the_view_draws_the_chart_and_names_every_series() {
        let r = range(month(2025, 4), month(2025, 5));
        let view = view(&tally(r, []));

        assert!(view.chart_svg.starts_with("<svg"));
        assert_eq!(view.group_headings.len(), Group::ALL.len());
        assert_eq!(view.months.len(), 2);
    }
}
