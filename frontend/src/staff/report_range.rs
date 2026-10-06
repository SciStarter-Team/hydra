//! The month range a monthly staff report is asked for, read from its query
//! string (`?from=YYYY-MM&to=YYYY-MM`, as `<input type="month">` writes
//! them). hydra's side of the shared report kit, which has no date type of
//! its own to read today's month from.

use common::report_kit::month::{DefaultSpan, Month, Range};
use jiff::{Timestamp, tz::TimeZone};
use serde::Deserialize;

/// A month as written by an `<input type="month">`: `YYYY-MM`.
fn deserialize_optional_month<'de, D>(deserializer: D) -> Result<Option<Month>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw: Option<String> = Deserialize::deserialize(deserializer)?;

    match raw.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        None => Ok(None),
        Some(s) => Month::parse(s)
            .map(Some)
            .ok_or_else(|| serde::de::Error::custom(format!("not a YYYY-MM month: {s}"))),
    }
}

#[derive(Debug, Default, Deserialize)]
pub struct RangeQuery {
    #[serde(default, deserialize_with = "deserialize_optional_month")]
    pub from: Option<Month>,
    #[serde(default, deserialize_with = "deserialize_optional_month")]
    pub to: Option<Month>,
}

impl RangeQuery {
    /// The range asked for, with a missing end filled in from `today` and
    /// the report's `span`.
    pub fn range(&self, today: jiff::civil::Date, span: DefaultSpan) -> Range {
        let this_month = Month::new(i32::from(today.year()), today.month() as u32)
            .expect("jiff months are 1-12");
        Range::resolve(self.from, self.to, this_month, span)
    }
}

/// Today, in UTC, which is how both sites date their months.
pub fn today() -> jiff::civil::Date {
    Timestamp::now().to_zoned(TimeZone::UTC).date()
}

/// When a report was computed, as its page says: `2 Oct 2026, 19:05 UTC`.
pub fn generated() -> String {
    Timestamp::now()
        .to_zoned(TimeZone::UTC)
        .strftime("%-d %b %Y, %H:%M UTC")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn months_parse_from_month_inputs() {
        let query: RangeQuery = serde_urlencoded::from_str("from=2025-01&to=2026-09").unwrap();
        assert_eq!(query.from, Month::new(2025, 1));
        assert_eq!(query.to, Month::new(2026, 9));

        let empty: RangeQuery = serde_urlencoded::from_str("from=&to=").unwrap();
        assert_eq!(empty.from, None);

        assert!(serde_urlencoded::from_str::<RangeQuery>("from=2025-13").is_err());
    }

    #[test]
    fn the_default_range_follows_todays_date_and_the_span() {
        let today = jiff::civil::date(2026, 10, 2);

        let since = RangeQuery::default().range(today, DefaultSpan::SinceJanuaryLastYear);
        assert_eq!(since.first, Month::new(2025, 1).unwrap());
        assert_eq!(since.last, Month::new(2026, 9).unwrap());

        let trailing = RangeQuery::default().range(today, DefaultSpan::Trailing(24));
        assert_eq!(trailing.first, Month::new(2024, 10).unwrap());
    }
}
