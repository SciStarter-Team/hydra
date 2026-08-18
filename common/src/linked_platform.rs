//! When participation from a linked outside account starts counting.
//!
//! SciStarter stopped backfilling participation from a linked account in
//! August 2026: what somebody did on iNaturalist or Zooniverse before
//! connecting it here is no longer imported, only what they do afterwards.
//! The connection time is the floor, and both bridges measure against it
//! through this module rather than each deciding for itself.

use std::str::FromStr;

use jiff::Timestamp;

/// The date the policy took effect.
///
/// Accounts connected before then have no recorded connection time -- it was
/// never a column -- so they are floored here instead. What was imported for
/// them before the cutover stays; nothing older arrives after it.
pub const RETROACTIVE_IMPORT_CUTOVER: &str = "2026-08-01T00:00:00Z";

/// The earliest activity on a linked platform that counts toward an account.
///
/// `linked_at` is when the account named its username on that platform.
/// `None` means the connection predates the policy, which is also where an
/// unparseable stored value lands: the cutover is the documented answer for
/// "we do not know when this was connected".
pub fn import_floor(linked_at: Option<&str>) -> Timestamp {
    let cutover = Timestamp::from_str(RETROACTIVE_IMPORT_CUTOVER)
        .expect("RETROACTIVE_IMPORT_CUTOVER is a valid timestamp");

    let Some(linked_at) = linked_at else {
        return cutover;
    };

    match Timestamp::from_str(linked_at) {
        Ok(floor) => floor,
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
    fn activity_before_the_connection_does_not_count() {
        assert!(!credits_activity(Some(LINKED), "2027-02-28T12:00:00Z"));
        assert!(credits_activity(Some(LINKED), "2027-03-02T12:00:00Z"));
    }

    #[test]
    fn the_connection_moment_itself_counts() {
        assert!(credits_activity(Some(LINKED), LINKED));
    }

    #[test]
    fn a_connection_predating_the_policy_floors_at_the_cutover() {
        assert_eq!(
            import_floor(None),
            import_floor(Some(RETROACTIVE_IMPORT_CUTOVER))
        );

        assert!(!credits_activity(None, "2026-07-31T23:59:59Z"));
        assert!(credits_activity(None, "2026-08-02T00:00:00Z"));
    }

    #[test]
    fn offset_and_zulu_spellings_compare_alike() {
        // The platforms disagree about this: iNaturalist reports an offset,
        // Zooniverse a Z. Comparing the strings would put "+00:00" before
        // "Z" whatever the instants are.
        assert!(credits_activity(
            Some("2027-03-01T12:00:00+00:00"),
            "2027-03-01T12:00:01Z"
        ));
        assert!(!credits_activity(
            Some("2027-03-01T12:00:00Z"),
            "2027-03-01T11:59:59+00:00"
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
