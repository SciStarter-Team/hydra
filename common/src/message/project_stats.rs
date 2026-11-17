//! A project's stats page, as its editors and SciStarter's staff read it,
//! and the feeds the log service gathers it from.
//!
//! The log service builds a snapshot of every project's numbers once a day
//! (`log-service/src/project_stats.rs`), since what each project is compared
//! with is the same for every project and costs the same to work out for one
//! as for all. The feeds below are what it asks the services that own the
//! rest of the data for: the frontend for which slugs name which project,
//! and the user service for joins, saves and who the viewers are. Each is
//! paged by a cursor, since a whole feed does not fit in one NATS message.

use serde::{Deserialize, Serialize};
use ulid::Ulid;

/// The `link_type` of a click through a project page's Visit button to the
/// project's own site, in the `clickthroughs` log.
pub const VISIT_LINK_TYPE: &str = "visit";

/// The `link_type` the join sync logs orrery's visits under: each of
/// orrery's joins began as the visit its Visit button recorded, one per
/// account and project.
pub const ORRERY_VISIT_LINK_TYPE: &str = "join";

/// One page of a feed: the rows after `after`, in the feed's order, at most
/// `limit` of them. A page shorter than `limit` is the last.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageRequest<K> {
    pub after: Option<K>,
    pub limit: u32,
}

impl<K> PageRequest<K> {
    pub fn first(limit: u32) -> PageRequest<K> {
        PageRequest { after: None, limit }
    }
}

/// The slug of the entity a site path addresses: `/projects/<slug>`, or the
/// bare `/<slug>` projects were served at before they had a prefix (and are
/// still redirected from). Lowercase, since slugs are matched without
/// regard to case. `None` for a path that addresses no single entity, such as
/// `/`, `/projects/<slug>/stats` or `/-admin/users`.
pub fn page_slug(path: &str) -> Option<String> {
    let path = path.split(['?', '#']).next()?.trim_end_matches('/');
    let rest = path
        .strip_prefix("/projects/")
        .or_else(|| path.strip_prefix('/'))?;

    if rest.is_empty() || rest.contains('/') || rest.starts_with('-') {
        return None;
    }

    Some(rest.to_lowercase())
}

/// The slugs a project has been known by, as `frontend.entity.project_slugs`
/// gives them: its current slug and every slug that redirects to it, which
/// is what a project renamed since a view or a click was logged is found
/// by. Paged by `project_id`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectSlugs {
    pub project_id: Ulid,
    pub published: bool,
    /// Lowercase, without any path prefix.
    pub slugs: Vec<String>,
}

/// One account's join to one project, as `user.stats.joins` gives it.
/// Paged by `(user_id, project_id)`, the table's primary key, so that each
/// page is one seek.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JoinPair {
    pub project_id: Ulid,
    pub user_id: Ulid,
    /// When the account joined, RFC 3339.
    pub when: String,
}

/// One account's current save of one project, as `user.stats.saves` gives
/// it. Paged by `(user_id, project_id)`, the table's primary key, so that
/// each page is one seek; the log service counts them by day.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Save {
    pub user_id: Ulid,
    pub project_id: Ulid,
    /// The UTC day it was made, `YYYY-MM-DD`.
    pub day: String,
}

/// What a project's stats say about one account that might view it, as
/// `user.stats.people` gives it. Only accounts with something to say are
/// listed: a gender, a birthday, or being staff, whose views are left out
/// of the audience as orrery left them out. Paged by `user_id`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Person {
    pub user_id: Ulid,
    pub gender: Gender,
    /// `YYYY-MM-DD`.
    pub birthday: Option<String>,
    pub is_staff: bool,
}

/// The genders the profile form offers, plus not having said.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Gender {
    Female,
    Male,
    AnotherIdentity,
    PreferNotToSay,
    Unknown,
}

impl Gender {
    pub const ALL: [Gender; 5] = [
        Gender::Female,
        Gender::Male,
        Gender::AnotherIdentity,
        Gender::PreferNotToSay,
        Gender::Unknown,
    ];

    /// The gender a profile's `gender` column records. The column is free
    /// text, so anything the form does not offer is unknown.
    pub fn from_profile(value: Option<&str>) -> Gender {
        match value.map(str::trim) {
            Some("Female") => Gender::Female,
            Some("Male") => Gender::Male,
            Some("Another identity") => Gender::AnotherIdentity,
            Some("Prefer not to say") => Gender::PreferNotToSay,
            _ => Gender::Unknown,
        }
    }

    /// The name the gender is stored under.
    pub fn key(self) -> &'static str {
        match self {
            Gender::Female => "female",
            Gender::Male => "male",
            Gender::AnotherIdentity => "another",
            Gender::PreferNotToSay => "prefer_not",
            Gender::Unknown => "unknown",
        }
    }

    /// The gender stored under `key`; unknown for a name no gender has.
    pub fn from_key(key: &str) -> Gender {
        Gender::ALL
            .into_iter()
            .find(|gender| gender.key() == key)
            .unwrap_or(Gender::Unknown)
    }

    pub fn label(self) -> &'static str {
        match self {
            Gender::Female => "Female",
            Gender::Male => "Male",
            Gender::AnotherIdentity => "Another Identity",
            Gender::PreferNotToSay => "Prefer Not To Say",
            Gender::Unknown => "Unknown",
        }
    }
}

/// The age groups orrery's stats divided viewers into.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum AgeGroup {
    Under18,
    From18To24,
    From25To34,
    From35To44,
    From45To54,
    From55To64,
    From65,
    Unknown,
}

impl AgeGroup {
    pub const ALL: [AgeGroup; 8] = [
        AgeGroup::Under18,
        AgeGroup::From18To24,
        AgeGroup::From25To34,
        AgeGroup::From35To44,
        AgeGroup::From45To54,
        AgeGroup::From55To64,
        AgeGroup::From65,
        AgeGroup::Unknown,
    ];

    /// The group of someone born on `birthday` (`YYYY-MM-DD`), as of `on`
    /// (`YYYY-MM-DD`). A birthday that does not parse is unknown.
    pub fn of(birthday: Option<&str>, on: &str) -> AgeGroup {
        let Some((born, today)) = birthday.and_then(ymd).zip(ymd(on)) else {
            return AgeGroup::Unknown;
        };

        let mut age = today.0 - born.0;
        if (today.1, today.2) < (born.1, born.2) {
            age -= 1;
        }

        match age {
            ..0 => AgeGroup::Unknown,
            0..18 => AgeGroup::Under18,
            18..25 => AgeGroup::From18To24,
            25..35 => AgeGroup::From25To34,
            35..45 => AgeGroup::From35To44,
            45..55 => AgeGroup::From45To54,
            55..65 => AgeGroup::From55To64,
            _ => AgeGroup::From65,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            AgeGroup::Under18 => "Under 18",
            AgeGroup::From18To24 => "18-24",
            AgeGroup::From25To34 => "25-34",
            AgeGroup::From35To44 => "35-44",
            AgeGroup::From45To54 => "45-54",
            AgeGroup::From55To64 => "55-64",
            AgeGroup::From65 => "65+",
            AgeGroup::Unknown => "Unknown",
        }
    }
}

/// Year, month and day of a `YYYY-MM-DD` date, or of the date a longer
/// timestamp starts with.
fn ymd(date: &str) -> Option<(i32, u32, u32)> {
    let mut parts = date.get(..10)?.splitn(3, '-');
    let year = parts.next()?.parse().ok()?;
    let month = parts.next()?.parse().ok()?;
    let day = parts.next()?.parse().ok()?;
    Some((year, month, day))
}

/// `log.project_stats`: the stats page of one project.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectStatsRequest {
    pub project_id: Ulid,
}

/// What `log.project_stats` answers. `Pending` until the first snapshot has
/// been built.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProjectStatsResponse {
    Pending,
    Ready(Box<ProjectStats>),
}

/// The numbers a project's stats page shows, as of the last snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectStats {
    /// When the snapshot was built, RFC 3339.
    pub updated: String,
    /// The last UTC day the snapshot counts, `YYYY-MM-DD`. Windows end with
    /// it.
    pub through: String,
    pub measures: Vec<MeasureStats>,
    /// The last 90 days, oldest first, ending with `through`.
    pub days: Vec<DayCounts>,
    pub shares: Vec<ShareStats>,
    pub audience: Audience,
    pub feedback: Feedback,
}

impl ProjectStats {
    /// How the project compares in `measure` over `window`, if that pair is
    /// one the page shows.
    pub fn comparison(&self, measure: Measure, window: Window) -> Option<&Comparison> {
        self.measures
            .iter()
            .find(|stats| stats.measure == measure && stats.window == window)
            .map(|stats| &stats.comparison)
    }

    /// The shares over `window`.
    pub fn shares(&self, window: Window) -> Option<&ShareStats> {
        self.shares.iter().find(|shares| shares.window == window)
    }
}

/// Something counted per project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Measure {
    /// Views of the project's page.
    Views,
    /// Clicks through to the project's own site.
    Visits,
    /// Saves to a dashboard that are still saved.
    Saves,
    /// Accounts that have joined.
    Joins,
    /// Contributions reported for the project, one per repetition.
    Contributions,
}

impl Measure {
    pub fn key(self) -> &'static str {
        match self {
            Measure::Views => "views",
            Measure::Visits => "visits",
            Measure::Saves => "saves",
            Measure::Joins => "joins",
            Measure::Contributions => "contributions",
        }
    }
}

/// A span of days a measure is counted over, ending with the snapshot's
/// last day.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Window {
    AllTime,
    Last30,
    Last7,
}

impl Window {
    pub fn key(self) -> &'static str {
        match self {
            Window::AllTime => "all_time",
            Window::Last30 => "last_30",
            Window::Last7 => "last_7",
        }
    }

    /// How many days the window holds; `None` for all of them.
    pub fn days(self) -> Option<u32> {
        match self {
            Window::AllTime => None,
            Window::Last30 => Some(30),
            Window::Last7 => Some(7),
        }
    }
}

/// One project's figure for a measure, beside the same figure for every
/// project that has any of it, as orrery's stats compared them
/// (`orrery/math.py`).
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Comparison {
    pub value: f64,
    pub median: f64,
    pub mean: f64,
    pub max: f64,
    /// The share of the other projects' figures below this one, 0 to 100.
    pub percentile: f64,
}

impl Comparison {
    /// `value` against `domain`, which must be sorted ascending. An empty
    /// domain compares as zeros.
    pub fn against(value: f64, domain: &[f64]) -> Comparison {
        if domain.is_empty() {
            return Comparison {
                value,
                ..Comparison::default()
            };
        }

        let len = domain.len();
        let middle = len / 2;
        let median = if len % 2 == 0 {
            (domain[middle - 1] + domain[middle]) / 2.0
        } else {
            domain[middle]
        };
        let below = domain.partition_point(|&other| other < value);

        Comparison {
            value,
            median,
            mean: domain.iter().sum::<f64>() / len as f64,
            max: domain[len - 1],
            percentile: 100.0 * below as f64 / len as f64,
        }
    }
}

/// How a project compares in one measure over one window.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeasureStats {
    pub measure: Measure,
    pub window: Window,
    pub comparison: Comparison,
}

/// A project's counts for one UTC day.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DayCounts {
    /// `YYYY-MM-DD`.
    pub day: String,
    pub views: i64,
    pub visits: i64,
    pub saves: i64,
    pub contributions: i64,
}

/// How often a project was shared over one window, by platform.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShareStats {
    pub window: Window,
    pub total: i64,
    /// Every platform anything has been shared to, as the share log spells
    /// it (`facebook`, `pinterest`, ...). The log keeps platforms orrery
    /// offered and hydra does not, so they are not a closed set.
    pub platforms: Vec<(String, Comparison)>,
}

/// Who views a project, from the views of signed-in accounts other than
/// staff.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Audience {
    pub views: i64,
    pub by_gender: Vec<(Gender, i64)>,
    pub by_age: Vec<(AgeGroup, i64)>,
    /// The published projects this project's viewers view most, with how
    /// many times they viewed each, most viewed first.
    pub also_viewed: Vec<(Ulid, i64)>,
}

/// What the signed-in accounts that went to a project's site did next:
/// joined it, or have not said. Each is compared both as a count and as a
/// percentage of the two together.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Feedback {
    pub joins: Comparison,
    pub joins_percent: Comparison,
    pub unknown: Comparison,
    pub unknown_percent: Comparison,
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert2::check;

    #[test]
    fn a_comparison_matches_orrerys_arithmetic() {
        let compared = Comparison::against(3.0, &[1.0, 2.0, 3.0, 10.0]);

        check!(compared.median == 2.5);
        check!(compared.mean == 4.0);
        check!(compared.max == 10.0);
        check!(compared.percentile == 50.0);
    }

    #[test]
    fn a_comparison_with_nothing_to_compare_is_zero() {
        let compared = Comparison::against(7.0, &[]);

        check!(compared.value == 7.0);
        check!(compared.median == 0.0);
        check!(compared.percentile == 0.0);
    }

    #[test]
    fn an_age_turns_on_the_birthday() {
        check!(AgeGroup::of(Some("2008-06-15"), "2026-06-14") == AgeGroup::Under18);
        check!(AgeGroup::of(Some("2008-06-15"), "2026-06-15") == AgeGroup::From18To24);
        check!(AgeGroup::of(Some("1950-01-01"), "2026-11-17") == AgeGroup::From65);
        check!(AgeGroup::of(None, "2026-11-17") == AgeGroup::Unknown);
        check!(AgeGroup::of(Some("not a date"), "2026-11-17") == AgeGroup::Unknown);
    }

    #[test]
    fn a_page_path_names_its_slug() {
        check!(page_slug("/projects/Bee-Count") == Some(String::from("bee-count")));
        check!(page_slug("/bee-count/") == Some(String::from("bee-count")));
        check!(page_slug("/projects/bee-count?x=1") == Some(String::from("bee-count")));
        check!(page_slug("/projects/bee-count/stats") == None);
        check!(page_slug("/") == None);
        check!(page_slug("/-admin") == None);
        check!(page_slug("bee-count") == None);
    }

    #[test]
    fn a_free_text_gender_is_unknown() {
        check!(Gender::from_profile(Some("Female")) == Gender::Female);
        check!(Gender::from_profile(Some("Events")) == Gender::Unknown);
        check!(Gender::from_profile(None) == Gender::Unknown);
    }
}
