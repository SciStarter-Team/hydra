use jiff::{Timestamp, civil, tz::TimeZone};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeFormat {
    Date,
    ShortDate,
    Time,
    DateTime,
    Month,
    Day,
    Year,
}

impl TimeFormat {
    fn as_str(self) -> &'static str {
        match self {
            TimeFormat::Date => "date",
            TimeFormat::ShortDate => "short-date",
            TimeFormat::Time => "time",
            TimeFormat::DateTime => "datetime",
            TimeFormat::Month => "month",
            TimeFormat::Day => "day",
            TimeFormat::Year => "year",
        }
    }
}

impl fmt::Display for TimeFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

fn parse_timestamp(input: &str) -> Option<Timestamp> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Ok(ts) = trimmed.parse::<Timestamp>() {
        return Some(ts);
    }
    let iso_like = trimmed.replacen(' ', "T", 1);
    if let Ok(ts) = format!("{iso_like}Z").parse::<Timestamp>() {
        return Some(ts);
    }
    if let Ok(dt) = iso_like.parse::<civil::DateTime>() {
        if let Ok(zoned) = dt.to_zoned(TimeZone::UTC) {
            return Some(zoned.timestamp());
        }
    }
    None
}

/// How `format` writes `ts` in UTC, the text a reader without scripts sees.
fn fallback(ts: Timestamp, format: TimeFormat) -> String {
    let utc = ts.to_zoned(TimeZone::UTC);
    match format {
        TimeFormat::Date => utc.strftime("%B %-d, %Y").to_string(),
        TimeFormat::ShortDate => utc.strftime("%b %-d, %Y").to_string(),
        TimeFormat::Time => utc.strftime("%-I:%M %p").to_string(),
        TimeFormat::DateTime => utc.strftime("%B %-d, %Y, %-I:%M %p").to_string(),
        TimeFormat::Month => utc.strftime("%b").to_string().to_uppercase(),
        TimeFormat::Day => utc.strftime("%-d").to_string(),
        TimeFormat::Year => utc.strftime("%Y").to_string(),
    }
}

/// Render an HTML `<time>` element whose text content will be replaced at
/// render time (in the browser) with a locale- and timezone-appropriate
/// string. The text content emitted by the server is a UTC fallback so that
/// crawlers and no-JS clients still see a readable value.
pub fn local_time(input: &str, format: TimeFormat) -> String {
    let Some(ts) = parse_timestamp(input) else {
        return String::new();
    };
    let iso = ts.to_string();
    let fallback = fallback(ts, format);
    format!(r#"<time datetime="{iso}" data-format="{format}">{fallback}</time>"#)
}

/// A span of time, from `begin` to `end`, as [`local_time`] renders one
/// moment. `local_time.js` writes it with `Intl.DateTimeFormat.formatRange`,
/// which writes what the two ends share -- the day, say -- only once.
///
/// Orrery's exchange writes event sessions the same way
/// (`local_time_range` in its src/module/exchange/event.rs).
pub fn local_time_range(begin: &str, end: &str, format: TimeFormat) -> String {
    let (Some(begin), Some(end)) = (parse_timestamp(begin), parse_timestamp(end)) else {
        return String::new();
    };
    format!(
        r#"<time datetime="{begin}" data-end="{end}" data-format="{format}">{} &ndash; {}</time>"#,
        fallback(begin, format),
        fallback(end, format)
    )
}

/// Whose clocks a time is told on.
#[derive(Debug, Clone, Copy)]
pub enum Clocks<'a> {
    /// The viewer's own, as the browser keeps them.
    Viewer,
    /// A place's: an event's venue, say.
    Zone(&'a TimeZone),
}

impl<'a> Clocks<'a> {
    /// The clocks of `zone`, a place's, or the viewer's when it has none: an
    /// event's dates are told on its venue's, and an event without one (held
    /// online, say) on its viewer's.
    pub fn of(zone: Option<&'a TimeZone>) -> Self {
        zone.map_or(Clocks::Viewer, Clocks::Zone)
    }

    /// The `data-` attributes telling `local_time.js` which clocks to use,
    /// and to name them.
    fn attributes(self) -> String {
        match self {
            Clocks::Viewer => " data-named-zone".to_string(),
            Clocks::Zone(tz) => match tz.iana_name() {
                Some(name) => format!(r#" data-named-zone data-zone="{name}""#),
                None => " data-named-zone".to_string(),
            },
        }
    }

    /// `ts` as `format` writes it on these clocks, naming the zone, for a
    /// reader without scripts: on a place's clocks as the browser would, and
    /// the viewer's, unknown here, as UTC.
    fn fallback(self, ts: Timestamp, format: TimeFormat) -> String {
        let zoned = match self {
            Clocks::Viewer => ts.to_zoned(TimeZone::UTC),
            Clocks::Zone(tz) => ts.to_zoned(tz.clone()),
        };
        let pattern = match format {
            TimeFormat::Date => "%B %-d, %Y",
            TimeFormat::ShortDate => "%b %-d, %Y",
            TimeFormat::Time => "%-I:%M %p %Z",
            TimeFormat::DateTime => "%B %-d, %Y, %-I:%M %p %Z",
            TimeFormat::Month => return zoned.strftime("%b").to_string().to_uppercase(),
            TimeFormat::Day => "%-d",
            TimeFormat::Year => "%Y",
        };
        zoned.strftime(pattern).to_string()
    }
}

/// A moment told on `clocks`, naming their zone ("March 5, 2027, 9:30 AM
/// EST"), as [`local_time`] tells one on the viewer's.
pub fn zoned_time(input: &str, format: TimeFormat, clocks: Clocks<'_>) -> String {
    let Some(ts) = parse_timestamp(input) else {
        return String::new();
    };
    format!(
        r#"<time datetime="{ts}" data-format="{format}"{}>{}</time>"#,
        clocks.attributes(),
        clocks.fallback(ts, format)
    )
}

/// A span told on `clocks`, as [`zoned_time`] tells a moment. A span that
/// ends as it begins is a moment, an event with no end.
pub fn zoned_time_range(begin: &str, end: &str, format: TimeFormat, clocks: Clocks<'_>) -> String {
    let (Some(begin_ts), Some(end_ts)) = (parse_timestamp(begin), parse_timestamp(end)) else {
        return String::new();
    };
    if begin_ts == end_ts {
        return zoned_time(begin, format, clocks);
    }
    format!(
        r#"<time datetime="{begin_ts}" data-end="{end_ts}" data-format="{format}"{}>{} &ndash; {}</time>"#,
        clocks.attributes(),
        clocks.fallback(begin_ts, format),
        clocks.fallback(end_ts, format)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_venues_time_names_its_zone() {
        let tz = TimeZone::get("America/New_York").unwrap();
        assert_eq!(
            zoned_time(
                "2027-03-05T14:30:00Z",
                TimeFormat::DateTime,
                Clocks::Zone(&tz)
            ),
            r#"<time datetime="2027-03-05T14:30:00Z" data-format="datetime" data-named-zone data-zone="America/New_York">March 5, 2027, 9:30 AM EST</time>"#
        );
        // Daylight time, by the date.
        assert!(
            zoned_time(
                "2027-07-05T14:30:00Z",
                TimeFormat::DateTime,
                Clocks::Zone(&tz)
            )
            .contains("10:30 AM EDT")
        );
    }

    #[test]
    fn a_span_with_no_end_is_a_moment() {
        assert_eq!(
            zoned_time_range(
                "2027-03-05T14:30:00Z",
                "2027-03-05T14:30:00Z",
                TimeFormat::DateTime,
                Clocks::Viewer
            ),
            r#"<time datetime="2027-03-05T14:30:00Z" data-format="datetime" data-named-zone>March 5, 2027, 2:30 PM UTC</time>"#
        );
    }

    #[test]
    fn a_range_carries_both_instants_and_a_utc_fallback() {
        assert_eq!(
            local_time_range(
                "2027-03-05T21:30:00Z",
                "2027-03-05T23:00:00Z",
                TimeFormat::DateTime
            ),
            r#"<time datetime="2027-03-05T21:30:00Z" data-end="2027-03-05T23:00:00Z" data-format="datetime">March 5, 2027, 9:30 PM &ndash; March 5, 2027, 11:00 PM</time>"#
        );
    }

    #[test]
    fn a_range_with_an_unreadable_end_is_left_out() {
        assert_eq!(
            local_time_range("2027-03-05T21:30:00Z", "soon", TimeFormat::DateTime),
            ""
        );
    }
}
