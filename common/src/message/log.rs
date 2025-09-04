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
