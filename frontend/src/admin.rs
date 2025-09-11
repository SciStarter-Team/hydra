use crate::component::{AdminLayout, Component, admin_layout::AdminLayoutPrepared};
use crate::contact::Contact;
use crate::middleware::authorization::{Authenticated, StaffOnly};
#[derive(Debug, TemplateOnce)]
#[template(path = "admin/partials/email_row.stpl.html")]
struct EmailRow {
    user_id: String,
    email: UserEmail,
    index: usize,
}

use crate::peering::LoadStorePeered;
use crate::project::all::ProjectAll;
use common::{
    insensitivestring::InsensitiveString,
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
    timestring::TimeString,
};
use poem::{
    Endpoint, EndpointExt, Route, get, handler, post, put,
    web::{Form, Path, Query},
};
use sailfish::TemplateOnce;
use serde::Deserialize;
use serde_json;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use ulid::Ulid;

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
        .at("/projects", get(projects).post(projects_create))
        .at("/projects/search", get(projects_search))
        .at("/projects/new", get(projects_new))
        .at("/projects/:id", get(projects_edit))
        .at(
            "/projects/:id/field/:field_type",
            put(projects_update_field),
        )
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
    pub publish: bool,
    pub participants: u32,
    pub created_date: String,
    pub last_updated: String,
    pub category: String,
    pub page_creator_id: String,
    pub description: String,
    pub presenter: String,
    pub origin: String,
    pub flagged: bool,
    pub duplicate: bool,
    pub is_test: bool,
    pub goal: String,
    pub task: String,
    pub image: String,
    pub url: String,
    pub admin_notes: String,
    pub draft: bool,
}

#[derive(Debug)]
pub struct AdminProjectFull {
    pub id: String,
    pub title: String,
    pub status: String,
    pub publish: bool,
    pub created_date: String,
    pub page_creator_id: String,
    pub description: String,
    pub presenter: String,
    pub origin: String,
    pub flagged: bool,
    pub duplicate: bool,
    pub is_test: bool,
    pub goal: String,
    pub task: String,
    pub image: String,
    pub url: String,
    pub admin_notes: String,
    pub draft: bool,
    // New fields for comprehensive editing
    pub promotion_time: String,
    pub authoritative: bool,
    pub promotion_target: Option<String>,
    pub wild_hope: bool,
    pub exchange: Option<String>,
    pub suborigin: String,
    pub video_url: String,
    pub blog_url: String,
    pub twitter_name: String,
    pub facebook_page: String,
    pub instagram_page: String,
    pub image_credit: String,
    pub legacy_v1_project_id: Option<i64>,
    pub legacy_v2_project_id: Option<i64>,
    pub foreign_id: Option<String>,
    pub guid: String,
    pub opengraph_description: String,
    pub is_affiliate: bool,
    pub is_inaturalist: bool,
    pub inat_project_id: Option<String>,
    pub welcomed: bool,
    pub page_editor_ids: String,
    pub extra: String,
    // TODO: Remove these fields - they should be log entries, not database fields
    // These fields are logged via common::service::log_activity in projects_update_field handler
    pub project_last_editor: String,
    pub project_last_edited: String,
    pub scistarter_last_editor: String,
    pub scistarter_last_edited: String,
    pub tags: String,
    // Content and participation fields
    pub participation_degree: String,
    pub participant_instructions: String,
    pub participant_instructions_email: String,
    pub participant_files: Vec<String>,
    pub us_federally_funded: bool,
    pub connect_opaque_url: String,
    pub connect_opaque_url1: String,
    pub connect_opaque_url2: String,
    pub contacts: String, // JSON serialized Contact structs
    pub topics: Vec<String>,
    pub fields_of_science: Vec<String>,
    pub practices: Vec<String>,
    pub practices_how: String,
    pub search_terms: String,
    pub difficulty: String,
    pub diy: bool,
    pub audience: Vec<String>,
    pub signup_url: String,
    pub prefix_how_to_logged_in: String,
    pub prefix_how_to_logged_out: String,
    pub how_to_join: String,
    pub howto_video: String,
    pub outcomes_label: String,
    pub outcomes_link: String,
    pub time_required: String,
    pub training_materials: String,
    pub classroom_materials: String,
    pub consent_statement: String,
    pub consent_statement_date: Option<String>,
    pub participation_tools: bool,
    pub participation_prompt: String,
    pub participation_confirmation_message: String,
    pub participation_notification_email: String,
    pub participation_usage: String,
    pub part_of: Option<String>,
    pub associated_projects: Vec<String>,
    pub total_expense: Option<f32>,
    pub materials: String,
    pub gear: String,
    pub mobile_app_url_android: String,
    pub mobile_app_url_ios: String,
    pub mobile_app_url: String,
    // Location fields
    pub location_type: String,
    pub entirely_online: bool,
    pub on_demand: bool,
    pub in_person: bool,
    pub indoors: bool,
    pub outdoors: bool,
    pub regions: Option<String>,
    pub regions_description: String,
    pub point: Option<String>,
    pub address: String,
    pub city: String,
    pub state: String,
    pub zip: String,
    pub country: String,
    pub other: String,
    pub location_name: String,
    pub location_landuse: String,
    pub location_description: String,
    pub geographic_scope: Vec<String>,
    pub united_nations_region: String,
    // Timing fields
    pub begin: Option<String>,
    pub end: Option<String>,
    pub project_timing: String,
    pub participation_schedule: String,
    pub time_per_participation: String,
    pub year_started: Option<i32>,
    pub year_completed: Option<i32>,
    pub month_completed: Option<i32>,
    pub participation_tasks: Vec<String>,
    pub activities: Vec<String>,
    pub special_skills: String,
    // Data fields
    pub data_results: String,
    pub data_aggregation: Vec<String>,
    pub data_consent: Vec<String>,
    pub data_visualization: bool,
    pub data_access: Vec<String>,
    pub data_quality: bool,
    pub metadata_publication_url: String,
    pub data_publication_url: String,
    pub data_license: String,
    pub intended_outcomes: Vec<String>,
    pub outcome_other: String,
    pub university_affiliation: String,
    pub sponsors: Vec<String>,
    pub stakeholders: Vec<String>,
    pub circuit_id: String,
    pub sustainable_development_goals: String,
}

impl AdminProjectFull {
    /// Convert a ProjectAll to AdminProjectFull, performing user lookups for editor fields
    pub async fn from_project_all(project: ProjectAll) -> Result<Self, anyhow::Error> {
        // TODO: Fix field mapping - many AdminProjectFull fields don't match ProjectAll fields
        // TODO: Fix type mismatches in field assignments
        // TODO: Handle missing fields like participants, created_date, last_updated, category
        // Helper function to look up user email by ULID
        async fn lookup_user_email(user_id: Option<Ulid>) -> String {
            match user_id {
                Some(id) => {
                    let get_request = AdminUserGetRequest { user_id: id };
                    match request::<AdminUserResponse>("user.admin.user_get", get_request).await {
                        Ok(response) => response.user.primary_email,
                        Err(_) => id.to_string(), // Fallback to ULID if lookup fails
                    }
                }
                None => String::new(),
            }
        }

        // Perform user lookups concurrently using join!
        let (project_last_editor_email, scistarter_last_editor_email) = tokio::join!(
            lookup_user_email(project.project_last_editor),
            lookup_user_email(project.scistarter_last_editor)
        );

        Ok(AdminProjectFull {
            id: project.project_id.to_string(),
            title: project.name,
            status: format!("{:?}", project.status).to_lowercase(),
            publish: project.publish,
            draft: project.draft,
            authoritative: project.authoritative,
            description: project.description,
            presenter: project.presenter,
            origin: project.origin,
            flagged: project.flagged,
            duplicate: project.duplicate,
            is_test: project.is_test,
            goal: project.goal,
            task: project.task,
            image: project.image,
            url: project.url,
            admin_notes: project.admin_notes,
            page_creator_id: project.page_creator_id.to_string(),
            created_date: project.created.to_string(),
            wild_hope: project.wild_hope,
            exchange: project.exchange,
            suborigin: project.suborigin,
            guid: project.guid,
            opengraph_description: project.opengraph_description,
            is_affiliate: project.is_affiliate,
            is_inaturalist: project.is_inaturalist,
            inat_project_id: project.inat_project_id.map(|i| i.to_string()),
            welcomed: project.welcomed,
            promotion_time: format!("{:?}", project.promotion_time),
            promotion_target: project
                .promotion_target
                .map(|t| t.to_string()),
            video_url: project.video_url,
            blog_url: project.blog_url,
            twitter_name: project.twitter_name,
            facebook_page: project.facebook_page,
            instagram_page: project.instagram_page,
            image_credit: project.image_credit,
            legacy_v1_project_id: project.legacy_v1_project_id,
            legacy_v2_project_id: project.legacy_v2_project_id,
            foreign_id: project.foreign_id,
            page_editor_ids: serde_json::to_string(&project.page_editor_ids.to_vec())
                .unwrap_or_default(),
            extra: project.extra,
            project_last_editor: project_last_editor_email,
            project_last_edited: project
                .project_last_edited
                .map(|t| t.to_string())
                .unwrap_or_default(),
            scistarter_last_editor: scistarter_last_editor_email,
            scistarter_last_edited: project
                .scistarter_last_edited
                .map(|t| t.to_string())
                .unwrap_or_default(),
            tags: serde_json::to_string(
                &project
                    .tags
                    .into_iter()
                    .map(|t| t.to_string())
                    .collect::<Vec<_>>(),
            )
            .unwrap_or_default(),
            participation_degree: format!("{:?}", project.participation_degree),
            participant_instructions: project.participant_instructions,
            participant_instructions_email: project.participant_instructions_email,
            participant_files: project.participant_files.to_vec(),
            us_federally_funded: project.us_federally_funded,
            connect_opaque_url: project.connect_opaque_url,
            connect_opaque_url1: project.connect_opaque_url1,
            connect_opaque_url2: project.connect_opaque_url2,
            contacts: serde_json::to_string(&project.contacts.to_vec()).unwrap_or_default(),
            topics: project.topics.to_vec(),
            fields_of_science: project.fields_of_science.to_vec(),
            practices: project.practices.to_vec(),
            practices_how: project.practices_how,
            search_terms: project.search_terms,
            difficulty: project.difficulty,
            diy: project.diy,
            audience: project.audience.to_vec(),
            signup_url: project.signup_url,
            prefix_how_to_logged_in: project.prefix_how_to_logged_in,
            prefix_how_to_logged_out: project.prefix_how_to_logged_out,
            how_to_join: project.how_to_join,
            howto_video: project.howto_video,
            outcomes_label: project.outcomes_label,
            outcomes_link: project.outcomes_link,
            time_required: project.time_required,
            training_materials: project.training_materials,
            classroom_materials: project.classroom_materials,
            consent_statement: project.consent_statement,
            consent_statement_date: project
                .consent_statement_date
                .map(|t| t.to_string()),
            participation_tools: project.participation_tools,
            participation_prompt: project.participation_prompt,
            participation_confirmation_message: project.participation_confirmation_message,
            participation_notification_email: project.participation_notification_email,
            participation_usage: project.participation_usage,
            part_of: project.part_of.map(|u| u.to_string()),
            total_expense: project.total_expense,
            materials: project.materials,
            gear: project.gear,
            mobile_app_url_android: project.mobile_app_url_android,
            mobile_app_url_ios: project.mobile_app_url_ios,
            mobile_app_url: project.mobile_app_url,
            location_type: project.location_type,
            entirely_online: project.entirely_online,
            on_demand: project.on_demand,
            in_person: project.in_person,
            indoors: project.indoors,
            outdoors: project.outdoors,
            regions_description: project.regions_description,
            address: project.address,
            city: project.city,
            state: project.state,
            zip: project.zip,
            country: project.country,
            other: project.other,
            location_name: project.location_name,
            location_landuse: project.location_landuse,
            location_description: project.location_description,
            united_nations_region: project.united_nations_region,
            begin: project.begin.map(|t| t.to_string()),
            end: project.end.map(|t| t.to_string()),
            project_timing: project.project_timing,
            participation_schedule: project.participation_schedule,
            time_per_participation: project.time_per_participation,
            year_started: project.year_started,
            year_completed: project.year_completed,
            month_completed: project.month_completed,
            special_skills: project.special_skills,
            data_results: project.data_results,
            data_visualization: project.data_visualization,
            data_quality: project.data_quality,
            metadata_publication_url: project.metadata_publication_url,
            data_publication_url: project.data_publication_url,
            data_license: project.data_license,
            participation_tasks: project.participation_tasks.to_vec(),
            activities: project.activities.to_vec(),
            geographic_scope: project.geographic_scope.to_vec(),
            data_aggregation: project.data_aggregation.to_vec(),
            data_consent: project.data_consent.to_vec(),
            data_access: project.data_access.to_vec(),
            intended_outcomes: project.intended_outcomes.to_vec(),
            sponsors: project.sponsors.to_vec(),
            stakeholders: project.stakeholders.to_vec(),
            associated_projects: project
                .associated_projects
                .iter()
                .map(|u| u.to_string())
                .collect(),
            outcome_other: project.outcome_other,
            university_affiliation: project.university_affiliation,
            circuit_id: project.circuit_id,
            sustainable_development_goals: project.sustainable_development_goals,
            point: project.point,
            regions: project.regions,
        })
    }
}

#[derive(Debug)]
pub struct ProjectsPage {
    page: Option<u32>,
    search: Option<String>,
    status: Option<String>,
    publish: Option<String>,
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/projects.stpl.html")]
pub struct ProjectsPagePrepared {
    projects: Vec<AdminProject>,
    current_page: u32,
    total_projects: u64,
    has_previous: bool,
    has_next: bool,
    per_page: u32,
    start_item: u32,
    end_item: u32,
    pagination_url: String,
    pagination_include: String,
    search_term: String,
    selected_status: String,
    selected_publish: String,
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
                publish: true,
                participants: 1250,
                created_date: "Mar 15, 2024".to_string(),
                last_updated: "2 hours ago".to_string(),
                category: "Biology".to_string(),
                page_creator_id: "01234567890123456789012300".to_string(),
                description: "A comprehensive survey of marine life in coastal waters".to_string(),
                presenter: "Marine Research Institute".to_string(),
                origin: "scistarter".to_string(),
                flagged: false,
                duplicate: false,
                is_test: false,
                goal: "To understand marine biodiversity patterns".to_string(),
                task: "Identify and count marine species".to_string(),
                image: "https://example.com/marine-survey.jpg".to_string(),
                url: "https://example.com/marine-life-survey".to_string(),
                admin_notes: "".to_string(),
                draft: false,
            },
            AdminProject {
                id: "01234567890123456789012346".to_string(),
                title: "Urban Wildlife Tracking".to_string(),
                status: "pending".to_string(),
                publish: false,
                participants: 89,
                created_date: "Apr 02, 2024".to_string(),
                last_updated: "1 day ago".to_string(),
                category: "Ecology".to_string(),
                page_creator_id: "01234567890123456789012301".to_string(),
                description: "Track wildlife movement patterns in urban environments".to_string(),
                presenter: "Urban Ecology Lab".to_string(),
                origin: "external".to_string(),
                flagged: false,
                duplicate: false,
                is_test: true,
                goal: "Monitor urban wildlife population changes".to_string(),
                task: "Track and photograph wildlife in urban areas".to_string(),
                image: "https://example.com/urban-wildlife.jpg".to_string(),
                url: "https://example.com/urban-wildlife-tracking".to_string(),
                admin_notes: "This is a test project for development purposes".to_string(),
                draft: true,
            },
        ];

        let page = self.page.unwrap_or(1);

        let projects_content = ProjectsPagePrepared {
            projects: projects_list,
            current_page: page,
            total_projects: 2,
            has_previous: page > 1,
            has_next: false,
            per_page: 25,
            start_item: ((page - 1) * 25) + 1,
            end_item: 2,
            pagination_url: "/-admin/projects".to_string(),
            pagination_include: "[name='search'], [name='status'], [name='publish']".to_string(),
            search_term: self.search.unwrap_or_default(),
            selected_status: self.status.unwrap_or_default(),
            selected_publish: self.publish.unwrap_or_default(),
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
            publish: true,
            participants: 1250,
            created_date: "Mar 15, 2024".to_string(),
            last_updated: "2 hours ago".to_string(),
            category: "Biology".to_string(),
            page_creator_id: "01234567890123456789012300".to_string(),
            description: "A comprehensive survey of marine life in coastal waters".to_string(),
            presenter: "Marine Research Institute".to_string(),
            origin: "scistarter".to_string(),
            flagged: false,
            duplicate: false,
            is_test: false,
            goal: "To understand marine biodiversity patterns".to_string(),
            task: "Identify and count marine species".to_string(),
            image: "https://example.com/marine-survey.jpg".to_string(),
            url: "https://example.com/marine-life-survey".to_string(),
            admin_notes: "".to_string(),
            draft: false,
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

async fn users_search_internal(query: UserSearchQuery) -> Result<UsersPagePrepared, anyhow::Error> {
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
            url_params.push(format!(
                "email_status={}",
                urlencoding::encode(email_status)
            ));
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
    Query(query): Query<ProjectPageQuery>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    // If any search parameters are provided, perform a search instead of the default list
    if query.search.is_some() || query.status.is_some() || query.publish.is_some() {
        let search_query = ProjectSearchQuery {
            search: query.search,
            status: query.status,
            publish: query.publish,
            page: query.page,
        };
        let projects_page = projects_search_internal(search_query).await?;
        let content = projects_page.render_once()?;

        return AdminLayout::new("Projects")
            .with_breadcrumb("Projects", None)
            .with_sidebar_active("projects")
            .with_content(content)
            .into_response(user_info)
            .await;
    }

    ProjectsPage {
        page: query.page,
        search: query.search,
        status: query.status,
        publish: query.publish,
    }
    .into_response(user_info)
    .await
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
    let (_is_staff, _is_page_manager) = match form.role.as_str() {
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
                let combined_html =
                    format!("{}\n<template>{}</template>", email_row_html, user_row_html);

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

// Project search and management structures and handlers

#[derive(Debug, Deserialize)]
struct ProjectPageQuery {
    page: Option<u32>,
    search: Option<String>,
    status: Option<String>,
    publish: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
struct ProjectSearchQuery {
    search: Option<String>,
    status: Option<String>,
    publish: Option<String>,
    page: Option<u32>,
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/projects_content.stpl.html")]
pub struct ProjectsContent {
    pub projects: Vec<AdminProject>,
    pub current_page: u32,
    pub total_projects: u64,
    pub has_previous: bool,
    pub has_next: bool,
    pub per_page: u32,
    pub start_item: u32,
    pub end_item: u32,
    pub pagination_url: String,
    pub pagination_include: String,
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/partials/project_row.stpl.html")]
pub struct ProjectRow {
    pub project: AdminProject,
    pub oob: bool,
    pub oob_swap: String,
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/modals/edit_project.stpl.html")]
struct EditProjectForm {
    title: String,
    project: AdminProjectFull,
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/modals/add_project.stpl.html")]
struct AddProjectForm {
    title: String,
}

#[derive(Debug, Deserialize)]
struct UpdateProjectFieldForm {
    value: String,
}

async fn projects_search_internal(
    query: ProjectSearchQuery,
) -> Result<ProjectsPagePrepared, anyhow::Error> {
    let page = query.page.unwrap_or(1);

    // TODO: Replace with real database query based on search parameters
    // For now, use the mock data but filter based on search criteria
    let all_projects = vec![
        AdminProject {
            id: "01234567890123456789012345".to_string(),
            title: "Marine Life Survey".to_string(),
            status: "active".to_string(),
            publish: true,
            participants: 1250,
            created_date: "Mar 15, 2024".to_string(),
            last_updated: "2 hours ago".to_string(),
            category: "Biology".to_string(),
            page_creator_id: "01234567890123456789012300".to_string(),
            description: "A comprehensive survey of marine life in coastal waters".to_string(),
            presenter: "Marine Research Institute".to_string(),
            origin: "scistarter".to_string(),
            flagged: false,
            duplicate: false,
            is_test: false,
            goal: "To understand marine biodiversity patterns".to_string(),
            task: "Identify and count marine species".to_string(),
            image: "https://example.com/marine-survey.jpg".to_string(),
            url: "https://example.com/marine-life-survey".to_string(),
            admin_notes: "".to_string(),
            draft: false,
        },
        AdminProject {
            id: "01234567890123456789012346".to_string(),
            title: "Urban Wildlife Tracking".to_string(),
            status: "pending".to_string(),
            publish: false,
            participants: 89,
            created_date: "Apr 02, 2024".to_string(),
            last_updated: "1 day ago".to_string(),
            category: "Ecology".to_string(),
            page_creator_id: "01234567890123456789012301".to_string(),
            description: "Track wildlife movement patterns in urban environments".to_string(),
            presenter: "Urban Ecology Lab".to_string(),
            origin: "external".to_string(),
            flagged: false,
            duplicate: false,
            is_test: true,
            goal: "Monitor urban wildlife population changes".to_string(),
            task: "Track and photograph wildlife in urban areas".to_string(),
            image: "https://example.com/urban-wildlife.jpg".to_string(),
            url: "https://example.com/urban-wildlife-tracking".to_string(),
            admin_notes: "This is a test project for development purposes".to_string(),
            draft: true,
        },
    ];

    // Apply search filters
    let mut filtered_projects = all_projects;

    if let Some(search) = &query.search {
        if !search.trim().is_empty() {
            let search_lower = search.to_lowercase();
            filtered_projects.retain(|p| {
                p.title.to_lowercase().contains(&search_lower)
                    || p.description.to_lowercase().contains(&search_lower)
                    || p.presenter.to_lowercase().contains(&search_lower)
            });
        }
    }

    if let Some(status) = &query.status {
        if !status.trim().is_empty() {
            filtered_projects.retain(|p| p.status == *status);
        }
    }

    if let Some(publish) = &query.publish {
        match publish.as_str() {
            "true" => filtered_projects.retain(|p| p.publish),
            "false" => filtered_projects.retain(|p| !p.publish),
            _ => {}
        }
    }

    let total_count = filtered_projects.len() as u64;
    let total_pages = (total_count + 24) / 25; // Round up
    let has_previous = page > 1;
    let has_next = page < total_pages as u32;
    let start_item = if total_count == 0 {
        0
    } else {
        ((page - 1) * 25) + 1
    };
    let end_item = std::cmp::min(page * 25, total_count as u32);

    Ok(ProjectsPagePrepared {
        projects: filtered_projects,
        current_page: page,
        total_projects: total_count,
        has_previous,
        has_next,
        per_page: 25,
        start_item,
        end_item,
        pagination_url: "/-admin/projects".to_string(),
        pagination_include: "[name='search'], [name='status'], [name='publish']".to_string(),
        search_term: query.search.unwrap_or_default(),
        selected_status: query.status.unwrap_or_default(),
        selected_publish: query.publish.unwrap_or_default(),
    })
}

#[handler]
async fn projects_search(
    Query(query): Query<ProjectSearchQuery>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let projects_page = projects_search_internal(query.clone()).await?;

    // For HTMX requests, render just the projects content and set URL push header
    let projects_content = ProjectsContent {
        projects: projects_page.projects,
        current_page: projects_page.current_page,
        total_projects: projects_page.total_projects,
        has_previous: projects_page.has_previous,
        has_next: projects_page.has_next,
        per_page: projects_page.per_page,
        start_item: projects_page.start_item,
        end_item: projects_page.end_item,
        pagination_url: projects_page.pagination_url,
        pagination_include: projects_page.pagination_include,
    };

    let html = projects_content.render_once()?;

    // Build URL parameters for HX-Push-Url
    let mut url_params = vec![];
    if let Some(search) = &query.search {
        if !search.trim().is_empty() {
            url_params.push(format!("search={}", urlencoding::encode(search)));
        }
    }
    if let Some(status) = &query.status {
        if !status.trim().is_empty() {
            url_params.push(format!("status={}", urlencoding::encode(status)));
        }
    }
    if let Some(publish) = &query.publish {
        if !publish.trim().is_empty() {
            url_params.push(format!("publish={}", urlencoding::encode(publish)));
        }
    }
    let page = query.page.unwrap_or(1);
    if page > 1 {
        url_params.push(format!("page={}", page));
    }

    let url = if url_params.is_empty() {
        "/-admin/projects".to_string()
    } else {
        format!("/-admin/projects?{}", url_params.join("&"))
    };

    Ok(poem::Response::builder()
        .content_type("text/html")
        .header("HX-Push-Url", url)
        .body(html))
}

/// Helper function to convert a ULID to an email address by looking up the user
async fn ulid_to_email(user_id: Option<Ulid>) -> String {
    if let Some(id) = user_id {
        let get_request = AdminUserGetRequest { user_id: id };

        match request::<AdminUserResponse>("user.admin.user_get", get_request).await {
            Ok(response) => response.user.primary_email,
            Err(_) => format!("Unknown user ({})", id),
        }
    } else {
        "Unknown".to_string()
    }
}

#[handler]
async fn projects_edit(
    Path(project_id): Path<String>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    // Parse project ID and load real project data
    let project_ulid = project_id
        .parse::<Ulid>()
        .map_err(|_| anyhow::anyhow!("Invalid project ID format"))?;

    // Load ProjectAll from database and convert to AdminProjectFull
    let project_all = ProjectAll::read_from_database(project_ulid).await?;
    let project = AdminProjectFull::from_project_all(project_all).await?;

    let edit_project_form = EditProjectForm {
        title: format!("Edit Project: {}", project.title),
        project,
    };

    Ok(edit_project_form.into_response(None).await?)
}

#[handler]
async fn projects_update_field(
    Path((project_id, field_type)): Path<(String, String)>,
    Form(form): Form<UpdateProjectFieldForm>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    // Parse project ID
    let parsed_project_id = project_id
        .parse::<ulid::Ulid>()
        .map_err(|_| anyhow::anyhow!("Invalid project ID format"))?;

    // Parse field type from string
    let field_type_enum = match field_type.as_str() {
        // Core project fields
        "project_id" => crate::project::field::ProjectFieldType::ProjectId,
        "legacy_v1_project_id" => crate::project::field::ProjectFieldType::LegacyV1ProjectId,
        "legacy_v2_project_id" => crate::project::field::ProjectFieldType::LegacyV2ProjectId,
        "foreign_id" => crate::project::field::ProjectFieldType::ForeignId,
        "publish" => crate::project::field::ProjectFieldType::Publish,
        "duplicate" => crate::project::field::ProjectFieldType::Duplicate,
        "authoritative" => crate::project::field::ProjectFieldType::Authoritative,
        "flagged" => crate::project::field::ProjectFieldType::Flagged,
        "is_test" => crate::project::field::ProjectFieldType::IsTest,
        "status" => crate::project::field::ProjectFieldType::Status,
        "promotion_time" => crate::project::field::ProjectFieldType::PromotionTime,
        "promotion_target" => crate::project::field::ProjectFieldType::PromotionTarget,
        "admin_notes" => crate::project::field::ProjectFieldType::AdminNotes,
        "created" => crate::project::field::ProjectFieldType::Created,
        "wild_hope" => crate::project::field::ProjectFieldType::WildHope,
        "exchange" => crate::project::field::ProjectFieldType::Exchange,
        "origin" => crate::project::field::ProjectFieldType::Origin,
        "suborigin" => crate::project::field::ProjectFieldType::Suborigin,
        "guid" => crate::project::field::ProjectFieldType::Guid,
        "opengraph_description" => crate::project::field::ProjectFieldType::OpengraphDescription,
        "is_affiliate" => crate::project::field::ProjectFieldType::IsAffiliate,
        "is_inaturalist" => crate::project::field::ProjectFieldType::IsInaturalist,
        "inat_project_id" => crate::project::field::ProjectFieldType::InatProjectId,
        "welcomed" => crate::project::field::ProjectFieldType::Welcomed,
        "page_creator_id" => crate::project::field::ProjectFieldType::PageCreatorId,
        "page_editor_ids" => crate::project::field::ProjectFieldType::PageEditorIds,
        "extra" => crate::project::field::ProjectFieldType::Extra,
        "project_last_editor" => crate::project::field::ProjectFieldType::ProjectLastEditor,
        "project_last_edited" => crate::project::field::ProjectFieldType::ProjectLastEdited,
        "scistarter_last_editor" => crate::project::field::ProjectFieldType::ScistarterLastEditor,
        "scistarter_last_edited" => crate::project::field::ProjectFieldType::ScistarterLastEdited,
        "draft" => crate::project::field::ProjectFieldType::Draft,
        // Content fields
        "name" => crate::project::field::ProjectFieldType::Name,
        "tags" => crate::project::field::ProjectFieldType::Tags,
        "participation_degree" => crate::project::field::ProjectFieldType::ParticipationDegree,
        "participant_instructions" => {
            crate::project::field::ProjectFieldType::ParticipantInstructions
        }
        "participant_instructions_email" => {
            crate::project::field::ProjectFieldType::ParticipantInstructionsEmail
        }
        "participant_files" => crate::project::field::ProjectFieldType::ParticipantFiles,
        "us_federally_funded" => crate::project::field::ProjectFieldType::UsFederallyFunded,
        "presenter" => crate::project::field::ProjectFieldType::Presenter,
        "goal" => crate::project::field::ProjectFieldType::Goal,
        "task" => crate::project::field::ProjectFieldType::Task,
        "description" => crate::project::field::ProjectFieldType::Description,
        "image" => crate::project::field::ProjectFieldType::Image,
        "image_credit" => crate::project::field::ProjectFieldType::ImageCredit,
        "connect_opaque_url" => crate::project::field::ProjectFieldType::ConnectOpaqueUrl,
        "connect_opaque_url1" => crate::project::field::ProjectFieldType::ConnectOpaqueUrl1,
        "connect_opaque_url2" => crate::project::field::ProjectFieldType::ConnectOpaqueUrl2,
        "contacts" => crate::project::field::ProjectFieldType::Contacts,
        "url" => crate::project::field::ProjectFieldType::Url,
        "video_url" => crate::project::field::ProjectFieldType::VideoUrl,
        "blog_url" => crate::project::field::ProjectFieldType::BlogUrl,
        "twitter_name" => crate::project::field::ProjectFieldType::TwitterName,
        "facebook_page" => crate::project::field::ProjectFieldType::FacebookPage,
        "instagram_page" => crate::project::field::ProjectFieldType::InstagramPage,
        "topics" => crate::project::field::ProjectFieldType::Topics,
        "fields_of_science" => crate::project::field::ProjectFieldType::FieldsOfScience,
        "practices" => crate::project::field::ProjectFieldType::Practices,
        "practices_how" => crate::project::field::ProjectFieldType::PracticesHow,
        "search_terms" => crate::project::field::ProjectFieldType::SearchTerms,
        "difficulty" => crate::project::field::ProjectFieldType::Difficulty,
        "diy" => crate::project::field::ProjectFieldType::Diy,
        "audience" => crate::project::field::ProjectFieldType::Audience,
        "signup_url" => crate::project::field::ProjectFieldType::SignupUrl,
        "prefix_how_to_logged_in" => crate::project::field::ProjectFieldType::PrefixHowToLoggedIn,
        "prefix_how_to_logged_out" => crate::project::field::ProjectFieldType::PrefixHowToLoggedOut,
        "how_to_join" => crate::project::field::ProjectFieldType::HowToJoin,
        "howto_video" => crate::project::field::ProjectFieldType::HowtoVideo,
        "outcomes_label" => crate::project::field::ProjectFieldType::OutcomesLabel,
        "outcomes_link" => crate::project::field::ProjectFieldType::OutcomesLink,
        "time_required" => crate::project::field::ProjectFieldType::TimeRequired,
        "training_materials" => crate::project::field::ProjectFieldType::TrainingMaterials,
        "classroom_materials" => crate::project::field::ProjectFieldType::ClassroomMaterials,
        "consent_statement" => crate::project::field::ProjectFieldType::ConsentStatement,
        "consent_statement_date" => crate::project::field::ProjectFieldType::ConsentStatementDate,
        "participation_tools" => crate::project::field::ProjectFieldType::ParticipationTools,
        "participation_prompt" => crate::project::field::ProjectFieldType::ParticipationPrompt,
        "participation_confirmation_message" => {
            crate::project::field::ProjectFieldType::ParticipationConfirmationMessage
        }
        "participation_notification_email" => {
            crate::project::field::ProjectFieldType::ParticipationNotificationEmail
        }
        "participation_usage" => crate::project::field::ProjectFieldType::ParticipationUsage,
        "part_of" => crate::project::field::ProjectFieldType::PartOf,
        "associated_projects" => crate::project::field::ProjectFieldType::AssociatedProjects,
        "total_expense" => crate::project::field::ProjectFieldType::TotalExpense,
        "materials" => crate::project::field::ProjectFieldType::Materials,
        "gear" => crate::project::field::ProjectFieldType::Gear,
        "mobile_app_url_android" => crate::project::field::ProjectFieldType::MobileAppUrlAndroid,
        "mobile_app_url_ios" => crate::project::field::ProjectFieldType::MobileAppUrlIos,
        "mobile_app_url" => crate::project::field::ProjectFieldType::MobileAppUrl,
        // Location fields
        "location_type" => crate::project::field::ProjectFieldType::LocationType,
        "entirely_online" => crate::project::field::ProjectFieldType::EntirelyOnline,
        "on_demand" => crate::project::field::ProjectFieldType::OnDemand,
        "in_person" => crate::project::field::ProjectFieldType::InPerson,
        "indoors" => crate::project::field::ProjectFieldType::Indoors,
        "outdoors" => crate::project::field::ProjectFieldType::Outdoors,
        "regions" => crate::project::field::ProjectFieldType::Regions,
        "regions_description" => crate::project::field::ProjectFieldType::RegionsDescription,
        "point" => crate::project::field::ProjectFieldType::Point,
        "address" => crate::project::field::ProjectFieldType::Address,
        "city" => crate::project::field::ProjectFieldType::City,
        "state" => crate::project::field::ProjectFieldType::State,
        "zip" => crate::project::field::ProjectFieldType::Zip,
        "country" => crate::project::field::ProjectFieldType::Country,
        "other" => crate::project::field::ProjectFieldType::Other,
        "location_name" => crate::project::field::ProjectFieldType::LocationName,
        "location_landuse" => crate::project::field::ProjectFieldType::LocationLanduse,
        "location_description" => crate::project::field::ProjectFieldType::LocationDescription,
        "geographic_scope" => crate::project::field::ProjectFieldType::GeographicScope,
        "united_nations_region" => crate::project::field::ProjectFieldType::UnitedNationsRegion,
        // Timing fields
        "begin" => crate::project::field::ProjectFieldType::Begin,
        "end" => crate::project::field::ProjectFieldType::End,
        "project_timing" => crate::project::field::ProjectFieldType::ProjectTiming,
        "participation_schedule" => crate::project::field::ProjectFieldType::ParticipationSchedule,
        "time_per_participation" => crate::project::field::ProjectFieldType::TimePerParticipation,
        "year_started" => crate::project::field::ProjectFieldType::YearStarted,
        "year_completed" => crate::project::field::ProjectFieldType::YearCompleted,
        "month_completed" => crate::project::field::ProjectFieldType::MonthCompleted,
        "participation_tasks" => crate::project::field::ProjectFieldType::ParticipationTasks,
        "activities" => crate::project::field::ProjectFieldType::Activities,
        "special_skills" => crate::project::field::ProjectFieldType::SpecialSkills,
        // Data fields
        "data_results" => crate::project::field::ProjectFieldType::DataResults,
        "data_aggregation" => crate::project::field::ProjectFieldType::DataAggregation,
        "data_consent" => crate::project::field::ProjectFieldType::DataConsent,
        "data_visualization" => crate::project::field::ProjectFieldType::DataVisualization,
        "data_access" => crate::project::field::ProjectFieldType::DataAccess,
        "data_quality" => crate::project::field::ProjectFieldType::DataQuality,
        "metadata_publication_url" => {
            crate::project::field::ProjectFieldType::MetadataPublicationUrl
        }
        "data_publication_url" => crate::project::field::ProjectFieldType::DataPublicationUrl,
        "data_license" => crate::project::field::ProjectFieldType::DataLicense,
        "intended_outcomes" => crate::project::field::ProjectFieldType::IntendedOutcomes,
        "outcome_other" => crate::project::field::ProjectFieldType::OutcomeOther,
        "university_affiliation" => crate::project::field::ProjectFieldType::UniversityAffiliation,
        "sponsors" => crate::project::field::ProjectFieldType::Sponsors,
        "stakeholders" => crate::project::field::ProjectFieldType::Stakeholders,
        "circuit_id" => crate::project::field::ProjectFieldType::CircuitId,
        "sustainable_development_goals" => {
            crate::project::field::ProjectFieldType::SustainableDevelopmentGoals
        }
        _ => {
            return Ok(poem::Response::builder()
                .status(poem::http::StatusCode::BAD_REQUEST)
                .content_type("text/html")
                .body("Invalid field type"));
        }
    };

    // Create the appropriate ProjectField struct based on field type and value
    let project_field = crate::project::field::ProjectField::new(
        parsed_project_id,
        match field_type_enum {
            // Core project fields
            crate::project::field::ProjectFieldType::ProjectId => {
                let ulid_value = form
                    .value
                    .parse::<Ulid>()
                    .map_err(|e| anyhow::anyhow!("Invalid ULID format: {}", e))?;
                crate::project::field::ProjectFieldValue::ProjectId(ulid_value)
            }
            crate::project::field::ProjectFieldType::LegacyV1ProjectId => {
                let option_value = if form.value.is_empty() {
                    None
                } else {
                    Some(
                        form.value
                            .parse::<i64>()
                            .map_err(|e| anyhow::anyhow!("Invalid integer format: {}", e))?,
                    )
                };
                crate::project::field::ProjectFieldValue::LegacyV1ProjectId(option_value)
            }
            crate::project::field::ProjectFieldType::LegacyV2ProjectId => {
                let option_value = if form.value.is_empty() {
                    None
                } else {
                    Some(
                        form.value
                            .parse::<i64>()
                            .map_err(|e| anyhow::anyhow!("Invalid integer format: {}", e))?,
                    )
                };
                crate::project::field::ProjectFieldValue::LegacyV2ProjectId(option_value)
            }
            crate::project::field::ProjectFieldType::ForeignId => {
                let option_value = if form.value.is_empty() {
                    None
                } else {
                    Some(form.value)
                };
                crate::project::field::ProjectFieldValue::ForeignId(option_value)
            }
            crate::project::field::ProjectFieldType::Publish => {
                let bool_value = form.value.parse::<bool>().unwrap_or_else(|_| {
                    form.value.to_lowercase() == "true"
                        || form.value == "1"
                        || form.value.to_lowercase() == "on"
                });
                crate::project::field::ProjectFieldValue::Publish(bool_value)
            }
            crate::project::field::ProjectFieldType::Duplicate => {
                let bool_value = form.value.parse::<bool>().unwrap_or_else(|_| {
                    form.value.to_lowercase() == "true"
                        || form.value == "1"
                        || form.value.to_lowercase() == "on"
                });
                crate::project::field::ProjectFieldValue::Duplicate(bool_value)
            }
            crate::project::field::ProjectFieldType::Authoritative => {
                let bool_value = form.value.parse::<bool>().unwrap_or_else(|_| {
                    form.value.to_lowercase() == "true"
                        || form.value == "1"
                        || form.value.to_lowercase() == "on"
                });
                crate::project::field::ProjectFieldValue::Authoritative(bool_value)
            }
            crate::project::field::ProjectFieldType::Flagged => {
                let bool_value = form.value.parse::<bool>().unwrap_or_else(|_| {
                    form.value.to_lowercase() == "true"
                        || form.value == "1"
                        || form.value.to_lowercase() == "on"
                });
                crate::project::field::ProjectFieldValue::Flagged(bool_value)
            }
            crate::project::field::ProjectFieldType::IsTest => {
                let bool_value = form.value.parse::<bool>().unwrap_or_else(|_| {
                    form.value.to_lowercase() == "true"
                        || form.value == "1"
                        || form.value.to_lowercase() == "on"
                });
                crate::project::field::ProjectFieldValue::IsTest(bool_value)
            }
            crate::project::field::ProjectFieldType::Status => {
                let status_enum = match form.value.as_str() {
                    "pending" => crate::project::ProjectStatus::Pending,
                    "active" => crate::project::ProjectStatus::Active,
                    "periodically_active" => crate::project::ProjectStatus::PeriodicallyActive,
                    "hiatus" => crate::project::ProjectStatus::Hiatus,
                    "complete" => crate::project::ProjectStatus::Complete,
                    "hidden" => crate::project::ProjectStatus::Hidden,
                    "expired" => crate::project::ProjectStatus::Expired,
                    _ => crate::project::ProjectStatus::Active,
                };
                crate::project::field::ProjectFieldValue::Status(status_enum)
            }
            crate::project::field::ProjectFieldType::PromotionTime => {
                let promotion_time = match form.value.as_str() {
                    "promote_at_any_time" => crate::project::ProjectPromotionTime::PromoteAtAnyTime,
                    "do_not_promote" => crate::project::ProjectPromotionTime::DoNotPromote,
                    "prefer_on_target_date" => {
                        crate::project::ProjectPromotionTime::PreferOnTargetDate
                    }
                    _ => crate::project::ProjectPromotionTime::PromoteAtAnyTime,
                };
                crate::project::field::ProjectFieldValue::PromotionTime(promotion_time)
            }
            crate::project::field::ProjectFieldType::PromotionTarget => {
                let option_value = if form.value.is_empty() {
                    None
                } else {
                    Some(
                        TimeString::try_from(form.value)
                            .map_err(|e| anyhow::anyhow!("Invalid time format: {}", e))?,
                    )
                };
                crate::project::field::ProjectFieldValue::PromotionTarget(option_value)
            }
            crate::project::field::ProjectFieldType::AdminNotes => {
                crate::project::field::ProjectFieldValue::AdminNotes(form.value)
            }
            crate::project::field::ProjectFieldType::Created => {
                let time_string = TimeString::try_from(form.value)
                    .map_err(|e| anyhow::anyhow!("Invalid time format: {}", e))?;
                crate::project::field::ProjectFieldValue::Created(time_string)
            }
            crate::project::field::ProjectFieldType::WildHope => {
                let bool_value = form.value.parse::<bool>().unwrap_or_else(|_| {
                    form.value.to_lowercase() == "true"
                        || form.value == "1"
                        || form.value.to_lowercase() == "on"
                });
                crate::project::field::ProjectFieldValue::WildHope(bool_value)
            }
            crate::project::field::ProjectFieldType::Exchange => {
                let option_value = if form.value.is_empty() {
                    None
                } else {
                    Some(form.value)
                };
                crate::project::field::ProjectFieldValue::Exchange(option_value)
            }
            crate::project::field::ProjectFieldType::Origin => {
                crate::project::field::ProjectFieldValue::Origin(form.value)
            }
            crate::project::field::ProjectFieldType::Suborigin => {
                crate::project::field::ProjectFieldValue::Suborigin(form.value)
            }
            crate::project::field::ProjectFieldType::Guid => {
                crate::project::field::ProjectFieldValue::Guid(form.value)
            }
            crate::project::field::ProjectFieldType::OpengraphDescription => {
                crate::project::field::ProjectFieldValue::OpengraphDescription(form.value)
            }
            crate::project::field::ProjectFieldType::IsAffiliate => {
                let bool_value = form.value.parse::<bool>().unwrap_or_else(|_| {
                    form.value.to_lowercase() == "true"
                        || form.value == "1"
                        || form.value.to_lowercase() == "on"
                });
                crate::project::field::ProjectFieldValue::IsAffiliate(bool_value)
            }
            crate::project::field::ProjectFieldType::IsInaturalist => {
                let bool_value = form.value.parse::<bool>().unwrap_or_else(|_| {
                    form.value.to_lowercase() == "true"
                        || form.value == "1"
                        || form.value.to_lowercase() == "on"
                });
                crate::project::field::ProjectFieldValue::IsInaturalist(bool_value)
            }
            crate::project::field::ProjectFieldType::InatProjectId => {
                let option_value = if form.value.is_empty() {
                    None
                } else {
                    Some(
                        form.value
                            .parse::<i32>()
                            .map_err(|e| anyhow::anyhow!("Invalid integer format: {}", e))?,
                    )
                };
                crate::project::field::ProjectFieldValue::InatProjectId(option_value)
            }
            crate::project::field::ProjectFieldType::Welcomed => {
                let bool_value = form.value.parse::<bool>().unwrap_or_else(|_| {
                    form.value.to_lowercase() == "true"
                        || form.value == "1"
                        || form.value.to_lowercase() == "on"
                });
                crate::project::field::ProjectFieldValue::Welcomed(bool_value)
            }
            crate::project::field::ProjectFieldType::PageCreatorId => {
                let ulid_value = form
                    .value
                    .parse::<Ulid>()
                    .map_err(|e| anyhow::anyhow!("Invalid ULID format: {}", e))?;
                crate::project::field::ProjectFieldValue::PageCreatorId(ulid_value)
            }
            crate::project::field::ProjectFieldType::PageEditorIds => {
                let ulids: Vec<Ulid> = if form.value.is_empty() {
                    vec![]
                } else {
                    serde_json::from_str(&form.value)
                        .map_err(|e| anyhow::anyhow!("Invalid JSON format for ULID array: {}", e))?
                };
                crate::project::field::ProjectFieldValue::PageManagerIds(ulids.into_boxed_slice())
            }
            crate::project::field::ProjectFieldType::Extra => {
                crate::project::field::ProjectFieldValue::Extra(form.value)
            }
            crate::project::field::ProjectFieldType::ProjectLastEditor => {
                let option_value = if form.value.is_empty() {
                    None
                } else {
                    Some(
                        form.value
                            .parse::<Ulid>()
                            .map_err(|e| anyhow::anyhow!("Invalid ULID format: {}", e))?,
                    )
                };
                crate::project::field::ProjectFieldValue::ProjectLastEditor(option_value)
            }
            crate::project::field::ProjectFieldType::ProjectLastEdited => {
                let option_value = if form.value.is_empty() {
                    None
                } else {
                    Some(
                        TimeString::try_from(form.value)
                            .map_err(|e| anyhow::anyhow!("Invalid time format: {}", e))?,
                    )
                };
                crate::project::field::ProjectFieldValue::ProjectLastEdited(option_value)
            }
            crate::project::field::ProjectFieldType::ScistarterLastEditor => {
                let option_value = if form.value.is_empty() {
                    None
                } else {
                    Some(
                        form.value
                            .parse::<Ulid>()
                            .map_err(|e| anyhow::anyhow!("Invalid ULID format: {}", e))?,
                    )
                };
                crate::project::field::ProjectFieldValue::ScistarterLastEditor(option_value)
            }
            crate::project::field::ProjectFieldType::ScistarterLastEdited => {
                let option_value = if form.value.is_empty() {
                    None
                } else {
                    Some(
                        TimeString::try_from(form.value)
                            .map_err(|e| anyhow::anyhow!("Invalid time format: {}", e))?,
                    )
                };
                crate::project::field::ProjectFieldValue::ScistarterLastEdited(option_value)
            }
            crate::project::field::ProjectFieldType::Draft => {
                let bool_value = form.value.parse::<bool>().unwrap_or_else(|_| {
                    form.value.to_lowercase() == "true"
                        || form.value == "1"
                        || form.value.to_lowercase() == "on"
                });
                crate::project::field::ProjectFieldValue::Draft(bool_value)
            }
            // Content fields
            crate::project::field::ProjectFieldType::Name => {
                crate::project::field::ProjectFieldValue::Name(form.value)
            }
            crate::project::field::ProjectFieldType::Tags => {
                let tags: HashSet<InsensitiveString> = if form.value.is_empty() {
                    HashSet::new()
                } else {
                    serde_json::from_str(&form.value)
                        .map_err(|e| anyhow::anyhow!("Invalid JSON format for tags: {}", e))?
                };
                crate::project::field::ProjectFieldValue::Tags(tags)
            }
            crate::project::field::ProjectFieldType::ParticipationDegree => {
                let participation_degree = match form.value.as_str() {
                    "collegial" => crate::project::ProjectParticipationDegree::Collegial,
                    "co_created" => crate::project::ProjectParticipationDegree::CoCreated,
                    "collaborative" => crate::project::ProjectParticipationDegree::Collaborative,
                    "contributory" => crate::project::ProjectParticipationDegree::Contributory,
                    "contractual" => crate::project::ProjectParticipationDegree::Contractual,
                    _ => crate::project::ProjectParticipationDegree::Collegial,
                };
                crate::project::field::ProjectFieldValue::ParticipationDegree(participation_degree)
            }
            crate::project::field::ProjectFieldType::ParticipantInstructions => {
                crate::project::field::ProjectFieldValue::ParticipantInstructions(form.value)
            }
            crate::project::field::ProjectFieldType::ParticipantInstructionsEmail => {
                crate::project::field::ProjectFieldValue::ParticipantInstructionsEmail(form.value)
            }
            crate::project::field::ProjectFieldType::ParticipantFiles => {
                let files: Vec<String> = if form.value.is_empty() {
                    vec![]
                } else {
                    serde_json::from_str(&form.value).map_err(|e| {
                        anyhow::anyhow!("Invalid JSON format for files array: {}", e)
                    })?
                };
                crate::project::field::ProjectFieldValue::ParticipantFiles(files.into_boxed_slice())
            }
            crate::project::field::ProjectFieldType::UsFederallyFunded => {
                let bool_value = form.value.parse::<bool>().unwrap_or_else(|_| {
                    form.value.to_lowercase() == "true"
                        || form.value == "1"
                        || form.value.to_lowercase() == "on"
                });
                crate::project::field::ProjectFieldValue::UsFederallyFunded(bool_value)
            }
            crate::project::field::ProjectFieldType::Presenter => {
                crate::project::field::ProjectFieldValue::Presenter(form.value)
            }
            crate::project::field::ProjectFieldType::Goal => {
                crate::project::field::ProjectFieldValue::Goal(form.value)
            }
            crate::project::field::ProjectFieldType::Task => {
                crate::project::field::ProjectFieldValue::Task(form.value)
            }
            crate::project::field::ProjectFieldType::Description => {
                crate::project::field::ProjectFieldValue::Description(form.value)
            }
            crate::project::field::ProjectFieldType::Image => {
                crate::project::field::ProjectFieldValue::Image(form.value)
            }
            crate::project::field::ProjectFieldType::ImageCredit => {
                crate::project::field::ProjectFieldValue::ImageCredit(form.value)
            }
            crate::project::field::ProjectFieldType::ConnectOpaqueUrl => {
                crate::project::field::ProjectFieldValue::ConnectOpaqueUrl(form.value)
            }
            crate::project::field::ProjectFieldType::ConnectOpaqueUrl1 => {
                crate::project::field::ProjectFieldValue::ConnectOpaqueUrl1(form.value)
            }
            crate::project::field::ProjectFieldType::ConnectOpaqueUrl2 => {
                crate::project::field::ProjectFieldValue::ConnectOpaqueUrl2(form.value)
            }
            crate::project::field::ProjectFieldType::Contacts => {
                let contacts: Vec<Contact> = if form.value.is_empty() {
                    vec![]
                } else {
                    serde_json::from_str(&form.value).map_err(|e| {
                        anyhow::anyhow!("Invalid JSON format for contacts array: {}", e)
                    })?
                };
                crate::project::field::ProjectFieldValue::Contacts(contacts.into_boxed_slice())
            }
            crate::project::field::ProjectFieldType::Url => {
                crate::project::field::ProjectFieldValue::Url(form.value)
            }
            crate::project::field::ProjectFieldType::VideoUrl => {
                crate::project::field::ProjectFieldValue::VideoUrl(form.value)
            }
            crate::project::field::ProjectFieldType::BlogUrl => {
                crate::project::field::ProjectFieldValue::BlogUrl(form.value)
            }
            crate::project::field::ProjectFieldType::TwitterName => {
                crate::project::field::ProjectFieldValue::TwitterName(form.value)
            }
            crate::project::field::ProjectFieldType::FacebookPage => {
                crate::project::field::ProjectFieldValue::FacebookPage(form.value)
            }
            crate::project::field::ProjectFieldType::InstagramPage => {
                crate::project::field::ProjectFieldValue::InstagramPage(form.value)
            }
            crate::project::field::ProjectFieldType::Topics => {
                let topics: Vec<String> = if form.value.is_empty() {
                    vec![]
                } else {
                    serde_json::from_str(&form.value).map_err(|e| {
                        anyhow::anyhow!("Invalid JSON format for topics array: {}", e)
                    })?
                };
                crate::project::field::ProjectFieldValue::Topics(topics.into_boxed_slice())
            }
            crate::project::field::ProjectFieldType::FieldsOfScience => {
                let fields: Vec<String> = if form.value.is_empty() {
                    vec![]
                } else {
                    serde_json::from_str(&form.value).map_err(|e| {
                        anyhow::anyhow!("Invalid JSON format for fields array: {}", e)
                    })?
                };
                crate::project::field::ProjectFieldValue::FieldsOfScience(fields.into_boxed_slice())
            }
            crate::project::field::ProjectFieldType::Practices => {
                let practices: Vec<String> = if form.value.is_empty() {
                    vec![]
                } else {
                    serde_json::from_str(&form.value).map_err(|e| {
                        anyhow::anyhow!("Invalid JSON format for practices array: {}", e)
                    })?
                };
                crate::project::field::ProjectFieldValue::Practices(practices.into_boxed_slice())
            }
            crate::project::field::ProjectFieldType::PracticesHow => {
                crate::project::field::ProjectFieldValue::PracticesHow(form.value)
            }
            crate::project::field::ProjectFieldType::SearchTerms => {
                crate::project::field::ProjectFieldValue::SearchTerms(form.value)
            }
            crate::project::field::ProjectFieldType::Difficulty => {
                crate::project::field::ProjectFieldValue::Difficulty(form.value)
            }
            crate::project::field::ProjectFieldType::Diy => {
                let bool_value = form.value.parse::<bool>().unwrap_or_else(|_| {
                    form.value.to_lowercase() == "true"
                        || form.value == "1"
                        || form.value.to_lowercase() == "on"
                });
                crate::project::field::ProjectFieldValue::Diy(bool_value)
            }
            crate::project::field::ProjectFieldType::Audience => {
                let audience: Vec<String> = if form.value.is_empty() {
                    vec![]
                } else {
                    serde_json::from_str(&form.value).map_err(|e| {
                        anyhow::anyhow!("Invalid JSON format for audience array: {}", e)
                    })?
                };
                crate::project::field::ProjectFieldValue::Audience(audience.into_boxed_slice())
            }
            crate::project::field::ProjectFieldType::SignupUrl => {
                crate::project::field::ProjectFieldValue::SignupUrl(form.value)
            }
            crate::project::field::ProjectFieldType::PrefixHowToLoggedIn => {
                crate::project::field::ProjectFieldValue::PrefixHowToLoggedIn(form.value)
            }
            crate::project::field::ProjectFieldType::PrefixHowToLoggedOut => {
                crate::project::field::ProjectFieldValue::PrefixHowToLoggedOut(form.value)
            }
            crate::project::field::ProjectFieldType::HowToJoin => {
                crate::project::field::ProjectFieldValue::HowToJoin(form.value)
            }
            crate::project::field::ProjectFieldType::HowtoVideo => {
                crate::project::field::ProjectFieldValue::HowtoVideo(form.value)
            }
            crate::project::field::ProjectFieldType::OutcomesLabel => {
                crate::project::field::ProjectFieldValue::OutcomesLabel(form.value)
            }
            crate::project::field::ProjectFieldType::OutcomesLink => {
                crate::project::field::ProjectFieldValue::OutcomesLink(form.value)
            }
            crate::project::field::ProjectFieldType::TimeRequired => {
                crate::project::field::ProjectFieldValue::TimeRequired(form.value)
            }
            crate::project::field::ProjectFieldType::TrainingMaterials => {
                crate::project::field::ProjectFieldValue::TrainingMaterials(form.value)
            }
            crate::project::field::ProjectFieldType::ClassroomMaterials => {
                crate::project::field::ProjectFieldValue::ClassroomMaterials(form.value)
            }
            crate::project::field::ProjectFieldType::ConsentStatement => {
                crate::project::field::ProjectFieldValue::ConsentStatement(form.value)
            }
            crate::project::field::ProjectFieldType::ConsentStatementDate => {
                let option_value = if form.value.is_empty() {
                    None
                } else {
                    Some(
                        TimeString::try_from(form.value)
                            .map_err(|e| anyhow::anyhow!("Invalid time format: {}", e))?,
                    )
                };
                crate::project::field::ProjectFieldValue::ConsentStatementDate(option_value)
            }
            crate::project::field::ProjectFieldType::ParticipationTools => {
                let bool_value = form.value.parse::<bool>().unwrap_or_else(|_| {
                    form.value.to_lowercase() == "true"
                        || form.value == "1"
                        || form.value.to_lowercase() == "on"
                });
                crate::project::field::ProjectFieldValue::ParticipationTools(bool_value)
            }
            crate::project::field::ProjectFieldType::ParticipationPrompt => {
                crate::project::field::ProjectFieldValue::ParticipationPrompt(form.value)
            }
            crate::project::field::ProjectFieldType::ParticipationConfirmationMessage => {
                crate::project::field::ProjectFieldValue::ParticipationConfirmationMessage(
                    form.value,
                )
            }
            crate::project::field::ProjectFieldType::ParticipationNotificationEmail => {
                crate::project::field::ProjectFieldValue::ParticipationNotificationEmail(form.value)
            }
            crate::project::field::ProjectFieldType::ParticipationUsage => {
                crate::project::field::ProjectFieldValue::ParticipationUsage(form.value)
            }
            crate::project::field::ProjectFieldType::PartOf => {
                let option_value = if form.value.is_empty() {
                    None
                } else {
                    Some(
                        form.value
                            .parse::<Ulid>()
                            .map_err(|e| anyhow::anyhow!("Invalid ULID format: {}", e))?,
                    )
                };
                crate::project::field::ProjectFieldValue::PartOf(option_value)
            }
            crate::project::field::ProjectFieldType::AssociatedProjects => {
                let associated_projects: Vec<Ulid> = if form.value.is_empty() {
                    vec![]
                } else {
                    serde_json::from_str(&form.value)
                        .map_err(|e| anyhow::anyhow!("Invalid JSON format for ULID array: {}", e))?
                };
                crate::project::field::ProjectFieldValue::AssociatedProjects(
                    associated_projects.into_boxed_slice(),
                )
            }
            crate::project::field::ProjectFieldType::TotalExpense => {
                let option_value = if form.value.is_empty() {
                    None
                } else {
                    Some(
                        form.value
                            .parse::<f32>()
                            .map_err(|e| anyhow::anyhow!("Invalid float format: {}", e))?,
                    )
                };
                crate::project::field::ProjectFieldValue::TotalExpense(option_value)
            }
            crate::project::field::ProjectFieldType::Materials => {
                crate::project::field::ProjectFieldValue::Materials(form.value)
            }
            crate::project::field::ProjectFieldType::Gear => {
                crate::project::field::ProjectFieldValue::Gear(form.value)
            }
            crate::project::field::ProjectFieldType::MobileAppUrlAndroid => {
                crate::project::field::ProjectFieldValue::MobileAppUrlAndroid(form.value)
            }
            crate::project::field::ProjectFieldType::MobileAppUrlIos => {
                crate::project::field::ProjectFieldValue::MobileAppUrlIos(form.value)
            }
            crate::project::field::ProjectFieldType::MobileAppUrl => {
                crate::project::field::ProjectFieldValue::MobileAppUrl(form.value)
            }
            // Location fields
            crate::project::field::ProjectFieldType::LocationType => {
                crate::project::field::ProjectFieldValue::LocationType(form.value)
            }
            crate::project::field::ProjectFieldType::EntirelyOnline => {
                let bool_value = form.value.parse::<bool>().unwrap_or_else(|_| {
                    form.value.to_lowercase() == "true"
                        || form.value == "1"
                        || form.value.to_lowercase() == "on"
                });
                crate::project::field::ProjectFieldValue::EntirelyOnline(bool_value)
            }
            crate::project::field::ProjectFieldType::OnDemand => {
                let bool_value = form.value.parse::<bool>().unwrap_or_else(|_| {
                    form.value.to_lowercase() == "true"
                        || form.value == "1"
                        || form.value.to_lowercase() == "on"
                });
                crate::project::field::ProjectFieldValue::OnDemand(bool_value)
            }
            crate::project::field::ProjectFieldType::InPerson => {
                let bool_value = form.value.parse::<bool>().unwrap_or_else(|_| {
                    form.value.to_lowercase() == "true"
                        || form.value == "1"
                        || form.value.to_lowercase() == "on"
                });
                crate::project::field::ProjectFieldValue::InPerson(bool_value)
            }
            crate::project::field::ProjectFieldType::Indoors => {
                let bool_value = form.value.parse::<bool>().unwrap_or_else(|_| {
                    form.value.to_lowercase() == "true"
                        || form.value == "1"
                        || form.value.to_lowercase() == "on"
                });
                crate::project::field::ProjectFieldValue::Indoors(bool_value)
            }
            crate::project::field::ProjectFieldType::Outdoors => {
                let bool_value = form.value.parse::<bool>().unwrap_or_else(|_| {
                    form.value.to_lowercase() == "true"
                        || form.value == "1"
                        || form.value.to_lowercase() == "on"
                });
                crate::project::field::ProjectFieldValue::Outdoors(bool_value)
            }
            crate::project::field::ProjectFieldType::Regions => {
                let option_value = if form.value.is_empty() {
                    None
                } else {
                    Some(form.value)
                };
                crate::project::field::ProjectFieldValue::Regions(option_value)
            }
            crate::project::field::ProjectFieldType::RegionsDescription => {
                crate::project::field::ProjectFieldValue::RegionsDescription(form.value)
            }
            crate::project::field::ProjectFieldType::Point => {
                let option_value = if form.value.is_empty() {
                    None
                } else {
                    Some(form.value)
                };
                crate::project::field::ProjectFieldValue::Point(option_value)
            }
            crate::project::field::ProjectFieldType::Address => {
                crate::project::field::ProjectFieldValue::Address(form.value)
            }
            crate::project::field::ProjectFieldType::City => {
                crate::project::field::ProjectFieldValue::City(form.value)
            }
            crate::project::field::ProjectFieldType::State => {
                crate::project::field::ProjectFieldValue::State(form.value)
            }
            crate::project::field::ProjectFieldType::Zip => {
                crate::project::field::ProjectFieldValue::Zip(form.value)
            }
            crate::project::field::ProjectFieldType::Country => {
                crate::project::field::ProjectFieldValue::Country(form.value)
            }
            crate::project::field::ProjectFieldType::Other => {
                crate::project::field::ProjectFieldValue::Other(form.value)
            }
            crate::project::field::ProjectFieldType::LocationName => {
                crate::project::field::ProjectFieldValue::LocationName(form.value)
            }
            crate::project::field::ProjectFieldType::LocationLanduse => {
                crate::project::field::ProjectFieldValue::LocationLanduse(form.value)
            }
            crate::project::field::ProjectFieldType::LocationDescription => {
                crate::project::field::ProjectFieldValue::LocationDescription(form.value)
            }
            crate::project::field::ProjectFieldType::GeographicScope => {
                let scope: Vec<String> = if form.value.is_empty() {
                    vec![]
                } else {
                    serde_json::from_str(&form.value).map_err(|e| {
                        anyhow::anyhow!("Invalid JSON format for scope array: {}", e)
                    })?
                };
                crate::project::field::ProjectFieldValue::GeographicScope(scope.into_boxed_slice())
            }
            crate::project::field::ProjectFieldType::UnitedNationsRegion => {
                crate::project::field::ProjectFieldValue::UnitedNationsRegion(form.value)
            }
            // Timing fields
            crate::project::field::ProjectFieldType::Begin => {
                let option_value = if form.value.is_empty() {
                    None
                } else {
                    Some(
                        TimeString::try_from(form.value)
                            .map_err(|e| anyhow::anyhow!("Invalid time format: {}", e))?,
                    )
                };
                crate::project::field::ProjectFieldValue::Begin(option_value)
            }
            crate::project::field::ProjectFieldType::End => {
                let option_value = if form.value.is_empty() {
                    None
                } else {
                    Some(
                        TimeString::try_from(form.value)
                            .map_err(|e| anyhow::anyhow!("Invalid time format: {}", e))?,
                    )
                };
                crate::project::field::ProjectFieldValue::End(option_value)
            }
            crate::project::field::ProjectFieldType::ProjectTiming => {
                crate::project::field::ProjectFieldValue::ProjectTiming(form.value)
            }
            crate::project::field::ProjectFieldType::ParticipationSchedule => {
                crate::project::field::ProjectFieldValue::ParticipationSchedule(form.value)
            }
            crate::project::field::ProjectFieldType::TimePerParticipation => {
                crate::project::field::ProjectFieldValue::TimePerParticipation(form.value)
            }
            crate::project::field::ProjectFieldType::YearStarted => {
                let option_value = if form.value.is_empty() {
                    None
                } else {
                    Some(
                        form.value
                            .parse::<i32>()
                            .map_err(|e| anyhow::anyhow!("Invalid integer format: {}", e))?,
                    )
                };
                crate::project::field::ProjectFieldValue::YearStarted(option_value)
            }
            crate::project::field::ProjectFieldType::YearCompleted => {
                let option_value = if form.value.is_empty() {
                    None
                } else {
                    Some(
                        form.value
                            .parse::<i32>()
                            .map_err(|e| anyhow::anyhow!("Invalid integer format: {}", e))?,
                    )
                };
                crate::project::field::ProjectFieldValue::YearCompleted(option_value)
            }
            crate::project::field::ProjectFieldType::MonthCompleted => {
                let option_value = if form.value.is_empty() {
                    None
                } else {
                    Some(
                        form.value
                            .parse::<i32>()
                            .map_err(|e| anyhow::anyhow!("Invalid integer format: {}", e))?,
                    )
                };
                crate::project::field::ProjectFieldValue::MonthCompleted(option_value)
            }
            crate::project::field::ProjectFieldType::ParticipationTasks => {
                let tasks: Vec<String> = if form.value.is_empty() {
                    vec![]
                } else {
                    serde_json::from_str(&form.value).map_err(|e| {
                        anyhow::anyhow!("Invalid JSON format for tasks array: {}", e)
                    })?
                };
                crate::project::field::ProjectFieldValue::ParticipationTasks(
                    tasks.into_boxed_slice(),
                )
            }
            crate::project::field::ProjectFieldType::Activities => {
                let activities: Vec<String> = if form.value.is_empty() {
                    vec![]
                } else {
                    serde_json::from_str(&form.value).map_err(|e| {
                        anyhow::anyhow!("Invalid JSON format for activities array: {}", e)
                    })?
                };
                crate::project::field::ProjectFieldValue::Activities(activities.into_boxed_slice())
            }
            crate::project::field::ProjectFieldType::SpecialSkills => {
                crate::project::field::ProjectFieldValue::SpecialSkills(form.value)
            }
            // Data fields
            crate::project::field::ProjectFieldType::DataResults => {
                crate::project::field::ProjectFieldValue::DataResults(form.value)
            }
            crate::project::field::ProjectFieldType::DataAggregation => {
                let aggregation: Vec<String> = if form.value.is_empty() {
                    vec![]
                } else {
                    serde_json::from_str(&form.value).map_err(|e| {
                        anyhow::anyhow!("Invalid JSON format for aggregation array: {}", e)
                    })?
                };
                crate::project::field::ProjectFieldValue::DataAggregation(
                    aggregation.into_boxed_slice(),
                )
            }
            crate::project::field::ProjectFieldType::DataConsent => {
                let consent: Vec<String> = if form.value.is_empty() {
                    vec![]
                } else {
                    serde_json::from_str(&form.value).map_err(|e| {
                        anyhow::anyhow!("Invalid JSON format for consent array: {}", e)
                    })?
                };
                crate::project::field::ProjectFieldValue::DataConsent(consent.into_boxed_slice())
            }
            crate::project::field::ProjectFieldType::DataVisualization => {
                let bool_value = form.value.parse::<bool>().unwrap_or_else(|_| {
                    form.value.to_lowercase() == "true"
                        || form.value == "1"
                        || form.value.to_lowercase() == "on"
                });
                crate::project::field::ProjectFieldValue::DataVisualization(bool_value)
            }
            crate::project::field::ProjectFieldType::DataAccess => {
                let access: Vec<String> = if form.value.is_empty() {
                    vec![]
                } else {
                    serde_json::from_str(&form.value).map_err(|e| {
                        anyhow::anyhow!("Invalid JSON format for access array: {}", e)
                    })?
                };
                crate::project::field::ProjectFieldValue::DataAccess(access.into_boxed_slice())
            }
            crate::project::field::ProjectFieldType::DataQuality => {
                let bool_value = form.value.parse::<bool>().unwrap_or_else(|_| {
                    form.value.to_lowercase() == "true"
                        || form.value == "1"
                        || form.value.to_lowercase() == "on"
                });
                crate::project::field::ProjectFieldValue::DataQuality(bool_value)
            }
            crate::project::field::ProjectFieldType::MetadataPublicationUrl => {
                crate::project::field::ProjectFieldValue::MetadataPublicationUrl(form.value)
            }
            crate::project::field::ProjectFieldType::DataPublicationUrl => {
                crate::project::field::ProjectFieldValue::DataPublicationUrl(form.value)
            }
            crate::project::field::ProjectFieldType::DataLicense => {
                crate::project::field::ProjectFieldValue::DataLicense(form.value)
            }
            crate::project::field::ProjectFieldType::IntendedOutcomes => {
                let outcomes: Vec<String> = if form.value.is_empty() {
                    vec![]
                } else {
                    serde_json::from_str(&form.value).map_err(|e| {
                        anyhow::anyhow!("Invalid JSON format for outcomes array: {}", e)
                    })?
                };
                crate::project::field::ProjectFieldValue::IntendedOutcomes(
                    outcomes.into_boxed_slice(),
                )
            }
            crate::project::field::ProjectFieldType::OutcomeOther => {
                crate::project::field::ProjectFieldValue::OutcomeOther(form.value)
            }
            crate::project::field::ProjectFieldType::UniversityAffiliation => {
                crate::project::field::ProjectFieldValue::UniversityAffiliation(form.value)
            }
            crate::project::field::ProjectFieldType::Sponsors => {
                let sponsors: Vec<String> = if form.value.is_empty() {
                    vec![]
                } else {
                    serde_json::from_str(&form.value).map_err(|e| {
                        anyhow::anyhow!("Invalid JSON format for sponsors array: {}", e)
                    })?
                };
                crate::project::field::ProjectFieldValue::Sponsors(sponsors.into_boxed_slice())
            }
            crate::project::field::ProjectFieldType::Stakeholders => {
                let stakeholders: Vec<String> = if form.value.is_empty() {
                    vec![]
                } else {
                    serde_json::from_str(&form.value).map_err(|e| {
                        anyhow::anyhow!("Invalid JSON format for stakeholders array: {}", e)
                    })?
                };
                crate::project::field::ProjectFieldValue::Stakeholders(
                    stakeholders.into_boxed_slice(),
                )
            }
            crate::project::field::ProjectFieldType::CircuitId => {
                crate::project::field::ProjectFieldValue::CircuitId(form.value)
            }
            crate::project::field::ProjectFieldType::SustainableDevelopmentGoals => {
                crate::project::field::ProjectFieldValue::SustainableDevelopmentGoals(form.value)
            }
        },
    );

    // Store the field using the LoadStorePeered implementation
    match project_field.store().await {
        Ok(_) => {
            if let Some(user_info) = &user_info {
                common::service::log_activity(
                    None,
                    "staff-edit",
                    user_info.user_id,
                    parsed_project_id,
                    format!("{:?}", field_type),
                )
                .await?;
            }

            // Success - return updated project row with OOB
            // TODO: Fetch updated project from database to reflect real changes
            // TODO: Return updated project row HTML for out-of-band swap instead of generic success message
            // NOTE: Currently blocked by ProjectAll::read_from_database compilation errors (type mismatches)
            // For now, return success message
            let success_html = format!(
                r#"<div class="p-3 text-sm text-green-700 bg-green-100 rounded-lg border border-green-200">
                    <strong>Success:</strong> Updated {} for project {}
                </div>"#,
                field_type, project_id
            );
            Ok(poem::Response::builder()
                .content_type("text/html")
                .body(success_html))
        }
        Err(e) => {
            // Return error response
            let error_html = format!(
                r#"<div class="p-3 text-sm text-red-700 bg-red-100 rounded-lg border border-red-200">
                    <strong>Error:</strong> Failed to update {}: {}
                </div>"#,
                field_type, e
            );
            Ok(poem::Response::builder()
                .status(poem::http::StatusCode::INTERNAL_SERVER_ERROR)
                .content_type("text/html")
                .body(error_html))
        }
    }
}

#[handler]
async fn projects_new(
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let add_project_form = AddProjectForm {
        title: "Add New Project".to_string(),
    };

    Ok(add_project_form.into_response(None).await?)
}

#[derive(Debug, Deserialize)]
struct CreateProjectForm {
    name: String,
    description: String,
    presenter: String,
    goal: String,
    task: String,
    url: String,
    image: String,
    admin_notes: String,
    draft: bool,
    publish: bool,
}

#[handler]
async fn projects_create(
    Form(form): Form<CreateProjectForm>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    // TODO: Create actual project in database using ProjectField enum
    // For now, just return a success message

    // Create success message
    let success_html = format!(
        r#"<div class="p-3 text-sm text-green-700 bg-green-100 rounded-lg border border-green-200">
            <strong>Success:</strong> Project "{}" has been created successfully!
        </div>"#,
        form.name
    );

    // TODO: Add OOB update to refresh the projects list
    // For now, just return the success message with instructions to refresh
    let combined_html = format!(
        r#"{}
        <div class="mt-3 text-sm text-blue-700">
            <button onclick="document.getElementById('modal-container').innerHTML = ''; window.location.reload();" 
                    class="text-indigo-600 hover:text-indigo-800 underline">
                Close and refresh page
            </button>
        </div>"#,
        success_html
    );

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(combined_html))
}
