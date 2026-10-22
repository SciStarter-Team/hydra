//! When participation from a linked outside account starts counting.
//!
//! SciStarter stopped backfilling participation from a linked account in
//! August 2026: what somebody did on iNaturalist or Zooniverse before
//! connecting it here is no longer imported, only what they do afterwards.
//! The connection time is the floor, and both bridges measure against it
//! through this module rather than each deciding for itself.

use std::str::FromStr;

use jiff::{Span, Timestamp};
use serde::{Deserialize, Serialize};

/// An outside platform whose participation is credited to an account once
/// the account names its username there. Mirrors orrery's `LinkedPlatform`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LinkedPlatform {
    INaturalist,
    Zooniverse,
}

impl LinkedPlatform {
    pub fn label(self) -> &'static str {
        match self {
            LinkedPlatform::INaturalist => "iNaturalist",
            LinkedPlatform::Zooniverse => "Zooniverse",
        }
    }

    /// Whether a username has to be given exactly as the platform spells it.
    ///
    /// iNaturalist's observation search matches a login regardless of case.
    /// A Zooniverse classification is matched to an account by comparing the
    /// Panoptes login with the stored username exactly, so a username that
    /// differs only in capitalization is never credited.
    pub fn username_case_sensitive(self) -> bool {
        match self {
            LinkedPlatform::INaturalist => false,
            LinkedPlatform::Zooniverse => true,
        }
    }
}

/// The date the policy took effect.
///
/// Accounts connected before then have no recorded connection time -- it was
/// never a column -- so they are floored here instead. What was imported for
/// them before the cutover stays; nothing older arrives after it.
pub const RETROACTIVE_IMPORT_CUTOVER: &str = "2026-08-01T00:00:00Z";

/// How far below its connection time a newly linked account still counts.
///
/// iNaturalist stamps an observation with the observer's own reading of the
/// clock rather than with a UTC instant -- orrery keeps it that way on
/// purpose, in `bridge/platforms.py::parse_inat_timestamp` -- while the
/// connection time is a real instant. For somebody west of UTC the two
/// disagree by their offset, so a participant who connects their account and
/// then spends the day recording would find that day falling below the
/// floor. Twelve hours is the westmost offset there is, which makes this
/// wide enough to cover the discrepancy rather than a guess at it.
///
/// Only a recorded connection time is given the allowance. The cutover is a
/// whole-day boundary picked months after the fact, and nothing is measured
/// against it closely enough to need one.
pub const RETROACTIVE_IMPORT_GRACE_HOURS: i64 = 12;

/// The earliest activity on a linked platform that counts toward an account.
///
/// `linked_at` is when the account named its username on that platform,
/// less `RETROACTIVE_IMPORT_GRACE_HOURS`. `None` means the connection
/// predates the policy, which is also where an unparseable stored value
/// lands: the cutover is the documented answer for "we do not know when this
/// was connected", and it stands exactly where it is written.
pub fn import_floor(linked_at: Option<&str>) -> Timestamp {
    let cutover = Timestamp::from_str(RETROACTIVE_IMPORT_CUTOVER)
        .expect("RETROACTIVE_IMPORT_CUTOVER is a valid timestamp");

    let Some(linked_at) = linked_at else {
        return cutover;
    };

    match Timestamp::from_str(linked_at) {
        Ok(linked) => linked
            .checked_sub(Span::new().hours(RETROACTIVE_IMPORT_GRACE_HOURS))
            .unwrap_or(linked),
        Err(e) => {
            eprintln!("Unparseable platform connection time {linked_at:?}: {e}");
            cutover
        }
    }
}

/// Whether activity at `when` counts toward an account connected at
/// `linked_at`.
///
/// An unreadable `when` does not count. The platforms report RFC 3339 and
/// both bridges pass it through untouched, so one that does not parse is a
/// record we cannot place in time -- and crediting it would mean crediting
/// something we cannot show falls after the connection.
pub fn credits_activity(linked_at: Option<&str>, when: &str) -> bool {
    match Timestamp::from_str(when) {
        Ok(when) => when >= import_floor(linked_at),
        Err(e) => {
            eprintln!("Unparseable platform activity time {when:?}: {e}");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINKED: &str = "2027-03-01T12:00:00Z";

    #[test]
    fn activity_well_before_the_connection_does_not_count() {
        assert!(!credits_activity(Some(LINKED), "2027-02-28T12:00:00Z"));
        assert!(credits_activity(Some(LINKED), "2027-03-02T12:00:00Z"));
    }

    #[test]
    fn a_newly_linked_account_is_given_grace_below_the_connection() {
        // Twelve hours below the connection, to the second: the allowance is
        // for a platform reporting a contribution in a frame we did not
        // stamp the connection in, not an open edge.
        assert!(credits_activity(Some(LINKED), "2027-03-01T00:00:01Z"));
        assert!(!credits_activity(Some(LINKED), "2027-02-28T23:59:59Z"));
    }

    #[test]
    fn the_connection_moment_itself_counts() {
        assert!(credits_activity(Some(LINKED), LINKED));
    }

    #[test]
    fn a_connection_predating_the_policy_floors_at_the_cutover() {
        // Exactly at the cutover, with no grace subtracted from it.
        assert_eq!(
            import_floor(None),
            Timestamp::from_str(RETROACTIVE_IMPORT_CUTOVER).unwrap()
        );

        assert!(!credits_activity(None, "2026-07-31T23:59:59Z"));
        assert!(credits_activity(None, "2026-08-02T00:00:00Z"));
    }

    #[test]
    fn offset_and_zulu_spellings_compare_alike() {
        // The platforms disagree about this: iNaturalist reports an offset,
        // Zooniverse a Z. One instant spelled both ways is what a string
        // comparison gets wrong -- '+' sorts before 'Z', so the activity
        // would read as earlier than the connection it is equal to.
        assert!(credits_activity(
            Some("2027-03-01T12:00:00Z"),
            "2027-03-01T12:00:00+00:00"
        ));
        assert!(!credits_activity(
            Some("2027-03-01T12:00:00+00:00"),
            "2027-02-28T12:00:00Z"
        ));
    }

    #[test]
    fn unreadable_activity_does_not_count() {
        assert!(!credits_activity(Some(LINKED), "sometime last spring"));
    }

    #[test]
    fn an_unreadable_connection_falls_back_to_the_cutover() {
        assert_eq!(import_floor(Some("whenever")), import_floor(None));
    }
}
