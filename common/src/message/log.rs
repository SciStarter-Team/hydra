use serde::{Deserialize, Serialize};

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
    // Future analytics types can be added here:
    // UserActivitySummary(UserActivityData),
    // GeographicBreakdown(Vec<LocationCount>),
    // etc.
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
