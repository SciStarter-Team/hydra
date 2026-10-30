pub mod inaturalist;
pub mod zooniverse;

use std::collections::HashMap;
use std::str::FromStr;

use anyhow::Result;
use common::linked_platform;
use jiff::Timestamp;
use log_service::database::tables::ParticipationRow;
use log_service::participation_feed::ParticipationEventData;

/// The participation the bridges here are the importer of record for.
///
/// orrery runs bridges to the same platforms, and its participation feed
/// carries what they credit. Where a bridge here credits a contribution, the
/// bridge is what counts, and the feed's copy is not imported as well:
/// importing both would count every such contribution twice. What a bridge
/// covers is what it would credit itself -- a contribution orrery filed as its
/// own import from the platform, to a project the bridge credits, by an
/// account that has linked the platform, no earlier than that link's import
/// floor. Anything else orrery files under the same origin, its GLOBE and
/// CoCoRaHS imports among it, is no bridge's here and is imported from the
/// feed.
///
/// Before the August 2026 import policy there were no import floors, and the
/// Zooniverse bridge credited every connected account from the moment it
/// began crediting a project. For an account connected before the policy,
/// orrery's Zooniverse participation from then on is already in the log, so
/// that era is covered too. Which accounts were connected at the time is not
/// recorded; an account connected now, with no connection time, is taken to
/// have been.
pub struct BridgeCoverage {
    platforms: [Platform; 2],
}

impl BridgeCoverage {
    pub async fn fetch() -> Result<BridgeCoverage> {
        Ok(BridgeCoverage {
            platforms: [
                inaturalist::coverage().await?,
                zooniverse::coverage().await?,
            ],
        })
    }

    pub fn covers(&self, contribution: Contribution) -> bool {
        self.platforms
            .iter()
            .any(|platform| platform.covers(contribution))
    }
}

/// What coverage is decided by: who contributed to what, when, and how orrery
/// filed it. An event from the feed and a row logged for one both say.
#[derive(Debug, Clone, Copy)]
pub struct Contribution<'a> {
    pub origin: i16,
    pub entity_id: &'a str,
    pub user_id: Option<&'a str>,
    pub when: &'a str,
}

impl<'a> Contribution<'a> {
    pub fn of_event(event: &'a ParticipationEventData) -> Contribution<'a> {
        Contribution {
            origin: event.origin,
            entity_id: &event.entity_ulid,
            user_id: event.profile_user_ulid.as_deref(),
            when: &event.when,
        }
    }

    pub fn of_row(row: &'a ParticipationRow) -> Contribution<'a> {
        Contribution {
            origin: row.origin,
            entity_id: &row.entity_id,
            user_id: row.user_id.as_deref(),
            when: &row.when,
        }
    }
}

/// What one bridge credits, as each bridge declares it beside the code that
/// does the crediting.
pub struct Platform {
    /// The origins orrery has filed its own imports from the platform under.
    pub orrery_origins: &'static [i16],
    /// The projects the bridge credits contributions to, each with when it
    /// began crediting every account connected before the import policy, if
    /// it did.
    pub projects: HashMap<String, Option<Timestamp>>,
    /// The accounts the bridge credits, by user id.
    pub links: HashMap<String, Link>,
}

impl Platform {
    fn covers(&self, contribution: Contribution) -> bool {
        let Some(user_id) = contribution.user_id else {
            return false;
        };

        if !self.orrery_origins.contains(&contribution.origin) {
            return false;
        }

        let (Some(credited_since), Some(link)) = (
            self.projects.get(contribution.entity_id),
            self.links.get(user_id),
        ) else {
            return false;
        };

        link.credits(contribution.when)
            || (link.linked_at.is_none()
                && link.matched_by(contribution.when)
                && credited_since.is_some_and(|since| {
                    Timestamp::from_str(contribution.when).is_ok_and(|when| when >= since)
                }))
    }
}

/// An account's connection to a platform, as the bridge crediting it reads
/// it. The bridge credits by this and coverage covers by it, so the two
/// cannot disagree about whose a contribution is.
#[derive(Debug, Clone, Default)]
pub struct Link {
    /// When the account connected; `None` is a connection that predates the
    /// August 2026 policy.
    pub linked_at: Option<String>,

    /// When the bridge could first match the account's contributions, if
    /// that came after it connected. orrery credited what came before, and
    /// its copy is the one that stands.
    pub matched_since: Option<Timestamp>,
}

impl Link {
    /// Whether the bridge credits activity at `when` to the account.
    pub fn credits(&self, when: &str) -> bool {
        self.matched_by(when) && linked_platform::credits_activity(self.linked_at.as_deref(), when)
    }

    /// Whether the bridge could match the account's activity at `when`. An
    /// unreadable `when` could not be matched, as it cannot be credited.
    fn matched_by(&self, when: &str) -> bool {
        self.matched_since
            .is_none_or(|since| Timestamp::from_str(when).is_ok_and(|when| when >= since))
    }
}

#[cfg(test)]
mod tests {
    use assert2::check;

    use super::*;

    const PROJECT: &str = "01JKXMGGMD577ZA8Q57S2GY1V8";
    const EARLY: &str = "01JKXMG2RVK1PZ1NM4H87ARHYW";
    const GLOBE: &str = "01JKXMFYFZ00000000000GLOBE";
    const LINKED: &str = "01JKXMFYFZ0000000000LINKED";
    const LEGACY: &str = "01JKXMFYFZ0000000000LEGACY";
    const STRANGER: &str = "01JKXMFYFZ000000000STRANGE";
    const LATE: &str = "01JKXMFYFZ00000000000LATE0";
    const LATE_LEGACY: &str = "01JKXMFYFZ0000000000LATE1";

    /// When the bridge could first match LATE and LATE_LEGACY, both well
    /// after they connected.
    const MATCHED: &str = "2026-10-01T00:00:00Z";

    fn platform() -> Platform {
        Platform {
            orrery_origins: &[5, 0],
            projects: HashMap::from([
                (PROJECT.to_string(), None),
                (
                    EARLY.to_string(),
                    Some("2026-03-11T00:00:00Z".parse().unwrap()),
                ),
            ]),
            links: HashMap::from([
                (
                    LINKED.to_string(),
                    Link {
                        linked_at: Some("2026-09-10T12:00:00Z".to_string()),
                        matched_since: None,
                    },
                ),
                (LEGACY.to_string(), Link::default()),
                (
                    LATE.to_string(),
                    Link {
                        linked_at: Some("2026-09-10T12:00:00Z".to_string()),
                        matched_since: Some(MATCHED.parse().unwrap()),
                    },
                ),
                (
                    LATE_LEGACY.to_string(),
                    Link {
                        linked_at: None,
                        matched_since: Some(MATCHED.parse().unwrap()),
                    },
                ),
            ]),
        }
    }

    fn event(origin: i16, entity: &str, user: Option<&str>, when: &str) -> ParticipationEventData {
        ParticipationEventData {
            origin,
            when: when.to_string(),
            duration: None,
            where_point: None,
            magnitude: None,
            authenticated: None,
            extra: None,
            repetitions: 1,
            fidelity: None,
            entity_ulid: entity.to_string(),
            profile_user_ulid: user.map(str::to_string),
            task_name: None,
        }
    }

    #[test]
    fn what_the_bridge_would_credit_is_covered() {
        check!(platform().covers(Contribution::of_event(&event(
            5,
            PROJECT,
            Some(LINKED),
            "2026-10-01T00:00:00Z"
        ))));
        check!(platform().covers(Contribution::of_event(&event(
            5,
            PROJECT,
            Some(LEGACY),
            "2026-08-02T00:00:00Z"
        ))));
    }

    #[test]
    fn what_came_before_the_bridge_could_match_an_account_is_left_to_orrery() {
        let late = &platform().links[LATE];

        for (when, matched) in [
            ("2026-09-20T00:00:00Z", false),
            ("2026-10-02T00:00:00Z", true),
        ] {
            check!(late.credits(when) == matched);
            check!(
                platform().covers(Contribution::of_event(&event(5, PROJECT, Some(LATE), when)))
                    == matched
            );
        }
    }

    #[test]
    fn an_account_matched_late_has_no_earlier_era_either() {
        for (when, matched) in [
            ("2026-04-01T00:00:00Z", false),
            ("2026-10-02T00:00:00Z", true),
        ] {
            check!(
                platform().covers(Contribution::of_event(&event(
                    5,
                    EARLY,
                    Some(LATE_LEGACY),
                    when
                ))) == matched
            );
        }
    }

    #[test]
    fn what_predates_the_connection_is_not() {
        check!(!platform().covers(Contribution::of_event(&event(
            5,
            PROJECT,
            Some(LINKED),
            "2026-09-01T00:00:00Z"
        ))));
        check!(!platform().covers(Contribution::of_event(&event(
            5,
            PROJECT,
            Some(LEGACY),
            "2026-07-01T00:00:00Z"
        ))));
    }

    #[test]
    fn what_the_bridge_credited_before_the_policy_is_covered() {
        check!(platform().covers(Contribution::of_event(&event(
            5,
            EARLY,
            Some(LEGACY),
            "2026-04-01T00:00:00Z"
        ))));
        check!(!platform().covers(Contribution::of_event(&event(
            5,
            EARLY,
            Some(LEGACY),
            "2026-03-01T00:00:00Z"
        ))));
    }

    #[test]
    fn an_account_connected_after_the_policy_has_no_earlier_era() {
        check!(!platform().covers(Contribution::of_event(&event(
            5,
            EARLY,
            Some(LINKED),
            "2026-04-01T00:00:00Z"
        ))));
    }

    #[test]
    fn what_orrery_filed_under_an_earlier_origin_for_the_platform_is_covered() {
        check!(platform().covers(Contribution::of_event(&event(
            0,
            PROJECT,
            Some(LINKED),
            "2026-10-01T00:00:00Z"
        ))));
    }

    #[test]
    fn a_logged_row_is_covered_as_its_event_is() {
        let covered = event(5, EARLY, Some(LEGACY), "2026-04-01T00:00:00Z");
        let row = log_service::participation_ledger::row_of(&covered);

        check!(platform().covers(Contribution::of_row(&row)));
    }

    #[test]
    fn another_import_filed_under_the_same_origin_is_not() {
        check!(!platform().covers(Contribution::of_event(&event(
            5,
            GLOBE,
            Some(LINKED),
            "2026-10-01T00:00:00Z"
        ))));
    }

    #[test]
    fn a_contribution_that_arrived_some_other_way_is_not() {
        check!(!platform().covers(Contribution::of_event(&event(
            3,
            PROJECT,
            Some(LINKED),
            "2026-10-01T00:00:00Z"
        ))));
    }

    #[test]
    fn an_account_that_never_linked_the_platform_is_not() {
        check!(!platform().covers(Contribution::of_event(&event(
            5,
            PROJECT,
            Some(STRANGER),
            "2026-10-01T00:00:00Z"
        ))));
        check!(!platform().covers(Contribution::of_event(&event(
            5,
            PROJECT,
            None,
            "2026-10-01T00:00:00Z"
        ))));
    }
}
