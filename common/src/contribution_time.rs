//! When a contribution a partner reports actually happened.
//!
//! Partners post the time of a contribution as a string, spelled however
//! their platform spells it, and log-service records every participation
//! posted to it through `recorded_contribution_time` so that what lands in the
//! table is always one canonical RFC 3339 instant. This is the counterpart of
//! orrery's `parse_contribution_time`, and the two read the same strings the
//! same way. The bridges, which fetch contributions rather than being sent
//! them, go through `recorded_platform_time` instead, and both are held to the
//! same ceiling by `not_after_arrival`.

use std::{fmt, str::FromStr};

use jiff::{SignedDuration, Timestamp, civil::DateTime, tz::TimeZone};

use crate::timestring::TimeString;

/// Neureka's project, which posts its timestamps wrong. See
/// `read_neureka_time`.
pub const NEUREKA: &str = "01JKXMFYFZ14AEQY4WN9GS6Z2B";

/// How far ahead of SciStarter a Neureka player's clock has been seen to run.
/// Most send within a minute of the moment they name; one ran an hour fast.
pub const NEUREKA_CLOCK_LEAD: SignedDuration = SignedDuration::from_hours(2);

/// A contribution time that names no moment we can place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnreadableTime(pub String);

impl fmt::Display for UnreadableTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unreadable contribution time {:?}", self.0)
    }
}

impl std::error::Error for UnreadableTime {}

/// Reads one sender's timestamps, given the moment the timestamp arrived.
/// `Ok(None)` means it is not that sender's mistake, and should be read like
/// anybody else's.
type SenderTimeReader = fn(&str, Timestamp) -> Result<Option<Timestamp>, UnreadableTime>;

/// The partners whose timestamps are wrong in a way that can be recognized
/// and mended, by the ULID of the project they post for.
fn sender_time_reader(sender: &str) -> Option<SenderTimeReader> {
    match sender {
        NEUREKA => Some(read_neureka_time),
        _ => None,
    }
}

/// When a Neureka contribution happened, from the timestamp Neureka sent.
///
/// Neureka formats its times with the month written as the minute and the
/// hour on a 12-hour clock with no AM/PM -- `yyyy-mm-ddThh:mm:ss`, where the
/// format meant `yyyy-MM-ddTHH:mm:ss` -- so `2026-10-19T03:10:47` says only
/// that the contribution was made at 3:10:47 or 15:10:47 on the 19th of some
/// month in 2026. The year, the day and the time within the half day are
/// right; the month and the half are gone. Neureka did not adopt the API
/// readily and cannot be counted on to fix it, so it is mended here.
///
/// The moment kept is the latest one the timestamp could name that is not
/// after `received`, allowing for `NEUREKA_CLOCK_LEAD`. Contributions arrive
/// within a minute or so of being made, a few within hours, so the latest is
/// the real one unless the contribution took more than ten hours to arrive;
/// then it is twelve hours late, but still on the right day.
///
/// `Ok(None)` when `when` is not in that shape: a minute that differs from
/// the month, or an hour outside 1-12, means it was written correctly.
/// `Err` when it is in that shape but names no moment before it arrived.
pub fn read_neureka_time(
    when: &str,
    received: Timestamp,
) -> Result<Option<Timestamp>, UnreadableTime> {
    let Some((year, month, day, hour, minute, second)) = timestamp_fields(when) else {
        return Ok(None);
    };

    if month != minute || !(1..=12).contains(&hour) {
        return Ok(None);
    }

    let latest = received
        .saturating_add(NEUREKA_CLOCK_LEAD)
        .unwrap_or(received);

    (1..=12)
        .flat_map(|month| [0, 12].map(|half| (month, half)))
        .filter_map(|(month, half)| {
            // `new` refuses the 31st of a 30-day month, and the like.
            DateTime::new(year, month, day, hour % 12 + half, minute, second, 0)
                .ok()?
                .to_zoned(TimeZone::UTC)
                .ok()
                .map(|zoned| zoned.timestamp())
        })
        .filter(|candidate| *candidate <= latest)
        .max()
        .map(Some)
        .ok_or_else(|| UnreadableTime(when.to_string()))
}

/// The six numbers of a `YYYY-MM-DDTHH:MM:SS` timestamp, whether or not they
/// make a date.
fn timestamp_fields(when: &str) -> Option<(i16, i8, i8, i8, i8, i8)> {
    let (date, clock) = when.split_once('T')?;

    let mut date = date.split('-');
    let year = date.next()?.parse().ok()?;
    let month = date.next()?.parse().ok()?;
    let day = date.next()?.parse().ok()?;

    let mut clock = clock.split(':');
    let hour = clock.next()?.parse().ok()?;
    let minute = clock.next()?.parse().ok()?;
    let second = clock.next()?.parse().ok()?;

    if date.next().is_some() || clock.next().is_some() {
        return None;
    }

    Some((year, month, day, hour, minute, second))
}

/// When a contribution happened, from however its sender spelled it.
///
/// RFC 3339 with a `Z` or an offset names an instant; a bare
/// `YYYY-MM-DDTHH:MM:SS`, with or without fractional seconds, is taken as
/// UTC, as orrery takes it. `sender` is the ULID of the project the time was
/// posted for, and `received` when it arrived: a sender with a known mistake
/// gets its own reader first, since its mistakes can otherwise parse as a
/// different, wrong moment.
pub fn parse_contribution_time(
    when: &str,
    sender: &str,
    received: Timestamp,
) -> Result<Timestamp, UnreadableTime> {
    if let Some(reader) = sender_time_reader(sender)
        && let Some(mended) = reader(when, received)?
    {
        return Ok(mended);
    }

    if let Ok(instant) = Timestamp::from_str(when) {
        return Ok(instant);
    }

    DateTime::from_str(when)
        .ok()
        .and_then(|civil| civil.to_zoned(TimeZone::UTC).ok())
        .map(|zoned| zoned.timestamp())
        .ok_or_else(|| UnreadableTime(when.to_string()))
}

/// The time to record a contribution under, as received just now.
///
/// No time at all means now. So does an unreadable one: a contribution
/// recorded at the wrong moment still beats one dropped on the floor, but the
/// substitution is said out loud, because a silent one is how every GLOBE
/// contribution orrery recorded came to be dated on delivery for years.
///
/// So does a time after now; see `not_after_arrival`.
pub fn recorded_contribution_time(when: Option<&str>, sender: &str) -> TimeString {
    let received = Timestamp::now();

    let Some(when) = when else {
        return received.into();
    };

    match parse_contribution_time(when, sender, received) {
        Ok(instant) => not_after_arrival(instant, received, sender).into(),
        Err(error) => {
            eprintln!("{error} posted for {sender}; recording it as now instead");
            received.into()
        }
    }
}

/// The time a contribution reported for `reported` is recorded under, given
/// that the report arrived at `received`.
///
/// Nobody participates in the future, so a time after the report's own
/// arrival is a data error -- a mistyped year, a sender's broken clock -- and
/// the best we know is that it happened by then. This is not
/// `parse_contribution_time`'s business: that says what moment the sender
/// meant, and a sender can mean one that has not come yet.
///
/// Every writer of participation applies this, whether the time was posted
/// to us (`recorded_contribution_time`) or fetched by a bridge
/// (`recorded_platform_time`), which is what makes it a ceiling rather than a
/// convention. Participation synced from orrery is the exception, because
/// orrery applies the same ceiling at its own intake.
pub fn not_after_arrival(reported: Timestamp, received: Timestamp, sender: &str) -> Timestamp {
    if reported <= received {
        return reported;
    }

    eprintln!(
        "Contribution time {reported} posted for {sender} is in the future; recording it as now"
    );

    received
}

/// A contribution time a bridge read from a platform, spelled `spelled` and
/// fetched at `received`: the moment it is recorded as, and how that moment
/// is written in the table.
///
/// The platform's own spelling is kept whenever the time stands, because the
/// bridges pass it through untouched -- iNaturalist's carries the observer's
/// offset, which `common::linked_platform` explains. Only a time in the future
/// is replaced, by the arrival, as `not_after_arrival` says.
pub fn recorded_platform_time(
    spelled: &str,
    received: Timestamp,
    sender: &str,
) -> Result<(Timestamp, String), UnreadableTime> {
    let reported = Timestamp::from_str(spelled).map_err(|_| UnreadableTime(spelled.to_string()))?;
    let recorded = not_after_arrival(reported, received, sender);

    if recorded == reported {
        Ok((recorded, spelled.to_string()))
    } else {
        Ok((recorded, recorded.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use assert2::check;

    use super::*;

    const SOMEONE_ELSE: &str = "01JKXMFYFZ00000000000000ZZ";

    fn at(when: &str) -> Timestamp {
        when.parse().unwrap()
    }

    fn neureka(when: &str, received: &str) -> Result<Timestamp, UnreadableTime> {
        parse_contribution_time(when, NEUREKA, at(received))
    }

    #[test]
    fn every_spelling_a_platform_sends_is_read() {
        let observed = at("2026-08-21T18:47:44Z");
        let received = at("2026-10-19T12:00:00Z");

        for spelled in [
            "2026-08-21T18:47:44Z",
            "2026-08-21T14:47:44-04:00",
            "2026-08-21T18:47:44",
            "2026-08-21T18:47:44.000000",
        ] {
            check!(parse_contribution_time(spelled, SOMEONE_ELSE, received) == Ok(observed));
        }
    }

    #[test]
    fn something_that_is_not_a_time_is_refused_rather_than_guessed() {
        let received = at("2026-10-19T12:00:00Z");

        check!(parse_contribution_time("last Tuesday", SOMEONE_ELSE, received).is_err());
    }

    #[test]
    fn a_morning_neureka_contribution_is_put_in_the_month_it_arrived() {
        check!(
            neureka("2026-10-19T03:10:47", "2026-10-19T03:11:30Z")
                == Ok(at("2026-10-19T03:10:47Z"))
        );
    }

    #[test]
    fn an_afternoon_neureka_contribution_is_put_in_the_afternoon() {
        check!(
            neureka("2026-09-18T09:09:02", "2026-10-18T21:10:00Z")
                == Ok(at("2026-10-18T21:09:02Z"))
        );
    }

    #[test]
    fn a_month_no_calendar_has_is_read_too() {
        // Sent at 37 minutes past, so its month is 37.
        check!(
            neureka("2026-37-26T11:37:48", "2026-10-26T23:38:10Z")
                == Ok(at("2026-10-26T23:37:48Z"))
        );
    }

    #[test]
    fn twelve_is_noon_or_midnight() {
        check!(
            neureka("2026-09-14T12:09:56", "2026-10-14T00:10:00Z")
                == Ok(at("2026-10-14T00:09:56Z"))
        );
        check!(
            neureka("2026-09-14T12:09:56", "2026-10-14T12:10:00Z")
                == Ok(at("2026-10-14T12:09:56Z"))
        );
    }

    #[test]
    fn a_neureka_contribution_hours_late_keeps_its_own_time() {
        check!(
            neureka("2026-02-15T03:02:08", "2026-10-15T06:00:00Z")
                == Ok(at("2026-10-15T03:02:08Z"))
        );
    }

    #[test]
    fn a_clock_running_ahead_is_allowed_for() {
        // One player's clock ran an hour fast.
        check!(
            neureka("2026-05-19T01:05:40", "2026-10-19T12:05:00Z")
                == Ok(at("2026-10-19T13:05:40Z"))
        );
    }

    #[test]
    fn the_day_before_a_new_month_stays_in_the_old_one() {
        check!(
            neureka("2026-58-30T11:58:30", "2026-11-01T00:00:10Z")
                == Ok(at("2026-10-30T23:58:30Z"))
        );
    }

    #[test]
    fn the_last_moments_of_a_year_stay_in_that_year() {
        check!(
            neureka("2026-58-31T11:58:30", "2027-01-01T00:00:10Z")
                == Ok(at("2026-12-31T23:58:30Z"))
        );
    }

    #[test]
    fn a_correctly_written_neureka_time_is_read_as_written() {
        // Should Neureka ever fix it: a minute that is not the month, or an
        // hour past 12, cannot be the mistake.
        let received = "2026-10-19T15:23:00Z";

        check!(neureka("2026-10-19T15:22:01", received) == Ok(at("2026-10-19T15:22:01Z")));
        check!(neureka("2026-10-19T03:22:01Z", received) == Ok(at("2026-10-19T03:22:01Z")));
    }

    #[test]
    fn the_mistake_naming_no_moment_before_it_arrived_is_refused() {
        check!(neureka("2027-05-03T04:05:00", "2026-10-19T12:00:00Z").is_err());
    }

    #[test]
    fn nobody_else_is_read_the_neureka_way() {
        let received = at("2026-10-19T03:11:30Z");

        check!(
            parse_contribution_time("2026-10-19T03:10:47", SOMEONE_ELSE, received)
                == Ok(at("2026-10-19T03:10:47Z"))
        );
        check!(parse_contribution_time("2026-37-26T11:37:48", SOMEONE_ELSE, received).is_err());
    }

    #[test]
    fn a_time_up_to_its_arrival_stands() {
        let received = at("2026-04-28T15:00:00Z");

        for reported in ["2026-04-28T14:59:59Z", "2026-04-28T15:00:00Z"] {
            check!(not_after_arrival(at(reported), received, SOMEONE_ELSE) == at(reported));
        }
    }

    #[test]
    fn a_neureka_time_ahead_of_its_arrival_is_meant_but_not_recorded() {
        // The clock lead lets the mended reading land a little ahead of the
        // arrival: that is the moment Neureka meant, and the arrival is the
        // moment recorded.
        let received = at("2026-10-19T12:05:00Z");
        let meant = neureka("2026-05-19T01:05:40", "2026-10-19T12:05:00Z").unwrap();

        check!(meant == at("2026-10-19T13:05:40Z"));
        check!(not_after_arrival(meant, received, NEUREKA) == received);
    }

    #[test]
    fn a_platform_time_that_stands_keeps_its_spelling() {
        let received = at("2026-10-19T12:00:00Z");

        check!(
            recorded_platform_time("2026-08-21T14:47:44-04:00", received, SOMEONE_ELSE)
                == Ok((
                    at("2026-08-21T18:47:44Z"),
                    "2026-08-21T14:47:44-04:00".to_string()
                ))
        );
    }

    #[test]
    fn a_future_platform_time_is_rewritten_as_its_arrival() {
        let received = at("2026-10-19T12:00:00Z");

        check!(
            recorded_platform_time("2027-08-21T14:47:44-04:00", received, SOMEONE_ELSE)
                == Ok((received, "2026-10-19T12:00:00Z".to_string()))
        );
    }

    #[test]
    fn an_unreadable_platform_time_is_refused() {
        let received = at("2026-10-19T12:00:00Z");

        check!(recorded_platform_time("yesterday", received, SOMEONE_ELSE).is_err());
    }

    #[test]
    fn a_missing_unreadable_or_future_time_is_recorded_as_now() {
        // The future one is a mistyped year Nature's Notebook once sent.
        for when in [None, Some("last Tuesday"), Some("2027-04-02T12:00:00Z")] {
            let recorded =
                Timestamp::try_from(recorded_contribution_time(when, SOMEONE_ELSE)).unwrap();

            check!(recorded.duration_until(Timestamp::now()).abs() < SignedDuration::from_mins(1));
        }
    }
}
