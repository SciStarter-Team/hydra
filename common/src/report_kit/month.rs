//! Calendar months and the ranges of them a report covers. **Shared file**;
//! see `report_kit.rs`.
//!
//! Months are a plain year and month rather than either site's date type, so
//! that the reports depend on neither.

use std::fmt;

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

const LONG_MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// A calendar month.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub struct Month {
    pub year: i32,
    /// 1 through 12.
    pub month: u32,
}

impl Month {
    pub fn new(year: i32, month: u32) -> Option<Month> {
        (1..=12).contains(&month).then_some(Month { year, month })
    }

    /// Reads `YYYY-MM`, as an `<input type="month">` writes it, or the start
    /// of an ISO timestamp.
    pub fn parse(text: &str) -> Option<Month> {
        let text = text.trim();
        let year = text.get(0..4)?;
        let month = text.get(5..7)?;

        if text.as_bytes().get(4) != Some(&b'-') || !year.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }

        if !month.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }

        Month::new(year.parse().ok()?, month.parse().ok()?)
    }

    pub fn next(self) -> Month {
        if self.month == 12 {
            Month {
                year: self.year + 1,
                month: 1,
            }
        } else {
            Month {
                year: self.year,
                month: self.month + 1,
            }
        }
    }

    pub fn previous(self) -> Month {
        if self.month == 1 {
            Month {
                year: self.year - 1,
                month: 12,
            }
        } else {
            Month {
                year: self.year,
                month: self.month - 1,
            }
        }
    }

    fn index(self) -> usize {
        self.month as usize - 1
    }

    /// `Sep`
    pub fn abbreviation(self) -> &'static str {
        MONTHS[self.index()]
    }

    /// `Sep 2025`
    pub fn short_label(self) -> String {
        format!("{} {}", self.abbreviation(), self.year)
    }

    /// `September 2025`
    pub fn long_label(self) -> String {
        format!("{} {}", LONG_MONTHS[self.index()], self.year)
    }
}

/// `2025-09`
impl fmt::Display for Month {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}", self.year, self.month)
    }
}

/// How far back a report reaches when the request names no start month,
/// counted back from its last month.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefaultSpan {
    /// From January of the year before the last month's year.
    SinceJanuaryLastYear,
    /// This many months, ending with the last month.
    Trailing(u32),
}

/// The months a report covers, first and last inclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Range {
    pub first: Month,
    pub last: Month,
}

impl Range {
    /// The requested range. The last month defaults to the last complete one
    /// before `this_month`, and the first to `span` back from the last. A
    /// start after the end is read as the same range written backwards; the
    /// page names the range it shows.
    pub fn resolve(
        from: Option<Month>,
        to: Option<Month>,
        this_month: Month,
        span: DefaultSpan,
    ) -> Range {
        let last = to.unwrap_or(this_month.previous());
        let first = from.unwrap_or_else(|| match span {
            DefaultSpan::SinceJanuaryLastYear => Month {
                year: last.year - 1,
                month: 1,
            },
            DefaultSpan::Trailing(months) => {
                (1..months.max(1)).fold(last, |month, _| month.previous())
            }
        });

        if first <= last {
            Range { first, last }
        } else {
            Range {
                first: last,
                last: first,
            }
        }
    }

    pub fn months(self) -> Vec<Month> {
        let mut months = Vec::new();
        let mut month = self.first;

        while month <= self.last {
            months.push(month);
            month = month.next();
        }

        months
    }

    /// The month after the last, where an exclusive upper bound falls.
    pub fn end(self) -> Month {
        self.last.next()
    }

    pub fn query_string(self) -> String {
        format!("from={}&to={}", self.first, self.last)
    }

    pub fn label(self) -> String {
        if self.first == self.last {
            self.first.long_label()
        } else {
            format!("{} – {}", self.first.long_label(), self.last.long_label())
        }
    }

    /// A file name for something covering this range: `stem-2025-01-to-2026-09`.
    pub fn file_stem(self, stem: &str) -> String {
        format!("{stem}-{}-to-{}", self.first, self.last)
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

    #[test]
    fn months_parse_from_month_inputs_and_timestamps() {
        assert_eq!(Month::parse("2025-09"), Some(month(2025, 9)));
        assert_eq!(Month::parse("2025-09-14T10:00:00Z"), Some(month(2025, 9)));
        assert_eq!(Month::parse("2025-13"), None);
        assert_eq!(Month::parse("2025-00"), None);
        assert_eq!(Month::parse("2025/09"), None);
        assert_eq!(Month::parse("25-09"), None);
        assert_eq!(Month::parse(""), None);
    }

    #[test]
    fn months_step_across_years() {
        assert_eq!(month(2025, 12).next(), month(2026, 1));
        assert_eq!(month(2026, 1).previous(), month(2025, 12));
    }

    #[test]
    fn months_write_their_labels() {
        assert_eq!(month(2025, 9).to_string(), "2025-09");
        assert_eq!(month(2025, 9).short_label(), "Sep 2025");
        assert_eq!(month(2025, 9).long_label(), "September 2025");
    }

    #[test]
    fn the_default_range_since_january_last_year_ends_last_month() {
        let r = Range::resolve(
            None,
            None,
            month(2026, 10),
            DefaultSpan::SinceJanuaryLastYear,
        );
        assert_eq!(r, range(month(2025, 1), month(2026, 9)));
    }

    #[test]
    fn in_january_the_default_reaches_back_two_calendar_years() {
        let r = Range::resolve(
            None,
            None,
            month(2027, 1),
            DefaultSpan::SinceJanuaryLastYear,
        );
        assert_eq!(r, range(month(2025, 1), month(2026, 12)));
    }

    #[test]
    fn a_trailing_default_counts_back_from_the_last_month() {
        let r = Range::resolve(None, None, month(2026, 10), DefaultSpan::Trailing(24));
        assert_eq!(r, range(month(2024, 10), month(2026, 9)));
        assert_eq!(r.months().len(), 24);

        let one = Range::resolve(None, None, month(2026, 10), DefaultSpan::Trailing(1));
        assert_eq!(one, range(month(2026, 9), month(2026, 9)));
    }

    #[test]
    fn a_range_written_backwards_is_read_forwards() {
        let r = Range::resolve(
            Some(month(2026, 3)),
            Some(month(2025, 6)),
            month(2026, 10),
            DefaultSpan::Trailing(24),
        );
        assert_eq!(r, range(month(2025, 6), month(2026, 3)));
    }

    #[test]
    fn a_range_lists_each_month_once() {
        assert_eq!(
            range(month(2025, 11), month(2026, 2)).months(),
            [
                month(2025, 11),
                month(2025, 12),
                month(2026, 1),
                month(2026, 2)
            ]
        );
        assert_eq!(
            range(month(2025, 11), month(2025, 12)).end(),
            month(2026, 1)
        );
    }

    #[test]
    fn a_range_names_itself() {
        let r = range(month(2025, 4), month(2025, 5));
        assert_eq!(r.label(), "April 2025 – May 2025");
        assert_eq!(r.query_string(), "from=2025-04&to=2025-05");
        assert_eq!(r.file_stem("report"), "report-2025-04-to-2025-05");
        assert_eq!(range(month(2025, 4), month(2025, 4)).label(), "April 2025");
    }
}
