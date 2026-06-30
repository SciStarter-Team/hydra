use crate::timestring::TimeString;
use serde::{Deserialize, Serialize};
use ulid::Ulid;

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
    pub identifier: String, // Can be path, program_id, or entity_id depending on criterion
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

/// Per-entity participation summary over a window (for the affiliate-activity
/// staff report). `activity` is the number of participation records in
/// `[first, last)`; `latest` is the most recent participation of all time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParticipationSummaryRequest {
    /// Window start (RFC3339, exclusive — matches the legacy report).
    pub first: String,
    /// Window end (RFC3339, exclusive).
    pub last: String,
    /// Entity (project) ULIDs to summarize.
    pub project_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParticipationSummaryEntry {
    pub project_id: String,
    pub activity: i64,
    pub latest: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParticipationSummaryResponse {
    pub entries: Vec<ParticipationSummaryEntry>,
}

/// A user's first participation timestamp in each project they participated in
/// (powers the join `when` in the check-join API).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserFirstParticipationRequest {
    /// The user's ULID.
    pub user_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserFirstParticipationEntry {
    pub project_id: String,
    /// The earliest participation timestamp for this user+project.
    pub when: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserFirstParticipationResponse {
    pub entries: Vec<UserFirstParticipationEntry>,
}

/// Participation counts grouped by year and entity, for users whose cached
/// email matches the given filters — for one-off client reports (e.g. Verizon).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParticipationByEmailRequest {
    /// Substring the email must contain (matched case-insensitively, e.g. "@verizon").
    pub email_contains: String,
    /// Suffix the email must end with (e.g. ".com").
    pub email_ends: String,
    /// Earliest 4-digit year, inclusive (e.g. "2020").
    pub since_year: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParticipationByEmailEntry {
    pub year: String,
    pub project_id: String,
    pub contributions: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParticipationByEmailResponse {
    pub entries: Vec<ParticipationByEmailEntry>,
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
    pub origin: Option<i16>,  // Source of participation (defaults to 0 = web)
    pub when: Option<String>, // Timestamp (defaults to now)
    pub duration: Option<f64>, // Duration in hours (defaults to 1.0)
    pub where_point: Option<String>, // GeoJSON point
    pub magnitude: Option<i32>, // Magnitude/intensity (defaults to 1)
    pub authenticated: Option<bool>, // Whether user was authenticated (defaults to true)
    pub extra: Option<String>, // Extra data (defaults to empty)
    pub fidelity: Option<f64>, // Fidelity score (defaults to 1.0)
    pub task_name: Option<String>, // Task name
}

// Exchange Analytics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExchangeEvent {
    pub event_id: String,        // ULID
    pub exchange_id: String,     // ULID
    pub user_id: Option<String>, // ULID, None for anonymous
    pub device_id: String,       // Device identifier (persistent)
    pub interaction_id: String,  // Interaction identifier (1 hour expiry)
    pub when_occurred: String,   // TimeString
    pub page: String,            // "home", "project", "results"
    pub page_id: Option<String>, // e.g., project_id
    pub action: String,          // "view", "search", "vote", "save"
    pub extra: Option<String>,   // JSON for additional context
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExchangeEngagementRequest {
    pub exchange_id: String, // ULID
    pub year: Option<i32>,   // None = all time
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExchangeEngagementResponse {
    pub exchange_visits: i32,
    pub visitors: i32,
    pub views: i32,
    pub saves: i32,
    pub clickthroughs: i32, // System-wide total
    pub contributions: i32, // System-wide total
    pub available_years: Vec<i32>,
    pub top_views: Vec<ProjectCount>,
    pub top_saves: Vec<ProjectCount>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectCount {
    pub project_id: String, // ULID
    pub count: i32,
}

// Search Analytics

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchEvent {
    pub when: String,
    pub device_id: String,
    pub interaction_id: String,
    pub user_id: Option<String>,
    pub query_text: Option<String>,
    pub lat: Option<f64>,
    pub lng: Option<f64>,
    pub location_name: Option<String>,
    pub location_type: Option<String>,
    pub topics: Option<String>,
    pub audiences: Option<String>,
    pub activities: Option<String>,
    pub misc_filters: Option<String>,
    pub show_completed: bool,
    pub page: i32,
    pub result_count: i32,
    pub search_type: String,
}

// External Links - Get or create ULIDs for URLs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalLinksRequest {
    pub slug: String, // Entity slug where links are found
    pub urls: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalLinksResponse {
    pub links: std::collections::HashMap<String, ulid::Ulid>, // URL -> ULID
}

// External Link Clickthrough - Get URL for ULID and record click
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalLinkClickthroughRequest {
    pub link_id: ulid::Ulid,
    pub when: Option<String>, // Optional timestamp, defaults to now
    pub ip_address: String,
    pub device_id: String,
    pub interaction_id: String,
    pub user_id: Option<ulid::Ulid>,
    pub link_type: String, // Type of link (e.g., "external_link")
    pub program_id: Option<String>,
    pub geolocation: Option<String>, // Optional GeoJSON point, will be resolved from IP if not provided
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalLinkClickthroughResponse {
    pub url: Option<String>, // None if link_id not found
}

// Share Analytics

/// The social platform a user shared an entity to. Stored as its lowercase
/// string form (`facebook`, `twitter`, `instagram`, `linkedin`) in the
/// `share_events` log table, and parsed from the share-click endpoint path so
/// only known platforms are recorded.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    strum::Display,
    strum::EnumString,
)]
#[strum(serialize_all = "lowercase")]
pub enum SharePlatform {
    Facebook,
    Twitter,
    Instagram,
    LinkedIn,
}

// Wall of Fame Contributors

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContributorsRequest {
    pub mode: Option<String>,
    pub page: Option<i32>,
    pub letter: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ContributorEntry {
    pub name: Option<String>,
    pub act: Option<String>,
    pub project: Option<String>,
    pub organization: Option<String>,
    #[serde(rename = "entity__slug")]
    pub entity_slug: Option<String>,
    pub date: Option<String>,
    #[serde(default)]
    pub featured: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContributorsResponse {
    pub contributors: Vec<ContributorEntry>,
    pub pages: i32,
}

// Acts of Science / Citizen Science Month

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(i16)]
pub enum ActsOfScienceSource {
    Empty = 0,
    Form = 1,
    Affiliate = 2,
    Incidental = 3,
    Admin = 4,
    GoogleForm = 5,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActsOfScienceCreateRequest {
    pub user_id: Option<Ulid>,
    pub email: Option<String>,
    pub country: Option<String>,
    pub newsletter: bool,
    pub name: Option<String>,
    pub contributions: i64,
    pub act: Option<String>,
    pub organization: Option<String>,
    pub num_others: i64,
    pub others: Option<String>,
    pub project: Option<String>,
    pub notes: Option<String>,
    pub source: ActsOfScienceSource,
    pub entity_slug: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActsOfScienceCreateResponse {
    pub act_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActsToDateRequest {
    pub year: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActsToDateResponse {
    pub date: String,
    pub number: i64,
    pub date_reached: Option<String>,
}

/// Request a per-user contribution overview (total count plus a small
/// list of recent contributions). Used by the dashboard overview page.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserParticipationRequest {
    pub user_id: Ulid,
    /// Maximum number of recent contributions to return.
    pub recent_limit: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserParticipationEntry {
    pub entity_id: String,
    pub when: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserParticipationResponse {
    pub total: i64,
    pub recent: Vec<UserParticipationEntry>,
}

/// Request the user's recent page-view paths over the last `days`
/// days. The caller is responsible for resolving paths against its
/// local entity catalog (entities are routed at `/<slug>`, but other
/// paths share the same shape, so log-service cannot classify them).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserRecentVisitsRequest {
    pub user_id: Ulid,
    pub days: i64,
    pub limit: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserRecentVisit {
    pub path: String,
    pub when: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserRecentVisitsResponse {
    pub visits: Vec<UserRecentVisit>,
}

/// Request the number of unique participants for a project.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectMemberCountRequest {
    pub project_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectMemberCountResponse {
    pub count: i64,
}

/// Request a project's like count, and optionally whether a given user has liked it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectLikesRequest {
    pub project_id: String,
    pub user_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectLikesResponse {
    pub count: i64,
    pub liked: bool,
}

/// Set a user's like state on a project. Returns the updated count and state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RatingSetRequest {
    pub project_id: String,
    pub user_id: String,
    pub liked: bool,
}

/// A single public review for display.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewEntry {
    pub user_id: String,
    pub text: String,
    pub when: String,
}

/// Request a project's public reviews (most recent first) and total public count.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectReviewsRequest {
    pub project_id: String,
    pub limit: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectReviewsResponse {
    pub reviews: Vec<ReviewEntry>,
    pub count: i64,
}

/// Create or update a user's review of a project. Returns the stored entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewSetRequest {
    pub project_id: String,
    pub user_id: String,
    pub text: String,
    pub private: bool,
}

/// Request the last-edited timestamp for a project or event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LastEditedRequest {
    pub target_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LastEditedResponse {
    /// ISO 8601 timestamp of the most recent edit, or empty if never edited.
    pub when: String,
}

/// Request the recent edit log for a project or event (staff view).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditLogRequest {
    pub target_id: String,
    pub limit: i64,
}

/// A single entry in the edit log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditLogEntry {
    pub when: String,
    pub user_id: String,
    pub field: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditLogResponse {
    pub entries: Vec<EditLogEntry>,
}
