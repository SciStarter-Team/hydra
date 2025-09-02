use crate::component::{AdminLayout, Component, admin_layout::AdminLayoutPrepared};
use crate::middleware::authorization::{Authenticated, StaffOnly};
use common::{
    message::health::{
        ForumServiceHealthResponse, FrontendServiceHealthResponse, LocationServiceHealthResponse,
        LogServiceHealthResponse, UserServiceHealthResponse,
    },
    message::user::BasicInfo,
    service::request_timeout,
};
use poem::{Endpoint, EndpointExt, Route, get, handler, web::Query};
use sailfish::TemplateOnce;
use serde::Deserialize;
use std::sync::Arc;
use std::time::Duration;

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
        // TODO: Replace with actual async data fetching
        let activities = vec![
            RecentActivity {
                icon: "user-plus".to_string(),
                title: "New user registration".to_string(),
                description: "john.doe@example.com joined".to_string(),
                time_ago: "2 minutes ago".to_string(),
            },
            RecentActivity {
                icon: "folder-plus".to_string(),
                title: "New project created".to_string(),
                description: "\"Marine Life Survey\" was published".to_string(),
                time_ago: "1 hour ago".to_string(),
            },
        ];

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

#[derive(Debug, Clone)]
pub struct AdminUser {
    pub id: String,
    pub name: String,
    pub email: String,
    pub status: String,
    pub role: String,
    pub joined_date: String,
    pub last_login: Option<String>,
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/users.stpl.html")]
pub struct UsersContent {
    pub users: Vec<AdminUser>,
    pub current_page: u32,
    pub total_users: u32,
    pub has_previous: bool,
    pub has_next: bool,
    pub start_item: u32,
    pub end_item: u32,
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

#[derive(Debug)]
pub struct UsersPage;

impl Component for UsersPage {
    type Param = Option<Arc<BasicInfo>>;
    type Prepared = AdminLayoutPrepared;

    async fn prepare(self, param: Self::Param) -> Result<Self::Prepared, anyhow::Error> {
        // TODO: Replace with actual async data fetching from database
        let users_list = vec![
            AdminUser {
                id: "01234567890123456789012345".to_string(),
                name: "John Doe".to_string(),
                email: "john.doe@example.com".to_string(),
                status: "active".to_string(),
                role: "User".to_string(),
                joined_date: "Mar 12, 2024".to_string(),
                last_login: Some("2 hours ago".to_string()),
            },
            AdminUser {
                id: "01234567890123456789012346".to_string(),
                name: "Jane Smith".to_string(),
                email: "jane.smith@example.com".to_string(),
                status: "inactive".to_string(),
                role: "Staff".to_string(),
                joined_date: "Feb 28, 2024".to_string(),
                last_login: Some("1 week ago".to_string()),
            },
        ];

        let users_content = UsersContent {
            users: users_list,
            current_page: 1,
            total_users: 2,
            has_previous: false,
            has_next: false,
            start_item: 1,
            end_item: 2,
        };

        let content = users_content.render_once()?;

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

        // Example: Adding quick actions section
        let quick_actions = vec![
            QuickAction {
                label: "Sync Users".to_string(),
                icon: "refresh".to_string(),
                method: "POST".to_string(),
                url: "/-admin/actions/sync-users".to_string(),
                target: "#main-content".to_string(),
                swap: "afterbegin".to_string(),
            },
            QuickAction {
                label: "Clear Cache".to_string(),
                icon: "trash".to_string(),
                method: "POST".to_string(),
                url: "/-admin/actions/clear-cache".to_string(),
                target: "#main-content".to_string(),
                swap: "afterbegin".to_string(),
            },
            QuickAction {
                label: "Export Data".to_string(),
                icon: "download".to_string(),
                method: "GET".to_string(),
                url: "/-admin/actions/export".to_string(),
                target: "#main-content".to_string(),
                swap: "afterbegin".to_string(),
            },
            QuickAction {
                label: "View Logs".to_string(),
                icon: "file-text".to_string(),
                method: "GET".to_string(),
                url: "/-admin/logs".to_string(),
                target: "#main-content".to_string(),
                swap: "innerHTML".to_string(),
            },
        ];
        let quick_actions_content = build_quick_actions_section(quick_actions)?;

        let sections = vec![
            DashboardSection::new("quick_actions", "Quick Actions")
                .with_content(quick_actions_content)
                .with_width("full"),
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
    DashboardPage.render(user_info).await
}

#[handler]
async fn users(Authenticated(user_info): Authenticated) -> Result<poem::Response, anyhow::Error> {
    UsersPage.render(user_info).await
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
    ProjectsPage.render(user_info).await
}

#[handler]
async fn events(Authenticated(user_info): Authenticated) -> Result<poem::Response, anyhow::Error> {
    EventsPage.render(user_info).await
}

#[handler]
async fn analytics(
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    AnalyticsPage.render(user_info).await
}

#[handler]
async fn settings(
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    SettingsPage.render(user_info).await
}

pub fn routes() -> impl Endpoint {
    Route::new()
        .at("/", get(dashboard))
        .at("/users", get(users))
        .at("/projects", get(projects))
        .at("/events", get(events))
        .at("/analytics", get(analytics))
        .at("/settings", get(settings))
        .at("/help", get(help))
        .with(StaffOnly)
}
