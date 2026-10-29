//! The People Finder: one message to the people who live in an area, held for
//! staff approval and then delivered as ordinary direct messages. Ported from
//! orrery's `people_near_count` / `people_near_send` (messaging/api.py) and
//! `PeopleFinderApproval` (messaging/models.py).
//!
//! Three services take part, each for what it alone knows:
//!
//! - the frontend serves the page, and knows who operates a published project
//!   or event (`frontend.people_finder.operators`);
//! - user-service knows where people live and which groups they belong to,
//!   so it answers who an audience reaches (`user.people_finder.*`);
//! - forum-service holds each request until staff decide it, then delivers it
//!   as one batch of messages (`forum.people_finder.*`).
//!
//! Recipients are worked out when a request is approved, not when it is
//! submitted, exactly as orrery does: an approval days later reaches whoever
//! is in the area then.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use ulid::Ulid;

/// The smallest radius the tool searches. orrery clamps to the same bounds.
pub const MIN_RADIUS_MILES: f64 = 10.0;

/// The largest radius the tool searches.
pub const MAX_RADIUS_MILES: f64 = 100.0;

const METERS_PER_MILE: f64 = 1609.344;

/// A circle on the map: everyone whose home point lies inside it.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub struct PeopleFinderArea {
    pub latitude: f64,
    pub longitude: f64,
    pub radius_miles: f64,
}

impl PeopleFinderArea {
    /// The area around a point, with the radius held to what the tool allows.
    /// `None` when the point is not a place on Earth.
    pub fn new(latitude: f64, longitude: f64, radius_miles: f64) -> Option<PeopleFinderArea> {
        if !(latitude.is_finite() && (-90.0..=90.0).contains(&latitude)) {
            return None;
        }

        if !(longitude.is_finite() && (-180.0..=180.0).contains(&longitude)) {
            return None;
        }

        let radius_miles = if radius_miles.is_nan() {
            MIN_RADIUS_MILES
        } else {
            radius_miles.clamp(MIN_RADIUS_MILES, MAX_RADIUS_MILES)
        };

        Some(PeopleFinderArea {
            latitude,
            longitude,
            radius_miles,
        })
    }

    pub fn radius_meters(&self) -> f64 {
        self.radius_miles * METERS_PER_MILE
    }

    /// "50 miles of 39.0000, -98.0000"
    pub fn describe(&self) -> String {
        let radius = if self.radius_miles.fract() == 0.0 {
            format!("{:.0}", self.radius_miles)
        } else {
            format!("{:.1}", self.radius_miles)
        };

        format!(
            "{radius} miles of {:.4}, {:.4}",
            self.latitude, self.longitude
        )
    }
}

/// A staff-only narrowing of the audience to one group. With several, a
/// recipient must be in all of them.
#[derive(
    Serialize,
    Deserialize,
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    strum::EnumIter,
    strum::EnumString,
    strum::AsRefStr,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum PeopleFinderFilter {
    /// Accounts named by a Library Network sign-up.
    LibraryNetwork,
    /// People who completed Citizen Science Champion training.
    Champions,
    /// Current SciStarter Ambassadors.
    Ambassadors,
}

impl PeopleFinderFilter {
    pub fn label(self) -> &'static str {
        match self {
            PeopleFinderFilter::LibraryNetwork => "Library Network accounts",
            PeopleFinderFilter::Champions => "Citizen Science Champions",
            PeopleFinderFilter::Ambassadors => "SciStarter Ambassadors",
        }
    }
}

/// Who in an audience a message goes to. The people in an audience divide into
/// those who operate a published project or event and everyone else; the
/// sender picks one side or both. Picking neither sends nothing, so it is not
/// a reach at all.
#[derive(
    Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, strum::AsRefStr, strum::EnumString,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum PeopleFinderReach {
    Participants,
    Operators,
    Everyone,
}

impl PeopleFinderReach {
    /// The reach the sender's two checkboxes choose, or `None` for neither.
    pub fn from_choices(participants: bool, operators: bool) -> Option<PeopleFinderReach> {
        match (participants, operators) {
            (true, true) => Some(PeopleFinderReach::Everyone),
            (true, false) => Some(PeopleFinderReach::Participants),
            (false, true) => Some(PeopleFinderReach::Operators),
            (false, false) => None,
        }
    }

    pub fn includes_participants(self) -> bool {
        matches!(
            self,
            PeopleFinderReach::Participants | PeopleFinderReach::Everyone
        )
    }

    pub fn includes_operators(self) -> bool {
        matches!(
            self,
            PeopleFinderReach::Operators | PeopleFinderReach::Everyone
        )
    }

    pub fn label(self) -> &'static str {
        match self {
            PeopleFinderReach::Participants => "participants",
            PeopleFinderReach::Operators => "project/event operators",
            PeopleFinderReach::Everyone => "participants and project/event operators",
        }
    }
}

/// Everyone in an area who passes every one of `filters`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct PeopleFinderAudience {
    pub area: PeopleFinderArea,
    #[serde(default)]
    pub filters: BTreeSet<PeopleFinderFilter>,
}

impl PeopleFinderAudience {
    /// "participants within 50 miles of 39.0000, -98.0000, only Library
    /// Network accounts and SciStarter Ambassadors": how staff are told who a
    /// request would reach.
    pub fn describe(&self, reach: PeopleFinderReach) -> String {
        let mut out = format!("{} within {}", reach.label(), self.area.describe());

        if !self.filters.is_empty() {
            let labels: Vec<&str> = self.filters.iter().map(|f| f.label()).collect();
            out.push_str(", only ");
            out.push_str(&labels.join(" and "));
        }

        out
    }
}

/// `user.people_finder.count`: how many people an audience holds, on each side
/// of the operator divide. `operators` is the frontend's answer to
/// `frontend.people_finder.operators`.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PeopleFinderCountRequest {
    pub audience: PeopleFinderAudience,
    pub operators: Vec<Ulid>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PeopleFinderCounts {
    pub participants: i64,
    pub operators: i64,
}

/// `user.people_finder.recipients`: one page of the people a request reaches,
/// in `user_id` order, starting after `after`. A page shorter than `limit`
/// is the last. Paged because one dense area holds tens of thousands of
/// people, more than fits in one NATS message.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PeopleFinderRecipientsRequest {
    pub audience: PeopleFinderAudience,
    pub reach: PeopleFinderReach,
    pub operators: Vec<Ulid>,
    pub after: Option<Ulid>,
    pub limit: i64,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct PeopleFinderRecipientsPage {
    pub user_ids: Vec<Ulid>,
}

/// `forum.people_finder.submit`: a message to an audience, to hold for review.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PeopleFinderSubmission {
    pub sender_id: Ulid,
    pub audience: PeopleFinderAudience,
    pub reach: PeopleFinderReach,
    pub subject: String,
    pub body: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PeopleFinderSubmitted {
    pub request_id: Ulid,
}

/// One request awaiting review, as `forum.people_finder.pending` lists it.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PeopleFinderPendingRequest {
    pub request_id: Ulid,
    pub sender_id: Ulid,
    /// The sender as staff see them: the real handle, never the private
    /// placeholder.
    pub sender: String,
    pub when: String,
    pub audience: PeopleFinderAudience,
    pub reach: PeopleFinderReach,
    pub subject: String,
    pub body: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeopleFinderVerdict {
    Approve,
    Deny,
}

/// `forum.people_finder.review`: staff's decision on one request.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PeopleFinderDecision {
    pub request_id: Ulid,
    pub verdict: PeopleFinderVerdict,
    /// The staff member deciding, kept on the request.
    pub reviewer_id: Ulid,
}

/// Whether this decision was the one that settled the request. `false` means
/// somebody else decided it first, and this decision changed nothing.
#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
pub struct PeopleFinderDecided {
    pub decided: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use strum::IntoEnumIterator;

    #[test]
    fn the_radius_is_held_to_the_tool_s_bounds() {
        let area = |r| PeopleFinderArea::new(39.0, -98.0, r).unwrap().radius_miles;

        assert_eq!(area(5.0), MIN_RADIUS_MILES);
        assert_eq!(area(50.0), 50.0);
        assert_eq!(area(500.0), MAX_RADIUS_MILES);
        assert_eq!(area(f64::NAN), MIN_RADIUS_MILES);
        assert_eq!(area(f64::INFINITY), MAX_RADIUS_MILES);
    }

    #[test]
    fn a_point_off_the_globe_is_no_area() {
        assert!(PeopleFinderArea::new(91.0, 0.0, 50.0).is_none());
        assert!(PeopleFinderArea::new(0.0, -181.0, 50.0).is_none());
        assert!(PeopleFinderArea::new(f64::NAN, 0.0, 50.0).is_none());
    }

    /// The page names each filter by its form field, and the request record
    /// stores the same names, so every one has to read back as itself.
    #[test]
    fn a_filter_is_recovered_from_its_name() {
        for filter in PeopleFinderFilter::iter() {
            assert_eq!(
                filter.as_ref().parse::<PeopleFinderFilter>().ok(),
                Some(filter)
            );
        }
    }

    #[test]
    fn neither_checkbox_is_no_reach() {
        assert_eq!(PeopleFinderReach::from_choices(false, false), None);
        assert_eq!(
            PeopleFinderReach::from_choices(true, true),
            Some(PeopleFinderReach::Everyone)
        );
        assert!(PeopleFinderReach::Everyone.includes_operators());
        assert!(!PeopleFinderReach::Participants.includes_operators());
        assert!(!PeopleFinderReach::Operators.includes_participants());
    }

    #[test]
    fn the_description_names_the_filters() {
        let audience = PeopleFinderAudience {
            area: PeopleFinderArea::new(39.0, -98.0, 50.0).unwrap(),
            filters: [
                PeopleFinderFilter::Ambassadors,
                PeopleFinderFilter::LibraryNetwork,
            ]
            .into_iter()
            .collect(),
        };

        assert_eq!(
            audience.describe(PeopleFinderReach::Participants),
            "participants within 50 miles of 39.0000, -98.0000, \
             only Library Network accounts and SciStarter Ambassadors"
        );
    }
}
