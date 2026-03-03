use common::message::log::{ActsOfScienceCreateRequest, ActsOfScienceCreateResponse, ActsOfScienceSource, ActsToDateRequest, ActsToDateResponse};
use common::message::user::BasicInfo;
use common::shared::db;
use poem::{Endpoint, Route, get, handler, Response};
use poem::session::Session;
use poem::web::{Form, Json, Query, RealIp};
use sailfish::TemplateOnce;
use serde::Deserialize;
use std::sync::Arc;

use crate::component::{Component, Head, StandardLayout};
use crate::middleware::authorization::Authenticated;

// Shared fragment templates
#[derive(TemplateOnce)]
#[template(path = "pages/csm/hero.stpl.html")]
struct CsmHero {
    phase: CsmPhase,
}

#[derive(TemplateOnce)]
#[template(path = "pages/csm/nav.stpl.html")]
struct CsmNav {
    phase: CsmPhase,
    active: &'static str,
}

#[derive(TemplateOnce)]
#[template(path = "pages/csm/sponsors.stpl.html")]
struct CsmSponsors;

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

    pub async fn detect(override_phase: Option<&str>) -> Self {
        if let Some(phase) = override_phase.and_then(Self::from_str) {
            return phase;
        }

        let today = jiff::Zoned::now().date();
        let month: i32 = today.month().into();

        if month < 4 {
            return CsmPhase::PreApril;
        }

        if month >= 6 {
            return CsmPhase::PreApril;
        }

        if month == 5 {
            return CsmPhase::May;
        }

        // month == 4, check if goal reached
        match acts_to_date(None).await {
            Ok(response) if response.date_reached.is_some() => CsmPhase::AprilReached,
            _ => CsmPhase::April,
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
    show_social_feed: bool,
}

#[handler]
async fn csm_home(
    _session: &Session,
    RealIp(_client_ip): RealIp,
    Authenticated(user_info): Authenticated,
    Query(query): Query<CsmQuery>,
) -> Result<Response, anyhow::Error> {
    let phase = CsmPhase::detect(query.phase.as_deref()).await;

    let show_social_feed = {
        let today = jiff::Zoned::now().date();
        let month: i32 = today.month().into();
        let day: i32 = today.day().into();
        month == 3 && day >= 25
    };

    let layout = StandardLayout::new(
        "/citizensciencemonth",
        CsmHomeContent { phase, show_social_feed },
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
}

#[handler]
async fn csm_participate(
    _session: &Session,
    RealIp(_client_ip): RealIp,
    Authenticated(user_info): Authenticated,
    Query(query): Query<CsmQuery>,
) -> Result<Response, anyhow::Error> {
    let phase = CsmPhase::detect(query.phase.as_deref()).await;

    let layout = StandardLayout::new(
        "/citizensciencemonth/participate",
        CsmParticipateContent { phase },
    )
    .prepare(user_info.clone())
    .await?;

    CsmParticipatePage {
        head: Head::default().with_slick().with_geosearch().prepare(None).await?,
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
}

#[handler]
async fn csm_report(
    _session: &Session,
    RealIp(_client_ip): RealIp,
    Authenticated(user_info): Authenticated,
) -> Result<Response, anyhow::Error> {
    let layout = StandardLayout::new(
        "/citizensciencemonth/report",
        CsmReportContent {
            user_info: user_info.clone(),
            error_message: None,
            success: false,
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

    let layout = StandardLayout::new(
        "/citizensciencemonth/report",
        CsmReportContent {
            user_info: user_info.clone(),
            error_message,
            success,
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
// API: Contributors / Wall of Fame (JSON)
// ============================================================================

#[derive(Deserialize)]
struct ContributorsQuery {
    mode: Option<String>,
    page: Option<i32>,
    letter: Option<String>,
}

#[handler]
pub async fn contributors_api(
    Query(query): Query<ContributorsQuery>,
) -> Result<Json<common::message::log::ContributorsResponse>, anyhow::Error> {
    use common::message::log::{ContributorsRequest, ContributorsResponse};

    let request = ContributorsRequest {
        mode: query.mode,
        page: query.page,
        letter: query.letter,
    };

    let response: ContributorsResponse =
        common::service::request("log.acts.contributors", request).await?;
    Ok(Json(response))
}

// ============================================================================
// HTMX: Staff Picks Fragment
// ============================================================================

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

    Ok(Response::builder()
        .content_type("text/html")
        .body(html))
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
    signature: bool,
    address: String,
    city: String,
    state: String,
}

impl CsmEvent {
    fn month_abbrev(&self) -> &str {
        if self.begin.len() >= 7 {
            match &self.begin[5..7] {
                "01" => "JAN",
                "02" => "FEB",
                "03" => "MAR",
                "04" => "APR",
                "05" => "MAY",
                "06" => "JUN",
                "07" => "JUL",
                "08" => "AUG",
                "09" => "SEP",
                "10" => "OCT",
                "11" => "NOV",
                "12" => "DEC",
                _ => "",
            }
        } else {
            ""
        }
    }

    fn day(&self) -> &str {
        if self.begin.len() >= 10 {
            self.begin[8..10].trim_start_matches('0')
        } else {
            ""
        }
    }

    fn location_display(&self) -> String {
        if self.entirely_online {
            return "Online".to_string();
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

#[handler]
pub async fn csm_events(
    Query(query): Query<CsmEventsQuery>,
) -> Result<Response, anyhow::Error> {
    let page = query.page.unwrap_or(1).max(1);
    let offset = (page - 1) * EVENTS_PAGE_SIZE;

    let (default_start, april_end) = get_april_range();
    let start_date = query.start_date.as_deref().filter(|s| !s.is_empty()).unwrap_or(&default_start);
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

    let signature_type = "Acts of Science Connected";

    // Build WHERE clause shared between count and results queries
    fn push_where_clause(
        qb: &mut sqlx::QueryBuilder<sqlx::Sqlite>,
        start_date: &str,
        april_end: &str,
        online_only: bool,
        signature_only: bool,
        signature_type: &str,
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

        if online_only {
            qb.push(" AND ev.entirely_online = 1");
        }
        if signature_only {
            qb.push(" AND ev.event_type = ");
            qb.push_bind(signature_type.to_string());
        }
        if has_keywords {
            qb.push(" AND lower(ev.name) LIKE ");
            qb.push_bind(keywords_like.to_string());
        }
        if has_location {
            // Include events near the location OR online events
            qb.push(" AND (ev.entirely_online = 1 OR (ev.point IS NOT NULL AND ST_Distance(ev.point, MakePoint(");
            qb.push_bind(lng);
            qb.push(", ");
            qb.push_bind(lat);
            qb.push(", 4326), 1) <= ");
            qb.push_bind(LOCATION_RADIUS_METERS);
            qb.push("))");
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

    let total_count: i64 = count_qb
        .build_query_scalar()
        .fetch_one(db())
        .await?;

    let total_pages = (total_count + EVENTS_PAGE_SIZE - 1) / EVENTS_PAGE_SIZE;

    // Results query
    let mut results_qb: sqlx::QueryBuilder<sqlx::Sqlite> = sqlx::QueryBuilder::new(
        "SELECT e.slug, ev.name, ei.begin, ei.end, \
         ev.entirely_online, ev.event_type, \
         ev.address, ev.city, ev.state \
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
    results_qb.push(" GROUP BY ev.event_id ORDER BY ei.begin ASC, ev.name ASC LIMIT ");
    results_qb.push_bind(EVENTS_PAGE_SIZE);
    results_qb.push(" OFFSET ");
    results_qb.push_bind(offset);

    let rows = results_qb
        .build_query_as::<(String, String, String, String, bool, String, String, String, String)>()
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
            signature: r.5 == "Acts of Science Connected",
            address: r.6,
            city: r.7,
            state: r.8,
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

    Ok(Response::builder()
        .content_type("text/html")
        .body(html))
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
