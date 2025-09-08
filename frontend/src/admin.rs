use crate::component::{AdminLayout, Component, admin_layout::AdminLayoutPrepared};
use crate::middleware::authorization::{Authenticated, StaffOnly};
#[derive(Debug, TemplateOnce)]
#[template(path = "admin/partials/email_row.stpl.html")]
struct EmailRow {
    user_id: String,
    email: UserEmail,
    index: usize,
}

use common::{
    message::health::{
        ForumServiceHealthResponse, FrontendServiceHealthResponse, LocationServiceHealthResponse,
        LogServiceHealthResponse, UserServiceHealthResponse,
    },
    message::log::{RecentActivityRequest, RecentActivityResponse},
    message::user::{
        AdminPasswordResetRequest, AdminPasswordResetResponse, AdminUser, AdminUserCreateRequest,
        AdminUserDeleteRequest, AdminUserDeleteResponse, AdminUserGetRequest, AdminUserListRequest,
        AdminUserListResponse, AdminUserResponse, AdminUserSearchRequest, AdminUserUpdateRequest,
        BasicInfo, UserEmail, UserEmailAddRequest, UserEmailDeleteRequest, UserEmailResponse,
        UserEmailToggleVerifiedRequest, UserEmailUpdateRequest,
    },
    service::{request, request_timeout},
};
use poem::{
    Endpoint, EndpointExt, Route, get, handler, post, put,
    web::{Form, Path, Query},
};
use sailfish::TemplateOnce;
use serde::Deserialize;
use std::sync::Arc;
use std::time::Duration;

pub fn routes() -> impl Endpoint {
    Route::new()
        .at("/", get(dashboard))
        .at("/users", get(users))
        .at("/users/search", get(users_search))
        .at("/users/new", get(users_new).post(users_create))
        .at(
            "/users/:id",
            get(users_edit).put(users_update).delete(users_delete),
        )
        .at("/users/:id/send-password-reset", post(users_password_reset))
        .at("/users/:id/emails", post(users_emails_add))
        .at(
            "/users/:id/emails/:email",
            put(users_emails_update).delete(users_emails_delete),
        )
        .at(
            "/users/:id/emails/:email/toggle-verified",
            post(users_emails_toggle_verified),
        )
        .at("/projects", get(projects))
        .at("/events", get(events))
        .at("/analytics", get(analytics))
        .at("/settings", get(settings))
        .at("/help", get(help))
        .at("/engine-room", get(engine_room))
        .at(
            "/engine-room/sync-page-managers",
            post(engine_room_sync_page_managers),
        )
        .with(StaffOnly)
}

#[derive(Debug, Clone)]
pub struct RecentActivity {
    pub icon: String,
    pub title: String,
    pub description: String,
    pub time_ago: String,
}

#[derive(Debug, Clone)]
pub struct DashboardStat {
    pub icon: String,
    pub label: String,
    pub value: String,
    pub color: String,
}

#[derive(Debug, Clone)]
pub struct DashboardSection {
    pub id: String,
    pub title: String,
    pub content_html: String,
    pub width: String, // "full", "half", "third", etc.
}

impl DashboardSection {
    pub fn new(id: &str, title: &str) -> Self {
        Self {
            id: id.to_string(),
            title: title.to_string(),
            content_html: String::new(),
            width: "full".to_string(),
        }
    }

    pub fn with_content(mut self, content: String) -> Self {
        self.content_html = content;
        self
    }

    pub fn with_width(mut self, width: &str) -> Self {
        self.width = width.to_string();
        self
    }

    pub async fn from_component<C>(
        id: &str,
        title: &str,
        component: C,
        param: C::Param,
    ) -> Result<Self, anyhow::Error>
    where
        C: Component,
    {
        let prepared = component.prepare(param).await?;
        let content = prepared.render_once()?;
        Ok(Self {
            id: id.to_string(),
            title: title.to_string(),
            content_html: content,
            width: "full".to_string(),
        })
    }
}

impl DashboardStat {
    pub fn new(label: &str, value: &str, icon: &str, color: &str) -> Self {
        Self {
            label: label.to_string(),
            value: value.to_string(),
            icon: icon.to_string(),
            color: color.to_string(),
        }
    }
}

#[derive(Debug)]
pub struct RecentActivitySection;

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/sections/recent_activity.stpl.html")]
pub struct RecentActivitySectionPrepared {
    activities: Vec<RecentActivity>,
}

impl Component for RecentActivitySection {
    type Param = ();
    type Prepared = RecentActivitySectionPrepared;

    async fn prepare(self, _param: Self::Param) -> Result<Self::Prepared, anyhow::Error> {
        let request = RecentActivityRequest { limit: Some(10) };

        let response = match common::service::request::<RecentActivityResponse>(
            "log.recent_activity",
            request,
        )
        .await
        {
            Ok(response) => response,
            Err(e) => {
                eprintln!("Failed to fetch recent activity: {}", e);
                // Return fallback data on error
                return Ok(RecentActivitySectionPrepared {
                    activities: vec![RecentActivity {
                        icon: "alert-circle".to_string(),
                        title: "Unable to load recent activity".to_string(),
                        description: "Service temporarily unavailable".to_string(),
                        time_ago: "now".to_string(),
                    }],
                });
            }
        };

        let activities = response
            .activities
            .into_iter()
            .map(|item| RecentActivity {
                icon: item.icon,
                title: item.action,
                description: item.formatted_description,
                time_ago: item.time_ago,
            })
            .collect();

        Ok(RecentActivitySectionPrepared { activities })
    }
}

#[derive(Debug, Clone)]
pub struct ServiceHealth {
    pub name: String,
    pub status: String,
    pub status_color: String,
    pub response_time: u64,
}

/// Fetch real service health data from all services in parallel
async fn fetch_service_health() -> Vec<ServiceHealth> {
    let timeout = Duration::from_secs(3); // 3 second timeout for health checks

    // Launch all health checks concurrently
    let user_health_future =
        request_timeout::<UserServiceHealthResponse>("user.health.check", (), timeout);
    let location_health_future =
        request_timeout::<LocationServiceHealthResponse>("location.health.check", (), timeout);
    let log_health_future =
        request_timeout::<LogServiceHealthResponse>("log.health.check", (), timeout);
    let frontend_health_future =
        request_timeout::<FrontendServiceHealthResponse>("frontend.health.check", (), timeout);
    let forum_health_future =
        request_timeout::<ForumServiceHealthResponse>("forum.health.check", (), timeout);

    // Wait for all health checks to complete
    let (user_result, location_result, log_result, frontend_result, forum_result) = tokio::join!(
        user_health_future,
        location_health_future,
        log_health_future,
        frontend_health_future,
        forum_health_future
    );

    let mut services = Vec::new();

    // Process user service health result
    match user_result {
        Ok(health) => {
            let status_color = match health.status.as_str() {
                "healthy" => "green",
                "degraded" => "yellow",
                _ => "red",
            };
            services.push(ServiceHealth {
                name: "User Service".to_string(),
                status: health.status,
                status_color: status_color.to_string(),
                response_time: health.response_time_ms,
            });
        }
        Err(_) => {
            services.push(ServiceHealth {
                name: "User Service".to_string(),
                status: "unreachable".to_string(),
                status_color: "red".to_string(),
                response_time: 0,
            });
        }
    }

    // Process location service health result
    match location_result {
        Ok(health) => {
            let status_color = match health.status.as_str() {
                "healthy" => "green",
                "degraded" => "yellow",
                _ => "red",
            };
            services.push(ServiceHealth {
                name: "Location Service".to_string(),
                status: health.status,
                status_color: status_color.to_string(),
                response_time: health.response_time_ms,
            });
        }
        Err(_) => {
            services.push(ServiceHealth {
                name: "Location Service".to_string(),
                status: "unreachable".to_string(),
                status_color: "red".to_string(),
                response_time: 0,
            });
        }
    }

    // Process log service health result
    match log_result {
        Ok(health) => {
            let status_color = match health.status.as_str() {
                "healthy" => "green",
                "degraded" => "yellow",
                _ => "red",
            };
            services.push(ServiceHealth {
                name: "Log Service".to_string(),
                status: health.status,
                status_color: status_color.to_string(),
                response_time: health.response_time_ms,
            });
        }
        Err(_) => {
            services.push(ServiceHealth {
                name: "Log Service".to_string(),
                status: "unreachable".to_string(),
                status_color: "red".to_string(),
                response_time: 0,
            });
        }
    }

    // Process frontend service health result
    match frontend_result {
        Ok(health) => {
            let status_color = match health.status.as_str() {
                "healthy" => "green",
                "degraded" => "yellow",
                _ => "red",
            };
            services.push(ServiceHealth {
                name: "Frontend Service".to_string(),
                status: health.status,
                status_color: status_color.to_string(),
                response_time: health.response_time_ms,
            });
        }
        Err(_) => {
            services.push(ServiceHealth {
                name: "Frontend Service".to_string(),
                status: "unreachable".to_string(),
                status_color: "red".to_string(),
                response_time: 0,
            });
        }
    }

    // Process forum service health result
    match forum_result {
        Ok(health) => {
            let status_color = match health.status.as_str() {
                "healthy" => "green",
                "degraded" => "yellow",
                _ => "red",
            };
            services.push(ServiceHealth {
                name: "Forum Service".to_string(),
                status: health.status,
                status_color: status_color.to_string(),
                response_time: health.response_time_ms,
            });
        }
        Err(_) => {
            services.push(ServiceHealth {
                name: "Forum Service".to_string(),
                status: "unreachable".to_string(),
                status_color: "red".to_string(),
                response_time: 0,
            });
        }
    }

    services
}

fn build_system_health_section(services: Vec<ServiceHealth>) -> Result<String, anyhow::Error> {
    #[derive(TemplateOnce)]
    #[template(path = "admin/sections/system_health.stpl.html")]
    struct SystemHealthSection {
        services: Vec<ServiceHealth>,
    }

    Ok(SystemHealthSection { services }.render_once()?)
}

#[derive(Debug, Clone)]
pub struct QuickAction {
    pub label: String,
    pub icon: String,
    pub method: String, // GET, POST, etc.
    pub url: String,
    pub target: String,
    pub swap: String,
}

fn build_quick_actions_section(actions: Vec<QuickAction>) -> Result<String, anyhow::Error> {
    #[derive(TemplateOnce)]
    #[template(path = "admin/sections/quick_actions.stpl.html")]
    struct QuickActionsSection {
        actions: Vec<QuickAction>,
    }

    Ok(QuickActionsSection { actions }.render_once()?)
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/dashboard.stpl.html")]
pub struct DashboardContent {
    pub stats: Vec<DashboardStat>,
    pub sections: Vec<DashboardSection>,
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/partials/user_row.stpl.html")]
pub struct UserRow {
    pub user: AdminUser,
    pub oob: bool,
    pub oob_swap: String,
}

#[derive(Debug, Clone)]
pub struct AdminProject {
    pub id: String,
    pub title: String,
    pub status: String,
    pub participants: u32,
    pub created_date: String,
    pub last_updated: String,
    pub category: String,
}

#[derive(Debug)]
pub struct ProjectsPage;

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/projects.stpl.html")]
pub struct ProjectsPagePrepared {
    projects: Vec<AdminProject>,
    current_page: u32,
    total_projects: u32,
    has_previous: bool,
    has_next: bool,
    start_item: u32,
    end_item: u32,
}

impl Component for ProjectsPage {
    type Param = Option<Arc<BasicInfo>>;
    type Prepared = AdminLayoutPrepared;

    async fn prepare(self, param: Self::Param) -> Result<Self::Prepared, anyhow::Error> {
        // TODO: Replace with actual async data fetching from database
        let projects_list = vec![
            AdminProject {
                id: "01234567890123456789012345".to_string(),
                title: "Marine Life Survey".to_string(),
                status: "active".to_string(),
                participants: 1250,
                created_date: "Mar 15, 2024".to_string(),
                last_updated: "2 hours ago".to_string(),
                category: "Biology".to_string(),
            },
            AdminProject {
                id: "01234567890123456789012346".to_string(),
                title: "Urban Wildlife Tracking".to_string(),
                status: "draft".to_string(),
                participants: 89,
                created_date: "Apr 02, 2024".to_string(),
                last_updated: "1 day ago".to_string(),
                category: "Ecology".to_string(),
            },
        ];

        let projects_content = ProjectsPagePrepared {
            projects: projects_list,
            current_page: 1,
            total_projects: 2,
            has_previous: false,
            has_next: false,
            start_item: 1,
            end_item: 2,
        };

        let content = projects_content.render_once()?;

        AdminLayout::new("Projects")
            .with_breadcrumb("Projects", None)
            .with_sidebar_active("projects")
            .with_content(content)
            .prepare(param)
            .await
    }
}

#[derive(Debug, Clone)]
pub struct AdminEvent {
    pub id: String,
    pub title: String,
    pub status: String,
    pub attendees: u32,
    pub event_date: String,
    pub location: String,
    pub event_type: String,
}

#[derive(Debug)]
pub struct EventsPage;

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/events.stpl.html")]
pub struct EventsPagePrepared {
    events: Vec<AdminEvent>,
    current_page: u32,
    total_events: u32,
    has_previous: bool,
    has_next: bool,
    start_item: u32,
    end_item: u32,
}

impl Component for EventsPage {
    type Param = Option<Arc<BasicInfo>>;
    type Prepared = AdminLayoutPrepared;

    async fn prepare(self, param: Self::Param) -> Result<Self::Prepared, anyhow::Error> {
        // TODO: Replace with actual async data fetching from database
        let events_list = vec![
            AdminEvent {
                id: "01234567890123456789012345".to_string(),
                title: "Citizen Science Workshop".to_string(),
                status: "upcoming".to_string(),
                attendees: 45,
                event_date: "May 15, 2024".to_string(),
                location: "Science Museum".to_string(),
                event_type: "workshop".to_string(),
            },
            AdminEvent {
                id: "01234567890123456789012346".to_string(),
                title: "Data Collection Training".to_string(),
                status: "completed".to_string(),
                attendees: 67,
                event_date: "Apr 20, 2024".to_string(),
                location: "Online".to_string(),
                event_type: "training".to_string(),
            },
        ];

        let events_content = EventsPagePrepared {
            events: events_list,
            current_page: 1,
            total_events: 2,
            has_previous: false,
            has_next: false,
            start_item: 1,
            end_item: 2,
        };

        let content = events_content.render_once()?;

        AdminLayout::new("Events")
            .with_breadcrumb("Events", None)
            .with_sidebar_active("events")
            .with_content(content)
            .prepare(param)
            .await
    }
}

#[derive(Debug)]
pub struct AnalyticsPage;

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/analytics.stpl.html")]
pub struct AnalyticsPagePrepared {
    total_users: u32,
    total_projects: u32,
    total_events: u32,
    monthly_signups: u32,
    top_projects: Vec<AdminProject>,
    user_growth_data: String, // JSON for charts
}

impl Component for AnalyticsPage {
    type Param = Option<Arc<BasicInfo>>;
    type Prepared = AdminLayoutPrepared;

    async fn prepare(self, param: Self::Param) -> Result<Self::Prepared, anyhow::Error> {
        // TODO: Replace with actual async data fetching from database/analytics service
        let top_projects = vec![AdminProject {
            id: "01234567890123456789012345".to_string(),
            title: "Marine Life Survey".to_string(),
            status: "active".to_string(),
            participants: 1250,
            created_date: "Mar 15, 2024".to_string(),
            last_updated: "2 hours ago".to_string(),
            category: "Biology".to_string(),
        }];

        let analytics_content = AnalyticsPagePrepared {
            total_users: 1234,
            total_projects: 567,
            total_events: 89,
            monthly_signups: 156,
            top_projects,
            user_growth_data:
                r#"{"labels": ["Jan", "Feb", "Mar", "Apr"], "data": [100, 150, 200, 280]}"#
                    .to_string(),
        };

        let content = analytics_content.render_once()?;

        AdminLayout::new("Analytics")
            .with_breadcrumb("Analytics", None)
            .with_sidebar_active("analytics")
            .with_content(content)
            .prepare(param)
            .await
    }
}

#[derive(Debug)]
pub struct SettingsPage;

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/settings.stpl.html")]
pub struct SettingsPagePrepared {
    site_name: String,
    site_description: String,
    maintenance_mode: bool,
    registration_enabled: bool,
    email_notifications: bool,
}

impl Component for SettingsPage {
    type Param = Option<Arc<BasicInfo>>;
    type Prepared = AdminLayoutPrepared;

    async fn prepare(self, param: Self::Param) -> Result<Self::Prepared, anyhow::Error> {
        // TODO: Replace with actual async data fetching from configuration service
        let settings_content = SettingsPagePrepared {
            site_name: "SciStarter Admin".to_string(),
            site_description: "Connecting people to citizen science projects worldwide".to_string(),
            maintenance_mode: false,
            registration_enabled: true,
            email_notifications: true,
        };

        let content = settings_content.render_once()?;

        AdminLayout::new("Settings")
            .with_breadcrumb("Settings", None)
            .with_sidebar_active("settings")
            .with_content(content)
            .prepare(param)
            .await
    }
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/users.stpl.html")]
pub struct UsersPagePrepared {
    pub users: Vec<AdminUser>,
    pub current_page: u32,
    pub total_users: u64,
    pub has_previous: bool,
    pub has_next: bool,
    pub per_page: u32,
    pub start_item: u32,
    pub end_item: u32,
    pub pagination_url: String,
    pub pagination_include: String,
    pub search_term: String,
    pub selected_role: String,
    pub selected_email_status: String,
}

#[derive(Debug)]
pub struct UsersPage {
    page: Option<u32>,
    search: Option<String>,
    role: Option<String>,
    email_status: Option<String>,
}

impl Component for UsersPage {
    type Param = Option<Arc<BasicInfo>>;
    type Prepared = AdminLayoutPrepared;

    async fn prepare(self, param: Self::Param) -> Result<Self::Prepared, anyhow::Error> {
        // Fetch real user data from user service
        let page = self.page.unwrap_or(1);
        let user_request = AdminUserListRequest {
            page: Some(page),
            limit: Some(25),
        };

        let response: AdminUserListResponse =
            request("user.admin.user_list", user_request)
                .await
                .map_err(|e| anyhow::anyhow!("Failed to fetch users: {}", e))?;

        let users_list = response.users;

        let users_content = UsersContent {
            users: users_list,
            current_page: response.page,
            total_users: response.total_count,
            has_previous: response.page > 1,
            has_next: (response.page * response.limit) < response.total_count as u32,
            per_page: response.limit,
            start_item: ((response.page - 1) * response.limit) + 1,
            end_item: std::cmp::min(response.page * response.limit, response.total_count as u32),
            pagination_url: "/-admin/users".to_string(),
            pagination_include: "".to_string(),
        };

        let users_page = UsersPagePrepared {
            users: users_content.users,
            current_page: users_content.current_page,
            total_users: users_content.total_users,
            has_previous: users_content.has_previous,
            has_next: users_content.has_next,
            per_page: users_content.per_page,
            start_item: users_content.start_item,
            end_item: users_content.end_item,
            pagination_url: users_content.pagination_url,
            pagination_include: users_content.pagination_include,
            search_term: self.search.unwrap_or_default(),
            selected_role: self.role.unwrap_or_default(),
            selected_email_status: self.email_status.unwrap_or_default(),
        };

        let content = users_page.render_once()?;

        AdminLayout::new("Users")
            .with_breadcrumb("Users", None)
            .with_sidebar_active("users")
            .with_content(content)
            .prepare(param)
            .await
    }
}

#[derive(Debug)]
pub struct DashboardPage;

impl Component for DashboardPage {
    type Param = Option<Arc<BasicInfo>>;
    type Prepared = AdminLayoutPrepared;

    async fn prepare(self, param: Self::Param) -> Result<Self::Prepared, anyhow::Error> {
        // Fetch real data from services and database
        let user_count = match common::service::request::<common::message::user::UserCountResponse>(
            "user.stats.verified_count",
            (),
        )
        .await
        {
            Ok(response) => response.count.to_string(),
            Err(_) => "N/A".to_string(),
        };

        let project_count = sqlx::query!("SELECT COUNT(*) as count FROM projects WHERE publish = true AND flagged = false AND duplicate = false AND is_test = false")
            .fetch_one(common::shared::db())
            .await?
            .count;

        let upcoming_events_count = sqlx::query!("SELECT COUNT(DISTINCT e.event_id) as count FROM events e JOIN event_instances ei ON e.event_id = ei.event_id WHERE e.publish = true AND e.flagged = false AND ei.begin > datetime('now')")
            .fetch_one(common::shared::db())
            .await?
            .count;

        let stats = vec![
            DashboardStat::new("Verified Users", &user_count, "users", "indigo"),
            DashboardStat::new(
                "Published Projects",
                &project_count.to_string(),
                "folder-open",
                "green",
            ),
            DashboardStat::new(
                "Upcoming Events",
                &upcoming_events_count.to_string(),
                "calendar",
                "blue",
            ),
        ];

        // Build sections using Components

        // Fetch real system health data
        let system_health = fetch_service_health().await;
        let system_health_content = build_system_health_section(system_health)?;

        // Quick actions - currently empty but infrastructure preserved for future use
        let quick_actions = vec![];

        let mut sections = vec![
            DashboardSection::new("system_health", "System Health")
                .with_content(system_health_content)
                .with_width("half"),
            DashboardSection::from_component(
                "recent_activity",
                "Recent Activity",
                RecentActivitySection,
                (),
            )
            .await?
            .with_width("full"),
        ];

        // Only add quick actions section if there are actions to display
        if !quick_actions.is_empty() {
            let quick_actions_content = build_quick_actions_section(quick_actions)?;
            sections.insert(
                0,
                DashboardSection::new("quick_actions", "Quick Actions")
                    .with_content(quick_actions_content)
                    .with_width("full"),
            );
        }

        let dashboard_content = DashboardContent { stats, sections };

        let content = dashboard_content.render_once()?;

        AdminLayout::new("Dashboard")
            .with_sidebar_active("dashboard")
            .with_content(content)
            .prepare(param)
            .await
    }
}

#[handler]
async fn dashboard(
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    DashboardPage.into_response(user_info).await
}

#[handler]
async fn users(
    Query(query): Query<UserPageQuery>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    // If any search parameters are provided, perform a search instead of the default list
    if query.search.is_some() || query.role.is_some() || query.email_status.is_some() {
        let search_query = UserSearchQuery {
            search: query.search,
            role: query.role,
            email_status: query.email_status,
            page: query.page,
        };
        let users_page = users_search_internal(search_query).await?;
        let content = users_page.render_once()?;

        return AdminLayout::new("Users")
            .with_breadcrumb("Users", None)
            .with_sidebar_active("users")
            .with_content(content)
            .into_response(user_info)
            .await;
    }

    UsersPage { 
        page: query.page,
        search: query.search,
        role: query.role,
        email_status: query.email_status,
    }
    .into_response(user_info)
    .await
}

#[derive(Debug, Deserialize)]
struct UserPageQuery {
    page: Option<u32>,
    search: Option<String>,
    role: Option<String>,
    email_status: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
struct UserSearchQuery {
    search: Option<String>,
    role: Option<String>,
    email_status: Option<String>,
    page: Option<u32>,
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/users_table_body.stpl.html")]
pub struct UsersTableBody {
    users: Vec<AdminUser>,
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/users_content.stpl.html")]
pub struct UsersContent {
    pub users: Vec<AdminUser>,
    pub current_page: u32,
    pub total_users: u64,
    pub has_previous: bool,
    pub has_next: bool,
    pub per_page: u32,
    pub start_item: u32,
    pub end_item: u32,
    pub pagination_url: String,
    pub pagination_include: String,
}

async fn users_search_internal(
    query: UserSearchQuery,
) -> Result<UsersPagePrepared, anyhow::Error> {
    let page = query.page.unwrap_or(1);
    let search_request = AdminUserSearchRequest {
        search: query.search.clone(),
        role: query.role.clone(),
        email_status: query.email_status.clone(),
        page: Some(page),
        limit: Some(25),
    };

    let response: AdminUserListResponse =
        request("user.admin.user_search", search_request)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to search users: {}", e))?;

    let users_list = response.users;

    // Calculate pagination info
    let total_pages = (response.total_count + 24) / 25; // Round up
    let has_previous = page > 1;
    let has_next = page < total_pages as u32;
    let start_item = if response.total_count == 0 {
        0
    } else {
        ((page - 1) * 25) + 1
    };
    let end_item = std::cmp::min(page * 25, response.total_count as u32);

    Ok(UsersPagePrepared {
        users: users_list,
        current_page: page,
        total_users: response.total_count,
        has_previous,
        has_next,
        per_page: response.limit,
        start_item,
        end_item,
        pagination_url: "/-admin/users".to_string(),
        pagination_include: "[name='search'], [name='role'], [name='email_status']".to_string(),
        search_term: query.search.unwrap_or_default(),
        selected_role: query.role.unwrap_or_default(),
        selected_email_status: query.email_status.unwrap_or_default(),
    })
}

#[handler]
async fn users_search(
    Query(query): Query<UserSearchQuery>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let users_page = users_search_internal(query.clone()).await?;
    
    // For HTMX requests, render just the users content and set URL push header
    let users_content = UsersContent {
        users: users_page.users,
        current_page: users_page.current_page,
        total_users: users_page.total_users,
        has_previous: users_page.has_previous,
        has_next: users_page.has_next,
        per_page: users_page.per_page,
        start_item: users_page.start_item,
        end_item: users_page.end_item,
        pagination_url: users_page.pagination_url,
        pagination_include: users_page.pagination_include,
    };

    let html = users_content.render_once()?;

    // Build URL parameters for HX-Push-Url
    let mut url_params = vec![];
    if let Some(search) = &query.search {
        if !search.trim().is_empty() {
            url_params.push(format!("search={}", urlencoding::encode(search)));
        }
    }
    if let Some(role) = &query.role {
        if !role.trim().is_empty() {
            url_params.push(format!("role={}", urlencoding::encode(role)));
        }
    }
    if let Some(email_status) = &query.email_status {
        if !email_status.trim().is_empty() {
            url_params.push(format!("email_status={}", urlencoding::encode(email_status)));
        }
    }
    let page = query.page.unwrap_or(1);
    if page > 1 {
        url_params.push(format!("page={}", page));
    }

    let url = if url_params.is_empty() {
        "/-admin/users".to_string()
    } else {
        format!("/-admin/users?{}", url_params.join("&"))
    };

    Ok(poem::Response::builder()
        .content_type("text/html")
        .header("HX-Push-Url", url)
        .body(html))
}

#[derive(Debug, Deserialize)]
struct HelpQuery {
    section: Option<String>,
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/help.stpl.html")]
struct HelpContent {
    section: String,
    content: String,
}

#[handler]
async fn help(
    Query(query): Query<HelpQuery>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let section = query.section.unwrap_or_else(|| "general".to_string());

    let content = match section.as_str() {
        "dashboard" => {
            "The dashboard provides an overview of your system's key metrics and recent activity. Use the statistics cards to quickly assess system health, and check the recent activity feed for the latest events."
        }
        "users" => {
            "The users section allows you to manage user accounts, view registration details, and monitor user activity. You can search for specific users, update their roles, and track login patterns."
        }
        "projects" => {
            "Manage citizen science projects here. You can create new projects, edit existing ones, and monitor project participation and engagement metrics."
        }
        "events" => {
            "The events section helps you organize and manage community events, workshops, and activities. Create event listings, track attendance, and manage event-related communications."
        }
        "analytics" => {
            "Access detailed analytics and reporting tools. View participation trends, geographic distribution of users, and generate custom reports for stakeholders."
        }
        "settings" => {
            "Configure system-wide settings, manage integrations, and adjust platform preferences. This section requires elevated permissions for security-sensitive changes."
        }
        _ => {
            "Welcome to the admin help system. Select a specific section from the sidebar to get contextual help for that area of the admin interface."
        }
    };

    let help_content = HelpContent {
        section: section.clone(),
        content: content.to_string(),
    };

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(help_content.render_once()?))
}

#[handler]
async fn projects(
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    ProjectsPage.into_response(user_info).await
}

#[handler]
async fn events(Authenticated(user_info): Authenticated) -> Result<poem::Response, anyhow::Error> {
    EventsPage.into_response(user_info).await
}

#[handler]
async fn analytics(
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    AnalyticsPage.into_response(user_info).await
}

#[handler]
async fn settings(
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    SettingsPage.into_response(user_info).await
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/modals/new_user.stpl.html")]
struct NewUserForm {
    title: String,
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/modals/edit_user.stpl.html")]
struct EditUserForm {
    title: String,
    user_id: String,
    first_name: String,
    last_name: String,
    email: String,
    handle: String,
    role: String,
    active: bool,
    emails: Vec<UserEmail>,
}

#[derive(Debug, Deserialize)]
struct CreateUserForm {
    first_name: String,
    last_name: String,
    email: String,
    handle: String,
    password: String,
    role: String,
    #[serde(default)]
    verified: bool,
}

#[derive(Debug, Deserialize)]
struct UpdateUserForm {
    first_name: String,
    last_name: String,
    email: String,
    handle: String,
    new_password: Option<String>,
    role: String,
    #[serde(default)]
    active: bool,
}

#[handler]
async fn users_new(
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let new_user_form = NewUserForm {
        title: "Add New User".to_string(),
    };

    let html = new_user_form.render_once()?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(html))
}

#[handler]
async fn users_edit(
    Path(user_id): Path<String>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    // Parse user ID
    let parsed_user_id = user_id
        .parse::<ulid::Ulid>()
        .map_err(|_| anyhow::anyhow!("Invalid user ID format"))?;

    // Fetch user data from user service
    let get_request = AdminUserGetRequest {
        user_id: parsed_user_id,
    };

    match request::<AdminUserResponse>("user.admin.user_get", get_request).await {
        Ok(response) => {
            let admin_user = response.user;
            let edit_user_form = EditUserForm {
                title: "Edit User".to_string(),
                user_id,
                first_name: admin_user.first_name,
                last_name: admin_user.last_name,
                email: admin_user.primary_email.clone(),
                handle: admin_user.handle,
                role: if admin_user.is_superuser {
                    "staff".to_string()
                } else if admin_user.is_page_manager {
                    "page_manager".to_string()
                } else {
                    "user".to_string()
                },
                active: admin_user.is_active,
                emails: admin_user.emails,
            };

            Ok(edit_user_form.into_response(None).await?)
        }
        Err(_e) => {
            // If user not found, return empty form with error state
            let edit_user_form = EditUserForm {
                title: "Edit User - Not Found".to_string(),
                user_id,
                first_name: "".to_string(),
                last_name: "".to_string(),
                email: "".to_string(),
                handle: "".to_string(),
                role: "user".to_string(),
                active: true,
                emails: vec![],
            };
            Ok(edit_user_form.into_response(None).await?)
        }
    }
}

#[handler]
async fn users_delete(
    Path(user_id): Path<String>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    // Parse user ID
    let parsed_user_id = user_id
        .parse::<ulid::Ulid>()
        .map_err(|_| anyhow::anyhow!("Invalid user ID format"))?;

    let delete_request = AdminUserDeleteRequest {
        user_id: parsed_user_id,
    };

    match request::<AdminUserDeleteResponse>("user.admin.user_delete", delete_request).await {
        Ok(response) => {
            if response.success {
                // Success - return empty response to remove the table row
                Ok(poem::Response::builder().content_type("text/html").body(""))
            } else {
                // Return error message
                Ok(poem::Response::builder()
                    .status(poem::http::StatusCode::BAD_REQUEST)
                    .content_type("text/html")
                    .body("Failed to delete user"))
            }
        }
        Err(e) => {
            // Return error response
            Ok(poem::Response::builder()
                .status(poem::http::StatusCode::INTERNAL_SERVER_ERROR)
                .content_type("text/html")
                .body(format!("Error: {}", e)))
        }
    }
}

#[derive(Debug, Deserialize)]
struct PasswordResetForm {
    reset_email: String,
}

#[derive(Debug, Deserialize)]
struct AddEmailForm {
    new_email: String,
}

#[derive(Debug, Deserialize)]
struct UpdateEmailForm {
    old_email: String,
    new_email: String,
}

#[handler]
async fn users_password_reset(
    Path(user_id): Path<String>,
    Form(form): Form<PasswordResetForm>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    // Parse user ID
    let parsed_user_id = user_id
        .parse::<ulid::Ulid>()
        .map_err(|_| anyhow::anyhow!("Invalid user ID format"))?;

    let reset_request = AdminPasswordResetRequest {
        user_id: parsed_user_id,
        email: form.reset_email,
    };

    match request::<AdminPasswordResetResponse>("user.admin.password_reset", reset_request).await {
        Ok(response) => {
            if response.success {
                // Success - show success message via out-of-band update
                let success_message = format!(
                    r#"<div class="mb-4 p-3 text-sm text-green-700 bg-green-100 rounded-lg border border-green-200" hx-swap-oob="innerHTML:#modal-message-container">
                        <strong>Success:</strong> Password reset email sent successfully: {}
                    </div>"#,
                    response.message
                );
                Ok(poem::Response::builder()
                    .content_type("text/html")
                    .body(success_message))
            } else {
                // Return error message
                Ok(poem::Response::builder()
                    .status(poem::http::StatusCode::BAD_REQUEST)
                    .content_type("text/html")
                    .body(format!(
                        "Failed to send password reset email: {}",
                        response.message
                    )))
            }
        }
        Err(e) => {
            // Return error response
            Ok(poem::Response::builder()
                .status(poem::http::StatusCode::INTERNAL_SERVER_ERROR)
                .content_type("text/html")
                .body(format!("Error: {}", e)))
        }
    }
}

#[handler]
async fn users_create(
    Form(form): Form<CreateUserForm>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    // Map role string to boolean flags
    let (is_staff, is_page_manager) = match form.role.as_str() {
        "staff" => (true, false),
        "page_manager" => (false, true),
        "user" | _ => (false, false),
    };

    let create_request = AdminUserCreateRequest {
        handle: form.handle,
        first_name: form.first_name,
        last_name: form.last_name,
        primary_email: form.email,
        password: form.password,
        role: form.role,
        verified: form.verified,
    };

    match request::<AdminUserResponse>("user.admin.user_create", create_request).await {
        Ok(_response) => {
            // Success - close modal and refresh users table with updated data
            match request::<AdminUserListResponse>(
                "user.admin.user_list",
                AdminUserListRequest {
                    page: Some(1),
                    limit: Some(25),
                },
            )
            .await
            {
                Ok(users_response) => {
                    let start_item = if users_response.total_count == 0 {
                        0
                    } else {
                        (users_response.page - 1) * users_response.limit + 1
                    };
                    let end_item = std::cmp::min(
                        users_response.page * users_response.limit,
                        users_response.total_count as u32,
                    );

                    let users_content = UsersContent {
                        users: users_response.users,
                        current_page: users_response.page,
                        total_users: users_response.total_count,
                        has_previous: users_response.page > 1,
                        has_next: u64::from(users_response.page * users_response.limit)
                            < users_response.total_count,
                        per_page: users_response.limit,
                        start_item,
                        end_item,
                        pagination_url: "/-admin/users".to_string(),
                        pagination_include: "#user-search-form".to_string(),
                    };

                    let users_html = users_content.render_once()?;
                    let response_html = format!(
                        r#"
                        <div hx-swap-oob="innerHTML:#modal-container"></div>
                        <div hx-swap-oob="innerHTML:#users-content">{}</div>
                        "#,
                        users_html
                    );

                    Ok(poem::Response::builder()
                        .content_type("text/html")
                        .body(response_html))
                }
                Err(_) => {
                    // Fallback - just close modal if we can't refresh the list
                    Ok(poem::Response::builder()
                        .content_type("text/html")
                        .body(r#"<div hx-swap-oob="innerHTML:#modal-container"></div>"#))
                }
            }
        }
        Err(e) => {
            // Show error in modal
            let error_html = format!(
                r#"<div class="p-4 mb-4 text-sm text-red-700 bg-red-100 rounded-lg border border-red-200">
                    <strong>Error:</strong> Failed to create user: {}
                </div>"#,
                e
            );
            Ok(poem::Response::builder()
                .content_type("text/html")
                .body(error_html))
        }
    }
}

#[handler]
async fn users_update(
    Path(user_id): Path<String>,
    Form(form): Form<UpdateUserForm>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    // Parse user ID
    let parsed_user_id = user_id
        .parse::<ulid::Ulid>()
        .map_err(|_| anyhow::anyhow!("Invalid user ID format"))?;

    let update_request = AdminUserUpdateRequest {
        user_id: parsed_user_id,
        handle: form.handle,
        first_name: form.first_name,
        last_name: form.last_name,
        primary_email: form.email,
        new_password: form.new_password.filter(|p| !p.trim().is_empty()),
        role: form.role,
        verified: false, // Will be auto-calculated by backend based on email verification
        is_active: form.active,
    };

    match request::<AdminUserResponse>("user.admin.user_update", update_request).await {
        Ok(response) => {
            // Success - close modal and update the specific user row with OOB
            let updated_user = &response.user;
            let user_row = UserRow {
                user: updated_user.clone(),
                oob: true,
                oob_swap: "true".to_string(), // Replace existing row
            };

            let html = user_row.render_once()?;
            Ok(poem::Response::builder()
                .content_type("text/html")
                .body(html))
        }
        Err(e) => {
            // Show error in modal
            let error_html = format!(
                r#"<div class="p-4 mb-4 text-sm text-red-700 bg-red-100 rounded-lg border border-red-200">
                    <strong>Error:</strong> Failed to update user: {}
                </div>"#,
                e
            );
            Ok(poem::Response::builder()
                .content_type("text/html")
                .body(error_html))
        }
    }
}

// Engine Room structures
#[derive(Debug, Clone)]
pub struct EngineOperation {
    pub id: String,
    pub title: String,
    pub description: String,
    pub endpoint: String,
    pub button_text: String,
    pub icon: String,
    pub color: String,
}

#[derive(sailfish::TemplateOnce)]
#[template(path = "admin/engine_room.stpl.html")]
struct EngineRoomPrepared {
    operations: Vec<EngineOperation>,
}

#[derive(Debug, Clone)]
pub struct EngineRoomPage;

impl Component for EngineRoomPage {
    type Param = Option<Arc<BasicInfo>>;
    type Prepared = AdminLayoutPrepared;

    async fn prepare(self, param: Self::Param) -> Result<Self::Prepared, anyhow::Error> {
        let operations = vec![
            EngineOperation {
                id: "sync-page-managers".to_string(),
                title: "Page Manager Sync".to_string(),
                description: "Synchronize page manager flags for all users based on current project and event editors.".to_string(),
                endpoint: "/-admin/engine-room/sync-page-managers".to_string(),
                button_text: "Run Bulk Sync".to_string(),
                icon: r#"<svg class="h-6 w-6" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M17 20h5v-2a3 3 0 00-5.356-1.857M17 20H7m10 0v-2c0-.656-.126-1.283-.356-1.857M7 20H2v-2a3 3 0 015.356-1.857M7 20v-2c0-.656.126-1.283.356-1.857m0 0a5.002 5.002 0 019.288 0M15 7a3 3 0 11-6 0 3 3 0 016 0zm6 3a2 2 0 11-4 0 2 2 0 014 0zM7 10a2 2 0 11-4 0 2 2 0 014 0z"></path>
                </svg>"#.to_string(),
                color: "blue".to_string(),
            },
        ];

        let engine_room_content = EngineRoomPrepared { operations };
        let content = engine_room_content.render_once()?;

        AdminLayout::new("Engine Room")
            .with_breadcrumb("Engine Room", None)
            .with_sidebar_active("engine-room")
            .with_content(content)
            .prepare(param)
            .await
    }
}

#[handler]
#[tracing::instrument]
async fn engine_room(
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    EngineRoomPage.into_response(user_info).await
}

#[handler]
#[tracing::instrument]
async fn engine_room_sync_page_managers(
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    match crate::page_manager::full_page_manager_sync().await {
        Ok(count) => {
            let success_html = format!(
                r#"<div class="p-3 text-sm text-green-700 bg-green-100 rounded-lg border border-green-200">
                    <strong>Success:</strong> Updated {} user page manager statuses
                </div>"#,
                count
            );
            Ok(poem::Response::builder()
                .content_type("text/html")
                .body(success_html))
        }
        Err(e) => {
            let error_html = format!(
                r#"<div class="p-3 text-sm text-red-700 bg-red-100 rounded-lg border border-red-200">
                    <strong>Error:</strong> Failed to sync page managers: {}
                </div>"#,
                e
            );
            Ok(poem::Response::builder()
                .content_type("text/html")
                .body(error_html))
        }
    }
}

#[handler]
async fn users_emails_add(
    Path(user_id): Path<String>,
    Form(form): Form<AddEmailForm>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let parsed_user_id = user_id
        .parse::<ulid::Ulid>()
        .map_err(|_| anyhow::anyhow!("Invalid user ID format"))?;

    let email_request = UserEmailAddRequest {
        user_id: parsed_user_id,
        email: form.new_email,
    };

    match request::<UserEmailResponse>("user.admin.email_add", email_request).await {
        Ok(response) => {
            if response.success {
                // Find the newly added email and render just that row
                if let Some(new_email) = response.emails.last() {
                    let new_index = response.emails.len() - 1;
                    let new_email_html = EmailRow {
                        user_id: user_id.clone(),
                        email: new_email.clone(),
                        index: new_index,
                    }
                    .render_once()
                    .unwrap_or_default();

                    Ok(poem::Response::builder()
                        .content_type("text/html")
                        .body(new_email_html))
                } else {
                    Ok(poem::Response::builder()
                        .status(poem::http::StatusCode::INTERNAL_SERVER_ERROR)
                        .content_type("text/html")
                        .body("Error: No email returned"))
                }
            } else {
                Ok(poem::Response::builder()
                    .status(poem::http::StatusCode::BAD_REQUEST)
                    .content_type("text/html")
                    .body(format!("Error: {}", response.message)))
            }
        }
        Err(e) => Ok(poem::Response::builder()
            .status(poem::http::StatusCode::INTERNAL_SERVER_ERROR)
            .content_type("text/html")
            .body(format!("Error: {}", e))),
    }
}

#[handler]
async fn users_emails_update(
    Path((user_id, _email)): Path<(String, String)>,
    Form(form): Form<UpdateEmailForm>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let parsed_user_id = user_id
        .parse::<ulid::Ulid>()
        .map_err(|_| anyhow::anyhow!("Invalid user ID format"))?;

    let new_email_clone = form.new_email.clone();
    let email_request = UserEmailUpdateRequest {
        user_id: parsed_user_id,
        old_email: form.old_email,
        new_email: form.new_email,
    };

    match request::<UserEmailResponse>("user.admin.email_update", email_request).await {
        Ok(response) => {
            if response.success {
                // Find the updated email and render just that row
                if let Some((index, updated_email)) = response
                    .emails
                    .into_iter()
                    .enumerate()
                    .find(|(_, email)| email.email == new_email_clone)
                {
                    let row_html = EmailRow {
                        user_id: user_id.clone(),
                        email: updated_email,
                        index,
                    }
                    .render_once()
                    .unwrap_or_default();

                    Ok(poem::Response::builder()
                        .content_type("text/html")
                        .body(row_html))
                } else {
                    Ok(poem::Response::builder()
                        .status(poem::http::StatusCode::INTERNAL_SERVER_ERROR)
                        .content_type("text/html")
                        .body("Error: Updated email not found in response".to_string()))
                }
            } else {
                Ok(poem::Response::builder()
                    .status(poem::http::StatusCode::BAD_REQUEST)
                    .content_type("text/html")
                    .body(format!("Error: {}", response.message)))
            }
        }
        Err(e) => Ok(poem::Response::builder()
            .status(poem::http::StatusCode::INTERNAL_SERVER_ERROR)
            .content_type("text/html")
            .body(format!("Error: {}", e))),
    }
}

#[handler]
async fn users_emails_delete(
    Path((user_id, email)): Path<(String, String)>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let parsed_user_id = user_id
        .parse::<ulid::Ulid>()
        .map_err(|_| anyhow::anyhow!("Invalid user ID format"))?;

    let email_request = UserEmailDeleteRequest {
        user_id: parsed_user_id,
        email: email.clone(),
    };

    match request::<UserEmailResponse>("user.admin.email_delete", email_request).await {
        Ok(response) => {
            if response.success {
                // Return empty content to remove the row
                Ok(poem::Response::builder()
                    .content_type("text/html")
                    .body("".to_string()))
            } else {
                Ok(poem::Response::builder()
                    .status(poem::http::StatusCode::BAD_REQUEST)
                    .content_type("text/html")
                    .body(format!("Error: {}", response.message)))
            }
        }
        Err(e) => Ok(poem::Response::builder()
            .status(poem::http::StatusCode::INTERNAL_SERVER_ERROR)
            .content_type("text/html")
            .body(format!("Error: {}", e))),
    }
}

#[handler]
async fn users_emails_toggle_verified(
    Path((user_id, email)): Path<(String, String)>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let parsed_user_id = user_id
        .parse::<ulid::Ulid>()
        .map_err(|_| anyhow::anyhow!("Invalid user ID format"))?;

    let email_request = UserEmailToggleVerifiedRequest {
        user_id: parsed_user_id,
        email: email.clone(),
    };

    match request::<AdminUserResponse>("user.admin.email_toggle_verified", email_request).await {
        Ok(response) => {
            // Find the toggled email and render just that row
            if let Some((index, updated_email)) = response
                .user
                .emails
                .iter()
                .enumerate()
                .find(|(_, e)| e.email == email)
            {
                let email_row_html = EmailRow {
                    user_id: user_id.clone(),
                    email: updated_email.clone(),
                    index,
                }
                .render_once()
                .unwrap_or_default();

                // Render the updated user row with OOB swap using the data we already have
                let user_row_html = UserRow {
                    user: response.user.clone(),
                    oob: true,
                    oob_swap: "outerHTML".to_string(), // Use element's existing ID
                }
                .render_once()
                .unwrap_or_default();

                // Combine email row update and user row OOB update
                // Wrap the user row in template tag for proper OOB swap per HTMX docs
                let combined_html = format!("{}\n<template>{}</template>", email_row_html, user_row_html);

                Ok(poem::Response::builder()
                    .content_type("text/html")
                    .body(combined_html))
            } else {
                Ok(poem::Response::builder()
                    .status(poem::http::StatusCode::INTERNAL_SERVER_ERROR)
                    .content_type("text/html")
                    .body("Error: Toggled email not found in response".to_string()))
            }
        }
        Err(e) => Ok(poem::Response::builder()
            .status(poem::http::StatusCode::INTERNAL_SERVER_ERROR)
            .content_type("text/html")
            .body(format!("Error: {}", e))),
    }
}
