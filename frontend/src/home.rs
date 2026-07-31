//! Home-page configuration: the data-driven welcome-message decision tree and
//! the "near you" list settings, edited via the Home Page / Dashboard admin.
//!
//! The welcome "decision tree" is modelled as an ordered, first-match-wins list
//! of [`WelcomeRule`]s with a [`HomeSettings::welcome_default`] fallback. Each
//! rule's [`Condition`]s are AND-combined and individually negatable, so the
//! flat list expresses the full boolean space (AND within a rule, OR across
//! rules, NOT per condition) while staying easy for staff to maintain — no
//! nested-tree UI.
//!
//! Adding a new condition is deliberately localized: add a [`ConditionKind`]
//! variant, its arm in [`ConditionKind::matches`], its [`Signal`] in
//! [`ConditionKind::required_signal`] (if it needs fetched data), and a control
//! in the admin form.

use std::collections::HashSet;
use std::str::FromStr;

use anyhow::Error;
use common::message::log::{
    UserLastViewRequest, UserLastViewResponse, UserParticipationRequest, UserParticipationResponse,
};
use common::message::user::{BasicInfo, normalize_tag};
use common::shared::db;
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use ulid::Ulid;

use crate::peering::LoadStorePeered;

/// `site_settings` key (and `LoadStorePeered::SPECIFIC`) for the home-page blob.
pub const SETTINGS_KEY: &str = "home";

/// A welcome message shown at the top of the home page for a logged-in user.
///
/// `{handle}` in `title`/`body` is replaced with the user's handle at render
/// time (see [`WelcomeMessage::personalized`]). Empty `cta_*` fields hide the
/// call-to-action button.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WelcomeMessage {
    pub title: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub cta_label: String,
    #[serde(default)]
    pub cta_url: String,
}

impl WelcomeMessage {
    /// Substitute `{handle}` with the user's handle in the visible copy.
    pub fn personalized(&self, handle: &str) -> WelcomeMessage {
        WelcomeMessage {
            title: self.title.replace("{handle}", handle),
            body: self.body.replace("{handle}", handle),
            cta_label: self.cta_label.clone(),
            cta_url: self.cta_url.clone(),
        }
    }

    /// Whether there is anything to show (an all-empty message renders nothing).
    pub fn is_empty(&self) -> bool {
        self.title.trim().is_empty() && self.body.trim().is_empty()
    }
}

/// How a day-count condition compares the elapsed days against its threshold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DayComparison {
    /// Elapsed days are at most `days` (recent / young).
    Within,
    /// Elapsed days are more than `days` (lapsed / old). Also true when the
    /// underlying event never happened (elapsed time is effectively infinite).
    MoreThan,
}

/// A single testable fact about the viewer's state.
///
/// Serialized with an internal `kind` tag so the stored JSON stays legible and
/// forward-compatible as variants are added.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum ConditionKind {
    /// The user has joined at least one project.
    HasParticipation,
    /// Days since the user's most recent participation.
    DaysSinceLastParticipation {
        comparison: DayComparison,
        days: i64,
    },
    /// Member of a specific community, or of any community when `None`.
    MemberOfCommunity {
        #[serde(default)]
        community_id: Option<Ulid>,
    },
    /// The user carries the given interest tag (case-insensitive).
    HasTag { tag: String },
    /// Days since the user's most recent page view.
    DaysSinceLastView {
        comparison: DayComparison,
        days: i64,
    },
    /// We resolved the viewer's location for this request.
    LocationKnown,
    /// The user is staff or a superuser.
    IsStaff,
    /// Days since the account was created (from the `user_id` ULID timestamp).
    AccountAgeDays {
        comparison: DayComparison,
        days: i64,
    },
}

impl ConditionKind {
    /// The fetched signal this condition needs, if any. Conditions answerable
    /// from `BasicInfo`/`user_id`/request context return `None`.
    fn required_signal(&self) -> Option<Signal> {
        match self {
            ConditionKind::HasParticipation | ConditionKind::DaysSinceLastParticipation { .. } => {
                Some(Signal::Participation)
            }
            ConditionKind::MemberOfCommunity { .. } => Some(Signal::Communities),
            ConditionKind::HasTag { .. } => Some(Signal::Tags),
            ConditionKind::DaysSinceLastView { .. } => Some(Signal::LastView),
            ConditionKind::LocationKnown
            | ConditionKind::IsStaff
            | ConditionKind::AccountAgeDays { .. } => None,
        }
    }

    /// Evaluate this condition (before negation) against a gathered state.
    ///
    /// A condition whose required signal could not be fetched evaluates to
    /// `false` (a rule never matches on missing data rather than guessing).
    fn matches(&self, state: &UserState) -> bool {
        match self {
            ConditionKind::HasParticipation => state.participation_total.unwrap_or(0) >= 1,
            ConditionKind::DaysSinceLastParticipation { comparison, days } => {
                compare_days_since(state.last_participation, *comparison, *days)
            }
            ConditionKind::MemberOfCommunity { community_id } => {
                match (community_id, &state.communities) {
                    (Some(id), Some(list)) => list.contains(id),
                    (None, Some(list)) => !list.is_empty(),
                    (_, None) => false,
                }
            }
            // Both sides normalized: the condition's tag is typed by whoever
            // authored the block, who may well write it as a hashtag, and the
            // account's own tags predate the rule that they are stored one way.
            ConditionKind::HasTag { tag } => {
                let wanted = normalize_tag(tag);

                state
                    .tags
                    .as_ref()
                    .is_some_and(|tags| tags.iter().any(|held| normalize_tag(held) == wanted))
            }
            ConditionKind::DaysSinceLastView { comparison, days } => {
                compare_days_since(state.last_view, *comparison, *days)
            }
            ConditionKind::LocationKnown => state.location_known,
            ConditionKind::IsStaff => state.is_staff,
            ConditionKind::AccountAgeDays { comparison, days } => {
                compare_days_since(ulid_created(state.user_id), *comparison, *days)
            }
        }
    }
}

/// A condition plus an optional NOT. `negate` flips the underlying test, e.g.
/// "NOT tagged `robotics`".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Condition {
    #[serde(default)]
    pub negate: bool,
    #[serde(flatten)]
    pub kind: ConditionKind,
}

impl Condition {
    fn matches(&self, state: &UserState) -> bool {
        self.kind.matches(state) ^ self.negate
    }
}

/// One entry in the decision tree: a message shown when every condition matches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WelcomeRule {
    #[serde(default)]
    pub conditions: Vec<Condition>,
    pub message: WelcomeMessage,
}

impl WelcomeRule {
    /// All conditions must match (an empty condition list always matches).
    fn matches(&self, state: &UserState) -> bool {
        self.conditions.iter().all(|c| c.matches(state))
    }
}

/// Admin-editable home-page configuration, stored as a singleton JSON blob and
/// synchronized to all frontends via [`LoadStorePeered`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HomeSettings {
    /// Ordered, first-match-wins welcome rules.
    #[serde(default)]
    pub welcome_rules: Vec<WelcomeRule>,
    /// Shown when no rule matches.
    #[serde(default)]
    pub welcome_default: WelcomeMessage,
    /// Whether to show the "projects & events near you" list.
    #[serde(default = "default_show_nearby")]
    pub show_nearby: bool,
    /// How many nearby items to list.
    #[serde(default = "default_nearby_limit")]
    pub nearby_limit: i64,
    /// Heading for the nearby list.
    #[serde(default = "default_nearby_heading")]
    pub nearby_heading: String,
    /// Whether to show the Flower concierge chat widget. Off by default so
    /// the feature rolls out deliberately.
    #[serde(default)]
    pub show_concierge: bool,
    /// Heading for the concierge section.
    #[serde(default = "default_concierge_heading")]
    pub concierge_heading: String,
}

pub fn default_concierge_heading() -> String {
    "Ask Flower: Find Your Project".to_string()
}

fn default_show_nearby() -> bool {
    true
}

fn default_nearby_limit() -> i64 {
    6
}

fn default_nearby_heading() -> String {
    "Projects & Events Near You".to_string()
}

impl Default for HomeSettings {
    fn default() -> Self {
        HomeSettings {
            welcome_rules: Vec::new(),
            welcome_default: WelcomeMessage {
                title: "Welcome back, {handle}!".to_string(),
                body: String::new(),
                cta_label: String::new(),
                cta_url: String::new(),
            },
            show_nearby: default_show_nearby(),
            nearby_limit: default_nearby_limit(),
            nearby_heading: default_nearby_heading(),
            show_concierge: false,
            concierge_heading: default_concierge_heading(),
        }
    }
}

impl HomeSettings {
    /// Load the current settings, falling back to defaults if never edited or
    /// unreadable.
    pub async fn current() -> HomeSettings {
        HomeSettings::load(()).await.unwrap_or_default()
    }

    /// The set of fetched signals any active rule depends on, so the gatherer
    /// fetches only what is actually referenced.
    fn required_signals(&self) -> HashSet<Signal> {
        self.welcome_rules
            .iter()
            .flat_map(|rule| rule.conditions.iter())
            .filter_map(|cond| cond.kind.required_signal())
            .collect()
    }

    /// Resolve the welcome message for a user: the first matching rule's
    /// message, or the default.
    pub fn resolve_welcome<'a>(&'a self, state: &UserState) -> &'a WelcomeMessage {
        self.welcome_rules
            .iter()
            .find(|rule| rule.matches(state))
            .map(|rule| &rule.message)
            .unwrap_or(&self.welcome_default)
    }
}

impl LoadStorePeered for HomeSettings {
    type DatabaseKey = ();

    const GROUP: &str = "site_settings";
    const SPECIFIC: &str = SETTINGS_KEY;

    async fn save_to_database(&self) -> Result<(), Error> {
        let json = serde_json::to_string(self)?;
        sqlx::query!(
            r#"INSERT OR REPLACE INTO "site_settings" ("key", "content") VALUES (?, ?)"#,
            SETTINGS_KEY,
            json,
        )
        .execute(db())
        .await?;
        Ok(())
    }

    async fn read_from_database(_key: ()) -> Result<Self, Error> {
        let row = sqlx::query!(
            r#"SELECT "content" FROM "site_settings" WHERE "key" = ?"#,
            SETTINGS_KEY,
        )
        .fetch_one(db())
        .await?;
        Ok(serde_json::from_str(&row.content)?)
    }
}

/// The fetched signals a condition may depend on. Kept private: callers request
/// gathering by passing the [`HomeSettings`], not raw signal sets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Signal {
    Participation,
    Communities,
    Tags,
    LastView,
}

/// A snapshot of the viewer's state, evaluated against the welcome rules.
///
/// Fields sourced from `BasicInfo`/`user_id`/request context are always
/// populated; fields backed by a fetched [`Signal`] are `Some` only when that
/// signal was both needed and successfully fetched.
#[derive(Debug, Default)]
pub struct UserState {
    pub user_id: Ulid,
    pub is_staff: bool,
    pub location_known: bool,
    pub participation_total: Option<i64>,
    pub last_participation: Option<Timestamp>,
    pub communities: Option<Vec<Ulid>>,
    pub tags: Option<Vec<String>>,
    pub last_view: Option<Timestamp>,
}

impl UserState {
    /// Gather the state a logged-in user's welcome rules need. Only the signals
    /// actually referenced by `settings` are fetched, and those are fetched
    /// concurrently.
    pub async fn gather(user: &BasicInfo, location_known: bool, settings: &HomeSettings) -> Self {
        let needed = settings.required_signals();
        let want = |s: Signal| needed.contains(&s);
        let user_id = user.user_id;

        let (participation, communities, tags, last_view) = tokio::join!(
            async {
                if want(Signal::Participation) {
                    fetch_participation(user_id).await
                } else {
                    None
                }
            },
            async {
                if want(Signal::Communities) {
                    fetch_communities(user_id).await
                } else {
                    None
                }
            },
            async {
                if want(Signal::Tags) {
                    fetch_tags(user_id).await
                } else {
                    None
                }
            },
            async {
                if want(Signal::LastView) {
                    fetch_last_view(user_id).await
                } else {
                    None
                }
            },
        );

        let (participation_total, last_participation) = match participation {
            Some((total, last)) => (Some(total), last),
            None => (None, None),
        };

        UserState {
            user_id,
            is_staff: user.is_staff || user.is_superuser,
            location_known,
            participation_total,
            last_participation,
            communities,
            tags,
            last_view,
        }
    }
}

async fn fetch_participation(user_id: Ulid) -> Option<(i64, Option<Timestamp>)> {
    let resp: UserParticipationResponse = common::service::request(
        "log.user_participation",
        UserParticipationRequest {
            user_id,
            recent_limit: 1,
            recent_days: 0,
        },
    )
    .await
    .ok()?;
    let last = resp
        .recent
        .first()
        .and_then(|entry| Timestamp::from_str(&entry.when).ok());
    Some((resp.total, last))
}

async fn fetch_communities(user_id: Ulid) -> Option<Vec<Ulid>> {
    let list = crate::communities::user_communities(user_id).await.ok()?;
    Some(list.into_iter().map(|c| c.community_id).collect())
}

async fn fetch_tags(user_id: Ulid) -> Option<Vec<String>> {
    common::service::request::<Vec<String>>("user.tags.get", user_id.to_string())
        .await
        .ok()
}

async fn fetch_last_view(user_id: Ulid) -> Option<Timestamp> {
    let resp: UserLastViewResponse =
        common::service::request("log.user_last_view", UserLastViewRequest { user_id })
            .await
            .ok()?;
    resp.when.and_then(|w| Timestamp::from_str(&w).ok())
}

/// Whole days elapsed from `when` to now (0 on error).
fn days_since(when: Timestamp) -> f64 {
    Timestamp::now()
        .since(when)
        .ok()
        .and_then(|span| span.total(jiff::Unit::Second).ok())
        .map(|secs| secs / 86_400.0)
        .unwrap_or(0.0)
}

/// Compare the days elapsed since `when` against `days`. A `None` timestamp
/// (event never happened) counts as infinitely long ago.
fn compare_days_since(when: Option<Timestamp>, comparison: DayComparison, days: i64) -> bool {
    match when {
        None => matches!(comparison, DayComparison::MoreThan),
        Some(w) => {
            let delta = days_since(w);
            match comparison {
                DayComparison::Within => delta <= days as f64,
                DayComparison::MoreThan => delta > days as f64,
            }
        }
    }
}

/// The account-creation timestamp encoded in a user's ULID.
fn ulid_created(user_id: Ulid) -> Option<Timestamp> {
    Timestamp::from_millisecond(user_id.timestamp_ms() as i64).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(title: &str) -> WelcomeMessage {
        WelcomeMessage {
            title: title.to_string(),
            ..Default::default()
        }
    }

    fn rule(conditions: Vec<Condition>, title: &str) -> WelcomeRule {
        WelcomeRule {
            conditions,
            message: msg(title),
        }
    }

    fn cond(negate: bool, kind: ConditionKind) -> Condition {
        Condition { negate, kind }
    }

    fn settings(rules: Vec<WelcomeRule>) -> HomeSettings {
        HomeSettings {
            welcome_rules: rules,
            welcome_default: msg("default"),
            ..Default::default()
        }
    }

    #[test]
    fn falls_back_to_default_when_nothing_matches() {
        let s = settings(vec![rule(
            vec![cond(false, ConditionKind::HasParticipation)],
            "participant",
        )]);
        let state = UserState {
            participation_total: Some(0),
            ..Default::default()
        };
        assert_eq!(s.resolve_welcome(&state).title, "default");
    }

    #[test]
    fn first_matching_rule_wins() {
        let s = settings(vec![
            rule(vec![cond(false, ConditionKind::IsStaff)], "staff"),
            rule(
                vec![cond(false, ConditionKind::HasParticipation)],
                "participant",
            ),
        ]);
        let state = UserState {
            is_staff: true,
            participation_total: Some(5),
            ..Default::default()
        };
        // Both rules match; the earlier (staff) rule wins.
        assert_eq!(s.resolve_welcome(&state).title, "staff");
    }

    #[test]
    fn conditions_are_anded_within_a_rule() {
        let s = settings(vec![rule(
            vec![
                cond(false, ConditionKind::IsStaff),
                cond(false, ConditionKind::HasParticipation),
            ],
            "staff-participant",
        )]);
        let staff_only = UserState {
            is_staff: true,
            participation_total: Some(0),
            ..Default::default()
        };
        assert_eq!(s.resolve_welcome(&staff_only).title, "default");

        let both = UserState {
            is_staff: true,
            participation_total: Some(1),
            ..Default::default()
        };
        assert_eq!(s.resolve_welcome(&both).title, "staff-participant");
    }

    #[test]
    fn negation_flips_a_condition() {
        let s = settings(vec![rule(
            vec![cond(
                true,
                ConditionKind::HasTag {
                    tag: "robotics".to_string(),
                },
            )],
            "not-robotics",
        )]);

        let tagged = UserState {
            tags: Some(vec!["robotics".to_string()]),
            ..Default::default()
        };
        assert_eq!(s.resolve_welcome(&tagged).title, "default");

        let untagged = UserState {
            tags: Some(vec!["astronomy".to_string()]),
            ..Default::default()
        };
        assert_eq!(s.resolve_welcome(&untagged).title, "not-robotics");
    }

    #[test]
    fn tag_match_is_case_insensitive() {
        let cond = cond(
            false,
            ConditionKind::HasTag {
                tag: "Robotics".to_string(),
            },
        );
        let state = UserState {
            tags: Some(vec!["robotics".to_string()]),
            ..Default::default()
        };
        assert!(cond.matches(&state));
    }

    #[test]
    fn missing_signal_never_matches() {
        // Rule needs participation data, but it was not fetched (None).
        let c = cond(false, ConditionKind::HasParticipation);
        let state = UserState::default();
        assert!(!c.matches(&state));
        // Negated, a missing-data condition still must not spuriously match on
        // the underlying test — NOT(false) is true here by design.
        let negated = cond(true, ConditionKind::HasParticipation);
        assert!(negated.matches(&state));
    }

    #[test]
    fn member_of_any_vs_specific_community() {
        let a = Ulid::from_string("01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap();
        let b = Ulid::from_string("01BX5ZZKBKACTAV9WEVGEMMVRZ").unwrap();

        let any = cond(
            false,
            ConditionKind::MemberOfCommunity { community_id: None },
        );
        let specific = cond(
            false,
            ConditionKind::MemberOfCommunity {
                community_id: Some(a),
            },
        );

        let member_of_b = UserState {
            communities: Some(vec![b]),
            ..Default::default()
        };
        assert!(any.matches(&member_of_b));
        assert!(!specific.matches(&member_of_b));

        let member_of_a = UserState {
            communities: Some(vec![a]),
            ..Default::default()
        };
        assert!(specific.matches(&member_of_a));

        let member_of_none = UserState {
            communities: Some(vec![]),
            ..Default::default()
        };
        assert!(!any.matches(&member_of_none));
    }

    #[test]
    fn day_comparisons_handle_never_happened() {
        // Never participated: "more than N days ago" is true, "within" is false.
        assert!(compare_days_since(None, DayComparison::MoreThan, 30));
        assert!(!compare_days_since(None, DayComparison::Within, 30));
    }

    #[test]
    fn required_signals_only_lists_referenced_fetches() {
        let s = settings(vec![
            rule(vec![cond(false, ConditionKind::HasParticipation)], "a"),
            rule(vec![cond(false, ConditionKind::IsStaff)], "b"),
        ]);
        let signals = s.required_signals();
        assert!(signals.contains(&Signal::Participation));
        assert!(!signals.contains(&Signal::Tags));
        assert!(!signals.contains(&Signal::Communities));
        assert!(!signals.contains(&Signal::LastView));
    }

    #[test]
    fn personalization_substitutes_handle() {
        let m = WelcomeMessage {
            title: "Hi {handle}".to_string(),
            body: "Welcome back, {handle}.".to_string(),
            ..Default::default()
        };
        let p = m.personalized("ada");
        assert_eq!(p.title, "Hi ada");
        assert_eq!(p.body, "Welcome back, ada.");
    }
}
