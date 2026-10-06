//! The Library Network's size, as recorded in the program's own tracker.
//!
//! **Shared file.** Byte-identical in orrery (`src/staff/moore/tracker.rs`)
//! and hydra (`frontend/src/staff/moore/tracker.rs`). Change both copies
//! together.
//!
//! The grant's "# libraries" indicator counts a network that existed before
//! SciStarter's sign-up form did, and grows through channels the platform
//! never sees — conferences, partner lists, outreach. The figure the program
//! reports is kept by hand in the "Moore Grant Data & Project Timeline" sheet,
//! one column per update, so the ledger reads it from there rather than
//! computing a different number of its own.
//!
//! The sheet is shared with anyone who has the link, read-only, so its Data
//! tab can be fetched as CSV from [`CSV_URL`] without credentials. Each site
//! does the fetching; everything after that is here.

use std::fmt;

/// Where staff read and edit it.
pub const VIEW_URL: &str =
    "https://docs.google.com/spreadsheets/d/1H6ah5DJbAGk_tKc7Jfx3v7TYjpyFr5q8j2HvRzV7lJ4/edit";

/// Its Data tab (`gid=0`) as CSV, exactly as the cells read.
pub const CSV_URL: &str = "https://docs.google.com/spreadsheets/d/1H6ah5DJbAGk_tKc7Jfx3v7TYjpyFr5q8j2HvRzV7lJ4/export?format=csv&gid=0";

/// The Data tab row holding the library count, by its "Deliverable" cell.
const LIBRARIES_ROW: &str = "Number of libraries";

/// The column holding each row's deliverable name.
const DELIVERABLE_COLUMN: usize = 1;

/// A calendar day.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Day {
    pub year: i32,
    /// 1 through 12.
    pub month: u32,
    pub day: u32,
}

/// One update in the tracker: the network's size as of that day.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    pub day: Day,
    pub count: i64,
}

/// Why the tracker could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrackerError {
    /// The CSV had no rows at all.
    Empty,
    /// No row's deliverable is "Number of libraries".
    NoLibrariesRow,
    /// A dated heading came before any heading that names its year, so its
    /// year can't be known.
    UndatableHeading(String),
    /// A dated column holds something other than a count.
    NotACount { heading: String, value: String },
}

impl fmt::Display for TrackerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TrackerError::Empty => f.write_str("the Data tab is empty"),
            TrackerError::NoLibrariesRow => {
                write!(f, "the Data tab has no “{LIBRARIES_ROW}” row")
            }
            TrackerError::UndatableHeading(heading) => {
                write!(f, "the heading “{heading}” has no year to count from")
            }
            TrackerError::NotACount { heading, value } => {
                write!(f, "“{value}” under “{heading}” is not a count")
            }
        }
    }
}

impl std::error::Error for TrackerError {}

/// Splits CSV text into rows of fields, per RFC 4180: fields may be quoted,
/// quoted fields may hold commas, line breaks, and doubled quotes.
fn parse_csv(text: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        if quoted {
            match ch {
                '"' if chars.peek() == Some(&'"') => {
                    field.push('"');
                    chars.next();
                }
                '"' => quoted = false,
                _ => field.push(ch),
            }
            continue;
        }

        match ch {
            '"' => quoted = true,
            ',' => row.push(std::mem::take(&mut field)),
            '\r' if chars.peek() == Some(&'\n') => {}
            '\n' | '\r' => {
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
            }
            _ => field.push(ch),
        }
    }

    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }

    rows
}

/// The month a heading word names, from its first three letters, so that
/// "Dec", "December" and "Sept" are all understood.
fn month_of(word: &str) -> Option<u32> {
    const MONTHS: [&str; 12] = [
        "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
    ];

    let prefix: String = word
        .chars()
        .take(3)
        .collect::<String>()
        .to_ascii_lowercase();
    MONTHS
        .iter()
        .position(|m| *m == prefix)
        .map(|index| index as u32 + 1)
}

/// A heading of the form "Month day" or "Month day, year": the month, the
/// day, and the year if it says one. Anything else is not a date heading.
fn heading_date(heading: &str) -> Option<(u32, u32, Option<i32>)> {
    let (date, year) = match heading.split_once(',') {
        Some((date, year)) => (date, Some(year.trim().parse::<i32>().ok()?)),
        None => (heading, None),
    };

    let mut words = date.split_whitespace();
    let month = month_of(words.next()?)?;
    let day: u32 = words.next()?.parse().ok()?;

    if words.next().is_some() || !(1..=31).contains(&day) {
        return None;
    }

    Some((month, day, year))
}

/// The day each dated column of the header stands for, by index. A heading
/// without a year takes the previous dated column's year, moving to the next
/// year when its month comes earlier than that column's, as the weekly
/// columns do on passing December.
fn column_days(header: &[String]) -> Result<Vec<(usize, Day)>, TrackerError> {
    let mut days = Vec::new();
    let mut previous: Option<Day> = None;

    for (index, heading) in header.iter().enumerate() {
        let Some((month, day, year)) = heading_date(heading.trim()) else {
            continue;
        };

        let year = match (year, previous) {
            (Some(year), _) => year,
            (None, Some(last)) if month < last.month => last.year + 1,
            (None, Some(last)) => last.year,
            (None, None) => return Err(TrackerError::UndatableHeading(heading.clone())),
        };

        let this = Day { year, month, day };
        days.push((index, this));
        previous = Some(this);
    }

    Ok(days)
}

/// Every update to the library count in the Data tab's CSV, oldest first.
/// Columns not yet filled in are skipped.
pub fn libraries(csv: &str) -> Result<Vec<Entry>, TrackerError> {
    let rows = parse_csv(csv);
    let header = rows.first().ok_or(TrackerError::Empty)?;
    let days = column_days(header)?;

    let row = rows
        .iter()
        .find(|row| {
            row.get(DELIVERABLE_COLUMN)
                .is_some_and(|cell| cell.trim().eq_ignore_ascii_case(LIBRARIES_ROW))
        })
        .ok_or(TrackerError::NoLibrariesRow)?;

    let mut entries = Vec::new();

    for (index, day) in days {
        let raw = row.get(index).map(|cell| cell.trim()).unwrap_or_default();

        if raw.is_empty() {
            continue;
        }

        let digits: String = raw.chars().filter(|ch| *ch != ',').collect();
        let count = digits.parse::<i64>().map_err(|_| TrackerError::NotACount {
            heading: header[index].trim().to_string(),
            value: raw.to_string(),
        })?;

        entries.push(Entry { day, count });
    }

    entries.sort_by_key(|entry| entry.day);
    Ok(entries)
}

/// The network's size as of `day`: the latest update on or before it, or
/// None before the first.
pub fn as_of(entries: &[Entry], day: Day) -> Option<i64> {
    entries
        .iter()
        .filter(|entry| entry.day <= day)
        .max_by_key(|entry| entry.day)
        .map(|entry| entry.count)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Data tab's header and libraries row as the sheet held them on
    /// 2 October 2026, abridged.
    const SAMPLE: &str = "Objective,Deliverable,2028 Goal,2028 Progress%,2026 Goal,2026 Progress%,\"Dec 31, 2024\",\"Dec 31, 2025\",April 1,July 6,July 13,September 28,October 5,December 27,January 3\r\n\
\"1: Expand and continue to co-create the library Ambassadors program, through resources\",Number of Champions,4000,16.93%,1500,45.13%,176,435,483,534,,677,,,\r\n\
2: Broaden engagement with and expansion of libraries,Number of libraries,2000,77.00%,1500,102.67%,908,\"1,146\",1316,1405,1408,1540,,,\r\n";

    fn day(year: i32, month: u32, day: u32) -> Day {
        Day { year, month, day }
    }

    #[test]
    fn the_library_row_is_read_with_its_dates() {
        let entries = libraries(SAMPLE).unwrap();

        assert_eq!(
            entries,
            [
                Entry {
                    day: day(2024, 12, 31),
                    count: 908
                },
                Entry {
                    day: day(2025, 12, 31),
                    count: 1146
                },
                Entry {
                    day: day(2026, 4, 1),
                    count: 1316
                },
                Entry {
                    day: day(2026, 7, 6),
                    count: 1405
                },
                Entry {
                    day: day(2026, 7, 13),
                    count: 1408
                },
                Entry {
                    day: day(2026, 9, 28),
                    count: 1540
                },
            ]
        );
    }

    #[test]
    fn headings_without_a_year_continue_past_december() {
        let header: Vec<String> = [
            "Deliverable",
            "Dec 31, 2025",
            "April 1",
            "December 27",
            "January 3",
        ]
        .into_iter()
        .map(String::from)
        .collect();

        let days: Vec<Day> = column_days(&header)
            .unwrap()
            .into_iter()
            .map(|(_, d)| d)
            .collect();
        assert_eq!(
            days,
            [
                day(2025, 12, 31),
                day(2026, 4, 1),
                day(2026, 12, 27),
                day(2027, 1, 3)
            ]
        );
    }

    #[test]
    fn goal_and_progress_columns_are_not_dates() {
        for heading in [
            "2028 Goal",
            "2026 Progress%",
            "Objective",
            "Deliverable",
            "May the 4th",
        ] {
            assert_eq!(heading_date(heading), None, "{heading}");
        }
        assert_eq!(heading_date("Sept 7"), Some((9, 7, None)));
        assert_eq!(heading_date("Dec 31, 2024"), Some((12, 31, Some(2024))));
    }

    #[test]
    fn a_heading_with_no_year_to_follow_is_an_error() {
        let csv = "Objective,Deliverable,April 1\n,Number of libraries,1316\n";
        assert_eq!(
            libraries(csv),
            Err(TrackerError::UndatableHeading(String::from("April 1")))
        );
    }

    #[test]
    fn a_value_that_is_not_a_count_is_an_error_not_a_guess() {
        let csv = "Objective,Deliverable,\"Dec 31, 2025\"\n,Number of libraries,about 1100\n";
        assert_eq!(
            libraries(csv),
            Err(TrackerError::NotACount {
                heading: String::from("Dec 31, 2025"),
                value: String::from("about 1100"),
            })
        );
    }

    #[test]
    fn a_tab_without_the_library_row_is_an_error() {
        let csv = "Objective,Deliverable,\"Dec 31, 2025\"\n,Number of Champions,435\n";
        assert_eq!(libraries(csv), Err(TrackerError::NoLibrariesRow));
        assert_eq!(libraries(""), Err(TrackerError::Empty));
    }

    #[test]
    fn the_count_as_of_a_day_is_the_latest_update_by_then() {
        let entries = libraries(SAMPLE).unwrap();

        assert_eq!(as_of(&entries, day(2024, 11, 30)), None);
        assert_eq!(as_of(&entries, day(2024, 12, 31)), Some(908));
        assert_eq!(as_of(&entries, day(2026, 1, 31)), Some(1146));
        assert_eq!(as_of(&entries, day(2026, 7, 12)), Some(1405));
        assert_eq!(as_of(&entries, day(2026, 10, 2)), Some(1540));
    }

    #[test]
    fn quoted_fields_keep_their_commas_quotes_and_line_breaks() {
        let rows = parse_csv("a,\"b, c\",\"say \"\"hi\"\"\"\r\n\"two\nlines\",x\n");
        assert_eq!(
            rows,
            [
                vec![
                    String::from("a"),
                    String::from("b, c"),
                    String::from("say \"hi\"")
                ],
                vec![String::from("two\nlines"), String::from("x")],
            ]
        );
    }
}
