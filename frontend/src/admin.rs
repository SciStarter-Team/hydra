use crate::component::{AdminLayout, Component, admin_layout::AdminLayoutPrepared};
use crate::contact::Contact;
use crate::gourl::{GoUrl, GoUrlType};
use crate::middleware::authorization::{Authenticated, StaffOnly};
use crate::project::field::{ProjectField, ProjectFieldType, ProjectFieldValue};
use crate::upload::storage::FileMetadata;

pub mod autocomplete;

use crate::peering::LoadStorePeered;
use crate::project::all::ProjectAll;
use crate::project::field::{
    ACTIVITIES_VOCABULARY, AUDIENCE_VOCABULARY, DATA_ACCESS_VOCABULARY,
    DATA_AGGREGATION_VOCABULARY, DATA_CONSENT_VOCABULARY, DIFFICULTY_VOCABULARY,
    FIELDS_OF_SCIENCE_VOCABULARY, GEOGRAPHIC_SCOPE_VOCABULARY, INTENDED_OUTCOMES_VOCABULARY,
    PARTICIPATION_TASKS_VOCABULARY, PRACTICES_VOCABULARY, SPONSORS_VOCABULARY,
    STAKEHOLDERS_VOCABULARY, SUSTAINABLE_DEVELOPMENT_GOALS_VOCABULARY, TOPICS_VOCABULARY,
};
use common::{
    insensitivestring::InsensitiveString,
    message::health::{
        ForumServiceHealthResponse, FrontendServiceHealthResponse, LocationServiceHealthResponse,
        LogServiceHealthResponse, UserServiceHealthResponse,
    },
    message::log::{
        AnalyticsQuery, AnalyticsResponse, RecentActivityRequest, RecentActivityResponse,
        GoUrlStatsRequest, GoUrlStatsResponse,
    },
    message::user::{
        AdminAnalyticsRequest, AdminAnalyticsResponse, AdminPasswordResetRequest,
        AdminPasswordResetResponse, AdminUser, AdminUserCreateRequest, AdminUserDeleteRequest,
        AdminUserDeleteResponse, AdminUserGetRequest, AdminUserListRequest, AdminUserListResponse,
        AdminUserResponse, AdminUserSearchRequest, AdminUserUpdateRequest, BasicInfo, UserEmail,
        UserEmailAddRequest, UserEmailDeleteRequest, UserEmailResponse,
        UserEmailToggleVerifiedRequest, UserEmailUpdateRequest,
    },
    service::{request, request_timeout, request_with_retry},
    shared::db,
    timestring::TimeString,
};
use jiff::{Timestamp, ToSpan, tz::TimeZone};
use poem::{
    Endpoint, EndpointExt, IntoResponse, Route, delete, get, handler, post, put,
    web::{Form, Json, Path, Query},
};
use sailfish::TemplateOnce;
use serde::{Deserialize, Serialize};
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
        .at(
            "/projects/:id/multiselect/:field_type",
            put(projects_update_multiselect_field),
        )
        .at(
            "/projects/:id/array/:field_type/items",
            get(projects_get_array_items).post(projects_add_array_item),
        )
        .at(
            "/projects/:id/array/:field_type/items/:index",
            put(projects_update_array_item).delete(projects_remove_array_item),
        )
        .at("/projects/:id/tags", post(projects_add_tag))
        .at("/projects/:id/tags/:tag", delete(projects_remove_tag))
        .at(
            "/projects/:id/contacts/items",
            get(projects_get_contacts).post(projects_add_contact),
        )
        .at(
            "/projects/:id/contacts/items/:contact_id",
            put(projects_update_contact).delete(projects_remove_contact),
        )
        .at("/events", get(events))
        .at("/events/search", get(events_search))
        .at("/analytics", get(analytics))
        .at("/help", get(help))
        .at("/engine-room", get(engine_room))
        .at(
            "/engine-room/sync-page-managers",
            post(engine_room_sync_page_managers),
        )
        .at("/files-links", get(files_links))
        .at("/files-links/search", get(files_links_search))
        .at("/files-links/redirect", post(files_links_create_redirect))
        .at("/files-links/:slug", delete(files_links_delete_go_url))
        .at("/files-links/:slug/stats", get(files_links_get_stats).post(files_links_post_stats))
        .at("/clear-modal", post(clear_modal))
        .nest("/autocomplete", autocomplete::routes())
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
    pub created_date: String,
    pub page_creator_id: String,
    pub page_creator_email: Option<String>,
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
    pub tags: Vec<String>,
}

impl AdminProject {
    /// Generate hash-based color classes for origin pill
    pub fn origin_color_classes(&self) -> String {
        self.hash_color_classes(&self.origin)
    }

    /// Generate color classes for status pill
    pub fn status_color_classes(&self) -> String {
        match self.status.as_str() {
            "Active" => "bg-green-100 text-green-800".to_string(),
            "Pending" => "bg-yellow-100 text-yellow-800".to_string(),
            "Hiatus" => "bg-orange-100 text-orange-800".to_string(),
            "Complete" => "bg-blue-100 text-blue-800".to_string(),
            "Hidden" => "bg-red-100 text-red-800".to_string(),
            _ => "bg-gray-100 text-gray-800".to_string(),
        }
    }

    /// Format created date and time in human-readable Eastern timezone format
    /// Returns (date_part, time_part) tuple
    pub fn formatted_created_date_time(&self) -> (String, String) {
        use jiff::{Timestamp, tz::TimeZone};

        // Try to parse the created_date string as a Timestamp
        if let Ok(timestamp) = self.created_date.parse::<Timestamp>() {
            let eastern_tz = TimeZone::get("America/New_York").unwrap();
            let zoned = timestamp.to_zoned(eastern_tz);
            let date_part = zoned.strftime("%b %d, %Y").to_string();
            let time_part = zoned.strftime("%I:%M %p %Z").to_string();
            (date_part, time_part)
        } else {
            // Fallback to original string if parsing fails
            (self.created_date.clone(), String::new())
        }
    }

    fn hash_color_classes(&self, value: &str) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        value.hash(&mut hasher);
        let hash = hasher.finish();

        // Define a set of pleasant, accessible color combinations
        let colors = [
            "bg-blue-100 text-blue-800",
            "bg-green-100 text-green-800",
            "bg-purple-100 text-purple-800",
            "bg-pink-100 text-pink-800",
            "bg-indigo-100 text-indigo-800",
            "bg-teal-100 text-teal-800",
            "bg-amber-100 text-amber-800",
            "bg-cyan-100 text-cyan-800",
        ];

        colors[(hash % colors.len() as u64) as usize].to_string()
    }
}

#[derive(Debug)]
pub struct AdminProjectFull {
    pub id: String,
    pub title: String,
    pub status: String,
    pub publish: bool,
    pub created_date: String,
    pub page_creator_id: String,
    pub page_creator_email: Option<String>,
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
    pub tags: Vec<String>,
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

impl AdminProjectFull {
    /// Convert a ProjectAll to AdminProjectFull, performing user lookups for editor fields
    pub async fn from_project_all(project: ProjectAll) -> Result<Self, anyhow::Error> {
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
            page_creator_email: Some(lookup_user_email(Some(project.page_creator_id)).await),
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
            promotion_target: project.promotion_target.map(|t| t.to_string()),
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
            tags: project
                .tags
                .into_iter()
                .map(|t| t.to_string())
                .collect::<Vec<_>>(),
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
            consent_statement_date: project.consent_statement_date.map(|t| t.to_string()),
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

async fn fetch_recent_projects() -> Result<Vec<AdminProject>, anyhow::Error> {
    fetch_projects_page(None, None, None, None, None, None, None, Some(1), Some(10)).await
}

async fn fetch_projects_page(
    search: Option<&str>,
    status_filter: Option<&str>,
    publish_filter: Option<&str>,
    flagged_filter: Option<&str>,
    spatial_overlap: Option<&str>, // GeoJSON geometry for overlap search
    spatial_point: Option<(f64, f64, f64)>, // (lat, lng, radius_meters) circle for intersection search
    proximity_sort: Option<(f64, f64)>,     // (lat, lng) for proximity sorting
    page: Option<u32>,
    per_page: Option<u32>,
) -> Result<Vec<AdminProject>, anyhow::Error> {
    use common::shared::db;
    use sqlx::QueryBuilder;

    let page = page.unwrap_or(1);
    let per_page = per_page.unwrap_or(25);
    let offset = (page - 1) * per_page;

    let mut query = QueryBuilder::new("");

    if let Some(search_term) = search.filter(|s| !s.trim().is_empty()) {
        query.push(
            "SELECT p.project_id, p.name, p.status, p.publish, p.created, p.presenter, 
                    p.origin, p.flagged, p.duplicate, p.is_test, p.goal, p.task, p.image, 
                    p.url, p.admin_notes, p.draft, p.description 
             FROM projects p 
             JOIN projects_fts f ON p.project_id = f.project_id 
             WHERE projects_fts MATCH ",
        );
        // With porter stemming enabled, use simple term matching for better flexibility
        query.push_bind(search_term);
    } else {
        query.push(
            "SELECT p.project_id, p.name, p.status, p.publish, p.created, p.presenter, 
                    p.origin, p.flagged, p.duplicate, p.is_test, p.goal, p.task, p.image, 
                    p.url, p.admin_notes, p.draft, p.description 
             FROM projects p WHERE 1=1",
        );
    }

    if let Some(status) = status_filter.filter(|s| !s.is_empty()) {
        query.push(" AND p.status = ");
        query.push_bind(status);
    }

    if let Some(publish) = publish_filter.filter(|s| !s.is_empty()) {
        let publish_bool = publish == "true";
        query.push(" AND p.publish = ");
        query.push_bind(publish_bool);
    }

    if let Some(flagged) = flagged_filter.filter(|s| !s.is_empty()) {
        let flagged_bool = flagged == "true";
        query.push(" AND p.flagged = ");
        query.push_bind(flagged_bool);
    }

    // Add spatial filtering
    if let Some(geojson) = spatial_overlap {
        query.push(" AND (ST_Intersects(p.regions, GeomFromGeoJSON(");
        query.push_bind(geojson);
        query.push(")) OR ST_Intersects(p.point, GeomFromGeoJSON(");
        query.push_bind(geojson);
        query.push(")))");
    }

    if let Some((lat, lng, radius)) = spatial_point {
        // Create a circle and test intersection with both regions and point
        query.push(" AND (ST_Intersects(p.regions, ST_Buffer(MakePoint(");
        query.push_bind(lng); // longitude first for MakePoint
        query.push(", ");
        query.push_bind(lat);
        query.push(", 4326), ");
        query.push_bind(radius);
        query.push(")) OR ST_DWithin(p.point, MakePoint(");
        query.push_bind(lng);
        query.push(", ");
        query.push_bind(lat);
        query.push(", 4326), ");
        query.push_bind(radius);
        query.push("))");
    }

    // Use different ordering based on whether this is a search query or spatial sort
    if let Some((lat, lng)) = proximity_sort {
        // For proximity sorting with special handling for regions vs points
        // For regions: when point is within regions, sort by area (smaller regions first)
        //              when point is outside regions, add large constant to distance to deprioritize
        // For points: just sort by straight distance
        query.push(
            " ORDER BY CASE WHEN p.regions IS NOT NULL AND ST_Contains(p.regions, MakePoint(",
        );
        query.push_bind(lng);
        query.push(", ");
        query.push_bind(lat);
        query.push(", 4326)) THEN ST_Area(p.regions) WHEN p.regions IS NOT NULL THEN ST_Distance(p.regions, MakePoint(");
        query.push_bind(lng);
        query.push(", ");
        query.push_bind(lat);
        query.push(
            ", 4326)) + 50000000 WHEN p.point IS NOT NULL THEN ST_Distance(p.point, MakePoint(",
        );
        query.push_bind(lng);
        query.push(", ");
        query.push_bind(lat);
        query.push(", 4326)) ELSE 999999999 END ASC, p.created DESC LIMIT "); // Very large value for projects with no geometry
    } else if search.filter(|s| !s.trim().is_empty()).is_some() {
        // For search queries, order by relevance (best match first), then by creation date
        query.push(" ORDER BY bm25(projects_fts), p.created DESC LIMIT ");
    } else {
        // For non-search queries, order by creation date only
        query.push(" ORDER BY p.created DESC LIMIT ");
    }
    query.push_bind(per_page);
    query.push(" OFFSET ");
    query.push_bind(offset);

    #[derive(sqlx::FromRow)]
    struct ProjectRow {
        project_id: String,
        name: String,
        status: String,
        publish: bool,
        created: String,
        presenter: String,
        origin: String,
        flagged: bool,
        duplicate: bool,
        is_test: bool,
        goal: String,
        task: String,
        image: String,
        url: String,
        admin_notes: String,
        draft: bool,
        description: String,
    }

    let project_rows = query.build_query_as::<ProjectRow>().fetch_all(db()).await?;

    let admin_projects = project_rows
        .into_iter()
        .map(|row| {
            AdminProject {
                id: row.project_id,
                title: row.name,
                status: row.status,
                publish: row.publish,
                created_date: row.created,
                page_creator_id: String::new(), // Not selected in ProjectRow query
                page_creator_email: None,       // Not needed for project list
                description: row.description,
                presenter: row.presenter,
                origin: row.origin,
                flagged: row.flagged,
                duplicate: row.duplicate,
                is_test: row.is_test,
                goal: row.goal,
                task: row.task,
                image: row.image,
                url: row.url,
                admin_notes: row.admin_notes,
                draft: row.draft,
                tags: Vec::new(), // Tags not needed for project list
            }
        })
        .collect();

    Ok(admin_projects)
}

async fn count_projects(
    search: Option<&str>,
    status_filter: Option<&str>,
    publish_filter: Option<&str>,
    flagged_filter: Option<&str>,
    spatial_overlap: Option<&str>,
    spatial_point: Option<(f64, f64, f64)>,
) -> Result<u64, anyhow::Error> {
    use common::shared::db;
    use sqlx::QueryBuilder;

    let mut query = QueryBuilder::new("");

    if let Some(search_term) = search.filter(|s| !s.trim().is_empty()) {
        query.push(
            "SELECT COUNT(*) FROM projects p 
             JOIN projects_fts f ON p.project_id = f.project_id 
             WHERE projects_fts MATCH ",
        );
        // With porter stemming enabled, use simple term matching for better flexibility
        query.push_bind(search_term);
    } else {
        query.push("SELECT COUNT(*) FROM projects p WHERE 1=1");
    }

    if let Some(status) = status_filter.filter(|s| !s.is_empty()) {
        query.push(" AND p.status = ");
        query.push_bind(status);
    }

    if let Some(publish) = publish_filter.filter(|s| !s.is_empty()) {
        let publish_bool = publish == "true";
        query.push(" AND p.publish = ");
        query.push_bind(publish_bool);
    }

    if let Some(flagged) = flagged_filter.filter(|s| !s.is_empty()) {
        let flagged_bool = flagged == "true";
        query.push(" AND p.flagged = ");
        query.push_bind(flagged_bool);
    }

    // Add spatial filtering (same as in fetch_projects_page)
    if let Some(geojson) = spatial_overlap {
        query.push(" AND (ST_Intersects(p.regions, GeomFromGeoJSON(");
        query.push_bind(geojson);
        query.push(")) OR ST_Intersects(p.point, GeomFromGeoJSON(");
        query.push_bind(geojson);
        query.push(")))");
    }

    if let Some((lat, lng, radius)) = spatial_point {
        // Create a circle and test intersection with both regions and point
        query.push(" AND (ST_Intersects(p.regions, ST_Buffer(MakePoint(");
        query.push_bind(lng); // longitude first for MakePoint
        query.push(", ");
        query.push_bind(lat);
        query.push(", 4326), ");
        query.push_bind(radius);
        query.push(")) OR ST_DWithin(p.point, MakePoint(");
        query.push_bind(lng);
        query.push(", ");
        query.push_bind(lat);
        query.push(", 4326), ");
        query.push_bind(radius);
        query.push("))");
    }

    let count: (i64,) = query.build_query_as::<(i64,)>().fetch_one(db()).await?;

    Ok(count.0 as u64)
}

fn humanize_event_date(datetime_str: Option<String>) -> String {
    match datetime_str {
        Some(date_str) => match TimeString::try_from(date_str) {
            Ok(time_string) => time_string.format_display(),
            Err(_) => "Invalid date".to_string(),
        },
        None => "No upcoming instances".to_string(),
    }
}

async fn fetch_events_page(
    search: Option<&str>,
    status_filter: Option<&str>,
    publish_filter: Option<&str>,
    page: Option<u32>,
    per_page: Option<u32>,
) -> Result<Vec<AdminEvent>, anyhow::Error> {
    use common::shared::db;
    use sqlx::QueryBuilder;

    let page = page.unwrap_or(1);
    let per_page = per_page.unwrap_or(25);
    let offset = (page - 1) * per_page;

    #[derive(sqlx::FromRow)]
    struct EventRow {
        event_id: String,
        name: String,
        publish: i32,
        created: String,
        location_name: String,
        event_type: String,
        entirely_online: bool,
        next_instance_begin: Option<String>,
        instance_count: i64,
        is_currently_ongoing: i32,
    }

    let mut query = QueryBuilder::new("");

    if let Some(search_term) = search.filter(|s| !s.trim().is_empty()) {
        query.push(
            "SELECT e.event_id, e.name, e.publish, e.created, e.location_name, e.event_type, e.entirely_online,
                    MIN(CASE WHEN ei.begin > datetime('now') THEN ei.begin END) as next_instance_begin,
                    COUNT(ei.begin) as instance_count,
                    CASE WHEN EXISTS(SELECT 1 FROM event_instances ei2 WHERE ei2.event_id = e.event_id AND ei2.begin <= datetime('now') AND ei2.end > datetime('now')) THEN 1 ELSE 0 END as is_currently_ongoing
             FROM events e
             JOIN events_fts f ON e.event_id = f.event_id
             LEFT JOIN event_instances ei ON e.event_id = ei.event_id
             WHERE events_fts MATCH "
        );
        query.push_bind(search_term);
    } else {
        query.push(
            "SELECT e.event_id, e.name, e.publish, e.created, e.location_name, e.event_type, e.entirely_online,
                    MIN(CASE WHEN ei.begin > datetime('now') THEN ei.begin END) as next_instance_begin,
                    COUNT(ei.begin) as instance_count,
                    CASE WHEN EXISTS(SELECT 1 FROM event_instances ei2 WHERE ei2.event_id = e.event_id AND ei2.begin <= datetime('now') AND ei2.end > datetime('now')) THEN 1 ELSE 0 END as is_currently_ongoing
             FROM events e
             LEFT JOIN event_instances ei ON e.event_id = ei.event_id
             WHERE 1=1"
        );
    }

    if let Some(status) = status_filter.filter(|s| !s.is_empty()) {
        match status {
            "Upcoming" => {
                query.push(" AND EXISTS (SELECT 1 FROM event_instances ei2 WHERE ei2.event_id = e.event_id AND ei2.end > datetime('now'))");
            }
            "Past" => {
                query.push(" AND NOT EXISTS (SELECT 1 FROM event_instances ei2 WHERE ei2.event_id = e.event_id AND ei2.end > datetime('now'))");
            }
            _ => {
                // "All Status" or any other value - no additional filtering
            }
        }
    }

    if let Some(publish) = publish_filter.filter(|s| !s.is_empty()) {
        let publish_bool = publish == "true";
        query.push(" AND e.publish = ");
        query.push_bind(publish_bool);
    }

    query.push(" GROUP BY e.event_id, e.name, e.publish, e.created, e.location_name, e.event_type, e.entirely_online");
    query.push(" ORDER BY e.created DESC LIMIT ");
    query.push_bind(per_page);
    query.push(" OFFSET ");
    query.push_bind(offset);

    let event_rows = query.build_query_as::<EventRow>().fetch_all(db()).await?;

    let admin_events = event_rows
        .into_iter()
        .map(|row| {
            let location = if row.entirely_online {
                "Online".to_string()
            } else {
                row.location_name
            };

            // Check if event is currently ongoing
            let event_date = if row.is_currently_ongoing != 0 {
                "Currently ongoing".to_string()
            } else {
                humanize_event_date(row.next_instance_begin)
            };

            AdminEvent {
                id: row.event_id,
                title: row.name,
                published: if row.publish != 0 {
                    "Published".to_string()
                } else {
                    "Unpublished".to_string()
                },
                instances: row.instance_count as u32,
                event_date,
                location,
                event_type: row.event_type,
            }
        })
        .collect();

    Ok(admin_events)
}

async fn count_events(
    search: Option<&str>,
    status_filter: Option<&str>,
    publish_filter: Option<&str>,
) -> Result<u64, anyhow::Error> {
    use common::shared::db;
    use sqlx::QueryBuilder;

    let mut query = QueryBuilder::new("");

    if let Some(search_term) = search.filter(|s| !s.trim().is_empty()) {
        query.push(
            "SELECT COUNT(*) FROM events e 
             JOIN events_fts f ON e.event_id = f.event_id 
             WHERE events_fts MATCH ",
        );
        query.push_bind(search_term);
    } else {
        query.push("SELECT COUNT(*) FROM events e WHERE 1=1");
    }

    if let Some(status) = status_filter.filter(|s| !s.is_empty()) {
        match status {
            "Upcoming" => {
                query.push(" AND EXISTS (SELECT 1 FROM event_instances ei WHERE ei.event_id = e.event_id AND ei.end > datetime('now'))");
            }
            "Past" => {
                query.push(" AND NOT EXISTS (SELECT 1 FROM event_instances ei WHERE ei.event_id = e.event_id AND ei.end > datetime('now'))");
            }
            _ => {
                // "All Status" or any other value - no additional filtering
            }
        }
    }

    if let Some(publish) = publish_filter.filter(|s| !s.is_empty()) {
        let publish_bool = publish == "true";
        query.push(" AND e.publish = ");
        query.push_bind(publish_bool);
    }

    let count: (i64,) = query.build_query_as::<(i64,)>().fetch_one(db()).await?;

    Ok(count.0 as u64)
}

#[derive(Debug)]
pub struct ProjectsPage {
    page: Option<u32>,
    search: Option<String>,
    status: Option<String>,
    publish: Option<String>,
    flagged: Option<String>,
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
    flagged: String,
}

impl Component for ProjectsPage {
    type Param = Option<Arc<BasicInfo>>;
    type Prepared = AdminLayoutPrepared;

    async fn prepare(self, param: Self::Param) -> Result<Self::Prepared, anyhow::Error> {
        let page = self.page.unwrap_or(1);
        let per_page = 25u32;

        let search_ref = self.search.as_deref();
        let status_ref = self.status.as_deref();
        let publish_ref = self.publish.as_deref();

        let flagged_ref = self.flagged.as_deref();
        let projects_list = match fetch_projects_page(
            search_ref,
            status_ref,
            publish_ref,
            flagged_ref,
            None, // spatial_overlap
            None, // spatial_point
            None, // proximity_sort
            Some(page),
            Some(per_page),
        )
        .await
        {
            Ok(project_results) => project_results,
            Err(e) => {
                tracing::warn!("Failed to fetch projects from database: {}", e);
                vec![]
            }
        };

        let total_projects = count_projects(
            search_ref,
            status_ref,
            publish_ref,
            flagged_ref,
            None, // spatial_overlap
            None, // spatial_point
        )
        .await
        .unwrap_or(0);
        let total_pages = (total_projects + per_page as u64 - 1) / per_page as u64;

        let projects_content = ProjectsPagePrepared {
            projects: projects_list.clone(),
            current_page: page,
            total_projects,
            has_previous: page > 1,
            has_next: page < total_pages as u32,
            per_page,
            start_item: ((page - 1) * per_page) + 1,
            end_item: std::cmp::min(page * per_page, total_projects as u32),
            pagination_url: "/-admin/projects".to_string(),
            pagination_include:
                "[name='search'], [name='status'], [name='publish'], [name='flagged']".to_string(),
            search_term: self.search.unwrap_or_default(),
            selected_status: self.status.unwrap_or_default(),
            selected_publish: self.publish.unwrap_or_default(),
            flagged: self.flagged.unwrap_or_default(),
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
    pub published: String,
    pub instances: u32, // Number of scheduled instances
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
    search_term: String,
    selected_status: String,
    selected_publish: String,
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/events_content.stpl.html")]
pub struct EventsContent {
    pub events: Vec<AdminEvent>,
    pub current_page: u32,
    pub total_events: u32,
    pub has_previous: bool,
    pub has_next: bool,
    pub per_page: u32,
    pub start_item: u32,
    pub end_item: u32,
    pub pagination_url: String,
    pub pagination_include: String,
    pub search_term: String,
    pub selected_status: String,
    pub selected_publish: String,
}

impl Component for EventsPage {
    type Param = Option<Arc<BasicInfo>>;
    type Prepared = AdminLayoutPrepared;

    async fn prepare(self, param: Self::Param) -> Result<Self::Prepared, anyhow::Error> {
        let page = 1u32; // For now, just show first page on dashboard
        let per_page = 10u32; // Show top 10 recent events

        let events_list =
            match fetch_events_page(None, None, None, Some(page), Some(per_page)).await {
                Ok(event_results) => event_results,
                Err(e) => {
                    tracing::warn!("Failed to fetch events from database: {}", e);
                    vec![]
                }
            };

        let total_events = count_events(None, None, None).await.unwrap_or(0);

        let events_content = EventsPagePrepared {
            events: events_list.clone(),
            current_page: page,
            total_events: total_events as u32,
            has_previous: false,
            has_next: total_events > per_page as u64,
            start_item: 1,
            end_item: std::cmp::min(per_page, total_events as u32),
            search_term: "".to_string(),
            selected_status: "".to_string(),
            selected_publish: "".to_string(),
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
    verified_users: u32,
    total_projects: u32,
    total_events: u32,
    monthly_signups: u32,
    weekly_signups: u32,
    daily_signups: u32,
    top_projects: Vec<AdminProject>,
    user_growth_data: String, // JSON for charts
}

impl Component for AnalyticsPage {
    type Param = Option<Arc<BasicInfo>>;
    type Prepared = AdminLayoutPrepared;

    async fn prepare(self, param: Self::Param) -> Result<Self::Prepared, anyhow::Error> {
        // Fetch database statistics for frontend data
        let total_projects_result =
            sqlx::query!("SELECT COUNT(*) as count FROM projects WHERE publish = true")
                .fetch_one(db())
                .await?;
        let total_projects = total_projects_result.count as u32;

        let total_events_result =
            sqlx::query!("SELECT COUNT(*) as count FROM events WHERE publish = true")
                .fetch_one(db())
                .await?;
        let total_events = total_events_result.count as u32;

        // Fetch user analytics from user service
        let user_analytics = request_timeout(
            "user.stats.admin_analytics",
            AdminAnalyticsRequest {},
            Duration::from_secs(5),
        )
        .await
        .unwrap_or(AdminAnalyticsResponse {
            total_users: 0,
            verified_users: 0,
            monthly_signups: 0,
            weekly_signups: 0,
            daily_signups: 0,
        });

        // Get top projects by using the existing projects query
        let top_projects = fetch_projects_page(
            None,           // no search
            Some("active"), // only active
            Some("true"),   // only published
            None,           // no flagged filter
            None,           // spatial_overlap
            None,           // spatial_point
            None,           // proximity_sort
            Some(1),        // first page
            Some(5),        // top 5
        )
        .await
        .unwrap_or_default();

        // Fetch user growth data from log service
        let user_growth_data = match request_timeout(
            "log.analytics.user_growth",
            AnalyticsQuery {
                start_date: Timestamp::now()
                    .to_zoned(TimeZone::UTC)
                    .date()
                    .checked_sub(120.days())
                    .unwrap()
                    .to_string(),
                end_date: Timestamp::now().to_zoned(TimeZone::UTC).date().to_string(),
            },
            Duration::from_secs(5),
        )
        .await
        {
            Ok(AnalyticsResponse::WeeklyCounts(counts)) => {
                // Convert to JSON for frontend charts
                let labels: Vec<String> = counts.iter().map(|c| c.week_start.clone()).collect();
                let data: Vec<i64> = counts.iter().map(|c| c.count).collect();
                serde_json::json!({
                    "labels": labels,
                    "data": data
                })
                .to_string()
            }
            Ok(AnalyticsResponse::MonthlyCounts(counts)) => {
                let labels: Vec<String> = counts.iter().map(|c| c.month_start.clone()).collect();
                let data: Vec<i64> = counts.iter().map(|c| c.count).collect();
                serde_json::json!({
                    "labels": labels,
                    "data": data
                })
                .to_string()
            }
            _ => {
                // Fallback to basic growth simulation based on total users
                let months = vec!["Jan", "Feb", "Mar", "Apr", "May", "Jun"];
                let base_growth = if user_analytics.total_users > 6 {
                    user_analytics.total_users / 6
                } else {
                    1
                };
                let data: Vec<u32> = (1..=6).map(|i| base_growth * i).collect();
                serde_json::json!({
                    "labels": months,
                    "data": data
                })
                .to_string()
            }
        };

        let analytics_content = AnalyticsPagePrepared {
            total_users: user_analytics.total_users,
            verified_users: user_analytics.verified_users,
            total_projects,
            total_events,
            monthly_signups: user_analytics.monthly_signups,
            weekly_signups: user_analytics.weekly_signups,
            daily_signups: user_analytics.daily_signups,
            top_projects,
            user_growth_data,
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
            pagination_include: "#user-search-form".to_string(),
            search_term: "".to_string(),
            selected_role: "".to_string(),
            selected_email_status: "".to_string(),
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
    pub search_term: String,
    pub selected_role: String,
    pub selected_email_status: String,
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
        search_term: query.search.as_deref().unwrap_or("").to_string(),
        selected_role: query.role.as_deref().unwrap_or("").to_string(),
        selected_email_status: query.email_status.as_deref().unwrap_or("").to_string(),
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
    if query.search.is_some()
        || query.status.is_some()
        || query.publish.is_some()
        || query.flagged.is_some()
    {
        let search_query = ProjectSearchQuery {
            search: query.search,
            status: query.status,
            publish: query.publish,
            flagged: query.flagged,
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
        flagged: query.flagged,
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

    match request_with_retry::<AdminUserDeleteResponse>("user.admin.user_delete", delete_request)
        .await
    {
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
        Err(_) => {
            // This should never happen since request_with_retry retries indefinitely
            // But we include it for completeness
            Ok(poem::Response::builder()
                .status(poem::http::StatusCode::INTERNAL_SERVER_ERROR)
                .content_type("text/html")
                .body("Service error"))
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

    match request_with_retry::<AdminPasswordResetResponse>(
        "user.admin.password_reset",
        reset_request,
    )
    .await
    {
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

    match request_with_retry::<AdminUserResponse>("user.admin.user_create", create_request).await {
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
                        search_term: "".to_string(),
                        selected_role: "".to_string(),
                        selected_email_status: "".to_string(),
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

    match request_with_retry::<AdminUserResponse>("user.admin.user_update", update_request).await {
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
    match crate::page_manager::full_page_editor_sync().await {
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

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/partials/email_row.stpl.html")]
struct EmailRow {
    user_id: String,
    email: UserEmail,
    index: usize,
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

    match request_with_retry::<UserEmailResponse>("user.admin.email_add", email_request).await {
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

    match request_with_retry::<UserEmailResponse>("user.admin.email_update", email_request).await {
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

    match request_with_retry::<UserEmailResponse>("user.admin.email_delete", email_request).await {
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

    match request_with_retry::<AdminUserResponse>("user.admin.email_toggle_verified", email_request)
        .await
    {
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
                let combined_html = format!("{}\n{}", email_row_html, user_row_html);

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
    flagged: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
struct ProjectSearchQuery {
    search: Option<String>,
    status: Option<String>,
    publish: Option<String>,
    flagged: Option<String>,
    page: Option<u32>,
}

#[derive(Debug, Deserialize, Clone)]
struct EventSearchQuery {
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
    pub search_term: String,
    pub selected_status: String,
    pub selected_publish: String,
    pub flagged: String,
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/partials/project_row.stpl.html")]
pub struct ProjectRow {
    pub project: AdminProject,
    pub oob: bool,
    pub oob_swap: String,
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/partials/redirect_success_response.stpl.html")]
pub struct RedirectSuccessResponse {
    pub slug: String,
    pub go_urls_list: GoUrlsList,
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/partials/upload_success_response.stpl.html")]
pub struct UploadSuccessResponse {
    pub slug: String,
    pub file_id: ulid::Ulid,
    pub go_urls_list: GoUrlsList,
}

#[derive(Debug)]
struct EditProjectPage {
    project: AdminProjectFull,
}

impl Component for EditProjectPage {
    type Param = Option<Arc<BasicInfo>>;
    type Prepared = AdminLayoutPrepared;

    async fn prepare(self, user_info: Self::Param) -> anyhow::Result<Self::Prepared> {
        let title = self.project.title.clone();
        let content = EditProjectContent {
            project: self.project,
        };
        let content_html = content.render_once()?;

        AdminLayout::new(&format!("Edit Project: {}", title))
            .with_breadcrumb("Projects", Some("/-admin/projects"))
            .with_breadcrumb(&title, None)
            .with_sidebar_active("projects")
            .with_content(content_html)
            .prepare(user_info)
            .await
    }
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/edit_project_content.stpl.html")]
struct EditProjectContent {
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
#[derive(Debug, Deserialize)]
struct UpdateProjectMultiselectForm {
    value: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct AddArrayItemForm {
    value: String,
}

#[derive(Debug, Deserialize)]
struct AddTagForm {
    tag: String,
}

#[derive(TemplateOnce)]
#[template(path = "admin/partials/tags_display.stpl.html")]
struct TagsDisplay {
    project_id: String,
    tags: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct UpdateArrayItemForm {
    value: String,
}

#[derive(Debug)]
struct ArrayItem {
    value: String,
    resolved_name: Option<String>,
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/partials/array_items.stpl.html")]
struct ArrayItems {
    project_id: Ulid,
    field_type: ProjectFieldType,
    items: Vec<ArrayItem>,
    vocabulary: Option<Vec<String>>,
}

impl ArrayItems {
    async fn new_with_resolved_names(
        project_id: Ulid,
        field_type: ProjectFieldType,
        items: Vec<String>,
    ) -> Result<Self, anyhow::Error> {
        use crate::admin::autocomplete::{resolve_project_names, resolve_user_names};

        let vocabulary = Self::get_vocabulary(field_type);

        let resolved_items = if field_type == ProjectFieldType::AssociatedProjects {
            // Parse ULIDs and resolve project names
            let ulids: Vec<Ulid> = items
                .iter()
                .filter_map(|s| s.parse::<Ulid>().ok())
                .collect();

            let resolved_names = resolve_project_names(ulids).await?;

            items
                .into_iter()
                .zip(
                    resolved_names
                        .into_iter()
                        .map(|(_, name)| Some(name))
                        .chain(std::iter::repeat(None)),
                )
                .map(|(value, resolved_name)| ArrayItem {
                    value,
                    resolved_name,
                })
                .collect()
        } else if field_type == ProjectFieldType::PageEditorIds {
            // Parse ULIDs and resolve user emails
            let ulids: Vec<Ulid> = items
                .iter()
                .filter_map(|s| s.parse::<Ulid>().ok())
                .collect();

            let resolved_names = resolve_user_names(ulids).await?;

            items
                .into_iter()
                .zip(
                    resolved_names
                        .into_iter()
                        .map(|(_, email)| Some(email))
                        .chain(std::iter::repeat(None)),
                )
                .map(|(value, resolved_name)| ArrayItem {
                    value,
                    resolved_name,
                })
                .collect()
        } else {
            items
                .into_iter()
                .map(|value| ArrayItem {
                    value,
                    resolved_name: None,
                })
                .collect()
        };

        Ok(Self {
            project_id,
            field_type,
            items: resolved_items,
            vocabulary,
        })
    }

    fn get_vocabulary(field_type: ProjectFieldType) -> Option<Vec<String>> {
        match field_type {
            ProjectFieldType::ParticipationTasks => Some(
                PARTICIPATION_TASKS_VOCABULARY
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
            ),
            ProjectFieldType::Activities => Some(
                ACTIVITIES_VOCABULARY
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
            ),
            ProjectFieldType::DataAggregation => Some(
                DATA_AGGREGATION_VOCABULARY
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
            ),
            ProjectFieldType::DataConsent => Some(
                DATA_CONSENT_VOCABULARY
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
            ),
            ProjectFieldType::DataAccess => Some(
                DATA_ACCESS_VOCABULARY
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
            ),
            ProjectFieldType::IntendedOutcomes => Some(
                INTENDED_OUTCOMES_VOCABULARY
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
            ),
            ProjectFieldType::Sponsors => {
                Some(SPONSORS_VOCABULARY.iter().map(|s| s.to_string()).collect())
            }
            ProjectFieldType::Stakeholders => Some(
                STAKEHOLDERS_VOCABULARY
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
            ),
            ProjectFieldType::Difficulty => Some(
                DIFFICULTY_VOCABULARY
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
            ),
            ProjectFieldType::Topics => {
                Some(TOPICS_VOCABULARY.iter().map(|s| s.to_string()).collect())
            }
            ProjectFieldType::FieldsOfScience => Some(
                FIELDS_OF_SCIENCE_VOCABULARY
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
            ),
            ProjectFieldType::Practices => {
                Some(PRACTICES_VOCABULARY.iter().map(|s| s.to_string()).collect())
            }
            ProjectFieldType::Audience => {
                Some(AUDIENCE_VOCABULARY.iter().map(|s| s.to_string()).collect())
            }
            ProjectFieldType::GeographicScope => Some(
                GEOGRAPHIC_SCOPE_VOCABULARY
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
            ),
            ProjectFieldType::SustainableDevelopmentGoals => Some(
                SUSTAINABLE_DEVELOPMENT_GOALS_VOCABULARY
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
            ),
            _ => None,
        }
    }
}

// Contact widget structures
#[derive(Debug)]
struct ContactItem {
    contact: Contact,
    read_only: bool,
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/partials/contact_items.stpl.html")]
struct ContactItems {
    project_id: Ulid,
    contacts: Vec<ContactItem>,
}

impl ContactItems {
    fn new(project_id: Ulid, contacts: Vec<Contact>) -> Self {
        let contact_items = contacts
            .into_iter()
            .map(|contact| ContactItem {
                contact,
                read_only: false, // Existing contacts are directly editable
            })
            .collect();

        Self {
            project_id,
            contacts: contact_items,
        }
    }
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/partials/contact_widget.stpl.html")]
struct ContactWidget {
    project_id: Ulid,
    contact: Option<Contact>,
    read_only: bool,
}

impl ContactWidget {
    fn new_readonly(project_id: Ulid, contact: Contact) -> Self {
        Self {
            project_id,
            contact: Some(contact),
            read_only: true,
        }
    }

    fn new_editable(project_id: Ulid, contact: Option<Contact>) -> Self {
        Self {
            project_id,
            contact,
            read_only: false,
        }
    }
}

// Contact form structures
#[derive(Debug, Deserialize)]
struct ContactForm {
    role: String,
    email: String,
    name: Option<String>,
    affiliation: Option<String>,
    phone: Option<String>,
    address: Option<String>,
}

impl ContactForm {
    fn to_contact(&self, contact_id: Ulid) -> Result<Contact, anyhow::Error> {
        use crate::contact::ContactRole;

        let role = match self.role.as_str() {
            "Manager" => ContactRole::Manager,
            "ContactPerson" => ContactRole::ContactPerson,
            "Technical" => ContactRole::Technical,
            "Notification" => ContactRole::Notification,
            _ => return Err(anyhow::anyhow!("Invalid contact role: {}", self.role)),
        };

        Ok(Contact {
            contact_id,
            role,
            name: self.name.clone().filter(|s| !s.is_empty()),
            affiliation: self.affiliation.clone().filter(|s| !s.is_empty()),
            email: self.email.clone(),
            phone: self.phone.clone().filter(|s| !s.is_empty()),
            address: self.address.clone().filter(|s| !s.is_empty()),
        })
    }
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/partials/success_message.stpl.html")]
struct SuccessMessage {
    message: String,
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/partials/error_message.stpl.html")]
struct ErrorMessage {
    message: String,
}

async fn projects_search_internal(
    query: ProjectSearchQuery,
) -> Result<ProjectsPagePrepared, anyhow::Error> {
    let page = query.page.unwrap_or(1);

    // Use the real database query function instead of mock data
    let filtered_projects = fetch_projects_page(
        query.search.as_deref(),
        query.status.as_deref(),
        query.publish.as_deref(),
        query.flagged.as_deref(),
        None, // spatial_overlap
        None, // spatial_point
        None, // proximity_sort
        Some(page),
        Some(20), // per_page
    )
    .await?;

    // Get total count for pagination (simplified for now)
    let total_count = count_projects(
        query.search.as_deref(),
        query.status.as_deref(),
        query.publish.as_deref(),
        query.flagged.as_deref(),
        None, // spatial_overlap
        None, // spatial_point
    )
    .await?;

    let per_page = 20;
    let total_pages = (total_count + per_page - 1) / per_page; // Round up
    let has_previous = page > 1;
    let has_next = (page as u64) < total_pages;
    let start_item = if total_count == 0 {
        0
    } else {
        (((page - 1) as u64) * per_page) + 1
    };
    let end_item = std::cmp::min((page as u64) * per_page, total_count) as u32;

    Ok(ProjectsPagePrepared {
        projects: filtered_projects,
        current_page: page,
        total_projects: total_count,
        has_previous,
        has_next,
        per_page: 25,
        start_item: start_item as u32,
        end_item,
        pagination_url: "/-admin/projects".to_string(),
        pagination_include: "[name='search'], [name='status'], [name='publish'], [name='flagged']"
            .to_string(),
        search_term: query.search.unwrap_or_default(),
        selected_status: query.status.unwrap_or_default(),
        selected_publish: query.publish.unwrap_or_default(),
        flagged: query.flagged.unwrap_or_default(),
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
        search_term: query.search.clone().unwrap_or_default(),
        selected_status: query.status.clone().unwrap_or_default(),
        selected_publish: query.publish.clone().unwrap_or_default(),
        flagged: query.flagged.clone().unwrap_or_default(),
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
    if let Some(flagged) = &query.flagged {
        if !flagged.trim().is_empty() {
            url_params.push(format!("flagged={}", urlencoding::encode(flagged)));
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

async fn events_search_internal(
    query: EventSearchQuery,
) -> Result<EventsPagePrepared, anyhow::Error> {
    let page = query.page.unwrap_or(1);
    let per_page = 25u32;

    let events_list = match fetch_events_page(
        query.search.as_deref(),
        query.status.as_deref(),
        query.publish.as_deref(),
        Some(page),
        Some(per_page),
    )
    .await
    {
        Ok(event_results) => event_results,
        Err(e) => {
            eprintln!("Error fetching events: {}", e);
            vec![]
        }
    };

    // Get accurate total count using the count_events function
    let total_events = count_events(
        query.search.as_deref(),
        query.status.as_deref(),
        query.publish.as_deref(),
    )
    .await
    .unwrap_or(0) as u32;

    let has_previous = page > 1;
    let has_next = (page * per_page) < total_events;

    let start_item = if total_events == 0 {
        0
    } else {
        ((page - 1) * per_page) + 1
    };
    let end_item = std::cmp::min(page * per_page, total_events as u32);

    Ok(EventsPagePrepared {
        events: events_list,
        current_page: page,
        total_events,
        has_previous,
        has_next,
        start_item,
        end_item,
        search_term: query.search.as_deref().unwrap_or("").to_string(),
        selected_status: query.status.as_deref().unwrap_or("").to_string(),
        selected_publish: query.publish.as_deref().unwrap_or("").to_string(),
    })
}

#[handler]
async fn events_search(
    Query(query): Query<EventSearchQuery>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let events_page = events_search_internal(query.clone()).await?;

    // For HTMX requests, render just the events content and set URL push header
    let events_content = EventsContent {
        events: events_page.events,
        current_page: events_page.current_page,
        total_events: events_page.total_events,
        has_previous: events_page.has_previous,
        has_next: events_page.has_next,
        per_page: 25,
        start_item: events_page.start_item,
        end_item: events_page.end_item,
        pagination_url: "/-admin/events/search".to_string(),
        pagination_include: "#event-search-form".to_string(),
        search_term: query.search.as_deref().unwrap_or("").to_string(),
        selected_status: query.status.as_deref().unwrap_or("").to_string(),
        selected_publish: query.publish.as_deref().unwrap_or("").to_string(),
    };

    let html = events_content.render_once()?;

    // Build URL parameters for HX-Push-Url
    let mut url_params = vec![];
    if let Some(search) = &query.search {
        if !search.trim().is_empty() {
            url_params.push(format!("search={}", search));
        }
    }
    if let Some(status) = &query.status {
        if !status.trim().is_empty() {
            url_params.push(format!("status={}", status));
        }
    }
    if let Some(publish) = &query.publish {
        if !publish.trim().is_empty() {
            url_params.push(format!("publish={}", publish));
        }
    }
    let page = query.page.unwrap_or(1);
    if page > 1 {
        url_params.push(format!("page={}", page));
    }

    let url = if url_params.is_empty() {
        "/-admin/events".to_string()
    } else {
        format!("/-admin/events?{}", url_params.join("&"))
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
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    // Parse project ID and load real project data
    let project_ulid = project_id
        .parse::<Ulid>()
        .map_err(|_| anyhow::anyhow!("Invalid project ID format"))?;

    // Load ProjectAll from database and convert to AdminProjectFull
    let project_all = ProjectAll::load(project_ulid).await?;
    let project = AdminProjectFull::from_project_all(project_all).await?;

    let edit_project_page = EditProjectPage { project };

    Ok(edit_project_page.into_response(user_info).await?)
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
                crate::project::field::ProjectFieldValue::PageEditorIds(ulids.into_boxed_slice())
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

            // Success - fetch updated project data directly from database
            let project_id_str = parsed_project_id.to_string();
            let updated_project = sqlx::query!(
                r#"SELECT 
                    project_id,
                    name,
                    status,
                    publish,
                    created,
                    page_creator_id,
                    description,
                    presenter,
                    origin,
                    flagged,
                    duplicate,
                    is_test,
                    goal,
                    task,
                    image,
                    url,
                    admin_notes,
                    draft
                FROM projects 
                WHERE project_id = ?"#,
                project_id_str
            )
            .fetch_one(db())
            .await
            .map_err(|e| anyhow::anyhow!("Failed to reload project after update: {}", e))?;

            // Resolve creator email
            let page_creator_email =
                if let Ok(creator_id) = updated_project.page_creator_id.parse::<Ulid>() {
                    let email = lookup_user_email(Some(creator_id)).await;
                    if email.is_empty() { None } else { Some(email) }
                } else {
                    None
                };

            let admin_project = AdminProject {
                id: updated_project.project_id,
                title: updated_project.name,
                status: updated_project.status,
                publish: updated_project.publish.unwrap_or(false),
                created_date: updated_project.created,
                page_creator_id: updated_project.page_creator_id,
                page_creator_email,
                description: updated_project.description,
                presenter: updated_project.presenter,
                origin: updated_project.origin,
                flagged: updated_project.flagged,
                duplicate: updated_project.duplicate,
                is_test: updated_project.is_test,
                goal: updated_project.goal,
                task: updated_project.task,
                image: updated_project.image,
                url: updated_project.url,
                admin_notes: updated_project.admin_notes,
                draft: updated_project.draft,
                tags: Vec::new(), // Tags not needed for OOB update
            };

            // Create the updated project row for out-of-band swap

            // For the new page-based edit form, send custom events instead of OOB updates
            let field_label = get_field_label(&field_type);
            let trigger_data = serde_json::json!({
                "field-updated": {
                    "field": field_type,
                    "label": field_label,
                    "success": true
                }
            });

            Ok(poem::Response::builder()
                .content_type("text/html")
                .header("HX-Trigger", trigger_data.to_string())
                .body(""))
        }
        Err(e) => {
            // Return error response with custom event
            let field_label = get_field_label(&field_type);
            let trigger_data = serde_json::json!({
                "field-updated": {
                    "field": field_type,
                    "label": field_label,
                    "success": false,
                    "error": e.to_string()
                }
            });

            Ok(poem::Response::builder()
                .status(poem::http::StatusCode::INTERNAL_SERVER_ERROR)
                .content_type("text/html")
                .header("HX-Trigger", trigger_data.to_string())
                .body(""))
        }
    }
}

// Helper function to get user-friendly field labels
fn get_field_label(field_type: &str) -> &str {
    match field_type {
        "name" => "Project Name",
        "description" => "Description",
        "goal" => "Scientific Goal",
        "task" => "Participant Task",
        "image" => "Image URL",
        "image_credit" => "Image Credit",
        "presenter" => "Presenter",
        "status" => "Status",
        "difficulty" => "Difficulty",
        "url" => "Project URL",
        "video_url" => "Video URL",
        "blog_url" => "Blog URL",
        "twitter_name" => "Twitter Handle",
        "entirely_online" => "Entirely Online",
        "in_person" => "In Person",
        "indoors" => "Indoors",
        "outdoors" => "Outdoors",
        // Multiselect fields
        "topics" => "Topics",
        "fields_of_science" => "Fields of Science",
        "practices" => "Practices",
        "audience" => "Audience",
        "sustainable_development_goals" => "Sustainable Development Goals",
        // Array fields
        "participant_files" => "Participant Files",
        "geographic_scope" => "Geographic Scope",
        "participation_tasks" => "Participation Tasks",
        "activities" => "Activities",
        "data_aggregation" => "Data Aggregation",
        "data_consent" => "Data Consent",
        "data_access" => "Data Access",
        "intended_outcomes" => "Intended Outcomes",
        "sponsors" => "Sponsors",
        "stakeholders" => "Stakeholders",
        "associated_projects" => "Associated Projects",
        "page_editor_ids" => "Page Editors",
        _ => field_type, // Fallback to field_type if no label found
    }
}

#[handler]
async fn projects_update_multiselect_field(
    Path((project_id, field_type)): Path<(String, String)>,
    body: poem::Body,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    use crate::project::field::{ProjectField, ProjectFieldType, ProjectFieldValue};

    // Read the form body and parse with serde_qs (with strict mode disabled)
    let body_str = body.into_string().await?;
    let config = serde_qs::Config::new(5, false); // max_depth=5, strict=false
    let form: UpdateProjectMultiselectForm = config.deserialize_str(&body_str)?;

    let project_ulid = project_id
        .parse::<Ulid>()
        .map_err(|_| anyhow::anyhow!("Invalid project ID format"))?;

    let field_type_enum = match field_type.as_str() {
        "topics" => ProjectFieldType::Topics,
        "fields_of_science" => ProjectFieldType::FieldsOfScience,
        "practices" => ProjectFieldType::Practices,
        "audience" => ProjectFieldType::Audience,
        "sustainable_development_goals" => ProjectFieldType::SustainableDevelopmentGoals,
        _ => {
            return Err(anyhow::anyhow!(
                "Unsupported multiselect field type: {}",
                field_type
            ));
        }
    };

    let field_value = match field_type_enum {
        ProjectFieldType::Topics => ProjectFieldValue::Topics(form.value.into_boxed_slice()),
        ProjectFieldType::FieldsOfScience => {
            ProjectFieldValue::FieldsOfScience(form.value.into_boxed_slice())
        }
        ProjectFieldType::Practices => ProjectFieldValue::Practices(form.value.into_boxed_slice()),
        ProjectFieldType::Audience => ProjectFieldValue::Audience(form.value.into_boxed_slice()),
        ProjectFieldType::SustainableDevelopmentGoals => {
            ProjectFieldValue::SustainableDevelopmentGoals(form.value.join(","))
        }
        _ => {
            return Err(anyhow::anyhow!(
                "Unsupported field type in multiselect handler"
            ));
        }
    };

    let project_field = ProjectField::new(project_ulid, field_value);

    let field_label = get_field_label(&field_type);

    match project_field.store().await {
        Ok(_) => {
            let trigger_data = serde_json::json!({
                "field-updated": {
                    "field": field_type,
                    "label": field_label,
                    "success": true
                }
            });

            Ok(poem::Response::builder()
                .content_type("text/html")
                .header("HX-Trigger", trigger_data.to_string())
                .body(""))
        }
        Err(e) => {
            let trigger_data = serde_json::json!({
                "field-updated": {
                    "field": field_type,
                    "label": field_label,
                    "success": false,
                    "error": e.to_string()
                }
            });

            Ok(poem::Response::builder()
                .content_type("text/html")
                .header("HX-Trigger", trigger_data.to_string())
                .body(""))
        }
    }
}

#[handler]
async fn projects_get_array_items(
    Path((project_id, field_type)): Path<(String, String)>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let project_ulid = project_id
        .parse::<Ulid>()
        .map_err(|_| anyhow::anyhow!("Invalid project ID format"))?;

    let field_enum = ProjectFieldType::try_from(field_type.as_str())?;
    let project_field = ProjectField::load((project_ulid, field_enum)).await?;
    let items: Vec<String> = (&project_field).try_into()?;

    let array_items = ArrayItems::new_with_resolved_names(project_ulid, field_enum, items).await?;
    let items_html = array_items.render_once()?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(items_html))
}

#[handler]
async fn projects_add_array_item(
    Path((project_id, field_type)): Path<(String, String)>,
    Form(form): Form<AddArrayItemForm>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let project_ulid = project_id
        .parse::<Ulid>()
        .map_err(|_| anyhow::anyhow!("Invalid project ID format"))?;

    let field_enum = ProjectFieldType::try_from(field_type.as_str())?;
    let project_field = ProjectField::load((project_ulid, field_enum)).await?;
    let mut items: Vec<String> = (&project_field).try_into()?;

    let new_item = form.value.trim().to_string();
    if !new_item.is_empty() && !items.contains(&new_item) {
        items.push(new_item);
    }

    let new_project_field = ProjectField::new_array(project_ulid, field_enum, items.clone())?;
    let field_label = get_field_label(&field_type);

    match new_project_field.store().await {
        Ok(_) => {
            let array_items =
                ArrayItems::new_with_resolved_names(project_ulid, field_enum, items).await?;
            let items_html = array_items.render_once()?;

            let trigger_data = serde_json::json!({
                "field-updated": {
                    "field": field_type,
                    "label": field_label,
                    "success": true
                }
            });

            Ok(poem::Response::builder()
                .content_type("text/html")
                .header("HX-Trigger", trigger_data.to_string())
                .body(items_html))
        }
        Err(e) => {
            let trigger_data = serde_json::json!({
                "field-updated": {
                    "field": field_type,
                    "label": field_label,
                    "success": false,
                    "error": e.to_string()
                }
            });

            Ok(poem::Response::builder()
                .content_type("text/html")
                .header("HX-Trigger", trigger_data.to_string())
                .body(""))
        }
    }
}

#[handler]
async fn projects_update_array_item(
    Path((project_id, field_type, index)): Path<(String, String, String)>,
    Form(form): Form<UpdateArrayItemForm>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let project_ulid = project_id
        .parse::<Ulid>()
        .map_err(|_| anyhow::anyhow!("Invalid project ID format"))?;
    let item_index = index
        .parse::<usize>()
        .map_err(|_| anyhow::anyhow!("Invalid index format"))?;

    let field_enum = ProjectFieldType::try_from(field_type.as_str())?;
    let project_field = ProjectField::load((project_ulid, field_enum)).await?;
    let mut items: Vec<String> = (&project_field).try_into()?;

    if item_index < items.len() {
        let new_value = form.value.trim().to_string();
        if !new_value.is_empty() {
            items[item_index] = new_value;
        }
    }

    let new_project_field = ProjectField::new_array(project_ulid, field_enum, items.clone())?;
    let field_label = get_field_label(&field_type);

    match new_project_field.store().await {
        Ok(_) => {
            let array_items =
                ArrayItems::new_with_resolved_names(project_ulid, field_enum, items).await?;
            let items_html = array_items.render_once()?;

            let trigger_data = serde_json::json!({
                "field-updated": {
                    "field": field_type,
                    "label": field_label,
                    "success": true
                }
            });

            Ok(poem::Response::builder()
                .content_type("text/html")
                .header("HX-Trigger", trigger_data.to_string())
                .body(items_html))
        }
        Err(e) => {
            let trigger_data = serde_json::json!({
                "field-updated": {
                    "field": field_type,
                    "label": field_label,
                    "success": false,
                    "error": e.to_string()
                }
            });

            Ok(poem::Response::builder()
                .content_type("text/html")
                .header("HX-Trigger", trigger_data.to_string())
                .body(""))
        }
    }
}

#[handler]
async fn projects_remove_array_item(
    Path((project_id, field_type, index)): Path<(String, String, String)>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let project_ulid = project_id
        .parse::<Ulid>()
        .map_err(|_| anyhow::anyhow!("Invalid project ID format"))?;
    let item_index = index
        .parse::<usize>()
        .map_err(|_| anyhow::anyhow!("Invalid index format"))?;

    let field_enum = ProjectFieldType::try_from(field_type.as_str())?;
    let project_field = ProjectField::load((project_ulid, field_enum)).await?;
    let mut items: Vec<String> = (&project_field).try_into()?;

    if item_index < items.len() {
        items.remove(item_index);
    }

    let new_project_field = ProjectField::new_array(project_ulid, field_enum, items.clone())?;
    let field_label = get_field_label(&field_type);

    match new_project_field.store().await {
        Ok(_) => {
            let array_items =
                ArrayItems::new_with_resolved_names(project_ulid, field_enum, items).await?;
            let items_html = array_items.render_once()?;

            let trigger_data = serde_json::json!({
                "field-updated": {
                    "field": field_type,
                    "label": field_label,
                    "success": true
                }
            });

            Ok(poem::Response::builder()
                .content_type("text/html")
                .header("HX-Trigger", trigger_data.to_string())
                .body(items_html))
        }
        Err(e) => {
            let trigger_data = serde_json::json!({
                "field-updated": {
                    "field": field_type,
                    "label": field_label,
                    "success": false,
                    "error": e.to_string()
                }
            });

            Ok(poem::Response::builder()
                .content_type("text/html")
                .header("HX-Trigger", trigger_data.to_string())
                .body(""))
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
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("User not authenticated"))?;

    // Generate a new ULID for the project
    let project_id = Ulid::new();
    let current_time = TimeString::now();

    // Create a new project with minimal required fields
    let mut new_project = crate::project::all::ProjectAll::default();
    new_project.project_id = project_id;
    new_project.name = form.name.clone();
    new_project.description = form.description;
    new_project.presenter = form.presenter;
    new_project.goal = form.goal;
    new_project.task = form.task;
    new_project.url = form.url;
    new_project.image = form.image;
    new_project.admin_notes = form.admin_notes;
    new_project.draft = form.draft;
    new_project.publish = form.publish;
    new_project.page_creator_id = user_info.user_id;
    new_project.created = current_time.clone();
    new_project.origin = "scistarter".to_string();
    new_project.authoritative = true;
    new_project.status = crate::project::ProjectStatus::Active;

    // Save the new project to the database using LoadStorePeered
    new_project.store().await?;

    // Resolve creator email for the current user
    let email = lookup_user_email(Some(user_info.user_id)).await;
    let page_creator_email = if email.is_empty() { None } else { Some(email) };

    // Create AdminProject for the UI
    let admin_project = AdminProject {
        id: project_id.to_string(),
        title: form.name.clone(),
        status: "active".to_string(),
        publish: form.publish,
        created_date: current_time.to_string(),
        page_creator_id: user_info.user_id.to_string(),
        page_creator_email,
        description: new_project.description.clone(),
        presenter: new_project.presenter.clone(),
        origin: new_project.origin.clone(),
        flagged: false,
        duplicate: false,
        is_test: false,
        goal: new_project.goal.clone(),
        task: new_project.task.clone(),
        image: new_project.image.clone(),
        url: new_project.url.clone(),
        admin_notes: new_project.admin_notes.clone(),
        draft: form.draft,
        tags: Vec::new(), // New projects start with no tags
    };

    // Create the new project row with OOB support to add to the top of the projects list
    use sailfish::TemplateOnce;
    let project_row = ProjectRow {
        project: admin_project,
        oob: true,
        oob_swap: "afterbegin".to_string(),
    };
    let project_row_html = project_row.render_once()?;

    // Create success message
    let success_html = format!(
        r#"<div class="p-3 text-sm text-green-700 bg-green-100 rounded-lg border border-green-200">
            <strong>Success:</strong> Project "{}" has been created successfully!
        </div>"#,
        form.name
    );

    // Return success message + the project row wrapped in template for OOB swap
    let combined_html = format!(
        r#"{}
        <div class="mt-3 text-sm text-blue-700">
            <button onclick="document.getElementById('modal-container').innerHTML = '';" 
                    class="text-indigo-600 hover:text-indigo-800 underline">
                Close
            </button>
        </div>
        {}"#,
        success_html, project_row_html
    );

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(combined_html))
}

#[handler]
async fn projects_add_tag(
    Path(project_id): Path<String>,
    Form(form): Form<AddTagForm>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let project_ulid = project_id
        .parse::<Ulid>()
        .map_err(|_| anyhow::anyhow!("Invalid project ID format"))?;

    let field_enum = ProjectFieldType::Tags;
    let project_field = ProjectField::load((project_ulid, field_enum)).await?;
    let mut tags: Vec<String> = (&project_field).try_into()?;

    let new_tag = form.tag.trim().to_string();
    if !new_tag.is_empty() && !tags.contains(&new_tag) {
        tags.push(new_tag);
    }

    let new_project_field = ProjectField::new_array(project_ulid, field_enum, tags.clone())?;
    new_project_field.store().await?;

    // Return the updated tags display
    use sailfish::TemplateOnce;
    let tags_display = TagsDisplay {
        project_id: project_id.clone(),
        tags,
    };

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(tags_display.render_once()?))
}

#[handler]
async fn projects_remove_tag(
    Path((project_id, tag)): Path<(String, String)>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let project_ulid = project_id
        .parse::<Ulid>()
        .map_err(|_| anyhow::anyhow!("Invalid project ID format"))?;

    let field_enum = ProjectFieldType::Tags;
    let project_field = ProjectField::load((project_ulid, field_enum)).await?;
    let mut tags: Vec<String> = (&project_field).try_into()?;

    // Remove the tag (Path extractor handles URL decoding automatically)
    tags.retain(|t| t != &tag);

    let new_project_field = ProjectField::new_array(project_ulid, field_enum, tags.clone())?;
    new_project_field.store().await?;

    // Return the updated tags display
    use sailfish::TemplateOnce;
    let tags_display = TagsDisplay {
        project_id: project_id.clone(),
        tags,
    };

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(tags_display.render_once()?))
}

// Contact handlers
#[handler]
async fn projects_get_contacts(
    Path(project_id): Path<String>,
) -> Result<poem::Response, poem::Error> {
    let project_ulid: Ulid = project_id
        .parse()
        .map_err(|_| poem::Error::from_status(poem::http::StatusCode::BAD_REQUEST))?;

    // Load the project field for contacts
    let project_field = ProjectField::load((project_ulid, ProjectFieldType::Contacts))
        .await
        .map_err(|e| {
            tracing::error!("Failed to load contacts field: {:?}", e);
            poem::Error::from_status(poem::http::StatusCode::INTERNAL_SERVER_ERROR)
        })?;

    let contacts = match project_field.field {
        ProjectFieldValue::Contacts(contacts) => contacts.into_vec(),
        _ => Vec::new(),
    };

    use sailfish::TemplateOnce;
    let contact_items = ContactItems::new(project_ulid, contacts);

    Ok(poem::Response::builder().content_type("text/html").body(
        contact_items
            .render_once()
            .map_err(|_| poem::Error::from_status(poem::http::StatusCode::INTERNAL_SERVER_ERROR))?,
    ))
}

#[handler]
async fn projects_add_contact(
    Path(project_id): Path<String>,
    poem::web::Form(form): poem::web::Form<ContactForm>,
) -> Result<poem::Response, poem::Error> {
    let project_ulid: Ulid = project_id
        .parse()
        .map_err(|_| poem::Error::from_status(poem::http::StatusCode::BAD_REQUEST))?;

    // Load existing contacts
    let project_field = ProjectField::load((project_ulid, ProjectFieldType::Contacts))
        .await
        .map_err(|_| poem::Error::from_status(poem::http::StatusCode::INTERNAL_SERVER_ERROR))?;

    let mut contacts = match project_field.field {
        ProjectFieldValue::Contacts(contacts) => contacts.into_vec(),
        _ => Vec::new(),
    };

    // Create new contact with generated ULID
    let new_contact = form
        .to_contact(Ulid::new())
        .map_err(|_| poem::Error::from_status(poem::http::StatusCode::BAD_REQUEST))?;

    contacts.push(new_contact);

    // Save updated contacts
    let new_project_field = ProjectField::new(
        project_ulid,
        ProjectFieldValue::Contacts(contacts.clone().into_boxed_slice()),
    );
    new_project_field
        .store()
        .await
        .map_err(|_| poem::Error::from_status(poem::http::StatusCode::INTERNAL_SERVER_ERROR))?;

    // Return updated contact list
    use sailfish::TemplateOnce;
    let contact_items = ContactItems::new(project_ulid, contacts);

    Ok(poem::Response::builder().content_type("text/html").body(
        contact_items
            .render_once()
            .map_err(|_| poem::Error::from_status(poem::http::StatusCode::INTERNAL_SERVER_ERROR))?,
    ))
}

#[handler]
async fn projects_update_contact(
    Path((project_id, contact_id)): Path<(String, String)>,
    poem::web::Form(form): poem::web::Form<ContactForm>,
) -> Result<poem::Response, poem::Error> {
    let project_ulid: Ulid = project_id
        .parse()
        .map_err(|_| poem::Error::from_status(poem::http::StatusCode::BAD_REQUEST))?;
    let contact_ulid: Ulid = contact_id
        .parse()
        .map_err(|_| poem::Error::from_status(poem::http::StatusCode::BAD_REQUEST))?;

    // Load existing contacts
    let project_field = ProjectField::load((project_ulid, ProjectFieldType::Contacts))
        .await
        .map_err(|_| poem::Error::from_status(poem::http::StatusCode::INTERNAL_SERVER_ERROR))?;

    let mut contacts = match project_field.field {
        ProjectFieldValue::Contacts(contacts) => contacts.into_vec(),
        _ => Vec::new(),
    };

    // Find and update the contact
    if let Some(contact) = contacts.iter_mut().find(|c| c.contact_id == contact_ulid) {
        *contact = form
            .to_contact(contact_ulid)
            .map_err(|_| poem::Error::from_status(poem::http::StatusCode::BAD_REQUEST))?;
    } else {
        return Err(poem::Error::from_status(poem::http::StatusCode::NOT_FOUND));
    }

    // Save updated contacts
    let new_project_field = ProjectField::new(
        project_ulid,
        ProjectFieldValue::Contacts(contacts.clone().into_boxed_slice()),
    );
    new_project_field
        .store()
        .await
        .map_err(|_| poem::Error::from_status(poem::http::StatusCode::INTERNAL_SERVER_ERROR))?;

    // Return updated contact list
    use sailfish::TemplateOnce;
    let contact_items = ContactItems::new(project_ulid, contacts);

    Ok(poem::Response::builder().content_type("text/html").body(
        contact_items
            .render_once()
            .map_err(|_| poem::Error::from_status(poem::http::StatusCode::INTERNAL_SERVER_ERROR))?,
    ))
}

#[handler]
async fn projects_remove_contact(
    Path((project_id, contact_id)): Path<(String, String)>,
) -> Result<poem::Response, poem::Error> {
    let project_ulid: Ulid = project_id
        .parse()
        .map_err(|_| poem::Error::from_status(poem::http::StatusCode::BAD_REQUEST))?;
    let contact_ulid: Ulid = contact_id
        .parse()
        .map_err(|_| poem::Error::from_status(poem::http::StatusCode::BAD_REQUEST))?;

    // Load existing contacts
    let project_field = ProjectField::load((project_ulid, ProjectFieldType::Contacts))
        .await
        .map_err(|_| poem::Error::from_status(poem::http::StatusCode::INTERNAL_SERVER_ERROR))?;

    let mut contacts = match project_field.field {
        ProjectFieldValue::Contacts(contacts) => contacts.into_vec(),
        _ => Vec::new(),
    };

    // Remove the contact
    contacts.retain(|c| c.contact_id != contact_ulid);

    // Save updated contacts
    let new_project_field = ProjectField::new(
        project_ulid,
        ProjectFieldValue::Contacts(contacts.clone().into_boxed_slice()),
    );
    new_project_field
        .store()
        .await
        .map_err(|_| poem::Error::from_status(poem::http::StatusCode::INTERNAL_SERVER_ERROR))?;

    // Return updated contact list
    use sailfish::TemplateOnce;
    let contact_items = ContactItems::new(project_ulid, contacts);

    Ok(poem::Response::builder().content_type("text/html").body(
        contact_items
            .render_once()
            .map_err(|_| poem::Error::from_status(poem::http::StatusCode::INTERNAL_SERVER_ERROR))?,
    ))
}

// Files & Links admin page structures
#[derive(Debug, Clone)]
pub struct GoUrlWithFileInfo {
    pub slug: String,
    pub url_type: GoUrlType,
    pub file_info: Option<FileMetadata>,
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/files_links.stpl.html")]
pub struct FilesLinksPage {
    pub go_urls_list: GoUrlsList,
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/go_urls_list.stpl.html")]
pub struct GoUrlsList {
    pub go_urls: Vec<GoUrlWithFileInfo>,
    pub pagination: PaginationInfo,
    pub search_query: Option<String>,
}

#[derive(Debug)]
pub struct PaginationInfo {
    pub current_page: u32,
    pub per_page: u32,
    pub total_count: u32,
    pub total_pages: u32,
    pub has_prev: bool,
    pub has_next: bool,
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/partials/go_url_row.stpl.html")]
pub struct GoUrlRow<'a> {
    pub go_url: &'a GoUrlWithFileInfo,
    pub oob: bool,
    pub oob_swap: String,
}

// Files & Links admin handlers
#[derive(Serialize, Deserialize)]
struct SearchQuery {
    page: Option<u32>,
    per_page: Option<u32>,
    search: Option<String>,
}

#[handler]
async fn files_links(
    Authenticated(user_info): Authenticated,
    Query(query): Query<SearchQuery>,
) -> Result<poem::Response, anyhow::Error> {
    let page = query.page.unwrap_or(1).max(1);
    let per_page = query.per_page.unwrap_or(20).clamp(1, 100); // Limit to reasonable range
    let search = query.search.filter(|s| !s.trim().is_empty());

    let go_urls_list = get_go_urls_with_file_info_searched(page, per_page, search.clone()).await?;
    let content = FilesLinksPage { go_urls_list }.render_once()?;

    AdminLayout::new("Files & Links")
        .with_breadcrumb("Files & Links", None)
        .with_sidebar_active("files-links")
        .with_content(content)
        .into_response(user_info)
        .await
}

#[handler]
async fn files_links_search(
    Authenticated(_user_info): Authenticated,
    Query(query): Query<SearchQuery>,
) -> Result<impl IntoResponse, poem::Error> {
    let page = query.page.unwrap_or(1).max(1);
    let per_page = query.per_page.unwrap_or(20).clamp(1, 100);
    let search = query.search.filter(|s| !s.trim().is_empty());

    match get_go_urls_with_file_info_searched(page, per_page, search.clone()).await {
        Ok(go_urls_list) => {
            use sailfish::TemplateOnce;
            match go_urls_list.render_once() {
                Ok(rendered) => {
                    // Build the URL for HX-Push-Url
                    let mut url = String::from("/-admin/files-links");
                    let mut params = Vec::new();

                    if page > 1 {
                        params.push(format!("page={}", page));
                    }
                    if per_page != 20 {
                        params.push(format!("per_page={}", per_page));
                    }
                    if let Some(ref search_term) = search {
                        params.push(format!("search={}", urlencoding::encode(search_term)));
                    }

                    if !params.is_empty() {
                        url.push('?');
                        url.push_str(&params.join("&"));
                    }

                    Ok(poem::Response::builder()
                        .status(poem::http::StatusCode::OK)
                        .content_type("text/html")
                        .header("HX-Push-Url", url)
                        .body(rendered))
                }
                Err(e) => {
                    tracing::error!("Failed to render go URLs list: {}", e);
                    Err(poem::Error::from_string(
                        "Failed to render search results",
                        poem::http::StatusCode::INTERNAL_SERVER_ERROR,
                    ))
                }
            }
        }
        Err(e) => {
            tracing::error!("Failed to search go URLs: {}", e);
            Err(poem::Error::from_string(
                "Search failed",
                poem::http::StatusCode::INTERNAL_SERVER_ERROR,
            ))
        }
    }
}

#[derive(Serialize, Deserialize)]
struct CreateRedirectRequest {
    slug: String,
    target_url: String,
}

#[handler]
async fn files_links_create_redirect(
    Form(request): Form<CreateRedirectRequest>,
    Authenticated(_user_info): Authenticated,
) -> Result<impl IntoResponse, poem::Error> {
    // Validate slug
    if !GoUrl::is_valid_slug(&request.slug) {
        return Ok(poem::Response::builder()
            .status(poem::http::StatusCode::BAD_REQUEST)
            .content_type("text/html")
            .body(r#"<div class="text-red-600">Invalid slug format</div>"#));
    }

    // Check if slug already exists (case-insensitive)
    match GoUrl::slug_exists(&request.slug).await {
        Ok(true) => {
            return Ok(poem::Response::builder()
                .status(poem::http::StatusCode::BAD_REQUEST)
                .content_type("text/html")
                .body(r#"<div class="text-red-600">Slug already exists</div>"#));
        }
        Err(e) => {
            tracing::error!("Failed to check slug existence: {}", e);
            return Ok(poem::Response::builder()
                .status(poem::http::StatusCode::INTERNAL_SERVER_ERROR)
                .content_type("text/html")
                .body(r#"<div class="text-red-600">Database error</div>"#));
        }
        Ok(false) => {
            // Slug doesn't exist, continue with creation
        }
    }

    // Save slug before it gets moved
    let slug = request.slug.clone();

    // Create and save redirect
    let go_url = GoUrl::new_redirect(request.slug, request.target_url);
    if let Err(e) = go_url.store().await {
        tracing::error!("Failed to create redirect: {}", e);
        return Ok(poem::Response::builder()
            .status(poem::http::StatusCode::INTERNAL_SERVER_ERROR)
            .content_type("text/html")
            .body(r#"<div class="text-red-600">Failed to create redirect</div>"#));
    }

    // Return success message with form clearing and go URLs list update
    let go_urls_list = get_go_urls_with_file_info().await.map_err(|e| {
        tracing::error!("Failed to get go URLs: {}", e);
        poem::Error::from_status(poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    use sailfish::TemplateOnce;
    let response = RedirectSuccessResponse {
        slug,
        go_urls_list,
    };

    let rendered = response.render_once().map_err(|e| {
        tracing::error!("Failed to render template: {}", e);
        poem::Error::from_status(poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    Ok(poem::Response::builder()
        .status(poem::http::StatusCode::OK)
        .content_type("text/html")
        .body(rendered))
}

#[handler]
async fn files_links_delete_go_url(
    Path(slug): Path<String>,
    Authenticated(_user_info): Authenticated,
) -> Result<impl IntoResponse, poem::Error> {
    // Check if the go URL exists (case-insensitive) before attempting deletion
    match GoUrl::slug_exists(&slug).await {
        Ok(false) => {
            return Ok(poem::Response::builder()
                .status(poem::http::StatusCode::NOT_FOUND)
                .content_type("text/html")
                .body(r#"<div class="text-red-600">Go URL not found</div>"#));
        }
        Err(e) => {
            tracing::error!("Failed to check slug existence: {}", e);
            return Ok(poem::Response::builder()
                .status(poem::http::StatusCode::INTERNAL_SERVER_ERROR)
                .content_type("text/html")
                .body(r#"<div class="text-red-600">Database error</div>"#));
        }
        Ok(true) => {
            // Go URL exists, continue with deletion
        }
    }

    // Delete the go URL
    if let Err(e) = GoUrl::delete(slug.clone()).await {
        tracing::error!("Failed to delete go URL {}: {}", slug, e);
        return Ok(poem::Response::builder()
            .status(poem::http::StatusCode::INTERNAL_SERVER_ERROR)
            .content_type("text/html")
            .body(r#"<div class="text-red-600">Failed to delete go URL</div>"#));
    }

    // Return empty response - HTMX will remove the targeted element
    Ok(poem::Response::builder()
        .status(poem::http::StatusCode::OK)
        .content_type("text/html")
        .body(""))
}

// Helper function to get paginated and searched go URLs with file information
pub async fn get_go_urls_with_file_info_searched(
    page: u32,
    per_page: u32,
    search: Option<String>,
) -> Result<GoUrlsList, anyhow::Error> {
    use common::shared::db;
    use sqlx::{QueryBuilder, Row};

    // Build count query
    let mut count_query: QueryBuilder<sqlx::Sqlite> =
        QueryBuilder::new("SELECT COUNT(*) FROM go_urls");
    if let Some(ref search_term) = search {
        count_query.push(" WHERE slug LIKE ");
        count_query.push_bind(format!("%{}%", search_term));
    }

    let total_count: i64 = count_query.build_query_scalar().fetch_one(db()).await?;
    let total_count = total_count as u32;
    let total_pages = (total_count + per_page - 1) / per_page; // Ceiling division
    let offset = (page - 1) * per_page;

    // Build main query
    let mut query: QueryBuilder<sqlx::Sqlite> =
        QueryBuilder::new("SELECT slug, url_type FROM go_urls");
    if let Some(ref search_term) = search {
        query.push(" WHERE slug LIKE ");
        query.push_bind(format!("%{}%", search_term));
    }
    query.push(" ORDER BY slug LIMIT ");
    query.push_bind(per_page);
    query.push(" OFFSET ");
    query.push_bind(offset);

    let rows = query.build().fetch_all(db()).await?;

    let mut go_urls = Vec::new();

    for row in rows {
        let slug: String = row.get("slug");
        let url_type_blob: Vec<u8> = row.get("url_type");
        let url_type: GoUrlType = flexbuffers::from_slice(&url_type_blob)?;

        let file_info = match &url_type {
            GoUrlType::TrackedFile { file_id } => {
                // Get file metadata if this is a tracked file
                use crate::upload::storage::FileMetadata;
                FileMetadata::load(*file_id).await.ok()
            }
            GoUrlType::Redirect { .. } => None,
        };

        go_urls.push(GoUrlWithFileInfo {
            slug,
            url_type,
            file_info,
        });
    }

    let pagination = PaginationInfo {
        current_page: page,
        per_page,
        total_count,
        total_pages,
        has_prev: page > 1,
        has_next: page < total_pages,
    };

    Ok(GoUrlsList {
        go_urls,
        pagination,
        search_query: search,
    })
}

// Helper function to get paginated go URLs with file information (for backward compatibility)
pub async fn get_go_urls_with_file_info_paginated(
    page: u32,
    per_page: u32,
) -> Result<GoUrlsList, anyhow::Error> {
    get_go_urls_with_file_info_searched(page, per_page, None).await
}

// Helper function to get all go URLs with file information (for backward compatibility)
pub async fn get_go_urls_with_file_info() -> Result<GoUrlsList, anyhow::Error> {
    get_go_urls_with_file_info_searched(1, 1000, None).await // Large page size for "all"
}

#[derive(sailfish::TemplateOnce)]
#[template(path = "admin/partials/go_url_stats_modal.stpl.html")]
pub struct GoUrlStatsModal {
    pub slug: String,
    pub total_views: i64,
    pub views_in_range: i64,
    pub chart_data: Vec<ChartPoint>,
    pub chart_max_views: i64,
    pub recent_views: Vec<RecentView>,
    pub date_range_start: String,
    pub date_range_end: String,
}

#[derive(Debug, Clone)]
pub struct ChartPoint {
    pub date: String,        // YYYY-MM-DD
    pub date_label: String,  // e.g., "Sep 23"
    pub views: i64,
    pub height_percent: f64, // 0-100 for chart display
}

#[derive(Debug, Clone)]
pub struct RecentView {
    pub timestamp: String,
    pub ip_address: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StatsDateRange {
    pub start_date: Option<String>,
    pub end_date: Option<String>,
}

#[derive(sailfish::TemplateOnce)]
#[template(path = "admin/partials/error_modal.stpl.html")]
pub struct ErrorModal {
    pub message: String,
}

async fn fetch_go_url_stats(
    slug: String,
    start_date: Option<String>,
    end_date: Option<String>,
) -> Result<GoUrlStatsModal, String> {
    use common::service::request;

    // Request stats from log service
    let stats_request = GoUrlStatsRequest {
        slug: slug.clone(),
        start_date,
        end_date,
    };

    match request::<GoUrlStatsResponse>("log.stats.go_url", stats_request).await {
        Ok(stats) => {
            let chart_data = stats.chart_data
                .into_iter()
                .map(|point| ChartPoint {
                    date: point.date,
                    date_label: point.date_label,
                    views: point.views,
                    height_percent: point.height_percent,
                })
                .collect();

            let recent_views = stats.recent_views
                .into_iter()
                .map(|view| RecentView {
                    timestamp: view.timestamp,
                    ip_address: view.ip_address,
                })
                .collect();

            Ok(GoUrlStatsModal {
                slug: stats.slug,
                total_views: stats.total_views,
                views_in_range: stats.views_in_range,
                chart_data,
                chart_max_views: stats.chart_max_views,
                recent_views,
                date_range_start: stats.date_range_start,
                date_range_end: stats.date_range_end,
            })
        }
        Err(e) => {
            tracing::error!("Failed to get go URL stats for {}: {}", slug, e);
            Err(format!("Failed to load stats for {}", slug))
        }
    }
}

#[handler]
async fn files_links_get_stats(
    Path(slug): Path<String>,
    Authenticated(_user_info): Authenticated,
) -> Result<impl IntoResponse, poem::Error> {
    match fetch_go_url_stats(slug, None, None).await {
        Ok(modal) => Ok(modal.render_once().unwrap()),
        Err(error_msg) => {
            let error_modal = ErrorModal { message: error_msg.clone() };
            match error_modal.render_once() {
                Ok(rendered) => {
                    tracing::info!("Rendered error modal: {}", rendered.len());
                    Ok(rendered)
                }
                Err(e) => {
                    tracing::error!("Failed to render error modal: {}", e);
                    Ok(format!(
                        r#"<div class="p-4 bg-red-100 border border-red-300 rounded">Error: {}</div>"#,
                        error_msg
                    ))
                }
            }
        }
    }
}

#[handler]
async fn files_links_post_stats(
    Path(slug): Path<String>,
    Form(date_range): Form<StatsDateRange>,
    Authenticated(_user_info): Authenticated,
) -> Result<impl IntoResponse, poem::Error> {
    match fetch_go_url_stats(slug, date_range.start_date, date_range.end_date).await {
        Ok(modal) => Ok(modal.render_once().unwrap()),
        Err(error_msg) => {
            let error_modal = ErrorModal { message: error_msg.clone() };
            match error_modal.render_once() {
                Ok(rendered) => {
                    tracing::info!("Rendered error modal: {}", rendered.len());
                    Ok(rendered)
                }
                Err(e) => {
                    tracing::error!("Failed to render error modal: {}", e);
                    Ok(format!(
                        r#"<div class="p-4 bg-red-100 border border-red-300 rounded">Error: {}</div>"#,
                        error_msg
                    ))
                }
            }
        }
    }
}

#[handler]
async fn clear_modal(
    Authenticated(_user_info): Authenticated,
) -> Result<impl IntoResponse, poem::Error> {
    // Return empty response to clear the modal container
    Ok("")
}
