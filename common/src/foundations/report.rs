//! The Foundations completions report: how many people finished the
//! Foundations of Citizen Science training each month, EvolveMe's share of
//! them, and the running total the training page's counter shows.
//!
//! **Shared file.** Byte-identical in orrery (`src/staff/foundations/`) and
//! hydra (`common/src/foundations/`), as are its siblings `chart.rs` and
//! `page.rs`, on top of the report kit (`report_kit/`). Each site supplies
//! only its monthly counts — [`MonthCount`]s — and today's month; everything
//! from there to the rendered rows is here.
//!
//! **What counts.** A person completes Foundations once: the training page's
//! "completed to date" counter counts each account whose progress is past the
//! module, and this report dates each of those accounts by the earliest
//! evidence that they finished it. Earning the badge is not required.

use std::collections::BTreeMap;

use super::super::report_kit::format::thousands;
use super::super::report_kit::month::{Month, Range};
use super::chart;

/// How many people first completed Foundations in one month, split by
/// whether their account came through EvolveMe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MonthCount {
    pub month: Month,
    pub evolveme: i64,
    pub others: i64,
}

impl MonthCount {
    /// Gathers `(month, through EvolveMe, completions)` rows, in any order and
    /// possibly several per month, into one count per month, oldest first.
    pub fn collect(rows: impl IntoIterator<Item = (Month, bool, i64)>) -> Vec<MonthCount> {
        let mut months: BTreeMap<Month, MonthCount> = BTreeMap::new();

        for (month, evolveme, count) in rows {
            let entry = months.entry(month).or_insert(MonthCount {
                month,
                evolveme: 0,
                others: 0,
            });

            if evolveme {
                entry.evolveme += count;
            } else {
                entry.others += count;
            }
        }

        months.into_values().collect()
    }

    pub fn total(&self) -> i64 {
        self.evolveme + self.others
    }
}

/// One month of the report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonthReport {
    pub month: Month,
    pub evolveme: i64,
    pub others: i64,
    /// Everyone who had completed Foundations by the end of the month,
    /// including every month before the range.
    pub to_date: i64,
}

impl MonthReport {
    pub fn completed(&self) -> i64 {
        self.evolveme + self.others
    }
}

/// Every month of `range`, with its completions and the running total, from
/// all of a site's monthly counts.
pub fn tally(range: Range, counts: &[MonthCount]) -> Vec<MonthReport> {
    let mut to_date: i64 = counts
        .iter()
        .filter(|c| c.month < range.first)
        .map(MonthCount::total)
        .sum();

    range
        .months()
        .into_iter()
        .map(|month| {
            let count = counts.iter().filter(|c| c.month == month).fold(
                MonthCount {
                    month,
                    evolveme: 0,
                    others: 0,
                },
                |sum, c| MonthCount {
                    month,
                    evolveme: sum.evolveme + c.evolveme,
                    others: sum.others + c.others,
                },
            );

            to_date += count.total();

            MonthReport {
                month,
                evolveme: count.evolveme,
                others: count.others,
                to_date,
            }
        })
        .collect()
}

/// Everyone who has completed Foundations, as the training page counts them.
pub fn completed_to_date(counts: &[MonthCount]) -> i64 {
    counts.iter().map(MonthCount::total).sum()
}

pub struct Row {
    pub label: String,
    pub completed: String,
    pub evolveme: String,
    pub others: String,
    pub to_date: String,
}

/// Everything the report page shows, ready to print.
pub struct ReportView {
    pub chart_svg: String,
    pub rows: Vec<Row>,
    pub completed_to_date: String,
}

pub fn view(reports: &[MonthReport], completed_to_date: i64) -> ReportView {
    ReportView {
        chart_svg: chart::render(reports),
        rows: reports
            .iter()
            .map(|r| Row {
                label: r.month.short_label(),
                completed: thousands(r.completed()),
                evolveme: thousands(r.evolveme),
                others: thousands(r.others),
                to_date: thousands(r.to_date),
            })
            .collect(),
        completed_to_date: thousands(completed_to_date),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn month(year: i32, month: u32) -> Month {
        Month::new(year, month).unwrap()
    }

    fn count(year: i32, m: u32, evolveme: i64, others: i64) -> MonthCount {
        MonthCount {
            month: month(year, m),
            evolveme,
            others,
        }
    }

    #[test]
    fn rows_are_gathered_into_one_count_per_month() {
        let counts = MonthCount::collect([
            (month(2025, 9), true, 4_876),
            (month(2025, 8), false, 906),
            (month(2025, 9), false, 1_051),
            (month(2025, 8), true, 7_758),
        ]);

        assert_eq!(
            counts,
            [count(2025, 8, 7_758, 906), count(2025, 9, 4_876, 1_051)]
        );
    }

    #[test]
    fn the_running_total_carries_in_everything_before_the_range() {
        let counts = [
            count(2021, 3, 0, 20_000),
            count(2025, 8, 7_758, 906),
            count(2025, 10, 3_607, 441),
        ];
        let range = Range {
            first: month(2025, 8),
            last: month(2025, 10),
        };
        let reports = tally(range, &counts);

        assert_eq!(reports.len(), 3);
        assert_eq!(reports[0].to_date, 28_664);
        // A month nobody completed it still gets a row, holding the total.
        assert_eq!(reports[1].completed(), 0);
        assert_eq!(reports[1].to_date, 28_664);
        assert_eq!(reports[2].to_date, 32_712);
    }

    #[test]
    fn completions_after_the_range_are_not_in_its_totals() {
        let counts = [count(2025, 8, 0, 10), count(2026, 1, 0, 99)];
        let range = Range {
            first: month(2025, 8),
            last: month(2025, 8),
        };

        assert_eq!(tally(range, &counts)[0].to_date, 10);
        assert_eq!(completed_to_date(&counts), 109);
    }

    #[test]
    fn the_view_prints_each_month() {
        let range = Range {
            first: month(2026, 9),
            last: month(2026, 9),
        };
        let reports = tally(range, &[count(2026, 9, 27, 516)]);
        let view = view(&reports, 92_803);

        assert_eq!(view.rows[0].label, "Sep 2026");
        assert_eq!(view.rows[0].completed, "543");
        assert_eq!(view.rows[0].evolveme, "27");
        assert_eq!(view.rows[0].others, "516");
        assert_eq!(view.completed_to_date, "92,803");
        assert!(view.chart_svg.starts_with("<svg"));
    }
}
