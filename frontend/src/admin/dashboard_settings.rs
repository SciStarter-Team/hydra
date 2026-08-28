//! Home Page / Dashboard admin.
//!
//! Edits the [`HomeSettings`] singleton: the welcome-message decision tree
//! (an ordered, first-match-wins list of rules) and the "near you" list
//! settings. Also surfaces a *second editing path* into the home-page content
//! managed elsewhere — featured schedules, news, staff picks, and communities —
//! by listing what is currently on the home page with links into each
//! subsystem's existing editor (the items are not moved or duplicated here).

use std::sync::Arc;

use common::message::user::BasicInfo;
use poem::{Endpoint, Route, get, handler};
use sailfish::TemplateOnce;
use serde::{Deserialize, Serialize};
use ulid::Ulid;

use crate::component::{AdminLayout, Component};
use crate::form::QsForm;
use crate::home::{
    Condition, ConditionKind, DayComparison, HomeSettings, WelcomeMessage, WelcomeRule,
};
use crate::middleware::authorization::Authenticated;
use crate::peering::LoadStorePeered;

// ==================== Rule editor: current-state view (Rust -> JSON) ====================

/// A condition flattened into a uniform shape for the client-side editor. Every
/// field is always present so the editor's data model is uniform regardless of
/// which `kind` is selected.
#[derive(Serialize)]
pub struct ConditionView {
    pub kind: String,
    pub negate: bool,
    pub tag: String,
    pub community_id: String,
    pub comparison: String,
    pub days: i64,
}

impl RuleView {
    /// An empty rule, for the editor's blank template.
    pub fn blank() -> Self {
        RuleView {
            title: String::new(),
            body: String::new(),
            cta_label: String::new(),
            cta_url: String::new(),
            conditions: Vec::new(),
        }
    }
}

impl ConditionView {
    /// A new condition, defaulting to the first selectable kind.
    pub fn blank() -> Self {
        ConditionView {
            kind: CONDITION_KINDS[0].0.to_string(),
            negate: false,
            tag: String::new(),
            community_id: String::new(),
            comparison: "Within".to_string(),
            days: 0,
        }
    }
}

/// One condition row in the welcome-rule editor.
///
/// Rendered both for the rules that exist and, with placeholder indices, into a
/// `<template>` that rule-builder.js clones when a condition is added -- so the
/// two can never drift.
#[derive(TemplateOnce)]
#[template(path = "admin/partials/welcome_condition.stpl.html")]
pub struct ConditionRow<'a> {
    pub rule_index: &'a str,
    pub index: &'a str,
    pub condition: &'a ConditionView,
    pub communities: &'a [CommunityOption],
}

/// One rule in the welcome-rule editor. See [`ConditionRow`].
#[derive(TemplateOnce)]
#[template(path = "admin/partials/welcome_rule.stpl.html")]
pub struct RuleRow<'a> {
    pub index: &'a str,
    /// What the heading shows. A real rule shows its position; the blank
    /// template carries a placeholder the script replaces.
    pub number: &'a str,
    pub rule: &'a RuleView,
    pub communities: &'a [CommunityOption],
}

/// The selectable condition kinds, paired with what the editor calls them.
/// The `value` must match the `kind` tag [`ConditionKind`] serializes to.
pub const CONDITION_KINDS: &[(&str, &str)] = &[
    ("HasParticipation", "has joined a project"),
    (
        "DaysSinceLastParticipation",
        "days since last participation",
    ),
    ("MemberOfCommunity", "member of community"),
    ("HasTag", "has interest tag"),
    ("DaysSinceLastView", "days since last visit"),
    ("LocationKnown", "location is known"),
    ("IsStaff", "is staff"),
    ("AccountAgeDays", "account age (days)"),
];

#[derive(Serialize)]
pub struct RuleView {
    pub title: String,
    pub body: String,
    pub cta_label: String,
    pub cta_url: String,
    pub conditions: Vec<ConditionView>,
}

fn comparison_str(comparison: DayComparison) -> String {
    match comparison {
        DayComparison::Within => "Within",
        DayComparison::MoreThan => "MoreThan",
    }
    .to_string()
}

fn condition_view(condition: &Condition) -> ConditionView {
    let mut view = ConditionView {
        kind: String::new(),
        negate: condition.negate,
        tag: String::new(),
        community_id: String::new(),
        comparison: "Within".to_string(),
        days: 0,
    };
    match &condition.kind {
        ConditionKind::HasParticipation => view.kind = "HasParticipation".to_string(),
        ConditionKind::DaysSinceLastParticipation { comparison, days } => {
            view.kind = "DaysSinceLastParticipation".to_string();
            view.comparison = comparison_str(*comparison);
            view.days = *days;
        }
        ConditionKind::MemberOfCommunity { community_id } => {
            view.kind = "MemberOfCommunity".to_string();
            view.community_id = community_id.map(|id| id.to_string()).unwrap_or_default();
        }
        ConditionKind::HasTag { tag } => {
            view.kind = "HasTag".to_string();
            view.tag = tag.clone();
        }
        ConditionKind::DaysSinceLastView { comparison, days } => {
            view.kind = "DaysSinceLastView".to_string();
            view.comparison = comparison_str(*comparison);
            view.days = *days;
        }
        ConditionKind::LocationKnown => view.kind = "LocationKnown".to_string(),
        ConditionKind::IsStaff => view.kind = "IsStaff".to_string(),
        ConditionKind::AccountAgeDays { comparison, days } => {
            view.kind = "AccountAgeDays".to_string();
            view.comparison = comparison_str(*comparison);
            view.days = *days;
        }
    }
    view
}

/// The current rules, flattened into the shape the editor's fields use.
fn rule_views(settings: &HomeSettings) -> Vec<RuleView> {
    settings
        .welcome_rules
        .iter()
        .map(|rule| RuleView {
            title: rule.message.title.clone(),
            body: rule.message.body.clone(),
            cta_label: rule.message.cta_label.clone(),
            cta_url: rule.message.cta_url.clone(),
            conditions: rule.conditions.iter().map(condition_view).collect(),
        })
        .collect()
}

// ==================== Rule editor: submitted form (form -> Rust) ====================

#[derive(Debug, Deserialize)]
struct WelcomeForm {
    #[serde(default)]
    rules: Vec<RuleForm>,
    #[serde(default)]
    default_title: String,
    #[serde(default)]
    default_body: String,
    #[serde(default)]
    default_cta_label: String,
    #[serde(default)]
    default_cta_url: String,
    /// Present (any value) when the "show nearby" checkbox is ticked.
    #[serde(default)]
    show_nearby: Option<String>,
    #[serde(default)]
    nearby_heading: String,
    #[serde(default)]
    nearby_limit: Option<i64>,
    /// Present (any value) when the "show concierge" checkbox is ticked.
    #[serde(default)]
    show_concierge: Option<String>,
    #[serde(default)]
    concierge_heading: String,
}

#[derive(Debug, Deserialize)]
struct RuleForm {
    #[serde(default)]
    title: String,
    #[serde(default)]
    body: String,
    #[serde(default)]
    cta_label: String,
    #[serde(default)]
    cta_url: String,
    #[serde(default)]
    conditions: Vec<ConditionForm>,
}

#[derive(Debug, Deserialize)]
struct ConditionForm {
    /// Present (any value) when the NOT checkbox is ticked.
    #[serde(default)]
    negate: Option<String>,
    #[serde(default)]
    kind: String,
    #[serde(default)]
    tag: String,
    #[serde(default)]
    community_id: String,
    #[serde(default)]
    comparison: String,
    #[serde(default)]
    days: String,
}

fn parse_comparison(value: &str) -> DayComparison {
    match value {
        "MoreThan" => DayComparison::MoreThan,
        _ => DayComparison::Within,
    }
}

fn parse_ulid_opt(value: &str) -> Option<Ulid> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        trimmed.parse().ok()
    }
}

impl ConditionForm {
    /// Build a typed [`Condition`], or `None` if the row is unusable (e.g. an
    /// empty tag or an unrecognized kind).
    fn into_condition(self) -> Option<Condition> {
        let days = || self.days.trim().parse().unwrap_or(0);
        let kind = match self.kind.as_str() {
            "HasParticipation" => ConditionKind::HasParticipation,
            "DaysSinceLastParticipation" => ConditionKind::DaysSinceLastParticipation {
                comparison: parse_comparison(&self.comparison),
                days: days(),
            },
            "MemberOfCommunity" => ConditionKind::MemberOfCommunity {
                community_id: parse_ulid_opt(&self.community_id),
            },
            "HasTag" => {
                let tag = self.tag.trim().to_string();
                if tag.is_empty() {
                    return None;
                }
                ConditionKind::HasTag { tag }
            }
            "DaysSinceLastView" => ConditionKind::DaysSinceLastView {
                comparison: parse_comparison(&self.comparison),
                days: days(),
            },
            "LocationKnown" => ConditionKind::LocationKnown,
            "IsStaff" => ConditionKind::IsStaff,
            "AccountAgeDays" => ConditionKind::AccountAgeDays {
                comparison: parse_comparison(&self.comparison),
                days: days(),
            },
            _ => return None,
        };
        Some(Condition {
            negate: self.negate.is_some(),
            kind,
        })
    }
}

impl RuleForm {
    /// Build a typed [`WelcomeRule`], or `None` for an empty message row (which
    /// the editor may leave behind).
    fn into_rule(self) -> Option<WelcomeRule> {
        let message = WelcomeMessage {
            title: self.title.trim().to_string(),
            body: self.body.trim().to_string(),
            cta_label: self.cta_label.trim().to_string(),
            cta_url: self.cta_url.trim().to_string(),
        };
        if message.is_empty() {
            return None;
        }
        let conditions = self
            .conditions
            .into_iter()
            .filter_map(ConditionForm::into_condition)
            .collect();
        Some(WelcomeRule {
            conditions,
            message,
        })
    }
}

fn to_settings(form: WelcomeForm) -> HomeSettings {
    let nearby_heading = form.nearby_heading.trim();
    let concierge_heading = form.concierge_heading.trim();
    HomeSettings {
        welcome_rules: form
            .rules
            .into_iter()
            .filter_map(RuleForm::into_rule)
            .collect(),
        welcome_default: WelcomeMessage {
            title: form.default_title.trim().to_string(),
            body: form.default_body.trim().to_string(),
            cta_label: form.default_cta_label.trim().to_string(),
            cta_url: form.default_cta_url.trim().to_string(),
        },
        show_nearby: form.show_nearby.is_some(),
        nearby_limit: form.nearby_limit.unwrap_or(6).clamp(1, 24),
        nearby_heading: if nearby_heading.is_empty() {
            "Projects & Events Near You".to_string()
        } else {
            nearby_heading.to_string()
        },
        show_concierge: form.show_concierge.is_some(),
        concierge_heading: if concierge_heading.is_empty() {
            crate::home::default_concierge_heading()
        } else {
            concierge_heading.to_string()
        },
    }
}

// ==================== Second editing path: managed content ====================

struct ManagedItem {
    label: String,
    sublabel: String,
    active: bool,
    edit_url: String,
}

struct ManagedSection {
    title: String,
    description: String,
    manage_url: String,
    new_url: String,
    items: Vec<ManagedItem>,
}

/// Build the "currently on the home page" listings, each linking into the
/// subsystem's own editor. Reuses each subsystem's existing read path.
async fn build_managed_sections() -> Vec<ManagedSection> {
    let schedules = crate::schedule::Schedule::list_featured_admin()
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|s| ManagedItem {
            label: s.title,
            sublabel: format!("{} – {}", s.start_date, s.end_date),
            active: s.active,
            edit_url: format!("/-admin/schedules/{}", s.schedule_id),
        })
        .collect();

    let news = crate::news::NewsItem::get_active_items()
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|n| ManagedItem {
            label: n.title,
            sublabel: n.display_date,
            active: n.active,
            edit_url: format!("/-admin/news/{}", n.news_id),
        })
        .collect();

    let picks = crate::staff_pick::StaffPick::get_active_picks()
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|p| ManagedItem {
            active: p.is_active(),
            edit_url: format!("/-admin/staff-picks/{}", p.id),
            label: p.label,
            sublabel: p.project_slug,
        })
        .collect();

    let communities = crate::communities::list_communities(true, true)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|c| ManagedItem {
            label: c.name,
            sublabel: format!("{} members", c.member_count),
            active: c.active,
            edit_url: format!("/-admin/communities/{}", c.community_id),
        })
        .collect();

    vec![
        ManagedSection {
            title: "Featured Projects & Events".to_string(),
            description: "Scheduled features shown in the home-page carousel.".to_string(),
            manage_url: "/-admin/schedules".to_string(),
            new_url: "/-admin/schedules/new".to_string(),
            items: schedules,
        },
        ManagedSection {
            title: "News".to_string(),
            description: "Active news items on the home page.".to_string(),
            manage_url: "/-admin/news".to_string(),
            new_url: "/-admin/news/new".to_string(),
            items: news,
        },
        ManagedSection {
            title: "Staff Picks".to_string(),
            description: "Currently-running staff picks.".to_string(),
            manage_url: "/-admin/staff-picks".to_string(),
            new_url: "/-admin/staff-picks/new".to_string(),
            items: picks,
        },
        ManagedSection {
            title: "Featured Communities".to_string(),
            description: "Featured communities shown on the home page.".to_string(),
            manage_url: "/-admin/communities".to_string(),
            new_url: "/-admin/communities/new".to_string(),
            items: communities,
        },
    ]
}

// ==================== Page ====================

/// A community option for the "member of community" condition dropdown.
pub struct CommunityOption {
    pub id: String,
    pub name: String,
}

#[derive(TemplateOnce)]
#[template(path = "admin/dashboard_settings.stpl.html")]
struct DashboardSettingsPage {
    saved: bool,
    rules: Vec<RuleView>,
    default_title: String,
    default_body: String,
    default_cta_label: String,
    default_cta_url: String,
    show_nearby: bool,
    nearby_heading: String,
    nearby_limit: i64,
    show_concierge: bool,
    concierge_heading: String,
    communities: Vec<CommunityOption>,
    sections: Vec<ManagedSection>,
}

async fn render_page(
    user_info: Option<Arc<BasicInfo>>,
    settings: &HomeSettings,
    saved: bool,
) -> Result<poem::Response, anyhow::Error> {
    let communities = crate::communities::list_communities(false, false)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|c| CommunityOption {
            id: c.community_id.to_string(),
            name: c.name,
        })
        .collect();

    let page = DashboardSettingsPage {
        saved,
        rules: rule_views(settings),
        default_title: settings.welcome_default.title.clone(),
        default_body: settings.welcome_default.body.clone(),
        default_cta_label: settings.welcome_default.cta_label.clone(),
        default_cta_url: settings.welcome_default.cta_url.clone(),
        show_nearby: settings.show_nearby,
        nearby_heading: settings.nearby_heading.clone(),
        nearby_limit: settings.nearby_limit,
        show_concierge: settings.show_concierge,
        concierge_heading: settings.concierge_heading.clone(),
        communities,
        sections: build_managed_sections().await,
    };

    let prepared = AdminLayout::new("Home Page / Dashboard", page)
        .with_sidebar_active("home-dashboard")
        .with_breadcrumb("Home Page", None)
        .prepare(user_info)
        .await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(prepared.render_once()?))
}

#[handler]
async fn edit(Authenticated(user_info): Authenticated) -> Result<poem::Response, anyhow::Error> {
    let settings = HomeSettings::current().await;
    render_page(user_info, &settings, false).await
}

#[handler]
async fn save(
    Authenticated(user_info): Authenticated,
    QsForm(form): QsForm<WelcomeForm>,
) -> Result<poem::Response, anyhow::Error> {
    let settings = to_settings(form);
    settings.store().await?;

    if let Some(user) = &user_info {
        common::service::log_activity(
            None,
            "home-settings-update",
            user.user_id,
            None::<Ulid>,
            format!("rules={}", settings.welcome_rules.len()),
        )
        .await?;
    }

    render_page(user_info, &settings, true).await
}

pub fn routes() -> impl Endpoint {
    Route::new().at("/", get(edit).post(save))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parse a form body the way `QsForm` does (serde_qs, max_depth 10).
    fn parse(qs: &str) -> WelcomeForm {
        serde_qs::Config::new(10, false)
            .deserialize_str(qs)
            .expect("form should parse")
    }

    #[test]
    fn parses_nested_rules_and_conditions() {
        let qs = "rules[0][title]=Hi&rules[0][body]=welcome\
                  &rules[0][conditions][0][kind]=HasTag&rules[0][conditions][0][tag]=robotics&rules[0][conditions][0][negate]=1\
                  &rules[0][conditions][1][kind]=DaysSinceLastView&rules[0][conditions][1][comparison]=MoreThan&rules[0][conditions][1][days]=30\
                  &default_title=Default&show_nearby=1&nearby_limit=8&nearby_heading=NearYou";
        let settings = to_settings(parse(qs));

        assert_eq!(settings.welcome_rules.len(), 1);
        let rule = &settings.welcome_rules[0];
        assert_eq!(rule.message.title, "Hi");
        assert_eq!(rule.conditions.len(), 2);

        assert!(rule.conditions[0].negate);
        assert!(
            matches!(&rule.conditions[0].kind, ConditionKind::HasTag { tag } if tag == "robotics")
        );

        assert!(!rule.conditions[1].negate);
        assert!(matches!(
            rule.conditions[1].kind,
            ConditionKind::DaysSinceLastView {
                comparison: DayComparison::MoreThan,
                days: 30
            }
        ));

        assert_eq!(settings.welcome_default.title, "Default");
        assert!(settings.show_nearby);
        assert_eq!(settings.nearby_limit, 8);
        assert_eq!(settings.nearby_heading, "NearYou");
    }

    #[test]
    fn drops_empty_rule_and_unchecked_checkbox() {
        // Empty message row is dropped; an absent show_nearby checkbox is false.
        let qs =
            "rules[0][title]=&rules[0][body]=&default_title=Only&nearby_limit=6&nearby_heading=H";
        let settings = to_settings(parse(qs));
        assert!(settings.welcome_rules.is_empty());
        assert!(!settings.show_nearby);
        assert_eq!(settings.welcome_default.title, "Only");
    }

    #[test]
    fn empty_tag_condition_is_dropped_but_rule_kept() {
        let qs =
            "rules[0][title]=T&rules[0][conditions][0][kind]=HasTag&rules[0][conditions][0][tag]=";
        let settings = to_settings(parse(qs));
        assert_eq!(settings.welcome_rules.len(), 1);
        assert!(settings.welcome_rules[0].conditions.is_empty());
    }

    #[test]
    fn member_of_any_when_community_id_blank() {
        let qs = "rules[0][title]=T&rules[0][conditions][0][kind]=MemberOfCommunity&rules[0][conditions][0][community_id]=";
        let settings = to_settings(parse(qs));
        assert!(matches!(
            settings.welcome_rules[0].conditions[0].kind,
            ConditionKind::MemberOfCommunity { community_id: None }
        ));
    }

    #[test]
    fn nearby_limit_is_clamped() {
        let settings = to_settings(parse("default_title=D&nearby_limit=999&nearby_heading=H"));
        assert_eq!(settings.nearby_limit, 24);
    }
}
