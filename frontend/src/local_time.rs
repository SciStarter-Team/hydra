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

/// Render an HTML `<time>` element whose text content will be replaced at
/// render time (in the browser) with a locale- and timezone-appropriate
/// string. The text content emitted by the server is a UTC fallback so that
/// crawlers and no-JS clients still see a readable value.
pub fn local_time(input: &str, format: TimeFormat) -> String {
    let Some(ts) = parse_timestamp(input) else {
        return String::new();
    };
    let iso = ts.to_string();
    let utc = ts.to_zoned(TimeZone::UTC);
    let fallback = match format {
        TimeFormat::Date => utc.strftime("%B %-d, %Y").to_string(),
        TimeFormat::ShortDate => utc.strftime("%b %-d, %Y").to_string(),
        TimeFormat::Time => utc.strftime("%-I:%M %p").to_string(),
        TimeFormat::DateTime => utc.strftime("%B %-d, %Y, %-I:%M %p").to_string(),
        TimeFormat::Month => utc.strftime("%b").to_string().to_uppercase(),
        TimeFormat::Day => utc.strftime("%-d").to_string(),
        TimeFormat::Year => utc.strftime("%Y").to_string(),
    };
    format!(r#"<time datetime="{iso}" data-format="{format}">{fallback}</time>"#)
}
