use serde::{Deserialize, Serialize};
use ulid::Ulid;
use crate::timestring::TimeString;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JoinRequest {
    pub when: String,
    pub entity_id: String,
    pub user_id: String,
    pub active: bool,
    pub consented: Option<String>,
    pub contacted: Option<String>,
    pub notified: bool,
    pub activity: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AffiliateParticipantsRequest {
    pub affiliate_id: Ulid,
    pub start: TimeString,
    pub end: TimeString,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AffiliateParticipantsResponse {
    pub user_ids: Vec<Ulid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyticsQuery {
    pub start_date: String, // ISO 8601 date (YYYY-MM-DD)
    pub end_date: String,   // ISO 8601 date (YYYY-MM-DD)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeeklyCount {
    pub week_start: String, // ISO 8601 date (YYYY-MM-DD)
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyCount {
    pub date: String, // ISO 8601 date (YYYY-MM-DD)
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonthlyCount {
    pub month_start: String, // ISO 8601 date (YYYY-MM-DD)
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AnalyticsResponse {
    WeeklyCounts(Vec<WeeklyCount>),
    DailyCounts(Vec<DailyCount>),
    MonthlyCounts(Vec<MonthlyCount>),
    TopPerformers(Vec<TopItemCount>),
    // Future analytics types can be added here:
    // UserActivitySummary(UserActivityData),
    // GeographicBreakdown(Vec<LocationCount>),
    // etc.
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopPerformersQuery {
    pub start_date: String, // ISO 8601 date (YYYY-MM-DD)
    pub end_date: String,   // ISO 8601 date (YYYY-MM-DD)
    pub criterion: String,  // "views", "clickthroughs", or "participation"
    pub limit: usize,       // number of top performers to return
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopItemCount {
    pub identifier: String,  // Can be path, program_id, or entity_id depending on criterion
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecentActivityRequest {
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecentActivityItem {
    pub when: String,
    pub action: String,
    pub user_id: Option<String>,
    pub target: Option<String>,
    pub extra: Option<String>,
    pub formatted_description: String,
    pub time_ago: String,
    pub icon: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecentActivityResponse {
    pub activities: Vec<RecentActivityItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoUrlStatsRequest {
    pub slug: String,
    pub start_date: Option<String>, // ISO 8601 date (YYYY-MM-DD), None = all time
    pub end_date: Option<String>,   // ISO 8601 date (YYYY-MM-DD), None = today
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoUrlViewEntry {
    pub timestamp: String,
    pub ip_address: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoUrlChartPoint {
    pub date: String,       // YYYY-MM-DD
    pub date_label: String, // e.g., "Sep 23"
    pub views: i64,
    pub height_percent: f64, // 0-100 for chart display
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoUrlStatsResponse {
    pub slug: String,
    pub total_views: i64,
    pub views_in_range: i64, // Views within the specified date range
    pub chart_data: Vec<GoUrlChartPoint>,
    pub chart_max_views: i64,
    pub recent_views: Vec<GoUrlViewEntry>,
    pub date_range_start: String, // The actual start date used (for display)
    pub date_range_end: String,   // The actual end date used (for display)
}

// Task Completion Checking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArticulateCompletionRequest {
    pub user_id: Ulid,
    pub activity: String, // e.g., "foundations-evolveme", "fundamentos-evolveme"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArticulateCompletionResponse {
    pub completed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectParticipationRequest {
    pub user_id: Ulid,
    pub project_id: String, // ULID as string
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectParticipationResponse {
    pub participated: bool,
}

// Recording Completions/Participations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArticulateCompletionRecord {
    pub user_id: Ulid,
    pub activity: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectParticipationRecord {
    pub user_id: Ulid,
    pub project_id: String,
    pub origin: Option<i16>,          // Source of participation (defaults to 0 = web)
    pub when: Option<String>,         // Timestamp (defaults to now)
    pub duration: Option<f64>,        // Duration in hours (defaults to 1.0)
    pub where_point: Option<String>,  // GeoJSON point
    pub magnitude: Option<i32>,       // Magnitude/intensity (defaults to 1)
    pub authenticated: Option<bool>,  // Whether user was authenticated (defaults to true)
    pub extra: Option<String>,        // Extra data (defaults to empty)
    pub fidelity: Option<f64>,        // Fidelity score (defaults to 1.0)
    pub task_name: Option<String>,    // Task name
}

// Exchange Analytics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExchangeEvent {
    pub event_id: String,          // ULID
    pub exchange_id: String,       // ULID
    pub user_id: Option<String>,   // ULID, None for anonymous
    pub device_id: String,         // Device identifier (persistent)
    pub interaction_id: String,    // Interaction identifier (1 hour expiry)
    pub when_occurred: String,     // TimeString
    pub page: String,              // "home", "project", "results"
    pub page_id: Option<String>,   // e.g., project_id
    pub action: String,            // "view", "search", "vote", "save"
    pub extra: Option<String>,     // JSON for additional context
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExchangeEngagementRequest {
    pub exchange_id: String,       // ULID
    pub year: Option<i32>,         // None = all time
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExchangeEngagementResponse {
    pub exchange_visits: i32,
    pub visitors: i32,
    pub views: i32,
    pub saves: i32,
    pub clickthroughs: i32,       // System-wide total
    pub contributions: i32,        // System-wide total
    pub available_years: Vec<i32>,
    pub top_views: Vec<ProjectCount>,
    pub top_saves: Vec<ProjectCount>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectCount {
    pub project_id: String,  // ULID
    pub count: i32,
}
