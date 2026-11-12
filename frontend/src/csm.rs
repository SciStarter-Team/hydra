use common::message::log::{
    ActsOfScienceCreateRequest, ActsOfScienceCreateResponse, ActsOfScienceSource,
    ActsToDateRequest, ActsToDateResponse, ContributorEntry, ContributorsRequest,
    ContributorsResponse,
};
use common::message::user::{BasicInfo, UserInfo};
use common::shared::db;
use poem::session::Session;
use poem::web::{Form, Json, Query, RealIp};
use poem::{Endpoint, Response, Route, get, handler};
use sailfish::TemplateOnce;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;

use crate::component::{Component, Head, StandardLayout, html_escape};
use crate::local_time::{TimeFormat, local_time};
use crate::middleware::authorization::Authenticated;
use crate::project::location::ANYWHERE;

// Shared fragment templates
#[derive(TemplateOnce)]
#[template(path = "pages/csm/hero.stpl.html")]
pub struct CsmHero {
    pub phase: CsmPhase,
    pub acts: Option<ActsToDateResponse>,
    pub phase_override: bool,
}

impl CsmHero {
    /// Compute test tube display values from acts data.
    /// Returns (fill_height_px, fill_top_px, counter_bottom_px, formatted_date, formatted_count, animation_duration_s, goal_reached, goal_reached_date)
    fn tube_values(&self) -> (f64, f64, f64, String, String, f64, bool, Option<String>) {
        let acts = match &self.acts {
            Some(a) => a,
            None => {
                return (
                    2.0,
                    40.0,
                    6.0,
                    String::new(),
                    String::new(),
                    0.1,
                    false,
                    None,
                );
            }
        };

        let capped = (acts.number as f64).min(2_500_000.0);
        let ratio = capped / 2_500_000.0;
        let base = ratio * 370.0;
        let fill_height_px = base + 2.0;
        let fill_top_px = base + 40.0;
        let counter_bottom_px = base + 6.0;
        let animation_duration_s = 0.1 + 2.9 * ratio;

        let formatted_date = format_acts_date(&acts.date);
        let formatted_count = crate::number::thousands(acts.number);

        let goal_reached = acts.date_reached.is_some();
        let goal_reached_date = acts
            .date_reached
            .as_ref()
            .map(|d| format!("{}!", format_acts_date(d)));

        (
            fill_height_px,
            fill_top_px,
            counter_bottom_px,
            formatted_date,
            formatted_count,
            animation_duration_s,
            goal_reached,
            goal_reached_date,
        )
    }
}

fn format_acts_date(date_str: &str) -> String {
    // Parse "YYYY-MM-DD" and format as "April 9"
    let parts: Vec<&str> = date_str.split('-').collect();
    if parts.len() < 3 {
        return date_str.to_string();
    }
    let month = match parts[1] {
        "01" => "January",
        "02" => "February",
        "03" => "March",
        "04" => "April",
        "05" => "May",
        "06" => "June",
        "07" => "July",
        "08" => "August",
        "09" => "September",
        "10" => "October",
        "11" => "November",
        "12" => "December",
        _ => return date_str.to_string(),
    };
    let day = parts[2].trim_start_matches('0');
    format!("{month} {day}")
}

#[derive(TemplateOnce)]
#[template(path = "pages/csm/nav.stpl.html")]
pub struct CsmNav {
    pub phase: CsmPhase,
    pub active: &'static str,
}

#[derive(TemplateOnce)]
#[template(path = "pages/csm/sponsors.stpl.html")]
pub struct CsmSponsors;

// Phase detection
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CsmPhase {
    PreApril,
    April,
    AprilReached,
    May,
}

impl CsmPhase {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "pre-april" => Some(CsmPhase::PreApril),
            "april" => Some(CsmPhase::April),
            "april-reached" => Some(CsmPhase::AprilReached),
            "may" => Some(CsmPhase::May),
            _ => None,
        }
    }
}

pub struct CsmPhaseInfo {
    pub phase: CsmPhase,
    pub acts_to_date: Option<ActsToDateResponse>,
    pub phase_override: bool,
}

impl CsmPhaseInfo {
    pub async fn detect(override_phase: Option<&str>) -> Self {
        if let Some(phase) = override_phase.and_then(CsmPhase::from_str) {
            let today = jiff::Zoned::now().date().to_string();
            let (acts, phase_override) = match phase {
                CsmPhase::April => (
                    Some(ActsToDateResponse {
                        date: today,
                        number: 1_234_567,
                        date_reached: None,
                    }),
                    true,
                ),
                CsmPhase::AprilReached => (
                    Some(ActsToDateResponse {
                        date: today.clone(),
                        number: 1_234_567,
                        date_reached: Some(today),
                    }),
                    true,
                ),
                CsmPhase::May => (
                    Some(ActsToDateResponse {
                        date: today.clone(),
                        number: 9_999_999,
                        date_reached: Some(today),
                    }),
                    true,
                ),
                CsmPhase::PreApril => (None, true),
            };
            return CsmPhaseInfo {
                phase,
                acts_to_date: acts,
                phase_override,
            };
        }

        let today = jiff::Zoned::now().date();
        let month: i32 = today.month().into();

        if month < 4 {
            return CsmPhaseInfo {
                phase: CsmPhase::PreApril,
                acts_to_date: None,
                phase_override: false,
            };
        }

        if month >= 6 {
            return CsmPhaseInfo {
                phase: CsmPhase::PreApril,
                acts_to_date: None,
                phase_override: false,
            };
        }

        if month == 5 {
            let acts = acts_to_date(None).await.ok();
            return CsmPhaseInfo {
                phase: CsmPhase::May,
                acts_to_date: acts,
                phase_override: false,
            };
        }

        // month == 4, check if goal reached
        match acts_to_date(None).await {
            Ok(response) => {
                let phase = if response.date_reached.is_some() {
                    CsmPhase::AprilReached
                } else {
                    CsmPhase::April
                };
                CsmPhaseInfo {
                    phase,
                    acts_to_date: Some(response),
                    phase_override: false,
                }
            }
            Err(_) => CsmPhaseInfo {
                phase: CsmPhase::April,
                acts_to_date: None,
                phase_override: false,
            },
        }
    }
}

#[derive(Deserialize)]
struct CsmQuery {
    phase: Option<String>,
}

async fn acts_to_date(year: Option<i32>) -> Result<ActsToDateResponse, anyhow::Error> {
    let response: ActsToDateResponse =
        common::service::request("log.acts.to_date", ActsToDateRequest { year }).await?;
    Ok(response)
}

// ============================================================================
// CSM Home Page
// ============================================================================

#[derive(TemplateOnce)]
#[template(path = "pages/csm_home.stpl.html")]
struct CsmHomePage {
    head: <Head as Component>::Prepared,
    layout: <StandardLayout<CsmHomeContent> as Component>::Prepared,
}

#[derive(TemplateOnce)]
#[template(path = "pages/csm_home_content.stpl.html")]
struct CsmHomeContent {
    phase: CsmPhase,
    acts: Option<ActsToDateResponse>,
    phase_override: bool,
    show_social_feed: bool,
}

#[handler]
async fn csm_home(
    _session: &Session,
    RealIp(_client_ip): RealIp,
    Authenticated(user_info): Authenticated,
    Query(query): Query<CsmQuery>,
) -> Result<Response, anyhow::Error> {
    let info = CsmPhaseInfo::detect(query.phase.as_deref()).await;

    let show_social_feed = {
        let today = jiff::Zoned::now().date();
        let month: i32 = today.month().into();
        let day: i32 = today.day().into();
        month == 3 && day >= 25
    };

    let layout = StandardLayout::new(
        "/citizensciencemonth",
        CsmHomeContent {
            phase: info.phase,
            acts: info.acts_to_date,
            phase_override: info.phase_override,
            show_social_feed,
        },
    )
    .prepare(user_info.clone())
    .await?;

    CsmHomePage {
        head: Head::default().with_slick().prepare(None).await?,
        layout,
    }
    .into_response(None)
    .await
}

// ============================================================================
// CSM Participate Page
// ============================================================================

#[derive(TemplateOnce)]
#[template(path = "pages/csm_participate.stpl.html")]
struct CsmParticipatePage {
    head: <Head as Component>::Prepared,
    layout: <StandardLayout<CsmParticipateContent> as Component>::Prepared,
}

#[derive(TemplateOnce)]
#[template(path = "pages/csm_participate_content.stpl.html")]
struct CsmParticipateContent {
    phase: CsmPhase,
    acts: Option<ActsToDateResponse>,
    phase_override: bool,
}

#[handler]
async fn csm_participate(
    _session: &Session,
    RealIp(_client_ip): RealIp,
    Authenticated(user_info): Authenticated,
    Query(query): Query<CsmQuery>,
) -> Result<Response, anyhow::Error> {
    let info = CsmPhaseInfo::detect(query.phase.as_deref()).await;

    let layout = StandardLayout::new(
        "/citizensciencemonth/participate",
        CsmParticipateContent {
            phase: info.phase,
            acts: info.acts_to_date,
            phase_override: info.phase_override,
        },
    )
    .prepare(user_info.clone())
    .await?;

    CsmParticipatePage {
        head: Head::default()
            .with_slick()
            .with_geosearch()
            .prepare(None)
            .await?,
        layout,
    }
    .into_response(None)
    .await
}

// ============================================================================
// CSM Report Page
// ============================================================================

#[derive(TemplateOnce)]
#[template(path = "pages/csm_report.stpl.html")]
struct CsmReportPage {
    head: <Head as Component>::Prepared,
    layout: <StandardLayout<CsmReportContent> as Component>::Prepared,
}

#[derive(TemplateOnce)]
#[template(path = "pages/csm_report_content.stpl.html")]
struct CsmReportContent {
    user_info: Option<Arc<BasicInfo>>,
    error_message: Option<String>,
    success: bool,
    reporting_open: bool,
}

#[handler]
async fn csm_report(
    _session: &Session,
    RealIp(_client_ip): RealIp,
    Authenticated(user_info): Authenticated,
) -> Result<Response, anyhow::Error> {
    let today = jiff::Zoned::now().date();
    let month: i32 = today.month().into();
    let day: i32 = today.day().into();
    let reporting_open = month == 4 || (month == 5 && day <= 3) || user_info.is_staff();

    let layout = StandardLayout::new(
        "/citizensciencemonth/report",
        CsmReportContent {
            user_info: user_info.clone(),
            error_message: None,
            success: false,
            reporting_open,
        },
    )
    .prepare(user_info.clone())
    .await?;

    CsmReportPage {
        head: Head::default().with_slick().prepare(None).await?,
        layout,
    }
    .into_response(None)
    .await
}

#[derive(Deserialize)]
struct ReportForm {
    report_type: String,
    email: Option<String>,
    name: String,
    act: String,
    contributions: Option<i64>,
    num_others: Option<i64>,
    organization: Option<String>,
    project: Option<String>,
    others: Option<String>,
    notes: Option<String>,
    country: Option<String>,
    newsletter: Option<String>,
}

#[handler]
async fn csm_report_submit(
    _session: &Session,
    RealIp(_client_ip): RealIp,
    Authenticated(user_info): Authenticated,
    Form(form): Form<ReportForm>,
) -> Result<Response, anyhow::Error> {
    let is_group = form.report_type == "group";
    let contributions = form.contributions.unwrap_or(1);
    let num_others = if is_group {
        form.num_others.unwrap_or(contributions)
    } else {
        0
    };

    let request = ActsOfScienceCreateRequest {
        user_id: user_info.as_ref().map(|u| u.user_id),
        email: form.email,
        country: form.country,
        newsletter: form.newsletter.is_some(),
        name: Some(form.name),
        contributions,
        act: Some(form.act),
        organization: form.organization,
        num_others,
        others: form.others,
        project: form.project,
        notes: form.notes,
        source: ActsOfScienceSource::Form,
        entity_slug: None,
    };

    let result: Result<ActsOfScienceCreateResponse, _> =
        common::service::request("log.acts.create", request).await;

    let (error_message, success) = match result {
        Ok(_) => (None, true),
        Err(e) => (Some(e.to_string()), false),
    };

    let today = jiff::Zoned::now().date();
    let month: i32 = today.month().into();
    let day: i32 = today.day().into();
    let reporting_open = month == 4 || (month == 5 && day <= 3) || user_info.is_staff();

    let layout = StandardLayout::new(
        "/citizensciencemonth/report",
        CsmReportContent {
            user_info: user_info.clone(),
            error_message,
            success,
            reporting_open,
        },
    )
    .prepare(user_info.clone())
    .await?;

    CsmReportPage {
        head: Head::default().with_slick().prepare(None).await?,
        layout,
    }
    .into_response(None)
    .await
}

// ============================================================================
// API: Acts to Date (JSON)
// ============================================================================

#[handler]
pub async fn acts_to_date_api(
    Query(query): Query<ActsToDateRequest>,
) -> Result<Json<ActsToDateResponse>, anyhow::Error> {
    let response = acts_to_date(query.year).await?;
    Ok(Json(response))
}

// ============================================================================
// HTMX: Wall of Fame Fragment
// ============================================================================

#[derive(Deserialize)]
struct WallOfFameQuery {
    mode: Option<String>,
    page: Option<i32>,
    letter: Option<String>,
    phase_override: Option<bool>,
}

pub struct GroupedContributor {
    pub date: Option<String>,
    pub act: Option<String>,
    pub name: Option<String>,
    pub project: Option<String>,
    pub organization: Option<String>,
    pub entity_slug: Option<String>,
    pub contributions: i32,
    pub featured: bool,
}

fn group_contributors(entries: Vec<ContributorEntry>) -> Vec<GroupedContributor> {
    let mut groups: HashMap<
        (
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
        ),
        GroupedContributor,
    > = HashMap::new();

    for entry in entries {
        let is_anon = entry.name.as_deref().map_or(true, |n| {
            n.trim().is_empty() || n.eq_ignore_ascii_case("anon")
        });
        if is_anon {
            continue;
        }

        let key = (
            entry.date.clone(),
            entry.act.clone(),
            entry.name.clone(),
            entry.project.clone(),
        );

        groups
            .entry(key)
            .and_modify(|g| {
                g.contributions += 1;
                g.featured = g.featured || entry.featured;
            })
            .or_insert(GroupedContributor {
                date: entry.date,
                act: entry.act,
                name: entry.name,
                project: entry.project,
                organization: entry.organization,
                entity_slug: entry.entity_slug,
                contributions: 1,
                featured: entry.featured,
            });
    }

    let mut contributors: Vec<GroupedContributor> = groups.into_values().collect();
    contributors.sort_by_key(|g| !g.featured);
    contributors
}

#[derive(TemplateOnce)]
#[template(path = "pages/csm/wall_of_fame.stpl.html")]
struct WallOfFameFragment {
    contributors: Vec<GroupedContributor>,
    pages: i32,
    mode: String,
    page: i32,
    letter: String,
    phase_override: bool,
}

fn fake_contributors() -> Vec<ContributorEntry> {
    vec![
        ContributorEntry {
            date: Some("Apr 17".into()),
            act: Some("Attended an event".into()),
            name: Some("David K.".into()),
            project: Some("BioBlitz".into()),
            organization: Some("National Geographic".into()),
            entity_slug: None,
            featured: false,
        },
        ContributorEntry {
            date: Some("Apr 15".into()),
            act: Some("Completed a project".into()),
            name: Some("Sarah L.".into()),
            project: Some("CoCoRaHS".into()),
            organization: Some("Colorado State University".into()),
            entity_slug: None,
            featured: false,
        },
        ContributorEntry {
            date: Some("Apr 13".into()),
            act: Some("Submitted observations".into()),
            name: Some("Kenji M.".into()),
            project: Some("eBird".into()),
            organization: Some("Cornell Lab of Ornithology".into()),
            entity_slug: None,
            featured: false,
        },
        ContributorEntry {
            date: Some("Apr 11".into()),
            act: Some("Completed training".into()),
            name: Some("Emily W.".into()),
            project: Some("Foundations of Citizen Science".into()),
            organization: Some("SciStarter".into()),
            entity_slug: None,
            featured: false,
        },
        ContributorEntry {
            date: Some("Apr 9".into()),
            act: Some("Analyzed data".into()),
            name: Some("Carlos R.".into()),
            project: Some("Zooniverse".into()),
            organization: Some("University of Oxford".into()),
            entity_slug: None,
            featured: false,
        },
        ContributorEntry {
            date: Some("Apr 7".into()),
            act: Some("Submitted observations".into()),
            name: Some("Priya S.".into()),
            project: Some("iNaturalist".into()),
            organization: Some("California Academy of Sciences".into()),
            entity_slug: None,
            featured: false,
        },
        ContributorEntry {
            date: Some("Apr 5".into()),
            act: Some("Attended an event".into()),
            name: Some("James T.".into()),
            project: Some("City Nature Challenge".into()),
            organization: Some("Natural History Museum".into()),
            entity_slug: None,
            featured: false,
        },
        ContributorEntry {
            date: Some("Apr 3".into()),
            act: Some("Completed a project".into()),
            name: Some("Maria G.".into()),
            project: Some("Globe Observer".into()),
            organization: Some("NASA".into()),
            entity_slug: None,
            featured: false,
        },
    ]
}

#[handler]
pub async fn csm_wall_of_fame(
    Query(query): Query<WallOfFameQuery>,
) -> Result<Response, anyhow::Error> {
    let mode = query.mode.unwrap_or_else(|| "date".to_string());
    let page = query.page.unwrap_or(1).max(1);
    let letter = query.letter.unwrap_or_else(|| "A".to_string());
    let phase_override = query.phase_override.unwrap_or(false);

    let (contributors, pages) = if phase_override {
        (fake_contributors(), 3)
    } else {
        let request = ContributorsRequest {
            mode: Some(mode.clone()),
            page: Some(page),
            letter: if mode == "name" {
                Some(letter.clone())
            } else {
                None
            },
        };
        let response: ContributorsResponse =
            common::service::request("log.acts.contributors", request).await?;
        (response.contributors, response.pages)
    };

    let contributors = group_contributors(contributors);

    let html = WallOfFameFragment {
        contributors,
        pages,
        mode,
        page,
        letter,
        phase_override,
    }
    .render_once()?;

    Ok(Response::builder().content_type("text/html").body(html))
}

// ============================================================================
// HTMX: Staff Picks Fragment
// ============================================================================

fn truncate_string(src: impl Into<String>, at: usize) -> String {
    let mut ret = src.into();
    ret.truncate(at);
    ret
}

#[derive(TemplateOnce)]
#[template(path = "pages/csm/picks.stpl.html")]
struct CsmPicksFragment {
    picks: Vec<crate::staff_pick::StaffPick>,
}

#[handler]
pub async fn csm_picks() -> Result<Response, anyhow::Error> {
    let picks = crate::staff_pick::StaffPick::get_active_picks()
        .await
        .unwrap_or_default();

    let html = CsmPicksFragment { picks }.render_once()?;

    Ok(Response::builder().content_type("text/html").body(html))
}

// ============================================================================
// HTMX: CSM Events Fragment
// ============================================================================

#[derive(Deserialize)]
pub struct CsmEventsQuery {
    pub lat: Option<f64>,
    pub lng: Option<f64>,
    pub location_name: Option<String>,
    pub start_date: Option<String>,
    pub keywords: Option<String>,
    pub online_only: Option<bool>,
    pub signature_only: Option<bool>,
    pub page: Option<i64>,
}

struct CsmEvent {
    slug: String,
    name: String,
    begin: String,
    end: String,
    entirely_online: bool,
    /// Held anywhere in the world: online, or in person wherever people are.
    anywhere: bool,
    signature: bool,
    address: String,
    city: String,
    state: String,
}

impl CsmEvent {
    fn location_display(&self) -> String {
        if self.entirely_online {
            return "Online".to_string();
        }
        if self.anywhere {
            return "Anywhere".to_string();
        }
        let mut parts = Vec::new();
        if !self.address.is_empty() {
            parts.push(self.address.as_str());
        }
        if !self.city.is_empty() {
            parts.push(self.city.as_str());
        }
        if !self.state.is_empty() {
            parts.push(self.state.as_str());
        }
        if parts.is_empty() {
            "Location TBD".to_string()
        } else {
            parts.join(", ")
        }
    }
}

#[derive(TemplateOnce)]
#[template(path = "pages/csm/events.stpl.html")]
struct CsmEventsFragment {
    events: Vec<CsmEvent>,
    query: CsmEventsQuery,
    page: i64,
    total_pages: i64,
    geosearch_key: String,
}

const EVENTS_PAGE_SIZE: i64 = 9;

fn get_april_range() -> (String, String) {
    let today = jiff::Zoned::now().date();
    let month: i32 = today.month().into();
    let year: i32 = today.year().into();

    let april_year = if month > 4 { year + 1 } else { year };

    let start = if month == 4 {
        today.to_string()
    } else {
        format!("{april_year}-04-01")
    };
    let end = format!("{april_year}-04-30T23:59:59");
    (start, end)
}

/// 100 miles in meters — radius for spatial location search
const LOCATION_RADIUS_METERS: f64 = 160934.0;

/// Push the condition for an event near `(lat, lng)`, as orrery decides it:
/// its point within [`LOCATION_RADIUS_METERS`], or its regions covering the
/// searcher, or -- when it has neither to measure -- that it can be joined
/// online, entirely or as well as on site. An event held anywhere in the
/// world is near everyone, whatever place it carries.
fn push_near(qb: &mut sqlx::QueryBuilder<sqlx::Sqlite>, lat: f64, lng: f64) {
    let push_searcher = |qb: &mut sqlx::QueryBuilder<sqlx::Sqlite>| {
        qb.push("MakePoint(");
        qb.push_bind(lng);
        qb.push(", ");
        qb.push_bind(lat);
        qb.push(", 4326)");
    };

    qb.push("((ev.point IS NOT NULL AND ST_Distance(ev.point, ");
    push_searcher(qb);
    qb.push(", 1) < ");
    qb.push_bind(LOCATION_RADIUS_METERS);
    qb.push(") OR (ev.regions IS NOT NULL AND ST_Intersects(");
    push_searcher(qb);
    qb.push(
        ", ST_MakeValid(ev.regions)) = 1) \
         OR (ev.point IS NULL AND ev.regions IS NULL \
             AND (ev.entirely_online = 1 OR ev.also_online = 1)) \
         OR ev.location_type = ",
    );
    qb.push_bind(ANYWHERE);
    qb.push(")");
}

#[handler]
pub async fn csm_events(Query(query): Query<CsmEventsQuery>) -> Result<Response, anyhow::Error> {
    let page = query.page.unwrap_or(1).max(1);
    let offset = (page - 1) * EVENTS_PAGE_SIZE;

    let (default_start, april_end) = get_april_range();
    let start_date = query
        .start_date
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or(&default_start);
    let online_only = query.online_only.unwrap_or(false);
    let signature_only = query.signature_only.unwrap_or(false);
    let keywords_filter = query
        .keywords
        .as_deref()
        .filter(|k| !k.is_empty())
        .map(|k| format!("%{}%", k.to_lowercase()));
    let has_keywords = keywords_filter.is_some();
    let keywords_like = keywords_filter.unwrap_or_default();

    let has_location = query.lat.is_some() && query.lng.is_some();
    let lat = query.lat.unwrap_or(0.0);
    let lng = query.lng.unwrap_or(0.0);

    // Signature events are the Acts of Science Connected ones.
    let signature_type = crate::choice::ACTS_OF_SCIENCE_CONNECTED;

    // Build WHERE clause shared between count and results queries
    fn push_where_clause(
        qb: &mut sqlx::QueryBuilder<sqlx::Sqlite>,
        start_date: &str,
        april_end: &str,
        online_only: bool,
        signature_only: bool,
        signature_type: (crate::choice::ChoiceList, &str),
        has_keywords: bool,
        keywords_like: &str,
        has_location: bool,
        lat: f64,
        lng: f64,
    ) {
        qb.push(" WHERE e.kind = 'Event' AND e.publish = 1 AND ei.begin >= ");
        qb.push_bind(start_date.to_string());
        qb.push(" AND ei.begin <= ");
        qb.push_bind(april_end.to_string());

        // Whatever can be joined online, an event held in person and online
        // too among it.
        if online_only {
            qb.push(" AND (ev.entirely_online = 1 OR ev.also_online = 1)");
        }
        if signature_only {
            let (list, value) = signature_type;
            qb.push(
                " AND EXISTS (SELECT 1 FROM choice_assignments a \
                 WHERE a.target = ev.event_id AND a.list = ",
            );
            qb.push_bind(list);
            qb.push(" AND a.value = ");
            qb.push_bind(value.to_string());
            qb.push(")");
        }
        if has_keywords {
            qb.push(" AND lower(ev.name) LIKE ");
            qb.push_bind(keywords_like.to_string());
        }
        if has_location {
            qb.push(" AND ");
            push_near(qb, lat, lng);
        }
    }

    // Count query
    let mut count_qb: sqlx::QueryBuilder<sqlx::Sqlite> = sqlx::QueryBuilder::new(
        "SELECT COUNT(DISTINCT ev.event_id) FROM events ev \
         JOIN entities e ON e.target = ev.event_id \
         JOIN event_instances ei ON ei.event_id = ev.event_id",
    );
    push_where_clause(
        &mut count_qb,
        start_date,
        &april_end,
        online_only,
        signature_only,
        signature_type,
        has_keywords,
        &keywords_like,
        has_location,
        lat,
        lng,
    );

    let total_count: i64 = count_qb.build_query_scalar().fetch_one(db()).await?;

    let total_pages = (total_count + EVENTS_PAGE_SIZE - 1) / EVENTS_PAGE_SIZE;

    // Results query
    let mut results_qb: sqlx::QueryBuilder<sqlx::Sqlite> = sqlx::QueryBuilder::new(
        "SELECT e.slug, ev.name, MIN(ei.begin), MIN(ei.end), \
         ev.entirely_online, ev.location_type = ",
    );
    results_qb.push_bind(ANYWHERE);
    results_qb.push(
        ", \
         EXISTS (SELECT 1 FROM choice_assignments a \
                 WHERE a.target = ev.event_id AND a.list = ",
    );
    results_qb.push_bind(signature_type.0);
    results_qb.push(" AND a.value = ");
    results_qb.push_bind(signature_type.1.to_string());
    results_qb.push(
        "), ev.address, ev.city, ev.state \
         FROM events ev \
         JOIN entities e ON e.target = ev.event_id \
         JOIN event_instances ei ON ei.event_id = ev.event_id",
    );
    push_where_clause(
        &mut results_qb,
        start_date,
        &april_end,
        online_only,
        signature_only,
        signature_type,
        has_keywords,
        &keywords_like,
        has_location,
        lat,
        lng,
    );
    results_qb.push(" GROUP BY ev.event_id ORDER BY MIN(ei.begin) ASC, ev.name ASC LIMIT ");
    results_qb.push_bind(EVENTS_PAGE_SIZE);
    results_qb.push(" OFFSET ");
    results_qb.push_bind(offset);

    let rows = results_qb
        .build_query_as::<(
            String,
            String,
            String,
            String,
            bool,
            bool,
            bool,
            String,
            String,
            String,
        )>()
        .fetch_all(db())
        .await?;

    let events: Vec<CsmEvent> = rows
        .into_iter()
        .map(|r| CsmEvent {
            slug: r.0,
            name: r.1,
            begin: r.2,
            end: r.3,
            entirely_online: r.4,
            anywhere: r.5,
            signature: r.6,
            address: r.7,
            city: r.8,
            state: r.9,
        })
        .collect();

    let html = CsmEventsFragment {
        events,
        query: CsmEventsQuery {
            lat: query.lat,
            lng: query.lng,
            location_name: query.location_name,
            start_date: query.start_date,
            keywords: query.keywords,
            online_only: Some(online_only),
            signature_only: Some(signature_only),
            page: Some(page),
        },
        page,
        total_pages,
        geosearch_key: env!("GEOSEARCH_KEY").to_string(),
    }
    .render_once()?;

    Ok(Response::builder().content_type("text/html").body(html))
}

// ============================================================================
// HTMX: CSM Project/Event Autocomplete (April-filtered)
// ============================================================================

#[derive(Deserialize)]
struct CsmAutocompleteQuery {
    project: Option<String>,
}

#[handler]
pub async fn csm_autocomplete_projects(
    Query(query): Query<CsmAutocompleteQuery>,
) -> Result<Response, anyhow::Error> {
    let search_term = query.project.as_deref().unwrap_or("").trim().to_string();
    if search_term.len() < 3 {
        return Ok(Response::builder()
            .content_type("text/html")
            .body(r#"<datalist id="projects"></datalist>"#));
    }

    let today = jiff::Zoned::now().date();
    let year: i32 = today.year().into();
    let april_start = format!("{}-04-01", year);
    let april_end = format!("{}-04-30T23:59:59", year);
    let search_pattern = format!("%{}%", search_term);

    // Search published projects overlapping April
    let project_rows = sqlx::query_as::<_, (String,)>(
        "SELECT p.name FROM projects p \
         JOIN entities e ON e.target = p.project_id AND e.kind = 'Project' \
         WHERE e.publish = 1 \
         AND p.name LIKE ?1 \
         AND (p.begin IS NULL OR p.begin = '' OR p.begin <= ?2) \
         AND (p.end IS NULL OR p.end = '' OR p.end >= ?3) \
         ORDER BY p.name LIMIT 10",
    )
    .bind(&search_pattern)
    .bind(&april_end)
    .bind(&april_start)
    .fetch_all(db())
    .await
    .unwrap_or_default();

    // Search published events with instances overlapping April
    let event_rows = sqlx::query_as::<_, (String,)>(
        "SELECT DISTINCT ev.name FROM events ev \
         JOIN entities e ON e.target = ev.event_id AND e.kind = 'Event' \
         JOIN event_instances ei ON ei.event_id = ev.event_id \
         WHERE e.publish = 1 \
         AND ev.name LIKE ?1 \
         AND ei.begin <= ?2 \
         AND ei.end >= ?3 \
         ORDER BY ev.name LIMIT 10",
    )
    .bind(&search_pattern)
    .bind(&april_end)
    .bind(&april_start)
    .fetch_all(db())
    .await
    .unwrap_or_default();

    let mut names: Vec<String> = project_rows
        .into_iter()
        .chain(event_rows.into_iter())
        .map(|(name,)| name)
        .collect();
    names.sort();
    names.truncate(20);

    let options: String = names
        .iter()
        .map(|n| format!(r#"<option value="{}"></option>"#, html_escape(n)))
        .collect();

    Ok(Response::builder()
        .content_type("text/html")
        .body(format!(r#"<datalist id="projects">{}</datalist>"#, options)))
}

// ============================================================================
// HTMX: CSM Country Autocomplete
// ============================================================================

#[derive(Deserialize)]
struct CsmCountryQuery {
    country: Option<String>,
}

const COUNTRIES: &[&str] = &include!("countries.txt");

#[handler]
pub async fn csm_autocomplete_countries(
    Query(query): Query<CsmCountryQuery>,
) -> Result<Response, anyhow::Error> {
    let search_term = query.country.as_deref().unwrap_or("").trim().to_lowercase();
    if search_term.len() < 2 {
        return Ok(Response::builder()
            .content_type("text/html")
            .body(r#"<datalist id="countries"></datalist>"#));
    }

    let abbreviations: &[(&str, &str)] = &[
        ("usa", "United States of America"),
        ("us", "United States of America"),
        ("uk", "United Kingdom of Great Britain and Northern Ireland"),
        ("uae", "United Arab Emirates"),
    ];

    let mut matches: Vec<&str> = COUNTRIES
        .iter()
        .filter(|c| !c.is_empty() && c.to_lowercase().contains(&search_term))
        .copied()
        .collect();

    if let Some((_, full)) = abbreviations.iter().find(|(abbr, _)| *abbr == search_term) {
        if !matches.iter().any(|m| m == full) {
            matches.insert(0, full);
        }
    }

    matches.truncate(20);

    let options: String = matches
        .iter()
        .map(|c| format!(r#"<option value="{}"></option>"#, html_escape(c)))
        .collect();

    Ok(Response::builder().content_type("text/html").body(format!(
        r#"<datalist id="countries">{}</datalist>"#,
        options
    )))
}

// ============================================================================
// Routes
// ============================================================================

pub fn routes() -> impl Endpoint {
    Route::new()
        .at("/", get(csm_home))
        .at("/participate", get(csm_participate))
        .at("/report", get(csm_report).post(csm_report_submit))
}

#[cfg(test)]
mod tests {
    use assert2::check;
    use sqlx::SqlitePool;
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

    /// The searcher, and a point about 111 km due north, inside the radius,
    /// and one about 222 km north, outside it.
    const LAT: f64 = -35.0;
    const LNG: f64 = -175.0;
    const INSIDE: &str = "POINT(-175 -34)";
    const OUTSIDE: &str = "POINT(-175 -33)";

    /// A box around the searcher, and one well away from them.
    const AROUND: &str = "MULTIPOLYGON(((-176 -36, -174 -36, -174 -34, -176 -34, -176 -36)))";
    const AWAY: &str = "MULTIPOLYGON(((10 10, 11 10, 11 11, 10 11, 10 10)))";

    /// A scratch database holding one event per row of `events`: its name,
    /// regions, point, whether it is entirely online and whether also online.
    async fn database(events: &[(&str, Option<&str>, Option<&str>, bool, bool)]) -> SqlitePool {
        // SAFETY: mod_spatialite is a trusted extension, the same one the
        // application loads.
        let options = unsafe { SqliteConnectOptions::new().extension("mod_spatialite") };

        // One connection, because every connection to ":memory:" is a
        // database of its own.
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options.filename(":memory:"))
            .await
            .unwrap();

        for statement in [
            "SELECT InitSpatialMetadata(1)",
            "CREATE TABLE events (name TEXT, regions BLOB, point BLOB, \
             entirely_online INTEGER, also_online INTEGER, \
             location_type TEXT NOT NULL DEFAULT '')",
        ] {
            sqlx::query(statement).execute(&pool).await.unwrap();
        }

        for (name, regions, point, entirely_online, also_online) in events {
            sqlx::query(
                "INSERT INTO events (name, regions, point, entirely_online, also_online) \
                 VALUES (?, GeomFromText(?, 4326), GeomFromText(?, 4326), ?, ?)",
            )
            .bind(name)
            .bind(regions)
            .bind(point)
            .bind(entirely_online)
            .bind(also_online)
            .execute(&pool)
            .await
            .unwrap();
        }

        pool
    }

    /// The events near the searcher, by name.
    async fn near(pool: &SqlitePool) -> Vec<String> {
        let mut qb = sqlx::QueryBuilder::new("SELECT ev.name FROM events ev WHERE ");
        super::push_near(&mut qb, LAT, LNG);
        qb.push(" ORDER BY ev.name");

        qb.build_query_scalar().fetch_all(pool).await.unwrap()
    }

    #[tokio::test]
    async fn an_event_is_near_by_its_point_or_its_regions() {
        let pool = database(&[
            ("close point", None, Some(INSIDE), false, false),
            ("far point", None, Some(OUTSIDE), false, false),
            ("covering", Some(AROUND), None, false, false),
            ("elsewhere", Some(AWAY), None, false, false),
            ("nowhere", None, None, false, false),
        ])
        .await;

        check!(near(&pool).await == ["close point", "covering"]);
    }

    #[tokio::test]
    async fn an_event_online_counts_as_near_only_without_a_place() {
        let pool = database(&[
            ("online", None, None, true, false),
            ("hybrid", None, None, false, true),
            ("far online", None, Some(OUTSIDE), true, false),
            ("far hybrid", None, Some(OUTSIDE), false, true),
            ("close hybrid", None, Some(INSIDE), false, true),
        ])
        .await;

        check!(near(&pool).await == ["close hybrid", "hybrid", "online"]);
    }

    #[tokio::test]
    async fn an_event_held_anywhere_is_near_wherever_its_place() {
        let pool = database(&[
            ("anywhere", None, None, false, false),
            ("anywhere far", None, Some(OUTSIDE), false, false),
            ("anywhere elsewhere", Some(AWAY), None, false, false),
            ("far point", None, Some(OUTSIDE), false, false),
        ])
        .await;
        sqlx::query("UPDATE events SET location_type = ? WHERE name LIKE 'anywhere%'")
            .bind(super::ANYWHERE)
            .execute(&pool)
            .await
            .unwrap();

        check!(near(&pool).await == ["anywhere", "anywhere elsewhere", "anywhere far"]);
    }

    #[test]
    fn an_event_held_anywhere_is_listed_so_unless_online() {
        let event = |entirely_online, anywhere| super::CsmEvent {
            slug: String::new(),
            name: String::new(),
            begin: String::new(),
            end: String::new(),
            entirely_online,
            anywhere,
            signature: false,
            address: "1 Main St".to_string(),
            city: String::new(),
            state: String::new(),
        };

        check!(event(false, true).location_display() == "Anywhere");
        check!(event(true, true).location_display() == "Online");
        check!(event(false, false).location_display() == "1 Main St");
    }
}
