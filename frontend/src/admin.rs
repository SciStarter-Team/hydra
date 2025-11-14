use crate::component::{AdminLayout, Component, admin_layout::AdminLayoutPrepared};
use crate::contact::Contact;
use crate::entity::{Entity, EntityKind, EntityTarget};
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
        EmailerServiceHealthResponse, ForumServiceHealthResponse, FrontendServiceHealthResponse,
        LocationServiceHealthResponse, LogServiceHealthResponse, UserServiceHealthResponse,
    },
    message::log::{
        GoUrlStatsRequest, GoUrlStatsResponse, RecentActivityRequest, RecentActivityResponse,
    },
    message::user::{
        AdminAnalyticsRequest, AdminAnalyticsResponse, AdminPasswordResetRequest,
        AdminPasswordResetResponse, AdminUser, AdminUserCreateRequest, AdminUserDeleteRequest,
        AdminUserDeleteResponse, AdminUserExportRequest, AdminUserExportResponse,
        AdminUserGetRequest, AdminUserListRequest, AdminUserListResponse, AdminUserResponse,
        AdminUserSearchRequest, AdminUserUpdateRequest, BasicInfo, UserEmail, UserEmailAddRequest,
        UserEmailDeleteRequest, UserEmailResponse, UserEmailToggleVerifiedRequest,
        UserEmailUpdateRequest,
    },
    service::{request, request_timeout, request_with_retry},
    shared::db,
    timestring::TimeString,
};
use jiff::Timestamp;
use poem::{
    Endpoint, EndpointExt, FromRequest, IntoResponse, Request, RequestBody, Result as PoemResult,
    Route, delete, get, handler, patch, post, put,
    web::{Form, Html, Path, Query},
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
            get(users_edit).post(users_update).delete(users_delete),
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
        .at("/events/:id", get(events_edit).delete(events_delete))
        .at("/events/:id/field/:field", put(events_update_field))
        .at("/analytics", get(analytics))
        .at(
            "/analytics/recent-activity",
            get(analytics_recent_activity_table),
        )
        .at("/analytics/top-performers", get(analytics_top_performers))
        .at("/analytics/export/users", get(analytics_export_users))
        .at("/analytics/export/projects", get(analytics_export_projects))
        .at("/analytics/export/events", get(analytics_export_events))
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
        .at(
            "/files-links/:slug/stats",
            get(files_links_get_stats).post(files_links_post_stats),
        )
        .at("/clear-modal", post(clear_modal))
        .at("/entities", get(entities))
        .at("/entities/search", get(entities_search))
        .at("/entities/new", get(entities_new).post(entities_create))
        .at("/entities/target-input", get(entities_target_input))
        .at(
            "/entities/edit/*slug",
            get(entities_edit)
                .post(entities_update)
                .delete(entities_delete),
        )
        .at("/entities/publish/*slug", post(entities_publish_toggle))
        .at("/wysiwyg", get(wysiwyg_list))
        .at("/wysiwyg/search", get(wysiwyg_search))
        .at("/wysiwyg/new", get(wysiwyg_new))
        .at("/pagecontent/DressedHtml", get(dressed_html_list))
        .at("/pagecontent/DressedHtml/search", get(dressed_html_search))
        .at("/pagecontent/DressedHtml/new", get(dressed_html_new))
        .at(
            "/pagecontent/DressedHtml/:id",
            get(dressed_html_edit)
                .post(dressed_html_update)
                .delete(dressed_html_delete),
        )
        .at("/pagecontent/RawHtml", get(raw_html_list))
        .at("/pagecontent/RawHtml/search", get(raw_html_search))
        .at("/pagecontent/RawHtml/new", get(raw_html_new))
        .at(
            "/pagecontent/RawHtml/:id",
            get(raw_html_edit)
                .post(raw_html_update)
                .delete(raw_html_delete),
        )
        .at("/pagecontent", post(pagecontent_create))
        .at(
            "/pagecontent/:kind/:id",
            get(pagecontent_edit)
                .post(pagecontent_update)
                .delete(pagecontent_delete),
        )
        .at("/schedules", get(schedules_list).post(schedules_create))
        .at("/schedules/search", get(schedules_search))
        .at("/schedules/new", get(schedules_new))
        .at(
            "/schedules/:id",
            get(schedules_edit)
                .post(schedules_update)
                .delete(schedules_delete),
        )
        .at("/email/oneoff", get(email_oneoff).post(email_oneoff_create))
        .at("/email/oneoff/new", get(email_oneoff_new))
        .at(
            "/email/oneoff/:id",
            get(email_oneoff_edit).post(email_oneoff_update),
        )
        .at("/email/oneoff/:id/send", post(email_oneoff_send))
        .at("/email/stored", get(email_stored).post(email_stored_create))
        .at("/email/stored/new", get(email_stored_new))
        .at(
            "/email/stored/:id",
            get(email_stored_edit).post(email_stored_update),
        )
        .at("/email/stored/:id/send", post(email_stored_send))
        .at("/email/stored/autocomplete", get(email_stored_autocomplete))
        .at(
            "/email/triggers",
            get(email_triggers).post(email_trigger_create),
        )
        .at("/email/triggers/:id/edit", get(email_trigger_edit))
        .at(
            "/email/triggers/:id",
            patch(email_trigger_update).delete(email_trigger_delete),
        )
        .at("/email/queue", get(email_queue))
        .at("/email/queue-content", get(email_queue_content))
        .at(
            "/email/dataset/:name/variables",
            get(email_dataset_variables),
        )
        .at("/email/merge-fields", get(email_merge_fields))
        .at("/email/address-sets", get(address_sets_list))
        .at(
            "/email/address-sets/new",
            get(address_sets_new).post(address_sets_create),
        )
        .at(
            "/email/address-sets/:slug",
            get(address_sets_edit)
                .post(address_sets_update)
                .delete(address_sets_delete),
        )
        .at(
            "/email/address-sets/:slug/members",
            post(address_sets_add_member),
        )
        .at(
            "/email/address-sets/:slug/members/:email",
            delete(address_sets_remove_member),
        )
        .at("/email/user-sets", get(user_sets_list))
        .at(
            "/email/user-sets/new",
            get(user_sets_new).post(user_sets_create),
        )
        .at(
            "/email/user-sets/criterion-fields",
            get(user_sets_criterion_fields),
        )
        .at(
            "/email/user-sets/explicit-list-widget",
            post(user_sets_explicit_list_add_user),
        )
        .at(
            "/email/user-sets/explicit-list-widget/:user_id",
            delete(user_sets_explicit_list_remove_user),
        )
        .at(
            "/email/user-sets/:slug",
            get(user_sets_edit)
                .post(user_sets_update)
                .delete(user_sets_delete),
        )
        .at(
            "/email/user-sets/:slug/criteria",
            post(user_sets_add_criterion),
        )
        .at(
            "/email/user-sets/:slug/criteria/:id",
            delete(user_sets_remove_criterion),
        )
        .at("/api/recipient-search", get(api_recipient_search))
        .at("/api/recipient-select", post(api_recipient_select))
        .at("/api/recipient-remove", delete(api_recipient_remove))
        .at("/exchanges", get(exchanges_list))
        .at("/exchanges/new", get(exchange_create_page).post(exchange_create))
        .at(
            "/exchanges/:id",
            get(exchange_edit_page).post(exchange_update).delete(exchange_delete),
        )
        .at("/badges", get(badges_list))
        .at("/badges/:badgeclass/holders", get(badges_holders))
        .at(
            "/badges/:badgeclass/holders-content",
            get(badges_holders_content),
        )
        .at("/badges/:badgeclass/grant", post(badges_grant))
        .at("/badges/:badgeclass/:user_id/revoke", post(badges_revoke))
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
    let emailer_health_future =
        request_timeout::<EmailerServiceHealthResponse>("emailer.health.check", (), timeout);

    // Wait for all health checks to complete
    let (user_result, location_result, log_result, frontend_result, forum_result, emailer_result) = tokio::join!(
        user_health_future,
        location_health_future,
        log_health_future,
        frontend_health_future,
        forum_health_future,
        emailer_health_future
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

    // Process emailer service health result
    match emailer_result {
        Ok(health) => {
            let status_color = match health.status.as_str() {
                "healthy" => "green",
                "degraded" => "yellow",
                _ => "red",
            };
            services.push(ServiceHealth {
                name: "Emailer Service".to_string(),
                status: health.status,
                status_color: status_color.to_string(),
                response_time: health.response_time_ms,
            });
        }
        Err(_) => {
            services.push(ServiceHealth {
                name: "Emailer Service".to_string(),
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

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/partials/recipient_picker.stpl.html")]
pub struct RecipientPickerPartial;

#[derive(Debug, Clone)]
pub struct AdminProject {
    pub id: String,
    pub title: String,
    pub status: String,
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

async fn fetch_projects_page(
    search: Option<&str>,
    status_filter: Option<&str>,
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
            "SELECT p.project_id, p.name, p.status, p.created, p.presenter,
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
            "SELECT p.project_id, p.name, p.status, p.created, p.presenter,
                    p.origin, p.flagged, p.duplicate, p.is_test, p.goal, p.task, p.image,
                    p.url, p.admin_notes, p.draft, p.description
             FROM projects p
             WHERE 1=1",
        );
    }

    if let Some(status) = status_filter.filter(|s| !s.is_empty()) {
        query.push(" AND p.status = ");
        query.push_bind(status);
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
        #[allow(dead_code)]
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
            "SELECT e.event_id, e.name, e.created, e.location_name, e.event_type, e.entirely_online,
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
            "SELECT e.event_id, e.name, e.created, e.location_name, e.event_type, e.entirely_online,
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

    query.push(
        " GROUP BY e.event_id, e.name, e.created, e.location_name, e.event_type, e.entirely_online",
    );
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

    let count: (i64,) = query.build_query_as::<(i64,)>().fetch_one(db()).await?;

    Ok(count.0 as u64)
}

#[derive(Debug)]
pub struct ProjectsPage {
    page: Option<u32>,
    search: Option<String>,
    status: Option<String>,
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
    flagged: String,
}

impl Component for ProjectsPage {
    type Param = Option<Arc<BasicInfo>>;
    type Prepared = AdminLayoutPrepared<ProjectsPagePrepared>;

    async fn prepare(self, param: Self::Param) -> Result<Self::Prepared, anyhow::Error> {
        let page = self.page.unwrap_or(1);
        let per_page = 25u32;

        let search_ref = self.search.as_deref();
        let status_ref = self.status.as_deref();
        let flagged_ref = self.flagged.as_deref();
        let projects_list = match fetch_projects_page(
            search_ref,
            status_ref,
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
            pagination_include: "[name='search'], [name='status'], [name='flagged']".to_string(),
            search_term: self.search.unwrap_or_default(),
            selected_status: self.status.unwrap_or_default(),
            flagged: self.flagged.unwrap_or_default(),
        };

        AdminLayout::new("Projects", projects_content)
            .with_breadcrumb("Projects", None)
            .with_sidebar_active("projects")
            .prepare(param)
            .await
    }
}

#[derive(Debug, Clone)]
pub struct AdminEvent {
    pub id: String,
    pub title: String,
    pub instances: u32, // Number of scheduled instances
    pub event_date: String,
    pub location: String,
    pub event_type: String,
}

#[derive(Debug, Clone)]
pub struct AdminEventFull {
    pub id: String,
    pub name: String,
    pub created_date: String,
    pub page_creator_id: String,
    pub page_creator_email: Option<String>,
    pub description: String,
    pub presenter: String,
    pub url: String,
    pub image: String,
    pub image_credit: String,
    pub how_to_join: String,
    pub admin_notes: String,
    pub flagged: bool,
    pub event_type: String,
    pub location_type: String,
    pub entirely_online: bool,
    pub location_name: String,
    pub location_description: String,
    pub address: String,
    pub city: String,
    pub state: String,
    pub zip: String,
    pub country: String,
    pub origin: String,
}

impl AdminEventFull {
    pub async fn from_event_all(event: crate::event::all::EventAll) -> Result<Self, anyhow::Error> {
        let email = lookup_user_email(Some(event.page_creator_id)).await;
        let page_creator_email = if email.is_empty() { None } else { Some(email) };

        Ok(AdminEventFull {
            id: event.event_id.to_string(),
            name: event.name,
            created_date: event.created.to_string(),
            page_creator_id: event.page_creator_id.to_string(),
            page_creator_email,
            description: event.description,
            presenter: event.presenter,
            url: event.url,
            image: event.image,
            image_credit: event.image_credit,
            how_to_join: event.how_to_join,
            admin_notes: event.admin_notes,
            flagged: event.flagged,
            event_type: event.event_type.to_string(),
            location_type: event.location_type,
            entirely_online: event.entirely_online,
            location_name: event.location_name,
            location_description: event.location_description,
            address: event.address,
            city: event.city,
            state: event.state,
            zip: event.zip,
            country: event.country,
            origin: event.origin,
        })
    }
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
}

impl Component for EventsPage {
    type Param = Option<Arc<BasicInfo>>;
    type Prepared = AdminLayoutPrepared<EventsPagePrepared>;

    async fn prepare(self, param: Self::Param) -> Result<Self::Prepared, anyhow::Error> {
        let page = 1u32; // For now, just show first page on dashboard
        let per_page = 10u32; // Show top 10 recent events

        let events_list = match fetch_events_page(None, None, Some(page), Some(per_page)).await {
            Ok(event_results) => event_results,
            Err(e) => {
                tracing::warn!("Failed to fetch events from database: {}", e);
                vec![]
            }
        };

        let total_events = count_events(None, None).await.unwrap_or(0);

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
        };

        AdminLayout::new("Events", events_content)
            .with_breadcrumb("Events", None)
            .with_sidebar_active("events")
            .prepare(param)
            .await
    }
}

#[derive(Debug)]
pub struct AnalyticsPage {
    start_date: String,
    end_date: String,
    selected_days: Option<u32>,
    activity_page: u32,
    top_performers_criterion: TopPerformersCriterion,
}

#[derive(Debug, Clone)]
pub struct GeographicStat {
    pub country: String,
    pub country_code: String,
    pub user_count: i64,
}

#[derive(Debug, Clone)]
pub struct ActivityStat {
    pub date: String,
    pub new_users: i64,
    pub total_users_to_date: i64,
    pub verified_users_to_date: i64,
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/analytics_recent_activity_table.stpl.html")]
pub struct AnalyticsRecentActivityTable {
    pub activity_stats: Vec<ActivityStat>,
    pub activity_page: u32,
    pub activity_total_pages: u32,
    pub activity_total_count: usize,
    pub selected_days: Option<u32>,
    pub start_date: String,
    pub end_date: String,
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/analytics.stpl.html")]
pub struct AnalyticsPagePrepared {
    total_users: u32,
    verified_users: u32,
    weekly_signups: u32,
    daily_signups: u32,
    top_performers: Vec<TopPerformerItem>,
    geographic_stats: Vec<GeographicStat>,
    recent_signups_chart: Vec<ActivityStat>, // Last 14 days for chart, not paginated
    activity_stats: Vec<ActivityStat>,
    activity_page: u32,
    activity_total_pages: u32,
    activity_total_count: usize,
    clickthrough_counts: Vec<WeeklyCount>,
    participation_counts: Vec<WeeklyCount>,
    selected_days: Option<u32>, // Time range filter (None if custom range)
    start_date: String,
    end_date: String,
    top_performers_criterion: TopPerformersCriterion,
}

#[derive(Debug, Clone)]
pub struct WeeklyCount {
    pub week_start: String,
    pub count: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TopPerformersCriterion {
    Views,
    Clickthroughs,
    Participation,
}

impl TopPerformersCriterion {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Views => "views",
            Self::Clickthroughs => "clickthroughs",
            Self::Participation => "participation",
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            Self::Views => "viewed pages",
            Self::Clickthroughs => "clicked projects",
            Self::Participation => "participated projects",
        }
    }

    pub fn is_views(&self) -> bool {
        matches!(self, Self::Views)
    }

    pub fn is_clickthroughs(&self) -> bool {
        matches!(self, Self::Clickthroughs)
    }

    pub fn is_participation(&self) -> bool {
        matches!(self, Self::Participation)
    }
}

impl Default for TopPerformersCriterion {
    fn default() -> Self {
        Self::Clickthroughs
    }
}

#[derive(Debug, Clone)]
pub struct TopPerformerItem {
    pub identifier: String,
    pub count: i64,
    pub display_name: Option<String>, // Project name if found, None for paths
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/partials/top_performers_box.stpl.html")]
pub struct TopPerformersBox {
    criterion: TopPerformersCriterion,
    items: Vec<TopPerformerItem>,
    days: Option<u32>,
    start_date: String,
    end_date: String,
}

impl Component for AnalyticsPage {
    type Param = Option<Arc<BasicInfo>>;
    type Prepared = AdminLayoutPrepared<AnalyticsPagePrepared>;

    async fn prepare(self, param: Self::Param) -> Result<Self::Prepared, anyhow::Error> {
        // Fetch database statistics for frontend data
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

        // Get top performers using the selected criterion
        let top_performers_response = request_timeout(
            "log.top_performers_analytics",
            common::message::log::TopPerformersQuery {
                start_date: self.start_date.clone(),
                end_date: self.end_date.clone(),
                criterion: self.top_performers_criterion.as_str().to_string(),
                limit: 5,
            },
            Duration::from_secs(5),
        )
        .await;

        let top_performers = match top_performers_response {
            Ok(common::message::log::AnalyticsResponse::TopPerformers(items)) => {
                let mut performers = Vec::new();

                for item in items {
                    // For clickthroughs and participation, try to look up project name
                    let display_name = if self.top_performers_criterion
                        == TopPerformersCriterion::Clickthroughs
                        || self.top_performers_criterion == TopPerformersCriterion::Participation
                    {
                        // Try to parse as ULID and look up project
                        if let Ok(_project_id) = item.identifier.parse::<Ulid>() {
                            sqlx::query!(
                                r#"SELECT name FROM projects WHERE project_id = ?"#,
                                item.identifier
                            )
                            .fetch_optional(db())
                            .await
                            .ok()
                            .flatten()
                            .map(|row| row.name)
                        } else {
                            None
                        }
                    } else {
                        // For views, just use the path as-is
                        None
                    };

                    performers.push(TopPerformerItem {
                        identifier: item.identifier,
                        count: item.count,
                        display_name,
                    });
                }

                performers
            }
            Err(e) => {
                eprintln!("Top performers analytics error: {:?}", e);
                Vec::new()
            }
            _ => {
                eprintln!("Top performers analytics returned unexpected variant");
                Vec::new()
            }
        };

        // Fetch geographic analytics using date range
        let geographic_response = request_timeout(
            "user.stats.geographic",
            common::message::user::GeographicAnalyticsRequest {
                start_date: self.start_date.clone(),
                end_date: self.end_date.clone(),
            },
            Duration::from_secs(5),
        )
        .await
        .unwrap_or(common::message::user::GeographicAnalyticsResponse { stats: vec![] });

        let geographic_stats: Vec<GeographicStat> = geographic_response
            .stats
            .into_iter()
            .map(|s| GeographicStat {
                country: s.country,
                country_code: s.country_code,
                user_count: s.user_count,
            })
            .collect();

        // Fetch last 14 days for the chart (not paginated)
        let chart_result: Result<common::message::user::ActivityAnalyticsResponse, _> =
            request_timeout(
                "user.stats.activity",
                common::message::user::ActivityAnalyticsRequest {
                    start_date: self.start_date.clone(),
                    end_date: self.end_date.clone(),
                    page: 1,
                    per_page: 14,
                },
                Duration::from_secs(5),
            )
            .await;

        let recent_signups_chart: Vec<ActivityStat> = match chart_result {
            Ok(response) => response
                .stats
                .into_iter()
                .map(|s| ActivityStat {
                    date: s.date,
                    new_users: s.new_users,
                    total_users_to_date: s.total_users_to_date,
                    verified_users_to_date: s.verified_users_to_date,
                })
                .collect(),
            Err(e) => {
                eprintln!("Chart data error: {:?}", e);
                Vec::new()
            }
        };

        // Fetch activity analytics using date range with pagination
        let per_page = 30;
        let activity_result: Result<common::message::user::ActivityAnalyticsResponse, _> =
            request_timeout(
                "user.stats.activity",
                common::message::user::ActivityAnalyticsRequest {
                    start_date: self.start_date.clone(),
                    end_date: self.end_date.clone(),
                    page: self.activity_page,
                    per_page,
                },
                Duration::from_secs(5),
            )
            .await;

        let activity_response = match activity_result {
            Ok(response) => response,
            Err(e) => {
                eprintln!("Activity analytics error: {:?}", e);
                common::message::user::ActivityAnalyticsResponse {
                    stats: vec![],
                    total_count: 0,
                }
            }
        };

        let activity_stats: Vec<ActivityStat> = activity_response
            .stats
            .into_iter()
            .map(|s| ActivityStat {
                date: s.date,
                new_users: s.new_users,
                total_users_to_date: s.total_users_to_date,
                verified_users_to_date: s.verified_users_to_date,
            })
            .collect();

        let activity_total_count = activity_response.total_count;
        let activity_total_pages =
            ((activity_total_count + per_page as usize - 1) / per_page as usize) as u32;
        let activity_page = self.activity_page.min(activity_total_pages.max(1));

        // Fetch clickthrough analytics from log service
        let clickthrough_analytics = request_timeout(
            "log.clickthrough_analytics",
            common::message::log::AnalyticsQuery {
                start_date: self.start_date.clone(),
                end_date: self.end_date.clone(),
            },
            Duration::from_secs(5),
        )
        .await;

        let clickthrough_counts = match clickthrough_analytics {
            Ok(common::message::log::AnalyticsResponse::WeeklyCounts(counts)) => counts
                .into_iter()
                .map(|c| WeeklyCount {
                    week_start: c.week_start,
                    count: c.count,
                })
                .collect(),
            Err(e) => {
                eprintln!("Clickthrough analytics error: {:?}", e);
                Vec::new()
            }
            _ => {
                eprintln!("Clickthrough analytics returned unexpected variant");
                Vec::new()
            }
        };

        // Fetch participation analytics from log service
        let participation_analytics = request_timeout(
            "log.participation_analytics",
            common::message::log::AnalyticsQuery {
                start_date: self.start_date.clone(),
                end_date: self.end_date.clone(),
            },
            Duration::from_secs(5),
        )
        .await;

        let participation_counts = match participation_analytics {
            Ok(common::message::log::AnalyticsResponse::WeeklyCounts(counts)) => counts
                .into_iter()
                .map(|c| WeeklyCount {
                    week_start: c.week_start,
                    count: c.count,
                })
                .collect(),
            Err(e) => {
                eprintln!("Participation analytics error: {:?}", e);
                Vec::new()
            }
            _ => {
                eprintln!("Participation analytics returned unexpected variant");
                Vec::new()
            }
        };

        let analytics_content = AnalyticsPagePrepared {
            total_users: user_analytics.total_users,
            verified_users: user_analytics.verified_users,
            weekly_signups: user_analytics.weekly_signups,
            daily_signups: user_analytics.daily_signups,
            top_performers,
            geographic_stats,
            recent_signups_chart,
            activity_stats,
            activity_page,
            activity_total_pages,
            activity_total_count,
            clickthrough_counts,
            participation_counts,
            selected_days: self.selected_days,
            start_date: self.start_date.clone(),
            end_date: self.end_date.clone(),
            top_performers_criterion: self.top_performers_criterion.clone(),
        };

        AdminLayout::new("Analytics", analytics_content)
            .with_breadcrumb("Analytics", None)
            .with_sidebar_active("analytics")
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
    type Prepared = AdminLayoutPrepared<UsersPagePrepared>;

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

        AdminLayout::new("Users", users_page)
            .with_breadcrumb("Users", None)
            .with_sidebar_active("users")
            .prepare(param)
            .await
    }
}

#[derive(Debug)]
pub struct DashboardPage;

impl Component for DashboardPage {
    type Param = Option<Arc<BasicInfo>>;
    type Prepared = AdminLayoutPrepared<DashboardContent>;

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

        let project_count = sqlx::query!(r#"SELECT COUNT(*) as count FROM projects p LEFT JOIN entities ent ON ent.target = p.project_id AND ent.kind = 'Project' WHERE ent.publish = 1 AND p.flagged = false AND p.duplicate = false AND p.is_test = false"#)
            .fetch_one(common::shared::db())
            .await?
            .count;

        let upcoming_events_count = sqlx::query!(r#"SELECT COUNT(DISTINCT e.event_id) as count FROM events e JOIN event_instances ei ON e.event_id = ei.event_id LEFT JOIN entities ent ON ent.target = e.event_id AND ent.kind = 'Event' WHERE ent.publish = 1 AND e.flagged = false AND ei.begin > datetime('now')"#)
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

        AdminLayout::new("Dashboard", dashboard_content)
            .with_sidebar_active("dashboard")
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

        return AdminLayout::new("Users", users_page)
            .with_breadcrumb("Users", None)
            .with_sidebar_active("users")
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
    if query.search.is_some() || query.status.is_some() || query.flagged.is_some() {
        let search_query = ProjectSearchQuery {
            search: query.search,
            status: query.status,
            flagged: query.flagged,
            page: query.page,
        };
        let projects_page = projects_search_internal(search_query).await?;

        return AdminLayout::new("Projects", projects_page)
            .with_breadcrumb("Projects", None)
            .with_sidebar_active("projects")
            .into_response(user_info)
            .await;
    }

    ProjectsPage {
        page: query.page,
        search: query.search,
        status: query.status,
        flagged: query.flagged,
    }
    .into_response(user_info)
    .await
}

#[handler]
async fn events(Authenticated(user_info): Authenticated) -> Result<poem::Response, anyhow::Error> {
    EventsPage.into_response(user_info).await
}

/// Helper function to determine date range from query parameters
/// Returns (start_date, end_date, selected_days)
fn determine_date_range(
    days: Option<u32>,
    start_date: Option<String>,
    end_date: Option<String>,
) -> (String, String, Option<u32>) {
    use jiff::Timestamp;
    use jiff::ToSpan;

    if let (Some(start), Some(end)) = (start_date, end_date) {
        // Use custom date range
        (start, end, None)
    } else {
        // Calculate date range from days parameter
        // "last N days" means N days including today, so we subtract (N-1) to get the start date
        let days_count = days.unwrap_or(30);
        let end = Timestamp::now().to_zoned(jiff::tz::TimeZone::UTC).date();
        let start = end
            .checked_sub(((days_count - 1) as i64).days())
            .unwrap_or(end);
        (start.to_string(), end.to_string(), Some(days_count))
    }
}

#[derive(Debug, serde::Deserialize)]
struct AnalyticsPageQuery {
    days: Option<u32>,
    start_date: Option<String>,
    end_date: Option<String>,
    activity_page: Option<u32>,
    top_performers_criterion: Option<TopPerformersCriterion>,
}

#[handler]
async fn analytics(
    Query(query): Query<AnalyticsPageQuery>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let (start_date, end_date, selected_days) =
        determine_date_range(query.days, query.start_date, query.end_date);

    AnalyticsPage {
        start_date,
        end_date,
        selected_days,
        activity_page: query.activity_page.unwrap_or(1),
        top_performers_criterion: query.top_performers_criterion.unwrap_or_default(),
    }
    .into_response(user_info)
    .await
}

#[derive(Debug, serde::Deserialize)]
struct RecentActivityTableQuery {
    days: Option<u32>,
    start_date: Option<String>,
    end_date: Option<String>,
    activity_page: Option<u32>,
}

#[handler]
async fn analytics_recent_activity_table(
    Query(query): Query<RecentActivityTableQuery>,
    Authenticated(_user_info): Authenticated,
) -> Result<impl IntoResponse, anyhow::Error> {
    let (start_date, end_date, selected_days) =
        determine_date_range(query.days, query.start_date, query.end_date);

    let activity_page = query.activity_page.unwrap_or(1);

    // Fetch paginated activity data
    let activity_result: Result<common::message::user::ActivityAnalyticsResponse, _> =
        request_timeout(
            "user.stats.activity",
            common::message::user::ActivityAnalyticsRequest {
                start_date: start_date.clone(),
                end_date: end_date.clone(),
                page: activity_page,
                per_page: 30,
            },
            Duration::from_secs(5),
        )
        .await;

    let (activity_stats, activity_total_count): (Vec<ActivityStat>, usize) = match activity_result {
        Ok(response) => {
            let stats = response
                .stats
                .into_iter()
                .map(|s| ActivityStat {
                    date: s.date,
                    new_users: s.new_users,
                    total_users_to_date: s.total_users_to_date,
                    verified_users_to_date: s.verified_users_to_date,
                })
                .collect();
            (stats, response.total_count)
        }
        Err(e) => {
            eprintln!("Activity stats error: {:?}", e);
            (Vec::new(), 0)
        }
    };

    let activity_total_pages = (activity_total_count as f64 / 30.0).ceil() as u32;

    Ok(Html(
        AnalyticsRecentActivityTable {
            activity_stats,
            activity_page,
            activity_total_pages,
            activity_total_count,
            selected_days,
            start_date,
            end_date,
        }
        .render_once()?,
    ))
}

#[derive(Debug, serde::Deserialize)]
struct TopPerformersQuery {
    days: Option<u32>,
    start_date: Option<String>,
    end_date: Option<String>,
    criterion: Option<TopPerformersCriterion>,
}

#[handler]
async fn analytics_top_performers(
    Query(query): Query<TopPerformersQuery>,
    Authenticated(_user_info): Authenticated,
) -> Result<impl IntoResponse, anyhow::Error> {
    let (start_date, end_date, days) =
        determine_date_range(query.days, query.start_date, query.end_date);

    let criterion = query.criterion.unwrap_or_default();

    // Fetch top performers
    let top_performers_response = request_timeout(
        "log.top_performers_analytics",
        common::message::log::TopPerformersQuery {
            start_date: start_date.clone(),
            end_date: end_date.clone(),
            criterion: criterion.as_str().to_string(),
            limit: 5,
        },
        Duration::from_secs(5),
    )
    .await;

    let items = match top_performers_response {
        Ok(common::message::log::AnalyticsResponse::TopPerformers(items)) => {
            let mut performers = Vec::new();

            for item in items {
                // For clickthroughs and participation, try to look up project name
                let display_name = if criterion == TopPerformersCriterion::Clickthroughs
                    || criterion == TopPerformersCriterion::Participation
                {
                    // Try to parse as ULID and look up project
                    if let Ok(_project_id) = item.identifier.parse::<Ulid>() {
                        sqlx::query!(
                            r#"SELECT name FROM projects WHERE project_id = ?"#,
                            item.identifier
                        )
                        .fetch_optional(db())
                        .await
                        .ok()
                        .flatten()
                        .map(|row| row.name)
                    } else {
                        None
                    }
                } else {
                    // For views, just use the path as-is
                    None
                };

                performers.push(TopPerformerItem {
                    identifier: item.identifier,
                    count: item.count,
                    display_name,
                });
            }

            performers
        }
        Err(e) => {
            eprintln!("Top performers analytics error: {:?}", e);
            Vec::new()
        }
        _ => {
            eprintln!("Top performers analytics returned unexpected variant");
            Vec::new()
        }
    };

    Ok(Html(
        TopPerformersBox {
            criterion,
            items,
            days,
            start_date,
            end_date,
        }
        .render_once()?,
    ))
}

/// Helper function to escape CSV field values
fn escape_csv_field(value: &str) -> String {
    if value.contains(',') || value.contains('"') || value.contains('\n') || value.contains('\r') {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

#[derive(Debug, serde::Deserialize)]
struct ExportQuery {
    days: Option<u32>,
    start_date: Option<String>,
    end_date: Option<String>,
}

#[handler]
async fn analytics_export_users(
    Query(query): Query<ExportQuery>,
    Authenticated(_user_info): Authenticated,
) -> Result<impl IntoResponse, anyhow::Error> {
    use jiff::ToSpan;

    // Parse date range
    let (start_date, end_date) =
        if let (Some(start), Some(end)) = (query.start_date, query.end_date) {
            (start, end)
        } else {
            let days = query.days.unwrap_or(30);
            let end = Timestamp::now().to_zoned(jiff::tz::TimeZone::UTC).date();
            let start = end.checked_sub(((days - 1) as i64).days()).unwrap_or(end);
            (start.to_string(), end.to_string())
        };

    // Build CSV incrementally, fetching users in pages to avoid NATS payload size limit
    let mut csv = String::new();
    csv.push_str("User ID,Handle,First Name,Last Name,Primary Email,Active,Staff,Superuser,Page Manager,Verified,Date Joined\n");

    let page_size = 500; // Fetch 500 users at a time
    let mut page = 1;

    loop {
        let export_response: AdminUserExportResponse = request_timeout(
            "user.admin.user_export",
            AdminUserExportRequest {
                start_date: start_date.clone(),
                end_date: end_date.clone(),
                page: Some(page),
                limit: Some(page_size),
            },
            Duration::from_secs(60),
        )
        .await?;

        let count = export_response.users.len();

        // Append this page's users to CSV immediately
        for user in export_response.users {
            csv.push_str(&format!(
                "{},{},{},{},{},{},{},{},{},{},{}\n",
                escape_csv_field(&user.user_id.to_string()),
                escape_csv_field(&user.handle),
                escape_csv_field(&user.first_name),
                escape_csv_field(&user.last_name),
                escape_csv_field(&user.primary_email),
                user.is_active,
                user.is_staff,
                user.is_superuser,
                user.is_page_manager,
                user.verified,
                escape_csv_field(&user.date_joined),
            ));
        }

        // If we got fewer than page_size results, we're done
        if count < page_size as usize {
            break;
        }

        page += 1;
    }

    Ok(poem::Response::builder()
        .content_type("text/csv")
        .header("Content-Disposition", "attachment; filename=\"users.csv\"")
        .body(csv))
}

#[handler]
async fn analytics_export_projects(
    Query(query): Query<ExportQuery>,
    Authenticated(_user_info): Authenticated,
) -> Result<impl IntoResponse, anyhow::Error> {
    use jiff::ToSpan;

    // Parse date range
    let (start_date, end_date) =
        if let (Some(start), Some(end)) = (query.start_date, query.end_date) {
            (start, end)
        } else {
            let days = query.days.unwrap_or(30);
            let end = Timestamp::now().to_zoned(jiff::tz::TimeZone::UTC).date();
            let start = end.checked_sub(((days - 1) as i64).days()).unwrap_or(end);
            (start.to_string(), end.to_string())
        };

    // Query projects from database
    struct ProjectRow {
        project_id: String,
        name: String,
        status: String,
        created: String,
        origin: String,
        presenter: String,
        url: String,
        publish: Option<bool>,
    }

    let project_rows = sqlx::query_as!(
        ProjectRow,
        r#"
        SELECT
            p.project_id,
            p.name,
            p.status,
            p.created,
            p.origin,
            p.presenter,
            p.url,
            e.publish
        FROM projects p
        LEFT JOIN entities e ON e.target = p.project_id AND e.kind = 'Project'
        WHERE p.created >= ? AND p.created <= ?
        ORDER BY p.created DESC
        "#,
        start_date,
        end_date
    )
    .fetch_all(db())
    .await?;

    // Build CSV
    let mut csv = String::new();
    csv.push_str("Project ID,Name,Status,Created,Origin,Presenter,URL,Published\n");

    for project in project_rows {
        let published = project.publish.unwrap_or(false);
        csv.push_str(&format!(
            "{},{},{},{},{},{},{},{}\n",
            escape_csv_field(&project.project_id),
            escape_csv_field(&project.name),
            escape_csv_field(&project.status),
            escape_csv_field(&project.created),
            escape_csv_field(&project.origin),
            escape_csv_field(&project.presenter),
            escape_csv_field(&project.url),
            published,
        ));
    }

    Ok(poem::Response::builder()
        .content_type("text/csv")
        .header(
            "Content-Disposition",
            "attachment; filename=\"projects.csv\"",
        )
        .body(csv))
}

#[handler]
async fn analytics_export_events(
    Query(query): Query<ExportQuery>,
    Authenticated(_user_info): Authenticated,
) -> Result<impl IntoResponse, anyhow::Error> {
    use jiff::ToSpan;

    // Parse date range
    let (start_date, end_date) =
        if let (Some(start), Some(end)) = (query.start_date, query.end_date) {
            (start, end)
        } else {
            let days = query.days.unwrap_or(30);
            let end = Timestamp::now().to_zoned(jiff::tz::TimeZone::UTC).date();
            let start = end.checked_sub(((days - 1) as i64).days()).unwrap_or(end);
            (start.to_string(), end.to_string())
        };

    // Query events from database
    struct EventRow {
        event_id: String,
        name: String,
        created: String,
        origin: String,
        presenter: String,
        url: String,
        event_type: String,
        location_type: String,
        publish: Option<bool>,
    }

    let event_rows = sqlx::query_as!(
        EventRow,
        r#"
        SELECT
            ev.event_id,
            ev.name,
            ev.created,
            ev.origin,
            ev.presenter,
            ev.url,
            ev.event_type,
            ev.location_type,
            e.publish
        FROM events ev
        LEFT JOIN entities e ON e.target = ev.event_id AND e.kind = 'Event'
        WHERE ev.created >= ? AND ev.created <= ?
        ORDER BY ev.created DESC
        "#,
        start_date,
        end_date
    )
    .fetch_all(db())
    .await?;

    // Build CSV
    let mut csv = String::new();
    csv.push_str("Event ID,Name,Created,Origin,Presenter,URL,Event Type,Location Type,Published\n");

    for event in event_rows {
        let published = event.publish.unwrap_or(false);
        csv.push_str(&format!(
            "{},{},{},{},{},{},{},{},{}\n",
            escape_csv_field(&event.event_id),
            escape_csv_field(&event.name),
            escape_csv_field(&event.created),
            escape_csv_field(&event.origin),
            escape_csv_field(&event.presenter),
            escape_csv_field(&event.url),
            escape_csv_field(&event.event_type),
            escape_csv_field(&event.location_type),
            published,
        ));
    }

    Ok(poem::Response::builder()
        .content_type("text/csv")
        .header("Content-Disposition", "attachment; filename=\"events.csv\"")
        .body(csv))
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/modals/new_user.stpl.html")]
struct NewUserForm {
    title: String,
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/pages/edit_user.stpl.html")]
struct EditUserPage {
    user_id: String,
    first_name: String,
    last_name: String,
    email: String,
    handle: String,
    role: String,
    active: bool,
    emails: Vec<UserEmail>,
    error_message: Option<String>,
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

    let content = match request::<AdminUserResponse>("user.admin.user_get", get_request).await {
        Ok(response) => {
            let admin_user = response.user;
            EditUserPage {
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
                error_message: None,
            }
        }
        Err(_e) => {
            // If user not found, return empty form with error state
            EditUserPage {
                user_id,
                first_name: "".to_string(),
                last_name: "".to_string(),
                email: "".to_string(),
                handle: "".to_string(),
                role: "user".to_string(),
                active: true,
                emails: vec![],
                error_message: Some("User not found".to_string()),
            }
        }
    };

    let prepared = crate::component::AdminLayout::new("Edit User", content)
        .with_sidebar_active("users")
        .with_breadcrumb("Users", Some("/-admin/users"))
        .with_breadcrumb("Edit", None)
        .prepare(user_info)
        .await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(prepared.render_once()?))
}

#[handler]
async fn users_delete(
    Path(user_id): Path<String>,
    Authenticated(user_info): Authenticated,
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
                // Log the activity
                if let Some(user_info) = &user_info {
                    common::service::log_activity(
                        None,
                        "user-delete",
                        user_info.user_id,
                        parsed_user_id,
                        None,
                    )
                    .await?;
                }

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
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    // Map role string to boolean flags
    let (_is_staff, _is_page_manager) = match form.role.as_str() {
        "staff" => (true, false),
        "page_manager" => (false, true),
        "user" | _ => (false, false),
    };

    let create_request = AdminUserCreateRequest {
        handle: form.handle.clone(),
        first_name: form.first_name,
        last_name: form.last_name,
        primary_email: form.email,
        password: form.password,
        role: form.role.clone(),
        verified: form.verified,
    };

    match request_with_retry::<AdminUserResponse>("user.admin.user_create", create_request).await {
        Ok(response) => {
            // Log the activity
            if let Some(user_info) = &user_info {
                common::service::log_activity(
                    None,
                    "user-create",
                    user_info.user_id,
                    response.user.user_id,
                    format!("handle={} role={}", form.handle, form.role),
                )
                .await?;
            }
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
                        <div hx-swap-oob="delete:dialog"></div>
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
                        .body(r#"<div hx-swap-oob="delete:dialog"></div>"#))
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
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    // Parse user ID
    let parsed_user_id = user_id
        .parse::<ulid::Ulid>()
        .map_err(|_| anyhow::anyhow!("Invalid user ID format"))?;

    // Validate
    if form.email.trim().is_empty() {
        let content = EditUserPage {
            user_id: user_id.clone(),
            first_name: form.first_name,
            last_name: form.last_name,
            email: form.email,
            handle: form.handle,
            role: form.role,
            active: form.active,
            emails: vec![],
            error_message: Some("Email is required".to_string()),
        };

        let prepared = crate::component::AdminLayout::new("Edit User", content)
            .with_sidebar_active("users")
            .with_breadcrumb("Users", Some("/-admin/users"))
            .with_breadcrumb("Edit", None)
            .prepare(user_info)
            .await?;

        return Ok(poem::Response::builder()
            .content_type("text/html")
            .body(prepared.render_once()?));
    }

    // Update
    let update_request = AdminUserUpdateRequest {
        user_id: parsed_user_id,
        handle: form.handle.clone(),
        first_name: form.first_name.clone(),
        last_name: form.last_name.clone(),
        primary_email: form.email.clone(),
        new_password: form.new_password.clone().filter(|p| !p.trim().is_empty()),
        role: form.role.clone(),
        verified: false, // Will be auto-calculated by backend based on email verification
        is_active: form.active,
    };

    match request_with_retry::<AdminUserResponse>("user.admin.user_update", update_request).await {
        Ok(_response) => {
            // Log the activity
            if let Some(user_info) = &user_info {
                common::service::log_activity(
                    None,
                    "user-update",
                    user_info.user_id,
                    parsed_user_id,
                    format!("handle={} role={}", form.handle, form.role),
                )
                .await?;
            }

            // Redirect back to list page
            Ok(poem::Response::builder()
                .status(poem::http::StatusCode::SEE_OTHER)
                .header("Location", "/-admin/users")
                .finish())
        }
        Err(e) => {
            // Show error on page
            let content = EditUserPage {
                user_id: user_id.clone(),
                first_name: form.first_name,
                last_name: form.last_name,
                email: form.email,
                handle: form.handle,
                role: form.role,
                active: form.active,
                emails: vec![],
                error_message: Some(format!("Failed to update user: {}", e)),
            };

            let prepared = crate::component::AdminLayout::new("Edit User", content)
                .with_sidebar_active("users")
                .with_breadcrumb("Users", Some("/-admin/users"))
                .with_breadcrumb("Edit", None)
                .prepare(user_info)
                .await?;

            Ok(poem::Response::builder()
                .content_type("text/html")
                .body(prepared.render_once()?))
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
pub struct EngineRoomPrepared {
    operations: Vec<EngineOperation>,
}

#[derive(Debug, Clone)]
pub struct EngineRoomPage;

impl Component for EngineRoomPage {
    type Param = Option<Arc<BasicInfo>>;
    type Prepared = AdminLayoutPrepared<EngineRoomPrepared>;

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

        AdminLayout::new("Engine Room", engine_room_content)
            .with_breadcrumb("Engine Room", None)
            .with_sidebar_active("engine-room")
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
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let parsed_user_id = user_id
        .parse::<ulid::Ulid>()
        .map_err(|_| anyhow::anyhow!("Invalid user ID format"))?;

    let email_request = UserEmailAddRequest {
        user_id: parsed_user_id,
        email: form.new_email.clone(),
    };

    match request_with_retry::<UserEmailResponse>("user.admin.email_add", email_request).await {
        Ok(response) => {
            if response.success {
                // Log the activity
                if let Some(user_info) = &user_info {
                    common::service::log_activity(
                        None,
                        "user-email-add",
                        user_info.user_id,
                        parsed_user_id,
                        format!("email={}", form.new_email),
                    )
                    .await?;
                }

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
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let parsed_user_id = user_id
        .parse::<ulid::Ulid>()
        .map_err(|_| anyhow::anyhow!("Invalid user ID format"))?;

    let new_email_clone = form.new_email.clone();
    let old_email_clone = form.old_email.clone();
    let email_request = UserEmailUpdateRequest {
        user_id: parsed_user_id,
        old_email: form.old_email,
        new_email: form.new_email,
    };

    match request_with_retry::<UserEmailResponse>("user.admin.email_update", email_request).await {
        Ok(response) => {
            if response.success {
                // Log the activity
                if let Some(user_info) = &user_info {
                    common::service::log_activity(
                        None,
                        "user-email-update",
                        user_info.user_id,
                        parsed_user_id,
                        format!("old={} new={}", old_email_clone, new_email_clone),
                    )
                    .await?;
                }
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
    Authenticated(user_info): Authenticated,
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
                // Log the activity
                if let Some(user_info) = &user_info {
                    common::service::log_activity(
                        None,
                        "user-email-delete",
                        user_info.user_id,
                        parsed_user_id,
                        format!("email={}", email),
                    )
                    .await?;
                }
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
    flagged: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
struct ProjectSearchQuery {
    search: Option<String>,
    status: Option<String>,
    flagged: Option<String>,
    page: Option<u32>,
}

#[derive(Debug, Deserialize, Clone)]
struct EventSearchQuery {
    search: Option<String>,
    status: Option<String>,
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
    type Prepared = AdminLayoutPrepared<EditProjectContent>;

    async fn prepare(self, user_info: Self::Param) -> anyhow::Result<Self::Prepared> {
        let title = self.project.title.clone();
        let content = EditProjectContent {
            project: self.project,
        };

        AdminLayout::new(&format!("Edit Project: {}", title), content)
            .with_breadcrumb("Projects", Some("/-admin/projects"))
            .with_breadcrumb(&title, None)
            .with_sidebar_active("projects")
            .prepare(user_info)
            .await
    }
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/edit_project_content.stpl.html")]
struct EditProjectContent {
    project: AdminProjectFull,
}

#[derive(Debug)]
struct EditEventPage {
    event: AdminEventFull,
}

impl Component for EditEventPage {
    type Param = Option<Arc<BasicInfo>>;
    type Prepared = AdminLayoutPrepared<EditEventContent>;

    async fn prepare(self, user_info: Self::Param) -> anyhow::Result<Self::Prepared> {
        let title = self.event.name.clone();
        let content = EditEventContent { event: self.event };

        AdminLayout::new(&format!("Edit Event: {}", title), content)
            .with_breadcrumb("Events", Some("/-admin/events"))
            .with_breadcrumb(&title, None)
            .with_sidebar_active("events")
            .prepare(user_info)
            .await
    }
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/edit_event_content.stpl.html")]
struct EditEventContent {
    event: AdminEventFull,
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/pages/add_project.stpl.html")]
struct AddProjectPage {
    error_message: Option<String>,
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
    #[allow(dead_code)]
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
}

impl ContactWidget {
    fn new_editable(project_id: Ulid, contact: Option<Contact>) -> Self {
        Self {
            project_id,
            contact,
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

async fn projects_search_internal(
    query: ProjectSearchQuery,
) -> Result<ProjectsPagePrepared, anyhow::Error> {
    let page = query.page.unwrap_or(1);

    // Use the real database query function instead of mock data
    let filtered_projects = fetch_projects_page(
        query.search.as_deref(),
        query.status.as_deref(),
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
        pagination_include: "[name='search'], [name='status'], [name='flagged']".to_string(),
        search_term: query.search.unwrap_or_default(),
        selected_status: query.status.unwrap_or_default(),
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
    let total_events = count_events(query.search.as_deref(), query.status.as_deref())
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

#[handler]
async fn events_edit(
    Path(event_id): Path<String>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    // Parse event ID and load real event data
    let event_ulid = event_id
        .parse::<Ulid>()
        .map_err(|_| anyhow::anyhow!("Invalid event ID format"))?;

    // Load EventAll from database and convert to AdminEventFull
    let event_all = crate::event::all::EventAll::load(event_ulid).await?;
    let event = AdminEventFull::from_event_all(event_all).await?;

    let edit_event_page = EditEventPage { event };

    Ok(edit_event_page.into_response(user_info).await?)
}

#[handler]
async fn events_delete(
    Path(event_id): Path<String>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let event_ulid = event_id
        .parse::<Ulid>()
        .map_err(|_| anyhow::anyhow!("Invalid event ID format"))?;
    let event_id_str = event_ulid.to_string();

    sqlx::query!(r#"DELETE FROM events WHERE event_id = ?"#, event_id_str)
        .execute(db())
        .await?;

    // Log the activity
    if let Some(user_info) = &user_info {
        common::service::log_activity(None, "event-delete", user_info.user_id, event_ulid, None)
            .await?;
    }

    // Check if there are any events left
    let exists: bool =
        sqlx::query_scalar!(r#"SELECT EXISTS(SELECT 1 FROM events) as "exists: bool""#)
            .fetch_one(db())
            .await?;

    let html = if !exists {
        // If no items left, return OOB swap with empty state
        let content = events_search_internal(EventSearchQuery {
            search: None,
            status: None,
            page: Some(1),
        })
        .await?;
        format!(
            r#"<div id="events-content" hx-swap-oob="outerHTML">{}</div>"#,
            content.render_once()?
        )
    } else {
        // Otherwise just return empty to remove the row
        String::new()
    };

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(html))
}

#[derive(Debug, Deserialize)]
struct UpdateEventFieldForm {
    value: String,
}

#[handler]
async fn events_update_field(
    Path((event_id, field_type)): Path<(String, String)>,
    Form(form): Form<UpdateEventFieldForm>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    use crate::event::field::{EventField, EventFieldValue};

    let event_ulid = event_id
        .parse::<Ulid>()
        .map_err(|_| anyhow::anyhow!("Invalid event ID format"))?;

    // Parse the field type and create the appropriate EventField
    let field_value = match field_type.as_str() {
        "name" => EventFieldValue::Name(form.value),
        "description" => EventFieldValue::Description(form.value),
        "presenter" => EventFieldValue::Presenter(form.value),
        "url" => EventFieldValue::Url(form.value),
        "image" => EventFieldValue::Image(form.value),
        "image_credit" => EventFieldValue::ImageCredit(form.value),
        "how_to_join" => EventFieldValue::HowToJoin(form.value),
        "location_type" => EventFieldValue::LocationType(form.value),
        "location_name" => EventFieldValue::LocationName(form.value),
        "location_description" => EventFieldValue::LocationDescription(form.value),
        "address" => EventFieldValue::Address(form.value),
        "city" => EventFieldValue::City(form.value),
        "state" => EventFieldValue::State(form.value),
        "zip" => EventFieldValue::Zip(form.value),
        "country" => EventFieldValue::Country(form.value),
        "admin_notes" => EventFieldValue::AdminNotes(form.value),
        "flagged" => EventFieldValue::Flagged(form.value == "true"),
        "entirely_online" => EventFieldValue::EntirelyOnline(form.value == "true"),
        _ => return Err(anyhow::anyhow!("Unknown field type: {}", field_type)),
    };

    let event_field = EventField::new(event_ulid, field_value);
    event_field.store().await?;

    // Log the activity
    if let Some(user_info) = &user_info {
        common::service::log_activity(
            None,
            "event-edit",
            user_info.user_id,
            event_ulid,
            field_type,
        )
        .await?;
    }

    Ok(poem::Response::builder().content_type("text/html").body(""))
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
            crate::project::field::ProjectFieldType::LegacyId => {
                let option_value = if form.value.is_empty() {
                    None
                } else {
                    Some(
                        form.value
                            .parse::<i64>()
                            .map_err(|e| anyhow::anyhow!("Invalid integer format: {}", e))?,
                    )
                };
                crate::project::field::ProjectFieldValue::LegacyId(option_value)
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
                    "project-edit",
                    user_info.user_id,
                    parsed_project_id,
                    format!("{:?}", field_type),
                )
                .await?;
            }

            // For the new page-based edit form, send custom events
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
    Authenticated(user_info): Authenticated,
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
            // Log the activity
            if let Some(user_info) = &user_info {
                common::service::log_activity(
                    None,
                    "project-edit",
                    user_info.user_id,
                    project_ulid,
                    field_type.clone(),
                )
                .await?;
            }

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
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let project_ulid = project_id
        .parse::<Ulid>()
        .map_err(|_| anyhow::anyhow!("Invalid project ID format"))?;

    let field_enum = ProjectFieldType::try_from(field_type.as_str())?;
    let project_field = ProjectField::load((project_ulid, field_enum)).await?;
    let mut items: Vec<String> = (&project_field).try_into()?;

    let new_item = form.value.trim().to_string();
    if !new_item.is_empty() && !items.contains(&new_item) {
        items.push(new_item.clone());
    }

    let new_project_field = ProjectField::new_array(project_ulid, field_enum, items.clone())?;
    let field_label = get_field_label(&field_type);

    match new_project_field.store().await {
        Ok(_) => {
            // Log the activity
            if let Some(user_info) = &user_info {
                common::service::log_activity(
                    None,
                    "project-edit",
                    user_info.user_id,
                    project_ulid,
                    format!("add={}[{}]", field_type, new_item),
                )
                .await?;
            }
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
    Authenticated(user_info): Authenticated,
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
            // Log the activity
            if let Some(user_info) = &user_info {
                common::service::log_activity(
                    None,
                    "project-edit",
                    user_info.user_id,
                    project_ulid,
                    format!("{}[{}]", field_type, item_index),
                )
                .await?;
            }
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
    Authenticated(user_info): Authenticated,
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
            // Log the activity
            if let Some(user_info) = &user_info {
                common::service::log_activity(
                    None,
                    "project-edit",
                    user_info.user_id,
                    project_ulid,
                    format!("remove={}[{}]", field_type, item_index),
                )
                .await?;
            }
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
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let content = AddProjectPage {
        error_message: None,
    };

    let prepared = crate::component::AdminLayout::new("Add New Project", content)
        .with_sidebar_active("projects")
        .with_breadcrumb("Projects", Some("/-admin/projects"))
        .with_breadcrumb("Add Project", None)
        .prepare(user_info)
        .await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(prepared.render_once()?))
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

    // Validate
    if form.name.trim().is_empty() {
        let content = AddProjectPage {
            error_message: Some("Project name is required".to_string()),
        };

        let prepared = crate::component::AdminLayout::new("Add New Project", content)
            .with_sidebar_active("projects")
            .with_breadcrumb("Projects", Some("/-admin/projects"))
            .with_breadcrumb("Add Project", None)
            .prepare(Some(user_info.clone().into()))
            .await?;

        return Ok(poem::Response::builder()
            .content_type("text/html")
            .body(prepared.render_once()?));
    }

    if form.description.trim().is_empty() {
        let content = AddProjectPage {
            error_message: Some("Description is required".to_string()),
        };

        let prepared = crate::component::AdminLayout::new("Add New Project", content)
            .with_sidebar_active("projects")
            .with_breadcrumb("Projects", Some("/-admin/projects"))
            .with_breadcrumb("Add Project", None)
            .prepare(Some(user_info.clone().into()))
            .await?;

        return Ok(poem::Response::builder()
            .content_type("text/html")
            .body(prepared.render_once()?));
    }

    if form.presenter.trim().is_empty() {
        let content = AddProjectPage {
            error_message: Some("Presenter/Organization is required".to_string()),
        };

        let prepared = crate::component::AdminLayout::new("Add New Project", content)
            .with_sidebar_active("projects")
            .with_breadcrumb("Projects", Some("/-admin/projects"))
            .with_breadcrumb("Add Project", None)
            .prepare(Some(user_info.clone().into()))
            .await?;

        return Ok(poem::Response::builder()
            .content_type("text/html")
            .body(prepared.render_once()?));
    }

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
    new_project.page_creator_id = user_info.user_id;
    new_project.created = current_time.clone();
    new_project.origin = "scistarter".to_string();
    new_project.authoritative = true;
    new_project.status = crate::project::ProjectStatus::Active;

    // Save the new project to the database using LoadStorePeered
    match new_project.store().await {
        Ok(_) => {
            // Create entity for the project
            let entity = crate::entity::Entity::new_by_name(
                &form.name,
                crate::entity::EntityKind::Project,
                crate::entity::EntityTarget::Ulid(project_id),
            )
            .await?;

            // Set publish status on entity
            let entity_with_publish = crate::entity::Entity::with_publish(
                entity.slug(),
                *entity.kind(),
                entity.target().clone(),
                form.publish,
            );
            entity_with_publish.store().await?;

            // Log the activity
            common::service::log_activity(
                None,
                "project-create",
                user_info.user_id,
                project_id,
                format!("name={}", form.name),
            )
            .await?;

            // Redirect to the project edit page
            Ok(poem::Response::builder()
                .status(poem::http::StatusCode::SEE_OTHER)
                .header("Location", format!("/-admin/projects/{}", project_id))
                .finish())
        }
        Err(e) => {
            let content = AddProjectPage {
                error_message: Some(format!("Failed to create project: {}", e)),
            };

            let prepared = crate::component::AdminLayout::new("Add New Project", content)
                .with_sidebar_active("projects")
                .with_breadcrumb("Projects", Some("/-admin/projects"))
                .with_breadcrumb("Add Project", None)
                .prepare(Some(user_info.into()))
                .await?;

            Ok(poem::Response::builder()
                .content_type("text/html")
                .body(prepared.render_once()?))
        }
    }
}

#[handler]
async fn projects_add_tag(
    Path(project_id): Path<String>,
    Form(form): Form<AddTagForm>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let project_ulid = project_id
        .parse::<Ulid>()
        .map_err(|_| anyhow::anyhow!("Invalid project ID format"))?;

    let field_enum = ProjectFieldType::Tags;
    let project_field = ProjectField::load((project_ulid, field_enum)).await?;
    let mut tags: Vec<String> = (&project_field).try_into()?;

    let new_tag = form.tag.trim().to_string();
    if !new_tag.is_empty() && !tags.contains(&new_tag) {
        tags.push(new_tag.clone());
    }

    let new_project_field = ProjectField::new_array(project_ulid, field_enum, tags.clone())?;
    new_project_field.store().await?;

    // Log the activity
    if let Some(user_info) = &user_info {
        common::service::log_activity(
            None,
            "project-edit",
            user_info.user_id,
            project_ulid,
            format!("add=tags[{}]", new_tag),
        )
        .await?;
    }

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
    Authenticated(user_info): Authenticated,
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

    // Log the activity
    if let Some(user_info) = &user_info {
        common::service::log_activity(
            None,
            "project-edit",
            user_info.user_id,
            project_ulid,
            format!("remove=tags[{}]", tag),
        )
        .await?;
    }

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
    Authenticated(user_info): Authenticated,
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
    let new_contact_id = new_contact.contact_id;

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

    // Log the activity
    if let Some(user_info) = &user_info {
        let _ = common::service::log_activity(
            None,
            "project-edit",
            user_info.user_id,
            project_ulid,
            format!("add=contact[{}]", new_contact_id),
        )
        .await;
    }

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
    Authenticated(user_info): Authenticated,
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

    // Log the activity
    if let Some(user_info) = &user_info {
        let _ = common::service::log_activity(
            None,
            "project-edit",
            user_info.user_id,
            project_ulid,
            format!("contact[{}]", contact_ulid),
        )
        .await;
    }

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
    Authenticated(user_info): Authenticated,
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

    // Log the activity
    if let Some(user_info) = &user_info {
        let _ = common::service::log_activity(
            None,
            "project-edit",
            user_info.user_id,
            project_ulid,
            format!("remove=contact[{}]", contact_ulid),
        )
        .await;
    }

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

    AdminLayout::new("Files & Links", FilesLinksPage { go_urls_list })
        .with_breadcrumb("Files & Links", None)
        .with_sidebar_active("files-links")
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
    Authenticated(user_info): Authenticated,
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

    // Log the activity
    if let Some(user_info) = &user_info {
        let _ = common::service::log_activity(
            None,
            "gourl-create",
            user_info.user_id,
            None,
            Some(slug.clone()),
        )
        .await;
    }

    // Return success message with form clearing and go URLs list update
    let go_urls_list = get_go_urls_with_file_info().await.map_err(|e| {
        tracing::error!("Failed to get go URLs: {}", e);
        poem::Error::from_status(poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    use sailfish::TemplateOnce;
    let response = RedirectSuccessResponse { slug, go_urls_list };

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
    Authenticated(user_info): Authenticated,
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

    // Log the activity
    if let Some(user_info) = &user_info {
        let _ = common::service::log_activity(
            None,
            "gourl-delete",
            user_info.user_id,
            None,
            Some(slug.clone()),
        )
        .await;
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
    pub date: String,       // YYYY-MM-DD
    pub date_label: String, // e.g., "Sep 23"
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
            let chart_data = stats
                .chart_data
                .into_iter()
                .map(|point| ChartPoint {
                    date: point.date,
                    date_label: point.date_label,
                    views: point.views,
                    height_percent: point.height_percent,
                })
                .collect();

            let recent_views = stats
                .recent_views
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
            let error_modal = ErrorModal {
                message: error_msg.clone(),
            };
            match error_modal.render_once() {
                Ok(rendered) => Ok(rendered),
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
            let error_modal = ErrorModal {
                message: error_msg.clone(),
            };
            match error_modal.render_once() {
                Ok(rendered) => Ok(rendered),
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

// Entity Management Data Structures

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/entities.stpl.html")]
pub struct EntitiesPage {
    pub entities_content: EntitiesContent,
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/entities_content.stpl.html")]
pub struct EntitiesContent {
    pub entities: Vec<EntityWithInfo>,
    pub current_page: i64,
    pub total_entities: i64,
    pub has_previous: bool,
    pub has_next: bool,
    pub per_page: i64,
    pub start_item: i64,
    pub end_item: i64,
    pub pagination_url: String,
    pub pagination_include: String,
    pub search_term: String,
    pub selected_kind: String,
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/partials/entity_row.stpl.html")]
pub struct EntityRow {
    pub entity: EntityWithInfo,
    pub oob: bool,
    pub oob_swap: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateEntityForm {
    pub slug: String,
    pub kind: String,
    pub target: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateEntityForm {
    pub slug: String,
    pub kind: String,
    pub target: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct EntitySearchQuery {
    pub search: Option<String>,
    pub kind: Option<String>,
    pub page: Option<i32>,
    pub per_page: Option<i32>,
}

#[derive(Debug, Deserialize)]
pub struct EntityTargetInputQuery {
    pub kind: String,
    pub current_value: Option<String>,
    pub mode: Option<String>, // "new" or "edit"
}

#[derive(Debug, sailfish::TemplateOnce)]
#[template(path = "admin/partials/entity_target_input.stpl.html")]
pub struct EntityTargetInputTemplate {
    pub kind: EntityKind,
    pub current_value: String,
    pub project_widget: Option<crate::admin::autocomplete::EntityAutocomplete>,
    pub event_widget: Option<crate::admin::autocomplete::EntityAutocomplete>,
    pub pagecontent_widget: Option<crate::admin::autocomplete::EntityAutocomplete>,
}

#[derive(Debug, Clone)]
pub struct EntityWithInfo {
    pub slug: String,
    pub kind: EntityKind,
    pub target: String,
    pub target_info: Option<String>, // Display name of the target (project name, event name, etc.)
    pub published: bool,             // Published status from project/event or true for other types
}

#[derive(TemplateOnce)]
#[template(path = "admin/modals/new_entity.stpl.html")]
pub struct NewEntityForm {
    pub title: String,
    pub error_message: Option<String>,
}

#[derive(TemplateOnce)]
#[template(path = "admin/pages/edit_entity.stpl.html")]
pub struct EditEntityPage {
    pub slug: String,
    pub kind: String,
    pub target: String,
    pub target_ulid: Option<Ulid>,
    pub target_input_template: EntityTargetInputTemplate,
    pub error_message: Option<String>,
}

// Entity Kind Helper Functions
impl EntityKind {
    pub fn display_name(&self) -> &'static str {
        match self {
            EntityKind::WordPress => "WordPress Page",
            EntityKind::Redirect => "Redirect",
            EntityKind::RawHtml => "Raw HTML",
            EntityKind::DressedHtml => "Dressed HTML",
            EntityKind::Wysiwyg => "Simple Editor",
            EntityKind::PageBuilder => "Page Builder",
            EntityKind::Form => "Form",
            EntityKind::Project => "Project",
            EntityKind::Event => "Event",
            EntityKind::Placeholder => "Placeholder",
        }
    }

    pub fn all_kinds() -> Vec<EntityKind> {
        vec![
            EntityKind::Project,
            EntityKind::Event,
            EntityKind::WordPress,
            EntityKind::Redirect,
            EntityKind::RawHtml,
            EntityKind::Wysiwyg,
            EntityKind::PageBuilder,
            EntityKind::Form,
            EntityKind::Placeholder,
        ]
    }
}

// Entity Handler Functions

#[handler]
async fn entities(
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("User not authenticated"))?;

    let entities_content = fetch_entities_content(&EntitySearchQuery {
        search: None,
        kind: None,
        page: Some(1),
        per_page: Some(25),
    })
    .await?;

    let page = EntitiesPage { entities_content };
    let prepared = AdminLayout::new("Slug Management", page)
        .with_breadcrumb("Slugs", None)
        .with_sidebar_active("entities")
        .prepare(Some(user_info))
        .await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(prepared.render_once()?))
}

#[handler]
async fn entities_search(
    Query(query): Query<EntitySearchQuery>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let entities_content = fetch_entities_content(&query).await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(entities_content.render_once()?))
}

#[handler]
async fn entities_target_input(
    Query(query): Query<EntityTargetInputQuery>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let kind = query.kind.parse::<EntityKind>()?;
    let current_value = query.current_value.unwrap_or_default();

    let (project_widget, event_widget, pagecontent_widget) = match kind {
        EntityKind::Project => {
            let widget = if !current_value.is_empty() {
                if let Ok(ulid) = current_value.parse::<Ulid>() {
                    if let Ok(Some(name)) =
                        crate::admin::autocomplete::resolve_project_name(ulid).await
                    {
                        crate::admin::autocomplete::EntityAutocomplete::new_project_selector(
                            "target",
                            "Target",
                            "Search for a project...",
                        )
                        .with_selected(ulid, name)
                    } else {
                        crate::admin::autocomplete::EntityAutocomplete::new_project_selector(
                            "target",
                            "Target",
                            "Search for a project...",
                        )
                    }
                } else {
                    crate::admin::autocomplete::EntityAutocomplete::new_project_selector(
                        "target",
                        "Target",
                        "Search for a project...",
                    )
                }
            } else {
                crate::admin::autocomplete::EntityAutocomplete::new_project_selector(
                    "target",
                    "Target",
                    "Search for a project...",
                )
            };
            (Some(widget), None, None)
        }
        EntityKind::Event => {
            let widget = if !current_value.is_empty() {
                if let Ok(ulid) = current_value.parse::<Ulid>() {
                    if let Ok(Some(name)) =
                        crate::admin::autocomplete::resolve_event_name(ulid).await
                    {
                        crate::admin::autocomplete::EntityAutocomplete::new_event_selector(
                            "target",
                            "Target",
                            "Search for an event...",
                        )
                        .with_selected(ulid, name)
                    } else {
                        crate::admin::autocomplete::EntityAutocomplete::new_event_selector(
                            "target",
                            "Target",
                            "Search for an event...",
                        )
                    }
                } else {
                    crate::admin::autocomplete::EntityAutocomplete::new_event_selector(
                        "target",
                        "Target",
                        "Search for an event...",
                    )
                }
            } else {
                crate::admin::autocomplete::EntityAutocomplete::new_event_selector(
                    "target",
                    "Target",
                    "Search for an event...",
                )
            };
            (None, Some(widget), None)
        }
        EntityKind::RawHtml => {
            let widget = create_pagecontent_widget(
                &current_value,
                crate::pagecontent::PageContentKind::RawHtml,
            )
            .await;
            (None, None, Some(widget))
        }
        EntityKind::DressedHtml => {
            let widget = create_pagecontent_widget(
                &current_value,
                crate::pagecontent::PageContentKind::DressedHtml,
            )
            .await;
            (None, None, Some(widget))
        }
        EntityKind::Wysiwyg => {
            let widget = create_pagecontent_widget(
                &current_value,
                crate::pagecontent::PageContentKind::Wysiwyg,
            )
            .await;
            (None, None, Some(widget))
        }
        EntityKind::PageBuilder => {
            let widget = create_pagecontent_widget(
                &current_value,
                crate::pagecontent::PageContentKind::PageBuilder,
            )
            .await;
            (None, None, Some(widget))
        }
        EntityKind::Form => {
            let widget = create_pagecontent_widget(
                &current_value,
                crate::pagecontent::PageContentKind::Form,
            )
            .await;
            (None, None, Some(widget))
        }
        _ => (None, None, None),
    };

    let template = EntityTargetInputTemplate {
        kind,
        current_value,
        project_widget,
        event_widget,
        pagecontent_widget,
    };

    let html = template.render_once()?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(html))
}

#[handler]
async fn entities_new(
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let form = NewEntityForm {
        title: "Add New Entity".to_string(),
        error_message: None,
    };

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(form.render_once()?))
}

#[handler]
async fn entities_create(
    Form(form): Form<CreateEntityForm>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    // Validate and create entity
    match (async || -> Result<String, anyhow::Error> {
        // Validate form
        if form.slug.trim().is_empty() {
            return Err(anyhow::anyhow!("Slug is required"));
        }

        // Check if slug already exists
        if Entity::slug_exists(&form.slug).await? {
            return Err(anyhow::anyhow!("Slug already exists"));
        }

        // Parse entity kind using strum
        let kind = form.kind.parse::<EntityKind>()?;

        // Parse target based on kind and form input
        let target = parse_target_for_kind(&kind, form.target.as_deref())?;

        let entity = Entity::new(&form.slug, kind, target);
        entity.store().await?;

        Ok(form.slug.clone())
    })()
    .await
    {
        Ok(slug) => {
            // Log the activity
            if let Some(user_info) = &user_info {
                common::service::log_activity(
                    None,
                    "entity-create",
                    user_info.user_id,
                    None,
                    format!("slug={} kind={}", slug, form.kind),
                )
                .await?;
            }
            // Success: Update entities list via OOB and close modal
            let entities_content = fetch_entities_content(&EntitySearchQuery {
                search: None,
                kind: None,
                page: Some(1),
                per_page: Some(25),
            })
            .await?;

            Ok(poem::Response::builder()
                .content_type("text/html")
                .body(format!(
                    r#"<div hx-swap-oob="innerHTML:#entities-content">{}</div>"#,
                    entities_content.render_once()?
                )))
        }
        Err(e) => {
            // Error: Return modal with error message
            let form = NewEntityForm {
                title: "Add New Entity".to_string(),
                error_message: Some(e.to_string()),
            };

            Ok(poem::Response::builder()
                .content_type("text/html")
                .body(form.render_once()?))
        }
    }
}

#[handler]
async fn entities_edit(
    Path(slug): Path<String>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let entity = Entity::load(slug.clone()).await?;

    let target_ulid = match entity.target() {
        crate::entity::EntityTarget::Ulid(ulid) => Some(*ulid),
        _ => None,
    };

    // Create the target input template for initial rendering
    let target_input_template =
        create_target_input_template(entity.kind(), &entity.target().to_string()).await;

    let content = EditEntityPage {
        slug: slug.clone(),
        kind: entity.kind().as_ref().to_string(),
        target: entity.target().to_string(),
        target_ulid,
        target_input_template,
        error_message: None,
    };

    let prepared = crate::component::AdminLayout::new("Edit Slug", content)
        .with_sidebar_active("entities")
        .with_breadcrumb("Slugs", Some("/-admin/entities"))
        .with_breadcrumb("Edit", None)
        .prepare(user_info)
        .await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(prepared.render_once()?))
}

#[handler]
async fn entities_update(
    Path(slug): Path<String>,
    Form(form): Form<UpdateEntityForm>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    // Validate and parse form data
    let kind = match form.kind.parse::<EntityKind>() {
        Ok(k) => k,
        Err(e) => {
            let entity = Entity::load(slug.clone()).await?;
            let target_ulid = match entity.target() {
                crate::entity::EntityTarget::Ulid(ulid) => Some(*ulid),
                _ => None,
            };
            let target_input_template =
                create_target_input_template(entity.kind(), &entity.target().to_string()).await;

            let content = EditEntityPage {
                slug: slug.clone(),
                kind: form.kind,
                target: form.target.unwrap_or_default(),
                target_ulid,
                target_input_template,
                error_message: Some(format!("Invalid entity kind: {}", e)),
            };

            let prepared = crate::component::AdminLayout::new("Edit Slug", content)
                .with_sidebar_active("entities")
                .with_breadcrumb("Slugs", Some("/-admin/entities"))
                .with_breadcrumb("Edit", None)
                .prepare(user_info)
                .await?;

            return Ok(poem::Response::builder()
                .content_type("text/html")
                .body(prepared.render_once()?));
        }
    };

    let target = match parse_target_for_kind(&kind, form.target.as_deref()) {
        Ok(t) => t,
        Err(e) => {
            let entity = Entity::load(slug.clone()).await?;
            let target_ulid = match entity.target() {
                crate::entity::EntityTarget::Ulid(ulid) => Some(*ulid),
                _ => None,
            };
            let target_input_template =
                create_target_input_template(&kind, form.target.as_deref().unwrap_or("")).await;

            let content = EditEntityPage {
                slug: slug.clone(),
                kind: form.kind,
                target: form.target.unwrap_or_default(),
                target_ulid,
                target_input_template,
                error_message: Some(format!("Invalid target: {}", e)),
            };

            let prepared = crate::component::AdminLayout::new("Edit Slug", content)
                .with_sidebar_active("entities")
                .with_breadcrumb("Slugs", Some("/-admin/entities"))
                .with_breadcrumb("Edit", None)
                .prepare(user_info)
                .await?;

            return Ok(poem::Response::builder()
                .content_type("text/html")
                .body(prepared.render_once()?));
        }
    };

    // Update entity
    let updated_entity = Entity::new(&slug, kind, target);
    match updated_entity.store().await {
        Ok(_) => {
            // Log the activity
            if let Some(user_info) = &user_info {
                common::service::log_activity(
                    None,
                    "entity-update",
                    user_info.user_id,
                    None,
                    format!("slug={} kind={}", slug, form.kind),
                )
                .await?;
            }

            // Redirect back to list page
            Ok(poem::Response::builder()
                .status(poem::http::StatusCode::SEE_OTHER)
                .header("Location", "/-admin/entities")
                .finish())
        }
        Err(e) => {
            let entity = Entity::load(slug.clone()).await?;
            let target_ulid = match entity.target() {
                crate::entity::EntityTarget::Ulid(ulid) => Some(*ulid),
                _ => None,
            };
            let target_input_template =
                create_target_input_template(entity.kind(), &entity.target().to_string()).await;

            let content = EditEntityPage {
                slug: slug.clone(),
                kind: entity.kind().as_ref().to_string(),
                target: entity.target().to_string(),
                target_ulid,
                target_input_template,
                error_message: Some(format!("Error saving entity: {}", e)),
            };

            let prepared = crate::component::AdminLayout::new("Edit Slug", content)
                .with_sidebar_active("entities")
                .with_breadcrumb("Slugs", Some("/-admin/entities"))
                .with_breadcrumb("Edit", None)
                .prepare(user_info)
                .await?;

            Ok(poem::Response::builder()
                .content_type("text/html")
                .body(prepared.render_once()?))
        }
    }
}

#[handler]
async fn entities_delete(
    Path(slug): Path<String>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    // Load entity before deletion for logging
    let entity = Entity::load(slug.clone()).await.ok();

    sqlx::query!(r#"DELETE FROM "entities" WHERE "slug" = ?"#, slug)
        .execute(db())
        .await?;

    // Log the activity
    if let Some(user_info) = &user_info {
        let extra = entity
            .as_ref()
            .map(|e| format!("slug={} kind={:?}", slug, e.kind()));
        common::service::log_activity(None, "entity-delete", user_info.user_id, None, extra)
            .await?;
    }

    // Check if there are any entities left
    let exists: bool =
        sqlx::query_scalar!(r#"SELECT EXISTS(SELECT 1 FROM entities) as "exists: bool""#)
            .fetch_one(db())
            .await?;

    let html = if !exists {
        // If no items left, return OOB swap with empty state
        let content = fetch_entities_content(&EntitySearchQuery {
            search: None,
            kind: None,
            page: Some(1),
            per_page: None,
        })
        .await?;
        format!(
            r#"<div id="entities-content" hx-swap-oob="outerHTML">{}</div>"#,
            content.render_once()?
        )
    } else {
        // Otherwise just return empty to remove the row
        String::new()
    };

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(html))
}

#[derive(serde::Deserialize)]
struct EntityPublishForm {
    publish: Option<String>,
}

#[handler]
async fn entities_publish_toggle(
    Path(slug): Path<String>,
    Form(form): Form<EntityPublishForm>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, poem::Error> {
    // Parse the publish value - hx-vals sends "true" or "false" based on switch checked state
    let publish = form.publish.as_deref() == Some("true");

    // Load the entity
    let mut entity = Entity::load(slug.clone())
        .await
        .map_err(|_| poem::Error::from_status(poem::http::StatusCode::NOT_FOUND))?;

    // Update the publish field using Entity::with_publish to create a new instance
    let updated_entity = Entity::with_publish(
        entity.slug(),
        *entity.kind(),
        entity.target().clone(),
        publish,
    );

    // Store the updated entity
    updated_entity
        .store()
        .await
        .map_err(|_| poem::Error::from_status(poem::http::StatusCode::INTERNAL_SERVER_ERROR))?;

    // Log the activity
    if let Some(user_info) = &user_info {
        let _ = common::service::log_activity(
            None,
            if publish {
                "entity-publish"
            } else {
                "entity-unpublish"
            },
            user_info.user_id,
            None,
            Some(slug.clone()),
        )
        .await;
    }

    // Return updated entity row with out-of-band swap
    let entity_with_info = create_entity_with_info(&updated_entity)
        .await
        .map_err(|_| poem::Error::from_status(poem::http::StatusCode::INTERNAL_SERVER_ERROR))?;

    let entity_row = EntityRow {
        entity: entity_with_info,
        oob: true,
        oob_swap: "true".to_string(),
    };

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(entity_row.render_once().unwrap()))
}

// Helper Functions

async fn create_pagecontent_widget(
    current_value: &str,
    pagecontent_kind: crate::pagecontent::PageContentKind,
) -> crate::admin::autocomplete::EntityAutocomplete {
    if !current_value.is_empty() {
        if let Ok(ulid) = current_value.parse::<Ulid>() {
            if let Ok(Some(name)) = crate::admin::autocomplete::resolve_pagecontent_name(ulid).await
            {
                return crate::admin::autocomplete::EntityAutocomplete::new_pagecontent_selector(
                    "target",
                    "Target",
                    "Search for content...",
                )
                .with_selected(ulid, name)
                .with_kind_filter(pagecontent_kind);
            }
        }
    }
    crate::admin::autocomplete::EntityAutocomplete::new_pagecontent_selector(
        "target",
        "Target",
        "Search for content...",
    )
    .with_kind_filter(pagecontent_kind)
}

async fn create_target_input_template(
    kind: &EntityKind,
    current_value: &str,
) -> EntityTargetInputTemplate {
    let (project_widget, event_widget, pagecontent_widget) = match kind {
        EntityKind::Project => {
            let widget = if !current_value.is_empty() {
                if let Ok(ulid) = current_value.parse::<Ulid>() {
                    if let Ok(Some(name)) =
                        crate::admin::autocomplete::resolve_project_name(ulid).await
                    {
                        crate::admin::autocomplete::EntityAutocomplete::new_project_selector(
                            "target",
                            "Target",
                            "Search for a project...",
                        )
                        .with_selected(ulid, name)
                    } else {
                        crate::admin::autocomplete::EntityAutocomplete::new_project_selector(
                            "target",
                            "Target",
                            "Search for a project...",
                        )
                    }
                } else {
                    crate::admin::autocomplete::EntityAutocomplete::new_project_selector(
                        "target",
                        "Target",
                        "Search for a project...",
                    )
                }
            } else {
                crate::admin::autocomplete::EntityAutocomplete::new_project_selector(
                    "target",
                    "Target",
                    "Search for a project...",
                )
            };
            (Some(widget), None, None)
        }
        EntityKind::Event => {
            let widget = if !current_value.is_empty() {
                if let Ok(ulid) = current_value.parse::<Ulid>() {
                    if let Ok(Some(name)) =
                        crate::admin::autocomplete::resolve_event_name(ulid).await
                    {
                        crate::admin::autocomplete::EntityAutocomplete::new_event_selector(
                            "target",
                            "Target",
                            "Search for an event...",
                        )
                        .with_selected(ulid, name)
                    } else {
                        crate::admin::autocomplete::EntityAutocomplete::new_event_selector(
                            "target",
                            "Target",
                            "Search for an event...",
                        )
                    }
                } else {
                    crate::admin::autocomplete::EntityAutocomplete::new_event_selector(
                        "target",
                        "Target",
                        "Search for an event...",
                    )
                }
            } else {
                crate::admin::autocomplete::EntityAutocomplete::new_event_selector(
                    "target",
                    "Target",
                    "Search for an event...",
                )
            };
            (None, Some(widget), None)
        }
        EntityKind::RawHtml => {
            let widget = create_pagecontent_widget(
                current_value,
                crate::pagecontent::PageContentKind::RawHtml,
            )
            .await;
            (None, None, Some(widget))
        }
        EntityKind::DressedHtml => {
            let widget = create_pagecontent_widget(
                current_value,
                crate::pagecontent::PageContentKind::DressedHtml,
            )
            .await;
            (None, None, Some(widget))
        }
        EntityKind::Wysiwyg => {
            let widget = create_pagecontent_widget(
                current_value,
                crate::pagecontent::PageContentKind::Wysiwyg,
            )
            .await;
            (None, None, Some(widget))
        }
        EntityKind::PageBuilder => {
            let widget = create_pagecontent_widget(
                current_value,
                crate::pagecontent::PageContentKind::PageBuilder,
            )
            .await;
            (None, None, Some(widget))
        }
        EntityKind::Form => {
            let widget =
                create_pagecontent_widget(current_value, crate::pagecontent::PageContentKind::Form)
                    .await;
            (None, None, Some(widget))
        }
        _ => (None, None, None),
    };

    EntityTargetInputTemplate {
        kind: *kind,
        current_value: current_value.to_string(),
        project_widget,
        event_widget,
        pagecontent_widget,
    }
}

fn parse_target_for_kind(
    kind: &EntityKind,
    target_str: Option<&str>,
) -> Result<EntityTarget, anyhow::Error> {
    match target_str {
        Some(s) if !s.trim().is_empty() => match kind {
            EntityKind::Redirect => {
                let url = s.parse::<url::Url>()?;
                Ok(EntityTarget::Url(url))
            }
            EntityKind::WordPress => Ok(EntityTarget::String(s.to_string())),
            EntityKind::RawHtml
            | EntityKind::DressedHtml
            | EntityKind::Wysiwyg
            | EntityKind::PageBuilder
            | EntityKind::Form
            | EntityKind::Project
            | EntityKind::Event => {
                let ulid = s.parse::<Ulid>()?;
                Ok(EntityTarget::Ulid(ulid))
            }
            EntityKind::Placeholder => Ok(EntityTarget::Nil),
        },
        _ => Ok(EntityTarget::Nil),
    }
}

#[derive(Debug)]
struct EntityDbRow {
    slug: String,
    kind: EntityKind,
    target: String,
    publish: bool,
    created: common::timestring::TimeString,
}

async fn fetch_entities_content(
    query: &EntitySearchQuery,
) -> Result<EntitiesContent, anyhow::Error> {
    let page = query.page.unwrap_or(1).max(1) as i64;
    let per_page = query.per_page.unwrap_or(25).min(100).max(1) as i64;
    let offset = (page - 1) * per_page;

    let search_term = query.search.as_deref().unwrap_or("").trim();
    let selected_kind = query.kind.as_deref().unwrap_or("");

    // Fetch all entities and filter/paginate in Rust to avoid SQLx Record conflicts
    let all_rows = sqlx::query_as!(
        EntityDbRow,
        r#"SELECT "slug", "kind" as "kind: EntityKind", "target", "publish", "created" as "created: common::timestring::TimeString" FROM "entities" ORDER BY "created" DESC"#
    ).fetch_all(db()).await?;

    // Filter
    let filtered_rows: Vec<_> = all_rows
        .into_iter()
        .filter(|row| {
            if !search_term.is_empty()
                && !row
                    .slug
                    .to_lowercase()
                    .contains(&search_term.to_lowercase())
            {
                return false;
            }
            if !selected_kind.is_empty() && row.kind.as_ref() != selected_kind {
                return false;
            }
            true
        })
        .collect();

    let total_entities = filtered_rows.len() as i64;

    // Paginate
    let entity_rows: Vec<_> = filtered_rows
        .into_iter()
        .skip(offset as usize)
        .take(per_page as usize)
        .collect();

    let mut entity_list = Vec::new();
    for row in entity_rows {
        let target = parse_database_target(row.kind, &row.target)?;
        let entity = Entity::with_created(&row.slug, row.kind, target, row.publish, row.created);
        let entity_with_info = create_entity_with_info(&entity).await?;
        entity_list.push(entity_with_info);
    }

    let has_previous = page > 1;
    let has_next = (page * per_page) < total_entities;
    let start_item = if total_entities > 0 { offset + 1 } else { 0 };
    let end_item = std::cmp::min(page * per_page, total_entities);

    Ok(EntitiesContent {
        entities: entity_list,
        current_page: page,
        total_entities,
        has_previous,
        has_next,
        per_page,
        start_item,
        end_item,
        pagination_url: "/-admin/entities/search".to_string(),
        pagination_include: "#entity-search-form".to_string(),
        search_term: search_term.to_string(),
        selected_kind: selected_kind.to_string(),
    })
}

fn parse_database_target(
    kind: EntityKind,
    target_str: &str,
) -> Result<EntityTarget, anyhow::Error> {
    if target_str.trim().is_empty() {
        return Ok(EntityTarget::Nil);
    }

    match kind {
        EntityKind::Redirect => match target_str.parse() {
            Ok(url) => Ok(EntityTarget::Url(url)),
            Err(_) => Ok(EntityTarget::Nil),
        },
        EntityKind::WordPress => Ok(EntityTarget::String(target_str.to_string())),
        EntityKind::RawHtml
        | EntityKind::DressedHtml
        | EntityKind::Wysiwyg
        | EntityKind::PageBuilder
        | EntityKind::Form
        | EntityKind::Project
        | EntityKind::Event => match target_str.parse() {
            Ok(ulid) => Ok(EntityTarget::Ulid(ulid)),
            Err(_) => Ok(EntityTarget::Nil),
        },
        EntityKind::Placeholder => Ok(EntityTarget::Nil),
    }
}

async fn create_entity_with_info(entity: &Entity) -> Result<EntityWithInfo, anyhow::Error> {
    use common::shared::db;

    // Fetch target_info (display name) based on entity type
    let target_info = match (entity.kind(), entity.target()) {
        (EntityKind::Project, EntityTarget::Ulid(ulid)) => {
            // Fetch only name from projects table
            let ulid_str = ulid.to_string();
            let result = sqlx::query!(
                r#"SELECT "name" FROM "projects" WHERE "project_id" = ?"#,
                ulid_str
            )
            .fetch_optional(db())
            .await?;

            result
                .map(|row| row.name)
                .or(Some("Project not found".to_string()))
        }
        (EntityKind::Event, EntityTarget::Ulid(ulid)) => {
            // Fetch only name from events table
            let ulid_str = ulid.to_string();
            let result = sqlx::query!(
                r#"SELECT "name" FROM "events" WHERE "event_id" = ?"#,
                ulid_str
            )
            .fetch_optional(db())
            .await?;

            result
                .map(|row| row.name)
                .or(Some("Event not found".to_string()))
        }
        (EntityKind::Redirect, EntityTarget::Url(url)) => Some(url.to_string()),
        (EntityKind::WordPress, EntityTarget::String(s)) => Some(format!("WordPress: {}", s)),
        (
            EntityKind::RawHtml
            | EntityKind::DressedHtml
            | EntityKind::Wysiwyg
            | EntityKind::PageBuilder
            | EntityKind::Form,
            _,
        ) => Some("Static content".to_string()),
        (_, EntityTarget::Nil) => None,
        _ => Some("Invalid target type".to_string()),
    };

    Ok(EntityWithInfo {
        slug: entity.slug().to_string(),
        kind: *entity.kind(),
        target: entity.target().to_string(),
        target_info,
        published: entity.publish(), // Use entity's publish field directly
    })
}

#[handler]
async fn clear_modal(
    Authenticated(_user_info): Authenticated,
) -> Result<impl IntoResponse, poem::Error> {
    // Return empty response to clear the modal container
    Ok("")
}

// WYSIWYG Content Management

#[derive(sailfish::TemplateOnce)]
#[template(path = "admin/wysiwyg.stpl.html")]
struct PageContentWrapper {
    title: String,
    new_endpoint: String,
    content: String,
}

#[derive(sailfish::TemplateOnce)]
#[template(path = "admin/wysiwyg_content.stpl.html")]
struct WysiwygContentTemplate {
    items: Vec<WysiwygListItem>,
    search_query: Option<String>,
    page: usize,
    per_page: usize,
    total_count: usize,
}

#[derive(Debug, Clone)]
struct WysiwygListItem {
    id: Ulid,
    name: String,
    excerpt: String,
    created_at: String,
}

#[derive(serde::Deserialize)]
struct WysiwygSearchQuery {
    search: Option<String>,
    page: Option<usize>,
    per_page: Option<usize>,
}

#[handler]
async fn wysiwyg_list(
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("User not authenticated"))?;

    let search_results = wysiwyg_search_impl(&WysiwygSearchQuery {
        search: None,
        page: Some(1),
        per_page: Some(25),
    })
    .await?;

    let content = format!(r#"<div id="wysiwyg-content">{}</div>"#, search_results);

    let page = PageContentWrapper {
        title: "Simple Pages".to_string(),
        new_endpoint: "/-admin/wysiwyg/new".to_string(),
        content,
    };

    let prepared = crate::component::AdminLayout::new("Simple Content", page)
        .with_sidebar_active("wysiwyg")
        .with_breadcrumb("Simple Pages", None)
        .with_quill()
        .prepare(Some(user_info))
        .await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(prepared.render_once()?))
}

async fn wysiwyg_search_impl(query: &WysiwygSearchQuery) -> Result<String, anyhow::Error> {
    let page = query.page.unwrap_or(1).max(1);
    let per_page = query.per_page.unwrap_or(25).clamp(10, 100);
    let offset = ((page - 1) * per_page) as i64;
    let per_page = per_page as i64;

    let search_pattern = query.search.as_ref().map(|s| format!("%{}%", s));
    let kind = "Wysiwyg";

    struct QueryRow {
        pagecontent_id: String,
        name: String,
        content: Vec<u8>,
    }

    let items: Vec<QueryRow> = if let Some(ref pattern) = search_pattern {
        sqlx::query_as!(
            QueryRow,
            r#"SELECT pagecontent_id, name, content FROM pagecontent
               WHERE kind = ? AND name LIKE ?
               ORDER BY pagecontent_id DESC
               LIMIT ? OFFSET ?"#,
            kind,
            pattern,
            per_page,
            offset
        )
        .fetch_all(db())
        .await?
    } else {
        sqlx::query_as!(
            QueryRow,
            r#"SELECT pagecontent_id, name, content FROM pagecontent
               WHERE kind = ?
               ORDER BY pagecontent_id DESC
               LIMIT ? OFFSET ?"#,
            kind,
            per_page,
            offset
        )
        .fetch_all(db())
        .await?
    };

    let total_count = if let Some(ref pattern) = search_pattern {
        sqlx::query_scalar!(
            r#"SELECT COUNT(*) as "count: i64" FROM pagecontent WHERE kind = 'Wysiwyg' AND name LIKE ?"#,
            pattern
        )
        .fetch_one(db())
        .await? as usize
    } else {
        sqlx::query_scalar!(
            r#"SELECT COUNT(*) as "count: i64" FROM pagecontent WHERE kind = 'Wysiwyg'"#
        )
        .fetch_one(db())
        .await? as usize
    };

    let items = items
        .into_iter()
        .map(|row| {
            let id: Ulid = row.pagecontent_id.parse().unwrap_or(Ulid::nil());
            let timestamp = jiff::Timestamp::from_millisecond(id.timestamp_ms() as i64).unwrap();
            let created_at = timestamp.strftime("%Y-%m-%d %H:%M").to_string();

            // Extract text excerpt from Delta JSON
            let excerpt = if let Ok(content_str) = std::str::from_utf8(&row.content) {
                common::quill::delta_to_excerpt(content_str, 100)
            } else {
                String::new()
            };

            WysiwygListItem {
                id,
                name: row.name,
                excerpt,
                created_at,
            }
        })
        .collect();

    let content = WysiwygContentTemplate {
        items,
        search_query: query.search.clone(),
        page: query.page.unwrap_or(1).max(1),
        per_page: query.per_page.unwrap_or(25).clamp(10, 100),
        total_count,
    };

    Ok(content.render_once()?)
}

#[handler]
async fn wysiwyg_search(
    Query(query): Query<WysiwygSearchQuery>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let html = wysiwyg_search_impl(&query).await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(html))
}

#[derive(sailfish::TemplateOnce)]
#[template(path = "admin/modals/new_wysiwyg.stpl.html")]
struct NewWysiwygForm {
    title: String,
    error_message: Option<String>,
}

#[derive(serde::Deserialize)]
struct CreatePageContentForm {
    kind: String,
    name: String,
    content: String,
}

#[handler]
async fn wysiwyg_new(
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let form = NewWysiwygForm {
        title: "Create New Simple Page".to_string(),
        error_message: None,
    };

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(form.render_once()?))
}

#[handler]
async fn pagecontent_create(
    Form(form): Form<CreatePageContentForm>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let kind: crate::pagecontent::PageContentKind = form.kind.parse()?;

    match (async || -> Result<Ulid, anyhow::Error> {
        if form.name.trim().is_empty() {
            return Err(anyhow::anyhow!("Name is required"));
        }

        let id = Ulid::new();
        let pagecontent = crate::pagecontent::PageContent::new_with_id(
            id,
            kind,
            form.name.clone(),
            form.content.as_bytes(),
        );
        pagecontent.store().await?;

        Ok(id)
    })()
    .await
    {
        Ok(id) => {
            // Log the activity
            if let Some(user_info) = &user_info {
                common::service::log_activity(
                    None,
                    "pagecontent-create",
                    user_info.user_id,
                    id,
                    format!("kind={} name={}", form.kind, form.name),
                )
                .await?;
            }
            // Close modal and refresh list via OOB
            let (content, target_id) = match kind {
                crate::pagecontent::PageContentKind::Wysiwyg => {
                    let content = wysiwyg_search_impl(&WysiwygSearchQuery {
                        search: None,
                        page: Some(1),
                        per_page: Some(25),
                    })
                    .await?;
                    (content, "wysiwyg-content")
                }
                crate::pagecontent::PageContentKind::DressedHtml => {
                    let content = html_search_impl(
                        "DressedHtml",
                        &HtmlSearchQuery {
                            search: None,
                            page: Some(1),
                            per_page: Some(25),
                        },
                    )
                    .await?;
                    (content, "dressed-html-content")
                }
                crate::pagecontent::PageContentKind::RawHtml => {
                    let content = html_search_impl(
                        "RawHtml",
                        &HtmlSearchQuery {
                            search: None,
                            page: Some(1),
                            per_page: Some(25),
                        },
                    )
                    .await?;
                    (content, "raw-html-content")
                }
                _ => return Err(anyhow::anyhow!("Unsupported page content kind")),
            };

            let html = format!(
                r#"<div id="{}" hx-swap-oob="innerHTML">{}</div>"#,
                target_id, content
            );

            Ok(poem::Response::builder()
                .content_type("text/html")
                .body(html))
        }
        Err(e) => {
            let html = match kind {
                crate::pagecontent::PageContentKind::Wysiwyg => {
                    let form = NewWysiwygForm {
                        title: "Create New Simple Page".to_string(),
                        error_message: Some(e.to_string()),
                    };
                    form.render_once()?
                }
                crate::pagecontent::PageContentKind::DressedHtml => {
                    let form = NewDressedHtmlForm {
                        title: "Create New Dressed HTML Page".to_string(),
                        error_message: Some(e.to_string()),
                    };
                    form.render_once()?
                }
                crate::pagecontent::PageContentKind::RawHtml => {
                    let form = NewRawHtmlForm {
                        title: "Create New Raw HTML Page".to_string(),
                        error_message: Some(e.to_string()),
                    };
                    form.render_once()?
                }
                _ => return Err(anyhow::anyhow!("Unsupported page content kind")),
            };

            Ok(poem::Response::builder()
                .content_type("text/html")
                .body(html))
        }
    }
}

#[derive(sailfish::TemplateOnce)]
#[template(path = "admin/pages/edit_wysiwyg.stpl.html")]
struct EditWysiwygPage {
    id: Ulid,
    kind: crate::pagecontent::PageContentKind,
    name: String,
    content: String,
    error_message: Option<String>,
}

#[handler]
async fn pagecontent_edit(
    Path((kind_str, id_str)): Path<(String, String)>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let kind: crate::pagecontent::PageContentKind = kind_str.parse()?;
    let id: Ulid = id_str.parse()?;

    let pagecontent = crate::pagecontent::PageContent::load((kind, id)).await?;

    let content = EditWysiwygPage {
        id,
        kind,
        name: pagecontent.name().to_string(),
        content: pagecontent.as_utf8()?.to_string(),
        error_message: None,
    };

    let prepared = crate::component::AdminLayout::new("Edit Simple Page", content)
        .with_sidebar_active("wysiwyg")
        .with_breadcrumb("Simple Pages", Some("/-admin/wysiwyg"))
        .with_breadcrumb("Edit", None)
        .with_quill()
        .prepare(user_info)
        .await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(prepared.render_once()?))
}

#[derive(serde::Deserialize)]
struct UpdatePageContentForm {
    name: String,
    content: String,
}

#[handler]
async fn pagecontent_update(
    Path((kind_str, id_str)): Path<(String, String)>,
    Form(form): Form<UpdatePageContentForm>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let kind: crate::pagecontent::PageContentKind = kind_str.parse()?;
    let id: Ulid = id_str.parse()?;

    // Validate
    if form.name.trim().is_empty() {
        let content = EditWysiwygPage {
            id,
            kind,
            name: form.name,
            content: form.content,
            error_message: Some("Name is required".to_string()),
        };

        let prepared = crate::component::AdminLayout::new("Edit Simple Page", content)
            .with_sidebar_active("wysiwyg")
            .with_breadcrumb("Simple Pages", Some("/-admin/wysiwyg"))
            .with_breadcrumb("Edit", None)
            .with_quill()
            .prepare(user_info)
            .await?;

        return Ok(poem::Response::builder()
            .content_type("text/html")
            .body(prepared.render_once()?));
    }

    // Update
    let mut pagecontent = crate::pagecontent::PageContent::load((kind, id)).await?;
    pagecontent.set_name(form.name.clone());
    pagecontent.clear();
    pagecontent.extend_from_slice(form.content.as_bytes());
    pagecontent.store().await?;

    // Log the activity
    if let Some(user_info) = &user_info {
        common::service::log_activity(
            None,
            "pagecontent-update",
            user_info.user_id,
            id,
            format!("kind={:?} name={}", kind, form.name),
        )
        .await?;
    }

    // Redirect back to list page
    Ok(poem::Response::builder()
        .status(poem::http::StatusCode::SEE_OTHER)
        .header("Location", "/-admin/wysiwyg")
        .finish())
}

#[handler]
async fn pagecontent_delete(
    Path((kind_str, id_str)): Path<(String, String)>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let kind: crate::pagecontent::PageContentKind = kind_str.parse()?;
    let id: Ulid = id_str.parse()?;
    let id_string = id.to_string();
    let kind_string = kind.as_ref();

    sqlx::query!(
        r#"DELETE FROM pagecontent WHERE pagecontent_id = ? AND kind = ?"#,
        id_string,
        kind_string,
    )
    .execute(db())
    .await?;

    // Log the activity
    if let Some(user_info) = &user_info {
        common::service::log_activity(
            None,
            "pagecontent-delete",
            user_info.user_id,
            id,
            format!("kind={:?}", kind),
        )
        .await?;
    }

    // Check if there are any items left
    let exists: bool = sqlx::query_scalar!(
        r#"SELECT EXISTS(SELECT 1 FROM pagecontent WHERE kind = ?) as "exists: bool""#,
        kind_string
    )
    .fetch_one(db())
    .await?;

    let html = if !exists {
        // If no items left, return OOB swap with empty state
        let content = wysiwyg_search_impl(&WysiwygSearchQuery {
            search: None,
            page: Some(1),
            per_page: Some(25),
        })
        .await?;
        format!(
            r#"<div id="wysiwyg-content" hx-swap-oob="innerHTML">{}</div>"#,
            content
        )
    } else {
        // Otherwise just return empty to remove the row
        String::new()
    };

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(html))
}

// ==================== DressedHtml Handlers ====================

#[derive(sailfish::TemplateOnce)]
#[template(path = "admin/dressed_html_content.stpl.html")]
struct DressedHtmlContentTemplate {
    items: Vec<HtmlListItem>,
    search_query: Option<String>,
    page: usize,
    per_page: usize,
    total_count: usize,
}

#[derive(sailfish::TemplateOnce)]
#[template(path = "admin/pages/edit_dressed_html.stpl.html")]
struct EditDressedHtmlPage {
    id: Ulid,
    kind: crate::pagecontent::PageContentKind,
    name: String,
    content: String,
    error_message: Option<String>,
}

#[derive(Debug, Clone)]
struct HtmlListItem {
    id: Ulid,
    name: String,
    excerpt: String,
    created_at: String,
}

#[derive(serde::Deserialize)]
struct HtmlSearchQuery {
    search: Option<String>,
    page: Option<usize>,
    per_page: Option<usize>,
}

#[handler]
async fn dressed_html_list(
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("User not authenticated"))?;

    let search_results = html_search_impl(
        "DressedHtml",
        &HtmlSearchQuery {
            search: None,
            page: Some(1),
            per_page: Some(25),
        },
    )
    .await?;

    let content = format!(r#"<div id="dressed-html-content">{}</div>"#, search_results);

    let page = PageContentWrapper {
        title: "Dressed HTML Pages".to_string(),
        new_endpoint: "/-admin/pagecontent/DressedHtml/new".to_string(),
        content,
    };

    let prepared = crate::component::AdminLayout::new("Dressed HTML", page)
        .with_sidebar_active("engine-room-dressed-html")
        .with_breadcrumb("Engine Room", None)
        .with_breadcrumb("Dressed HTML", None)
        .prepare(Some(user_info))
        .await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(prepared.render_once()?))
}

async fn html_search_impl(kind: &str, query: &HtmlSearchQuery) -> Result<String, anyhow::Error> {
    let page = query.page.unwrap_or(1).max(1);
    let per_page = query.per_page.unwrap_or(25).clamp(10, 100);
    let offset = ((page - 1) * per_page) as i64;
    let per_page_i64 = per_page as i64;

    let search_pattern = query.search.as_ref().map(|s| format!("%{}%", s));

    struct QueryRow {
        pagecontent_id: String,
        name: String,
        content: Vec<u8>,
    }

    let items: Vec<QueryRow> = if let Some(ref pattern) = search_pattern {
        sqlx::query_as!(
            QueryRow,
            r#"SELECT pagecontent_id, name, content FROM pagecontent
               WHERE kind = ? AND name LIKE ?
               ORDER BY pagecontent_id DESC
               LIMIT ? OFFSET ?"#,
            kind,
            pattern,
            per_page_i64,
            offset
        )
        .fetch_all(db())
        .await?
    } else {
        sqlx::query_as!(
            QueryRow,
            r#"SELECT pagecontent_id, name, content FROM pagecontent
               WHERE kind = ?
               ORDER BY pagecontent_id DESC
               LIMIT ? OFFSET ?"#,
            kind,
            per_page_i64,
            offset
        )
        .fetch_all(db())
        .await?
    };

    let total_count = if let Some(ref pattern) = search_pattern {
        sqlx::query_scalar!(
            r#"SELECT COUNT(*) as "count: i64" FROM pagecontent WHERE kind = ? AND name LIKE ?"#,
            kind,
            pattern
        )
        .fetch_one(db())
        .await? as usize
    } else {
        sqlx::query_scalar!(
            r#"SELECT COUNT(*) as "count: i64" FROM pagecontent WHERE kind = ?"#,
            kind
        )
        .fetch_one(db())
        .await? as usize
    };

    let items = items
        .into_iter()
        .map(|row| {
            let id: Ulid = row.pagecontent_id.parse().unwrap_or(Ulid::nil());
            let timestamp = jiff::Timestamp::from_millisecond(id.timestamp_ms() as i64).unwrap();
            let created_at = timestamp.strftime("%Y-%m-%d %H:%M").to_string();

            // Extract text excerpt from HTML
            let excerpt = if let Ok(content_str) = std::str::from_utf8(&row.content) {
                let text = content_str.chars().take(100).collect::<String>();
                text.trim().to_string()
            } else {
                String::new()
            };

            HtmlListItem {
                id,
                name: row.name,
                excerpt,
                created_at,
            }
        })
        .collect();

    let content = if kind == "DressedHtml" {
        DressedHtmlContentTemplate {
            items,
            search_query: query.search.clone(),
            page,
            per_page,
            total_count,
        }
        .render_once()?
    } else {
        RawHtmlContentTemplate {
            items,
            search_query: query.search.clone(),
            page,
            per_page,
            total_count,
        }
        .render_once()?
    };

    Ok(content)
}

#[handler]
async fn dressed_html_search(
    Query(query): Query<HtmlSearchQuery>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let html = html_search_impl("DressedHtml", &query).await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(html))
}

#[handler]
async fn dressed_html_edit(
    Path(id_str): Path<String>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let kind = crate::pagecontent::PageContentKind::DressedHtml;
    let id: Ulid = id_str.parse()?;

    let pagecontent = crate::pagecontent::PageContent::load((kind, id)).await?;

    let content = EditDressedHtmlPage {
        id,
        kind,
        name: pagecontent.name().to_string(),
        content: pagecontent.as_utf8()?.to_string(),
        error_message: None,
    };

    let prepared = crate::component::AdminLayout::new("Edit Dressed HTML", content)
        .with_sidebar_active("engine-room-dressed-html")
        .with_breadcrumb("Engine Room", None)
        .with_breadcrumb("Dressed HTML", Some("/-admin/pagecontent/DressedHtml"))
        .with_breadcrumb("Edit", None)
        .prepare(user_info)
        .await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(prepared.render_once()?))
}

#[handler]
async fn dressed_html_update(
    Path(id_str): Path<String>,
    Form(form): Form<UpdatePageContentForm>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let kind = crate::pagecontent::PageContentKind::DressedHtml;
    let id: Ulid = id_str.parse()?;

    // Validate
    if form.name.trim().is_empty() {
        let content = EditDressedHtmlPage {
            id,
            kind,
            name: form.name,
            content: form.content,
            error_message: Some("Name is required".to_string()),
        };

        let prepared = crate::component::AdminLayout::new("Edit Dressed HTML", content)
            .with_sidebar_active("engine-room-dressed-html")
            .with_breadcrumb("Engine Room", None)
            .with_breadcrumb("Dressed HTML", Some("/-admin/pagecontent/DressedHtml"))
            .with_breadcrumb("Edit", None)
            .prepare(user_info)
            .await?;

        return Ok(poem::Response::builder()
            .content_type("text/html")
            .body(prepared.render_once()?));
    }

    // Update
    let mut pagecontent = crate::pagecontent::PageContent::load((kind, id)).await?;
    pagecontent.set_name(form.name);
    pagecontent.clear();
    pagecontent.extend_from_slice(form.content.as_bytes());
    pagecontent.store().await?;

    // Log the activity
    if let Some(user_info) = &user_info {
        let _ =
            common::service::log_activity(None, "dressedhtml-edit", user_info.user_id, id, None)
                .await;
    }

    // Redirect back to list page
    Ok(poem::Response::builder()
        .status(poem::http::StatusCode::SEE_OTHER)
        .header("Location", "/-admin/pagecontent/DressedHtml")
        .finish())
}

#[handler]
async fn dressed_html_delete(
    Path(id_str): Path<String>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let kind = crate::pagecontent::PageContentKind::DressedHtml;
    let id: Ulid = id_str.parse()?;
    let id_string = id.to_string();
    let kind_string = kind.as_ref();

    sqlx::query!(
        r#"DELETE FROM pagecontent WHERE pagecontent_id = ? AND kind = ?"#,
        id_string,
        kind_string
    )
    .execute(db())
    .await?;

    // Log the activity
    if let Some(user_info) = &user_info {
        let _ =
            common::service::log_activity(None, "dressedhtml-delete", user_info.user_id, id, None)
                .await;
    }

    // Check if there are any items left
    let exists: bool = sqlx::query_scalar!(
        r#"SELECT EXISTS(SELECT 1 FROM pagecontent WHERE kind = ?) as "exists: bool""#,
        kind_string
    )
    .fetch_one(db())
    .await?;

    let html = if !exists {
        // If no items left, return OOB swap with empty state
        let content = html_search_impl(
            "DressedHtml",
            &HtmlSearchQuery {
                search: None,
                page: Some(1),
                per_page: Some(25),
            },
        )
        .await?;
        format!(
            r#"<div id="dressed-html-content" hx-swap-oob="innerHTML">{}</div>"#,
            content
        )
    } else {
        // Otherwise just return empty to remove the row
        String::new()
    };

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(html))
}

// ==================== RawHtml Handlers ====================

#[derive(sailfish::TemplateOnce)]
#[template(path = "admin/raw_html_content.stpl.html")]
struct RawHtmlContentTemplate {
    items: Vec<HtmlListItem>,
    search_query: Option<String>,
    page: usize,
    per_page: usize,
    total_count: usize,
}

#[derive(sailfish::TemplateOnce)]
#[template(path = "admin/pages/edit_raw_html.stpl.html")]
struct EditRawHtmlPage {
    id: Ulid,
    kind: crate::pagecontent::PageContentKind,
    name: String,
    content: String,
    error_message: Option<String>,
}

#[handler]
async fn raw_html_list(
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("User not authenticated"))?;

    let search_results = html_search_impl(
        "RawHtml",
        &HtmlSearchQuery {
            search: None,
            page: Some(1),
            per_page: Some(25),
        },
    )
    .await?;

    let content = format!(r#"<div id="raw-html-content">{}</div>"#, search_results);

    let page = PageContentWrapper {
        title: "Raw HTML Pages".to_string(),
        new_endpoint: "/-admin/pagecontent/RawHtml/new".to_string(),
        content,
    };

    let prepared = crate::component::AdminLayout::new("Raw HTML", page)
        .with_sidebar_active("engine-room-raw-html")
        .with_breadcrumb("Engine Room", None)
        .with_breadcrumb("Raw HTML", None)
        .prepare(Some(user_info))
        .await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(prepared.render_once()?))
}

#[handler]
async fn raw_html_search(
    Query(query): Query<HtmlSearchQuery>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let html = html_search_impl("RawHtml", &query).await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(html))
}

#[handler]
async fn raw_html_edit(
    Path(id_str): Path<String>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let kind = crate::pagecontent::PageContentKind::RawHtml;
    let id: Ulid = id_str.parse()?;

    let pagecontent = crate::pagecontent::PageContent::load((kind, id)).await?;

    let content = EditRawHtmlPage {
        id,
        kind,
        name: pagecontent.name().to_string(),
        content: pagecontent.as_utf8()?.to_string(),
        error_message: None,
    };

    let prepared = crate::component::AdminLayout::new("Edit Raw HTML", content)
        .with_sidebar_active("engine-room-raw-html")
        .with_breadcrumb("Engine Room", None)
        .with_breadcrumb("Raw HTML", Some("/-admin/pagecontent/RawHtml"))
        .with_breadcrumb("Edit", None)
        .prepare(user_info)
        .await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(prepared.render_once()?))
}

#[handler]
async fn raw_html_update(
    Path(id_str): Path<String>,
    Form(form): Form<UpdatePageContentForm>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let kind = crate::pagecontent::PageContentKind::RawHtml;
    let id: Ulid = id_str.parse()?;

    // Validate
    if form.name.trim().is_empty() {
        let content = EditRawHtmlPage {
            id,
            kind,
            name: form.name,
            content: form.content,
            error_message: Some("Name is required".to_string()),
        };

        let prepared = crate::component::AdminLayout::new("Edit Raw HTML", content)
            .with_sidebar_active("engine-room-raw-html")
            .with_breadcrumb("Engine Room", None)
            .with_breadcrumb("Raw HTML", Some("/-admin/pagecontent/RawHtml"))
            .with_breadcrumb("Edit", None)
            .prepare(user_info)
            .await?;

        return Ok(poem::Response::builder()
            .content_type("text/html")
            .body(prepared.render_once()?));
    }

    // Update
    let mut pagecontent = crate::pagecontent::PageContent::load((kind, id)).await?;
    pagecontent.set_name(form.name);
    pagecontent.clear();
    pagecontent.extend_from_slice(form.content.as_bytes());
    pagecontent.store().await?;

    // Log the activity
    if let Some(user_info) = &user_info {
        let _ =
            common::service::log_activity(None, "rawhtml-edit", user_info.user_id, id, None).await;
    }

    // Redirect back to list page
    Ok(poem::Response::builder()
        .status(poem::http::StatusCode::SEE_OTHER)
        .header("Location", "/-admin/pagecontent/RawHtml")
        .finish())
}

#[handler]
async fn raw_html_delete(
    Path(id_str): Path<String>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let kind = crate::pagecontent::PageContentKind::RawHtml;
    let id: Ulid = id_str.parse()?;
    let id_string = id.to_string();
    let kind_string = kind.as_ref();

    sqlx::query!(
        r#"DELETE FROM pagecontent WHERE pagecontent_id = ? AND kind = ?"#,
        id_string,
        kind_string
    )
    .execute(db())
    .await?;

    // Log the activity
    if let Some(user_info) = &user_info {
        let _ = common::service::log_activity(None, "rawhtml-delete", user_info.user_id, id, None)
            .await;
    }

    // Check if there are any items left
    let exists: bool = sqlx::query_scalar!(
        r#"SELECT EXISTS(SELECT 1 FROM pagecontent WHERE kind = ?) as "exists: bool""#,
        kind_string
    )
    .fetch_one(db())
    .await?;

    let html = if !exists {
        // If no items left, return OOB swap with empty state
        let content = html_search_impl(
            "RawHtml",
            &HtmlSearchQuery {
                search: None,
                page: Some(1),
                per_page: Some(25),
            },
        )
        .await?;
        format!(
            r#"<div id="raw-html-content" hx-swap-oob="innerHTML">{}</div>"#,
            content
        )
    } else {
        // Otherwise just return empty to remove the row
        String::new()
    };

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(html))
}

// ==================== New Pagecontent Handlers ====================

#[derive(sailfish::TemplateOnce)]
#[template(path = "admin/modals/new_dressed_html.stpl.html")]
struct NewDressedHtmlForm {
    title: String,
    error_message: Option<String>,
}

#[handler]
async fn dressed_html_new(
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let form = NewDressedHtmlForm {
        title: "Create New Dressed HTML Page".to_string(),
        error_message: None,
    };

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(form.render_once()?))
}

#[derive(sailfish::TemplateOnce)]
#[template(path = "admin/modals/new_raw_html.stpl.html")]
struct NewRawHtmlForm {
    title: String,
    error_message: Option<String>,
}

#[handler]
async fn raw_html_new(
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let form = NewRawHtmlForm {
        title: "Create New Raw HTML Page".to_string(),
        error_message: None,
    };

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(form.render_once()?))
}
// ==================== Schedule Handlers ====================

#[derive(sailfish::TemplateOnce)]
#[template(path = "admin/schedules.stpl.html")]
struct SchedulesPage {
    schedules_content: String,
}

#[derive(sailfish::TemplateOnce)]
#[template(path = "admin/schedules_content.stpl.html")]
struct SchedulesContentTemplate {
    items: Vec<crate::schedule::Schedule>,
    search_query: Option<String>,
    page: usize,
    per_page: usize,
    total_count: usize,
}

struct ScheduleRow {
    schedule_id: String,
    schedule_type: String,
    target_type: Option<String>,
    target: Option<String>,
    featured_entity: Option<String>,
    title: String,
    content: Option<String>,
    url: Option<String>,
    start_date: String,
    end_date: String,
    active: i64,
}

#[derive(serde::Deserialize)]
struct SchedulesSearchQuery {
    search: Option<String>,
    page: Option<usize>,
}

#[handler]
async fn schedules_list(
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("User not authenticated"))?;

    let search_results = schedules_search_impl(&SchedulesSearchQuery {
        search: None,
        page: Some(1),
    })
    .await?;

    let content = format!(r#"<div id="schedules-content">{}</div>"#, search_results);

    let page = SchedulesPage {
        schedules_content: content,
    };

    let prepared = crate::component::AdminLayout::new("Scheduling", page)
        .with_sidebar_active("schedules")
        .with_breadcrumb("Scheduling", None)
        .prepare(Some(user_info))
        .await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(prepared.render_once()?))
}

#[handler]
async fn schedules_search(
    Query(query): Query<SchedulesSearchQuery>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let content = schedules_search_impl(&query).await?;
    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(content))
}

async fn schedules_search_impl(query: &SchedulesSearchQuery) -> Result<String, anyhow::Error> {
    let page = query.page.unwrap_or(1).max(1);
    let per_page: i64 = 25;
    let offset = (page - 1) as i64 * per_page;

    let items_raw = if let Some(ref search) = query.search {
        let pattern = format!("%{}%", search);
        sqlx::query_as!(
            ScheduleRow,
            r#"SELECT schedule_id,
                      schedule_type,
                      target_type,
                      target,
                      featured_entity,
                      title,
                      content,
                      url,
                      start_date,
                      end_date,
                      active
               FROM schedules
               WHERE title LIKE ?1
               ORDER BY start_date DESC
               LIMIT ?2 OFFSET ?3"#,
            pattern,
            per_page,
            offset
        )
        .fetch_all(db())
        .await?
    } else {
        sqlx::query_as!(
            ScheduleRow,
            r#"SELECT schedule_id,
                      schedule_type,
                      target_type,
                      target,
                      featured_entity,
                      title,
                      content,
                      url,
                      start_date,
                      end_date,
                      active
               FROM schedules
               ORDER BY start_date DESC
               LIMIT ?1 OFFSET ?2"#,
            per_page,
            offset
        )
        .fetch_all(db())
        .await?
    };

    let items = items_raw
        .into_iter()
        .map(|row| -> Result<_, anyhow::Error> {
            Ok(crate::schedule::Schedule {
                schedule_id: row.schedule_id.parse()?,
                schedule_type: row.schedule_type.parse()?,
                target_type: row.target_type.as_deref().map(|s| s.parse()).transpose()?,
                target: row.target,
                featured_entity: row.featured_entity,
                title: row.title,
                content: row.content,
                url: row.url,
                start_date: row.start_date,
                end_date: row.end_date,
                active: row.active != 0,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    let total_count: i64 = if let Some(ref search) = query.search {
        let pattern = format!("%{}%", search);
        sqlx::query_scalar!("SELECT COUNT(*) FROM schedules WHERE title LIKE ?", pattern)
            .fetch_one(db())
            .await?
    } else {
        sqlx::query_scalar!("SELECT COUNT(*) FROM schedules")
            .fetch_one(db())
            .await?
    };

    let template = SchedulesContentTemplate {
        items,
        search_query: query.search.clone(),
        page,
        per_page: per_page as usize,
        total_count: total_count as usize,
    };

    Ok(template.render_once()?)
}

#[derive(sailfish::TemplateOnce)]
#[template(path = "admin/modals/new_schedule.stpl.html")]
struct NewScheduleForm {
    error_message: Option<String>,
}

#[handler]
async fn schedules_new(
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let form = NewScheduleForm {
        error_message: None,
    };

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(form.render_once()?))
}

#[derive(serde::Deserialize)]
struct CreateScheduleForm {
    title: String,
    schedule_type: String,
    target_type: Option<String>,
    target: Option<String>,
    featured_entity: Option<String>,
    content: Option<String>,
    url: Option<String>,
    start_date: String,
    end_date: String,
    active: Option<String>,
}

#[handler]
async fn schedules_create(
    Form(form): Form<CreateScheduleForm>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    use crate::peering::LoadStorePeered;
    use crate::schedule::{Schedule, ScheduleType, TargetType};

    match (async || -> Result<Ulid, anyhow::Error> {
        if form.title.trim().is_empty() {
            return Err(anyhow::anyhow!("Title is required"));
        }

        let schedule_type: ScheduleType = form.schedule_type.parse()?;
        let target_type: Option<TargetType> = form
            .target_type
            .as_ref()
            .and_then(|t| {
                if t.is_empty() {
                    None
                } else {
                    Some(t.parse().ok())
                }
            })
            .flatten();

        let target = form
            .target
            .as_ref()
            .and_then(|t| if t.is_empty() { None } else { Some(t.clone()) });

        let featured_entity = form
            .featured_entity
            .as_ref()
            .and_then(|t| if t.is_empty() { None } else { Some(t.clone()) });

        let mut schedule = Schedule::new(
            schedule_type,
            target_type,
            target,
            featured_entity,
            form.title.clone(),
            form.content,
            form.url,
            form.start_date,
            form.end_date,
        );

        // Override active status if checkbox was unchecked
        if form.active.is_none() {
            schedule.active = false;
        }

        let id = schedule.schedule_id;
        schedule.store().await?;

        // Log the activity
        if let Some(user_info) = &user_info {
            common::service::log_activity(
                None,
                "schedule-create",
                user_info.user_id,
                id,
                format!("type={} title={}", schedule_type, form.title),
            )
            .await?;
        }

        Ok(id)
    })()
    .await
    {
        Ok(_id) => {
            // Return OOB swap with refreshed list
            let content = schedules_search_impl(&SchedulesSearchQuery {
                search: None,
                page: Some(1),
            })
            .await?;

            let html = format!(
                r#"<div id="schedules-content" hx-swap-oob="innerHTML">{}</div>"#,
                content
            );

            Ok(poem::Response::builder()
                .content_type("text/html")
                .body(html))
        }
        Err(e) => {
            let form = NewScheduleForm {
                error_message: Some(e.to_string()),
            };

            Ok(poem::Response::builder()
                .content_type("text/html")
                .body(form.render_once()?))
        }
    }
}

#[derive(sailfish::TemplateOnce)]
#[template(path = "admin/edit_schedule_content.stpl.html")]
struct EditScheduleContent {
    schedule: crate::schedule::Schedule,
    error_message: Option<String>,
}

#[handler]
async fn schedules_edit(
    Path(id_str): Path<String>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    use crate::peering::LoadStorePeered;

    let schedule_id: Ulid = id_str.parse()?;
    let schedule = crate::schedule::Schedule::load(schedule_id).await?;

    let content = EditScheduleContent {
        schedule,
        error_message: None,
    };

    let layout = AdminLayout::new("Edit Schedule", content)
        .with_breadcrumb("Schedules", Some("/-admin/schedules"))
        .with_breadcrumb("Edit", None)
        .with_sidebar_active("schedules")
        .prepare(user_info)
        .await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(layout.render_once()?))
}

#[derive(serde::Deserialize)]
struct UpdateScheduleForm {
    title: String,
    schedule_type: String,
    target_type: Option<String>,
    target: Option<String>,
    featured_entity: Option<String>,
    content: Option<String>,
    url: Option<String>,
    start_date: String,
    end_date: String,
    active: Option<String>,
}

#[handler]
async fn schedules_update(
    Path(id_str): Path<String>,
    Form(form): Form<UpdateScheduleForm>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    use crate::peering::LoadStorePeered;
    use crate::schedule::{Schedule, ScheduleType, TargetType};

    let schedule_id: Ulid = id_str.parse()?;

    match (async || -> Result<(), anyhow::Error> {
        if form.title.trim().is_empty() {
            return Err(anyhow::anyhow!("Title is required"));
        }

        let schedule_type: ScheduleType = form.schedule_type.parse()?;
        let target_type: Option<TargetType> = form
            .target_type
            .as_ref()
            .and_then(|t| {
                if t.is_empty() {
                    None
                } else {
                    Some(t.parse().ok())
                }
            })
            .flatten();

        let target = form
            .target
            .as_ref()
            .and_then(|t| if t.is_empty() { None } else { Some(t.clone()) });

        let featured_entity = form
            .featured_entity
            .as_ref()
            .and_then(|t| if t.is_empty() { None } else { Some(t.clone()) });

        let active = form.active.is_some();

        let schedule = Schedule {
            schedule_id,
            schedule_type,
            target_type,
            target,
            featured_entity,
            title: form.title.clone(),
            content: form.content,
            url: form.url,
            start_date: form.start_date,
            end_date: form.end_date,
            active,
        };

        schedule.store().await?;

        // Log the activity
        if let Some(user_info) = &user_info {
            common::service::log_activity(
                None,
                "schedule-update",
                user_info.user_id,
                schedule_id,
                format!("type={} title={}", schedule_type, form.title),
            )
            .await?;
        }

        Ok(())
    })()
    .await
    {
        Ok(()) => {
            // Redirect back to schedules list
            Ok(poem::Response::builder()
                .status(poem::http::StatusCode::SEE_OTHER)
                .header("Location", "/-admin/schedules")
                .finish())
        }
        Err(e) => {
            // Reload the page with error message
            let schedule = Schedule::load(schedule_id).await?;
            let content = EditScheduleContent {
                schedule,
                error_message: Some(e.to_string()),
            };

            let layout = AdminLayout::new("Edit Schedule", content)
                .with_breadcrumb("Schedules", Some("/-admin/schedules"))
                .with_breadcrumb("Edit", None)
                .with_sidebar_active("schedules")
                .prepare(user_info)
                .await?;

            Ok(poem::Response::builder()
                .content_type("text/html")
                .body(layout.render_once()?))
        }
    }
}

#[handler]
async fn schedules_delete(
    Path(id_str): Path<String>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    use crate::peering::LoadStorePeered;

    let schedule_id: Ulid = id_str.parse()?;

    // Load schedule before deletion for logging
    let schedule = crate::schedule::Schedule::load(schedule_id).await.ok();

    // Use LoadStorePeered delete to properly notify peers
    crate::schedule::Schedule::delete(schedule_id).await?;

    // Log the activity
    if let Some(user_info) = &user_info {
        let extra = schedule
            .as_ref()
            .map(|s| format!("type={} title={}", s.schedule_type, s.title));
        common::service::log_activity(
            None,
            "schedule-delete",
            user_info.user_id,
            schedule_id,
            extra,
        )
        .await?;
    }

    // Check if there are any schedules left
    let exists: bool =
        sqlx::query_scalar!(r#"SELECT EXISTS(SELECT 1 FROM schedules) as "exists: bool""#)
            .fetch_one(db())
            .await?;

    let html = if !exists {
        // If no items left, return OOB swap with empty state
        let content = schedules_search_impl(&SchedulesSearchQuery {
            search: None,
            page: Some(1),
        })
        .await?;
        format!(
            r#"<div id="schedules-content" hx-swap-oob="outerHTML">{}</div>"#,
            content
        )
    } else {
        // Otherwise just return empty to remove the row
        String::new()
    };

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(html))
}

// Email admin pages

// Query parameters for email pages
#[derive(Debug, Deserialize)]
pub struct EmailPageQuery {
    page: Option<u32>,
    search: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct OneoffEmailPageQuery {
    page: Option<u32>,
    include_archived: Option<bool>,
}

// Stored emails list
#[derive(Debug, TemplateOnce)]
#[template(path = "admin/email_stored.stpl.html")]
pub struct EmailStoredContent {
    pub emails: Vec<common::message::email::StoredEmail>,
    pub current_page: u32,
    pub total: u32,
    pub has_previous: bool,
    pub has_next: bool,
    pub per_page: u32,
    pub start_item: u32,
    pub end_item: u32,
}

#[handler]
async fn email_stored(
    Query(query): Query<EmailPageQuery>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let page = query.page.unwrap_or(1);
    let per_page = 25;

    let request = common::message::email::ListStoredEmailsRequest {
        page,
        per_page,
        search: query.search,
    };

    let response: common::message::email::ListStoredEmailsResponse =
        common::service::request("emailer.stored.list", request)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to list stored emails: {}", e))?;

    let total_pages = (response.total + per_page - 1) / per_page;
    let has_previous = page > 1;
    let has_next = page < total_pages;
    let start_item = if response.total == 0 {
        0
    } else {
        ((page - 1) * per_page) + 1
    };
    let end_item = std::cmp::min(page * per_page, response.total);

    let content = EmailStoredContent {
        emails: response.emails,
        current_page: page,
        total: response.total,
        has_previous,
        has_next,
        per_page,
        start_item,
        end_item,
    };

    AdminLayout::new("Stored Emails", content)
        .with_breadcrumb("Stored Emails", None)
        .with_sidebar_active("email-stored")
        .into_response(user_info)
        .await
}

// Oneoff emails list
#[derive(Debug, TemplateOnce)]
#[template(path = "admin/email_oneoff.stpl.html")]
pub struct EmailOneoffContent {
    pub emails: Vec<common::message::email::OneoffEmail>,
    pub current_page: u32,
    pub total: u32,
    pub has_previous: bool,
    pub has_next: bool,
    pub per_page: u32,
    pub start_item: u32,
    pub end_item: u32,
    pub include_archived: bool,
}

#[handler]
async fn email_oneoff(
    Query(query): Query<OneoffEmailPageQuery>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let page = query.page.unwrap_or(1);
    let per_page = 25;
    let include_archived = query.include_archived.unwrap_or(false);

    let request = common::message::email::ListOneoffEmailsRequest {
        page,
        per_page,
        include_archived,
    };

    let response: common::message::email::ListOneoffEmailsResponse =
        common::service::request("emailer.oneoff.list", request)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to list oneoff emails: {}", e))?;

    let total_pages = (response.total + per_page - 1) / per_page;
    let has_previous = page > 1;
    let has_next = page < total_pages;
    let start_item = if response.total == 0 {
        0
    } else {
        ((page - 1) * per_page) + 1
    };
    let end_item = std::cmp::min(page * per_page, response.total);

    let content = EmailOneoffContent {
        emails: response.emails,
        current_page: page,
        total: response.total,
        has_previous,
        has_next,
        per_page,
        start_item,
        end_item,
        include_archived,
    };

    AdminLayout::new("One-off Emails", content)
        .with_breadcrumb("One-off Emails", None)
        .with_sidebar_active("email-oneoff")
        .into_response(user_info)
        .await
}

// Stored email new page
#[derive(Debug, TemplateOnce)]
#[template(path = "admin/pages/new_stored_email.stpl.html")]
pub struct NewStoredEmailPage {
    pub error_message: Option<String>,
    pub datasets: Vec<common::message::email::DatasetInfo>,
}

#[handler]
async fn email_stored_new(
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    // Fetch available datasets
    let datasets_response =
        common::service::request::<common::message::email::ListDatasetsResponse>(
            "emailer.datasets.list",
            common::message::email::ListDatasetsRequest {},
        )
        .await
        .unwrap_or(common::message::email::ListDatasetsResponse { datasets: vec![] });

    let content = NewStoredEmailPage {
        error_message: None,
        datasets: datasets_response.datasets,
    };
    AdminLayout::new("Create Stored Email", content)
        .with_breadcrumb("Stored Emails", Some("/-admin/email/stored"))
        .with_breadcrumb("Create", None)
        .with_sidebar_active("email-stored")
        .with_quill()
        .into_response(user_info)
        .await
}

// Stored email create handler
#[derive(Debug, Deserialize)]
pub struct CreateStoredEmailForm {
    slug: String,
    subject: String,
    body_delta: String,
    #[serde(default, deserialize_with = "deserialize_string_or_vec")]
    datasets: Vec<String>,
}

#[handler]
async fn email_stored_create(
    Form(form): Form<CreateStoredEmailForm>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let request = common::message::email::CreateStoredEmailRequest {
        slug: form.slug,
        subject: form.subject,
        body_delta: form.body_delta,
        datasets: form.datasets,
    };

    match common::service::request::<common::message::email::CreateStoredEmailResponse>(
        "emailer.stored.create",
        request,
    )
    .await
    {
        Ok(_response) => Ok(poem::Response::builder()
            .status(poem::http::StatusCode::SEE_OTHER)
            .header("Location", "/-admin/email/stored")
            .finish()),
        Err(e) => {
            // Re-fetch datasets for error display
            let datasets_response =
                common::service::request::<common::message::email::ListDatasetsResponse>(
                    "emailer.datasets.list",
                    common::message::email::ListDatasetsRequest {},
                )
                .await
                .unwrap_or(common::message::email::ListDatasetsResponse { datasets: vec![] });

            let content = NewStoredEmailPage {
                error_message: Some(format!("Failed to create stored email: {}", e)),
                datasets: datasets_response.datasets,
            };
            AdminLayout::new("Create Stored Email", content)
                .with_breadcrumb("Stored Emails", Some("/-admin/email/stored"))
                .with_breadcrumb("Create", None)
                .with_sidebar_active("email-stored")
                .into_response(user_info)
                .await
        }
    }
}

// Stored email edit page
#[derive(Debug, TemplateOnce)]
#[template(path = "admin/pages/edit_stored_email.stpl.html")]
pub struct EditStoredEmailPage {
    pub stored_email_id: ulid::Ulid,
    pub slug: String,
    pub subject: String,
    pub body_delta: String,
    pub selected_datasets: Vec<String>,
    pub error_message: Option<String>,
    pub available_datasets: Vec<common::message::email::DatasetInfo>,
}

#[handler]
async fn email_stored_edit(
    Path(id): Path<String>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let stored_email_id = id
        .parse::<ulid::Ulid>()
        .map_err(|_| anyhow::anyhow!("Invalid stored email ID format"))?;

    let request = common::message::email::GetStoredEmailRequest { stored_email_id };

    // Fetch datasets
    let datasets_response =
        common::service::request::<common::message::email::ListDatasetsResponse>(
            "emailer.datasets.list",
            common::message::email::ListDatasetsRequest {},
        )
        .await
        .unwrap_or(common::message::email::ListDatasetsResponse { datasets: vec![] });

    match common::service::request::<common::message::email::GetStoredEmailResponse>(
        "emailer.stored.get",
        request,
    )
    .await
    {
        Ok(response) => {
            let email = response.email;
            let content = EditStoredEmailPage {
                stored_email_id: email.stored_email_id,
                slug: email.slug,
                subject: email.subject,
                body_delta: email.body_delta,
                selected_datasets: email.datasets,
                error_message: None,
                available_datasets: datasets_response.datasets,
            };
            AdminLayout::new("Edit Stored Email", content)
                .with_breadcrumb("Stored Emails", Some("/-admin/email/stored"))
                .with_breadcrumb("Edit", None)
                .with_sidebar_active("email-stored")
                .with_quill()
                .into_response(user_info)
                .await
        }
        Err(e) => Err(anyhow::anyhow!("Failed to load stored email: {}", e)),
    }
}

// Stored email update handler
#[derive(Debug, Deserialize)]
pub struct UpdateStoredEmailForm {
    action: String,
    slug: Option<String>,
    subject: Option<String>,
    body_delta: Option<String>,
    #[serde(default, deserialize_with = "deserialize_string_or_vec")]
    datasets: Vec<String>,
}

#[handler]
async fn email_stored_update(
    Path(id): Path<String>,
    Form(form): Form<UpdateStoredEmailForm>,
    Authenticated(_user_info): Authenticated,
    req: &poem::Request,
) -> Result<poem::Response, anyhow::Error> {
    let stored_email_id = id
        .parse::<ulid::Ulid>()
        .map_err(|_| anyhow::anyhow!("Invalid stored email ID format"))?;

    match form.action.as_str() {
        "delete" => {
            let request = common::message::email::DeleteStoredEmailRequest { stored_email_id };
            common::service::request::<common::message::email::DeleteStoredEmailResponse>(
                "emailer.stored.delete",
                request,
            )
            .await?;
        }
        "save" => {
            let request = common::message::email::UpdateStoredEmailRequest {
                stored_email_id,
                slug: form.slug,
                subject: form.subject,
                body_delta: form.body_delta,
                datasets: if form.datasets.is_empty() {
                    None
                } else {
                    Some(form.datasets)
                },
            };
            common::service::request::<common::message::email::UpdateStoredEmailResponse>(
                "emailer.stored.update",
                request,
            )
            .await?;
        }
        _ => return Err(anyhow::anyhow!("Invalid action")),
    }

    // If this is an HTMX request, just return 200 OK
    if req.headers().get("HX-Request").is_some() {
        Ok(poem::Response::builder()
            .status(poem::http::StatusCode::OK)
            .finish())
    } else {
        // Otherwise redirect to the list page
        Ok(poem::Response::builder()
            .status(poem::http::StatusCode::SEE_OTHER)
            .header("Location", "/-admin/email/stored")
            .finish())
    }
}

#[handler]
async fn email_stored_send(
    Path(id): Path<String>,
    QsForm(form): QsForm<SendOneoffEmailForm>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let stored_email_id = id
        .parse::<ulid::Ulid>()
        .map_err(|_| anyhow::anyhow!("Invalid stored email ID format"))?;

    // Get the email template
    let email_response: common::message::email::GetStoredEmailResponse = common::service::request(
        "emailer.stored.get",
        common::message::email::GetStoredEmailRequest { stored_email_id },
    )
    .await?;

    // Build recipients from parallel arrays
    if form.recipient_types.len() != form.recipient_values.len() {
        return Err(anyhow::anyhow!("Mismatched recipient data"));
    }

    if form.recipient_types.is_empty() {
        return Err(anyhow::anyhow!("No recipients specified"));
    }

    let mut recipients = Vec::new();
    for (recipient_type, recipient_value) in form
        .recipient_types
        .iter()
        .zip(form.recipient_values.iter())
    {
        let recipient = match recipient_type.as_str() {
            "Email" => common::message::email::EmailRecipient::Email(recipient_value.clone()),
            "User" => {
                let user_id = recipient_value
                    .parse::<ulid::Ulid>()
                    .map_err(|_| anyhow::anyhow!("Invalid user ID"))?;
                common::message::email::EmailRecipient::UserId(user_id)
            }
            "UserSet" => common::message::email::EmailRecipient::UserSet {
                slug: recipient_value.clone(),
            },
            "AddressSet" => common::message::email::EmailRecipient::AddressSet {
                slug: recipient_value.clone(),
            },
            _ => return Err(anyhow::anyhow!("Invalid recipient type")),
        };
        recipients.push(recipient);
    }

    // Convert delta to HTML
    let template_html = common::quill::delta_to_html(&email_response.email.body_delta)?;

    // Send the email
    let _response: common::message::email::SendEmailResponse = common::service::request(
        "emailer.send",
        common::message::email::SendEmailRequest {
            template_html,
            subject: email_response.email.subject,
            recipients,
            data_sets: email_response.email.datasets,
            additional_data: None,
        },
    )
    .await?;

    Ok(poem::Response::builder()
        .status(poem::http::StatusCode::SEE_OTHER)
        .header("Location", format!("/-admin/email/stored/{}", id))
        .finish())
}

// Oneoff email new page
#[derive(Debug, TemplateOnce)]
#[template(path = "admin/pages/new_oneoff_email.stpl.html")]
pub struct NewOneoffEmailPage {
    pub error_message: Option<String>,
    pub datasets: Vec<common::message::email::DatasetInfo>,
}

#[handler]
async fn email_oneoff_new(
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    // Fetch available datasets
    let datasets_response =
        common::service::request::<common::message::email::ListDatasetsResponse>(
            "emailer.datasets.list",
            common::message::email::ListDatasetsRequest {},
        )
        .await
        .unwrap_or(common::message::email::ListDatasetsResponse { datasets: vec![] });

    let content = NewOneoffEmailPage {
        error_message: None,
        datasets: datasets_response.datasets,
    };
    AdminLayout::new("Create One-off Email", content)
        .with_breadcrumb("One-off Emails", Some("/-admin/email/oneoff"))
        .with_breadcrumb("Create", None)
        .with_sidebar_active("email-oneoff")
        .with_quill()
        .into_response(user_info)
        .await
}

// Oneoff email create handler
#[derive(Debug, Deserialize)]
pub struct CreateOneoffEmailForm {
    subject: String,
    body_delta: String,
    #[serde(default, deserialize_with = "deserialize_string_or_vec")]
    datasets: Vec<String>,
}

#[handler]
async fn email_oneoff_create(
    Form(form): Form<CreateOneoffEmailForm>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let request = common::message::email::CreateOneoffEmailRequest {
        subject: form.subject,
        body_delta: form.body_delta,
        datasets: form.datasets,
    };

    match common::service::request::<common::message::email::CreateOneoffEmailResponse>(
        "emailer.oneoff.create",
        request,
    )
    .await
    {
        Ok(_response) => Ok(poem::Response::builder()
            .status(poem::http::StatusCode::SEE_OTHER)
            .header("Location", "/-admin/email/oneoff")
            .finish()),
        Err(e) => {
            // Re-fetch datasets for error display
            let datasets_response =
                common::service::request::<common::message::email::ListDatasetsResponse>(
                    "emailer.datasets.list",
                    common::message::email::ListDatasetsRequest {},
                )
                .await
                .unwrap_or(common::message::email::ListDatasetsResponse { datasets: vec![] });

            let content = NewOneoffEmailPage {
                error_message: Some(format!("Failed to create one-off email: {}", e)),
                datasets: datasets_response.datasets,
            };
            AdminLayout::new("Create One-off Email", content)
                .with_breadcrumb("One-off Emails", Some("/-admin/email/oneoff"))
                .with_breadcrumb("Create", None)
                .with_sidebar_active("email-oneoff")
                .into_response(user_info)
                .await
        }
    }
}

// Oneoff email edit page
#[derive(Debug, TemplateOnce)]
#[template(path = "admin/pages/edit_oneoff_email.stpl.html")]
pub struct EditOneoffEmailPage {
    pub oneoff_email_id: ulid::Ulid,
    pub subject: String,
    pub body_delta: String,
    pub archived: bool,
    pub selected_datasets: Vec<String>,
    pub error_message: Option<String>,
    pub available_datasets: Vec<common::message::email::DatasetInfo>,
}

#[handler]
async fn email_oneoff_edit(
    Path(id): Path<String>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let oneoff_email_id = id
        .parse::<ulid::Ulid>()
        .map_err(|_| anyhow::anyhow!("Invalid oneoff email ID format"))?;

    let request = common::message::email::GetOneoffEmailRequest { oneoff_email_id };

    // Fetch datasets
    let datasets_response =
        common::service::request::<common::message::email::ListDatasetsResponse>(
            "emailer.datasets.list",
            common::message::email::ListDatasetsRequest {},
        )
        .await
        .unwrap_or(common::message::email::ListDatasetsResponse { datasets: vec![] });

    match common::service::request::<common::message::email::GetOneoffEmailResponse>(
        "emailer.oneoff.get",
        request,
    )
    .await
    {
        Ok(response) => {
            let email = response.email;
            let content = EditOneoffEmailPage {
                oneoff_email_id: email.oneoff_email_id,
                subject: email.subject,
                body_delta: email.body_delta,
                archived: email.archived,
                selected_datasets: email.datasets,
                error_message: None,
                available_datasets: datasets_response.datasets,
            };
            AdminLayout::new("Edit One-off Email", content)
                .with_breadcrumb("One-off Emails", Some("/-admin/email/oneoff"))
                .with_breadcrumb("Edit", None)
                .with_sidebar_active("email-oneoff")
                .with_quill()
                .into_response(user_info)
                .await
        }
        Err(e) => Err(anyhow::anyhow!("Failed to load one-off email: {}", e)),
    }
}

// Oneoff email update handler
#[derive(Debug, Deserialize)]
pub struct UpdateOneoffEmailForm {
    action: String,
    subject: Option<String>,
    body_delta: Option<String>,
    #[serde(default, deserialize_with = "deserialize_string_or_vec")]
    datasets: Vec<String>,
}

#[handler]
async fn email_oneoff_update(
    Path(id): Path<String>,
    Form(form): Form<UpdateOneoffEmailForm>,
    Authenticated(_user_info): Authenticated,
    req: &poem::Request,
) -> Result<poem::Response, anyhow::Error> {
    let oneoff_email_id = id
        .parse::<ulid::Ulid>()
        .map_err(|_| anyhow::anyhow!("Invalid oneoff email ID format"))?;

    match form.action.as_str() {
        "delete" => {
            let request = common::message::email::DeleteOneoffEmailRequest { oneoff_email_id };
            common::service::request::<common::message::email::DeleteOneoffEmailResponse>(
                "emailer.oneoff.delete",
                request,
            )
            .await?;
        }
        "archive" => {
            let request = common::message::email::ArchiveOneoffEmailRequest {
                oneoff_email_id,
                archived: true,
            };
            common::service::request::<common::message::email::ArchiveOneoffEmailResponse>(
                "emailer.oneoff.archive",
                request,
            )
            .await?;
        }
        "unarchive" => {
            let request = common::message::email::ArchiveOneoffEmailRequest {
                oneoff_email_id,
                archived: false,
            };
            common::service::request::<common::message::email::ArchiveOneoffEmailResponse>(
                "emailer.oneoff.archive",
                request,
            )
            .await?;
        }
        "save" => {
            let request = common::message::email::UpdateOneoffEmailRequest {
                oneoff_email_id,
                subject: form.subject,
                body_delta: form.body_delta,
                datasets: if form.datasets.is_empty() {
                    None
                } else {
                    Some(form.datasets)
                },
            };
            common::service::request::<common::message::email::UpdateOneoffEmailResponse>(
                "emailer.oneoff.update",
                request,
            )
            .await?;
        }
        _ => return Err(anyhow::anyhow!("Invalid action")),
    }

    // If this is an HTMX request, just return 200 OK
    if req.headers().get("HX-Request").is_some() {
        Ok(poem::Response::builder()
            .status(poem::http::StatusCode::OK)
            .finish())
    } else {
        // Otherwise redirect to the list page
        Ok(poem::Response::builder()
            .status(poem::http::StatusCode::SEE_OTHER)
            .header("Location", "/-admin/email/oneoff")
            .finish())
    }
}

#[derive(Debug, Deserialize)]
struct SendOneoffEmailForm {
    #[serde(default)]
    recipient_types: Vec<String>,
    #[serde(default)]
    recipient_values: Vec<String>,
}

#[handler]
async fn email_oneoff_send(
    Path(id): Path<String>,
    QsForm(form): QsForm<SendOneoffEmailForm>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let oneoff_email_id = id
        .parse::<ulid::Ulid>()
        .map_err(|_| anyhow::anyhow!("Invalid oneoff email ID format"))?;

    // Get the email template
    let email_response: common::message::email::GetOneoffEmailResponse = common::service::request(
        "emailer.oneoff.get",
        common::message::email::GetOneoffEmailRequest { oneoff_email_id },
    )
    .await?;

    // Build recipients from parallel arrays
    if form.recipient_types.len() != form.recipient_values.len() {
        return Err(anyhow::anyhow!("Mismatched recipient data"));
    }

    if form.recipient_types.is_empty() {
        return Err(anyhow::anyhow!("No recipients specified"));
    }

    let mut recipients = Vec::new();
    for (recipient_type, recipient_value) in form
        .recipient_types
        .iter()
        .zip(form.recipient_values.iter())
    {
        let recipient = match recipient_type.as_str() {
            "Email" => common::message::email::EmailRecipient::Email(recipient_value.clone()),
            "User" => {
                let user_id = recipient_value
                    .parse::<ulid::Ulid>()
                    .map_err(|_| anyhow::anyhow!("Invalid user ID"))?;
                common::message::email::EmailRecipient::UserId(user_id)
            }
            "UserSet" => common::message::email::EmailRecipient::UserSet {
                slug: recipient_value.clone(),
            },
            "AddressSet" => common::message::email::EmailRecipient::AddressSet {
                slug: recipient_value.clone(),
            },
            _ => return Err(anyhow::anyhow!("Invalid recipient type")),
        };
        recipients.push(recipient);
    }

    // Convert delta to HTML
    let template_html = common::quill::delta_to_html(&email_response.email.body_delta)?;

    // Send the email
    let _response: common::message::email::SendEmailResponse = common::service::request(
        "emailer.send",
        common::message::email::SendEmailRequest {
            template_html,
            subject: email_response.email.subject,
            recipients,
            data_sets: email_response.email.datasets,
            additional_data: None,
        },
    )
    .await?;

    Ok(poem::Response::builder()
        .status(poem::http::StatusCode::SEE_OTHER)
        .header("Location", format!("/-admin/email/oneoff/{}", id))
        .finish())
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/email_triggers.stpl.html")]
pub struct EmailTriggersContent {
    pub triggers: Vec<common::message::email::TriggerInfo>,
    pub available_trigger_events: Vec<(String, String, String)>, // (name, description, category)
    pub stored_emails: std::collections::HashMap<ulid::Ulid, (String, String)>, // id -> (slug, subject)
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/partials/trigger_row.stpl.html")]
pub struct TriggerRow {
    pub trigger: common::message::email::TriggerInfo,
    pub stored_email: Option<(String, String)>, // (slug, subject)
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/partials/trigger_list.stpl.html")]
pub struct TriggerList {
    pub triggers: Vec<common::message::email::TriggerInfo>,
    pub stored_emails: std::collections::HashMap<ulid::Ulid, (String, String)>, // id -> (slug, subject)
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/modals/edit_trigger.stpl.html")]
pub struct EditTriggerModal {
    pub trigger: common::message::email::TriggerInfo,
    pub available_trigger_events: Vec<(String, String, String)>, // (name, description, category)
    pub stored_email: Option<(String, String)>,                  // (slug, subject)
}

#[handler]
async fn email_triggers(
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    use common::message::email::{
        GetStoredEmailRequest, GetStoredEmailResponse, ListTriggersRequest, ListTriggersResponse,
    };
    use common::service::{TriggerEvent, request};
    use std::collections::HashMap;
    use strum::IntoEnumIterator;

    // Get all configured triggers
    let triggers_response: ListTriggersResponse = request(
        "emailer.triggers.list",
        ListTriggersRequest { active_only: false },
    )
    .await?;

    // Get all available trigger events from the enum
    let available_trigger_events: Vec<(String, String, String)> = TriggerEvent::iter()
        .map(|trigger| {
            (
                trigger.as_ref().to_string(),
                trigger.description().to_string(),
                trigger.category().to_string(),
            )
        })
        .collect();

    // Fetch stored email info for triggers (only the ones referenced)
    let mut stored_emails = HashMap::new();
    for trigger in &triggers_response.triggers {
        if !stored_emails.contains_key(&trigger.stored_email_id) {
            if let Ok(email_response) = request::<GetStoredEmailResponse>(
                "emailer.stored.get",
                GetStoredEmailRequest {
                    stored_email_id: trigger.stored_email_id,
                },
            )
            .await
            {
                stored_emails.insert(
                    email_response.email.stored_email_id,
                    (email_response.email.slug, email_response.email.subject),
                );
            }
        }
    }

    let content = EmailTriggersContent {
        triggers: triggers_response.triggers,
        available_trigger_events,
        stored_emails,
    };

    AdminLayout::new("Email Triggers", content)
        .with_breadcrumb("Email Triggers", None)
        .with_sidebar_active("email-triggers")
        .into_response(user_info)
        .await
}

// Stored email autocomplete for trigger forms
#[derive(Debug, Deserialize)]
pub struct StoredEmailAutocompleteQuery {
    q: Option<String>,
}

#[handler]
async fn email_stored_autocomplete(
    Query(query): Query<StoredEmailAutocompleteQuery>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    use common::message::email::{ListStoredEmailsRequest, ListStoredEmailsResponse};
    use common::service::request;

    let search = query.q.filter(|s| !s.is_empty());

    let response: ListStoredEmailsResponse = request(
        "emailer.stored.list",
        ListStoredEmailsRequest {
            page: 1,
            per_page: 20,
            search,
        },
    )
    .await?;

    if response.emails.is_empty() {
        return Ok(poem::Response::builder()
            .content_type("text/html")
            .body(r#"<div class="hidden"></div>"#));
    }

    let results = response
        .emails
        .iter()
        .map(|email| {
            format!(
                r#"<div class="cursor-pointer px-3 py-2 hover:bg-gray-100 border-b border-gray-100 last:border-b-0"
                     hx-on:click="
                         let widget = htmx.closest(this, '[data-widget=stored-email-autocomplete]');
                         htmx.find(widget, '[data-part=stored-email-id]').value = '{}';
                         htmx.find(widget, '[data-part=stored-email-search]').value = '{} - {}';
                         htmx.find(widget, '[data-part=results]').classList.add('hidden');
                     ">
                    <div class="font-medium text-gray-900">{}</div>
                    <div class="text-sm text-gray-500">{}</div>
                </div>"#,
                email.stored_email_id,
                email.slug,
                email.subject,
                email.slug,
                email.subject
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let html = format!(
        r#"<div data-part="results" class="absolute z-10 w-full mt-1 bg-white border border-gray-300 rounded-md shadow-lg max-h-60 overflow-auto">
            {}
        </div>"#,
        results
    );

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(html))
}

// Create email trigger
#[derive(Debug, Deserialize)]
pub struct CreateTriggerForm {
    trigger_name: String,
    stored_email_id: String,
    action_type: String,
    delay_seconds: Option<String>,
    cancellation_trigger_name: Option<String>,
}

#[handler]
async fn email_trigger_create(
    Form(form): Form<CreateTriggerForm>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    use common::message::email::{
        CreateTriggerRequest, CreateTriggerResponse, GetStoredEmailRequest, GetStoredEmailResponse,
        ListTriggersRequest, ListTriggersResponse, TriggerActionType,
    };
    use common::service::request;
    use std::collections::HashMap;

    let stored_email_id = form
        .stored_email_id
        .parse::<ulid::Ulid>()
        .map_err(|_| anyhow::anyhow!("Invalid stored email ID"))?;

    let action_type = TriggerActionType::from_str(&form.action_type)
        .ok_or_else(|| anyhow::anyhow!("Invalid action type"))?;

    let delay_seconds = if action_type == TriggerActionType::Delayed {
        Some(
            form.delay_seconds
                .and_then(|s| s.parse::<i64>().ok())
                .ok_or_else(|| anyhow::anyhow!("Delay seconds required for delayed triggers"))?,
        )
    } else {
        None
    };

    let cancellation_trigger_name = form.cancellation_trigger_name.filter(|s| !s.is_empty());

    let create_request = CreateTriggerRequest {
        trigger_name: form.trigger_name,
        stored_email_id,
        action_type,
        delay_seconds,
        cancellation_trigger_name,
    };

    let _response: CreateTriggerResponse =
        request("emailer.triggers.create", create_request).await?;

    // Fetch updated triggers list
    let triggers_response: ListTriggersResponse = request(
        "emailer.triggers.list",
        ListTriggersRequest { active_only: false },
    )
    .await?;

    // Fetch stored email info for triggers
    let mut stored_emails = HashMap::new();
    for trigger in &triggers_response.triggers {
        if !stored_emails.contains_key(&trigger.stored_email_id) {
            if let Ok(email_response) = request::<GetStoredEmailResponse>(
                "emailer.stored.get",
                GetStoredEmailRequest {
                    stored_email_id: trigger.stored_email_id,
                },
            )
            .await
            {
                stored_emails.insert(
                    trigger.stored_email_id,
                    (email_response.email.slug, email_response.email.subject),
                );
            }
        }
    }

    // Render the updated list
    let list = TriggerList {
        triggers: triggers_response.triggers,
        stored_emails,
    };

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(list.render_once()?))
}

// Get edit trigger form
#[handler]
async fn email_trigger_edit(
    Path(id): Path<String>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    use common::message::email::{
        GetStoredEmailRequest, GetStoredEmailResponse, GetTriggerRequest, GetTriggerResponse,
    };
    use common::service::{TriggerEvent, request};
    use strum::IntoEnumIterator;

    let trigger_id = id
        .parse::<ulid::Ulid>()
        .map_err(|_| anyhow::anyhow!("Invalid trigger ID"))?;

    // Fetch the trigger
    let trigger_response: GetTriggerResponse =
        request("emailer.triggers.get", GetTriggerRequest { trigger_id }).await?;

    let trigger = trigger_response.trigger;

    // Get all available trigger events
    let available_trigger_events: Vec<(String, String, String)> = TriggerEvent::iter()
        .map(|trigger| {
            (
                trigger.as_ref().to_string(),
                trigger.description().to_string(),
                trigger.category().to_string(),
            )
        })
        .collect();

    // Fetch stored email info
    let stored_email = if let Ok(email_response) = request::<GetStoredEmailResponse>(
        "emailer.stored.get",
        GetStoredEmailRequest {
            stored_email_id: trigger.stored_email_id,
        },
    )
    .await
    {
        Some((email_response.email.slug, email_response.email.subject))
    } else {
        None
    };

    // Render the modal
    let modal = EditTriggerModal {
        trigger,
        available_trigger_events,
        stored_email,
    };

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(modal.render_once()?))
}

// Update email trigger
#[derive(Debug, Deserialize)]
pub struct UpdateTriggerForm {
    stored_email_id: Option<String>,
    action_type: Option<String>,
    delay_seconds: Option<String>,
    cancellation_trigger_name: Option<String>,
    active: Option<String>,
}

#[handler]
async fn email_trigger_update(
    Path(id): Path<String>,
    Form(form): Form<UpdateTriggerForm>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    use common::message::email::{
        GetStoredEmailRequest, GetStoredEmailResponse, GetTriggerRequest, GetTriggerResponse,
        TriggerActionType, UpdateTriggerRequest, UpdateTriggerResponse,
    };
    use common::service::request;

    let trigger_id = match id.parse::<ulid::Ulid>() {
        Ok(id) => id,
        Err(_) => {
            return Ok(poem::Response::builder()
                .content_type("text/html")
                .body(r#"<div id="edit-trigger-error" class="bg-red-50 border border-red-200 text-red-700 px-4 py-3 rounded mb-4" hx-swap-oob="true">Error: Invalid trigger ID</div>"#.to_string()));
        }
    };

    // If stored_email_id is provided (even as empty string), validate it
    let stored_email_id = if let Some(ref email_id_str) = form.stored_email_id {
        if email_id_str.is_empty() {
            return Ok(poem::Response::builder()
                .content_type("text/html")
                .body(r#"<div id="edit-trigger-error" class="bg-red-50 border border-red-200 text-red-700 px-4 py-3 rounded mb-4" hx-swap-oob="true">Error: Please select an email template</div>"#.to_string()));
        }
        match email_id_str.parse::<ulid::Ulid>() {
            Ok(id) => Some(id),
            Err(_) => {
                return Ok(poem::Response::builder()
                    .content_type("text/html")
                    .body(r#"<div id="edit-trigger-error" class="bg-red-50 border border-red-200 text-red-700 px-4 py-3 rounded mb-4" hx-swap-oob="true">Error: Invalid stored email ID</div>"#.to_string()));
            }
        }
    } else {
        None
    };

    let action_type = form
        .action_type
        .and_then(|s| TriggerActionType::from_str(&s));

    let delay_seconds = form.delay_seconds.and_then(|s| s.parse::<i64>().ok());

    let cancellation_trigger_name = form.cancellation_trigger_name.filter(|s| !s.is_empty());

    let active = form.active.map(|s| s == "true" || s == "1");

    let update_request = UpdateTriggerRequest {
        trigger_id,
        stored_email_id,
        action_type,
        delay_seconds,
        cancellation_trigger_name,
        active,
    };

    let update_result: Result<UpdateTriggerResponse, _> =
        request("emailer.triggers.update", update_request).await;

    if let Err(err) = update_result {
        return Ok(poem::Response::builder()
            .content_type("text/html")
            .body(format!(r#"<div id="edit-trigger-error" class="bg-red-50 border border-red-200 text-red-700 px-4 py-3 rounded mb-4" hx-swap-oob="true">Error: {}</div>"#, err)));
    }

    // Fetch the updated trigger to return the row HTML
    let trigger_response: GetTriggerResponse =
        request("emailer.triggers.get", GetTriggerRequest { trigger_id }).await?;

    let trigger = trigger_response.trigger;

    // Fetch stored email info
    let stored_email = if let Ok(email_response) = request::<GetStoredEmailResponse>(
        "emailer.stored.get",
        GetStoredEmailRequest {
            stored_email_id: trigger.stored_email_id,
        },
    )
    .await
    {
        Some((email_response.email.slug, email_response.email.subject))
    } else {
        None
    };

    // Render the updated table row using template
    let row = TriggerRow {
        trigger: trigger.clone(),
        stored_email,
    };

    let row_html = row.render_once()?;
    let row_with_oob = row_html.replace(
        &format!(r#"id="trigger-row-{}""#, trigger.trigger_id),
        &format!(
            r#"id="trigger-row-{}" hx-swap-oob="true""#,
            trigger.trigger_id
        ),
    );

    let body = format!(
        r#"{}<div id="edit-trigger-dialog" hx-swap-oob="delete"></div>"#,
        row_with_oob
    );

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(body))
}

// Delete email trigger
#[handler]
async fn email_trigger_delete(
    Path(id): Path<String>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    use common::message::email::{DeleteTriggerRequest, DeleteTriggerResponse};
    use common::service::request;

    let trigger_id = id
        .parse::<ulid::Ulid>()
        .map_err(|_| anyhow::anyhow!("Invalid trigger ID"))?;

    let _response: DeleteTriggerResponse = request(
        "emailer.triggers.delete",
        DeleteTriggerRequest { trigger_id },
    )
    .await?;

    // Return empty response - HTMX will delete the row with hx-swap="delete"
    Ok(poem::Response::builder()
        .status(poem::http::StatusCode::OK)
        .body(""))
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/email_queue.stpl.html")]
pub struct EmailQueueContent {
    queue_status: common::message::email::QueueStatusResponse,
    sendings: Vec<common::message::email::SendingMetrics>,
    total: u32,
}

#[handler]
async fn email_queue(
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    use common::message::email::{ListSendingsRequest, ListSendingsResponse, QueueStatusResponse};
    use common::service::request;

    // Get queue status
    let queue_status: QueueStatusResponse = request("emailer.analytics.queue_status", ()).await?;

    // Get recent mass sendings (2+ targets, first page)
    let sendings_response: ListSendingsResponse = request(
        "emailer.analytics.list_sendings",
        ListSendingsRequest {
            page: 1,
            per_page: 20,
            min_queued: Some(2),
        },
    )
    .await?;

    let content = EmailQueueContent {
        queue_status,
        sendings: sendings_response.sendings,
        total: sendings_response.total,
    };

    AdminLayout::new("Email Queue", content)
        .with_breadcrumb("Email Queue", None)
        .with_sidebar_active("email-queue")
        .into_response(user_info)
        .await
}

#[handler]
async fn email_queue_content(
    Authenticated(_user_info): Authenticated,
) -> Result<impl poem::IntoResponse, anyhow::Error> {
    use common::message::email::{ListSendingsRequest, ListSendingsResponse, QueueStatusResponse};
    use common::service::request;

    // Get queue status
    let queue_status: QueueStatusResponse = request("emailer.analytics.queue_status", ()).await?;

    // Get recent mass sendings (2+ targets, first page)
    let sendings_response: ListSendingsResponse = request(
        "emailer.analytics.list_sendings",
        ListSendingsRequest {
            page: 1,
            per_page: 20,
            min_queued: Some(2),
        },
    )
    .await?;

    let content = EmailQueueContent {
        queue_status,
        sendings: sendings_response.sendings,
        total: sendings_response.total,
    }
    .render_once()?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(content))
}

// Helper to deserialize either a single string or array of strings
fn deserialize_string_or_vec<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Deserialize;

    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StringOrVec {
        String(String),
        Vec(Vec<String>),
    }

    match StringOrVec::deserialize(deserializer)? {
        StringOrVec::String(s) => Ok(vec![s]),
        StringOrVec::Vec(v) => Ok(v),
    }
}

// Custom form extractor using serde_qs to support array notation (field[])
pub struct QsForm<T>(pub T);

impl<'a, T: serde::de::DeserializeOwned> FromRequest<'a> for QsForm<T> {
    async fn from_request(_req: &'a Request, body: &mut RequestBody) -> PoemResult<Self> {
        let data = body.take()?.into_vec().await?;
        let config = serde_qs::Config::new(10, false); // max_depth=10, strict=false
        let form = config.deserialize_bytes(&data).map_err(|e| {
            poem::Error::from_string(
                format!("Failed to parse form data: {}", e),
                poem::http::StatusCode::BAD_REQUEST,
            )
        })?;
        Ok(QsForm(form))
    }
}

// HTMX endpoint to fetch merge fields HTML fragment for selected datasets
#[derive(Debug, Deserialize)]
pub struct MergeFieldsQuery {
    #[serde(default, deserialize_with = "deserialize_string_or_vec")]
    datasets: Vec<String>,
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/partials/merge_fields.stpl.html")]
pub struct MergeFieldsPartial {
    pub fields: Vec<common::message::email::VariableInfo>,
}

#[handler]
async fn email_merge_fields(
    Query(query): Query<MergeFieldsQuery>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let mut all_fields = Vec::new();

    // Fetch variables for each selected dataset
    for dataset_name in query.datasets {
        let request = common::message::email::GetDatasetVariablesRequest {
            dataset_name: dataset_name.clone(),
        };

        if let Ok(response) = common::service::request::<
            common::message::email::GetDatasetVariablesResponse,
        >("emailer.datasets.variables", request)
        .await
        {
            all_fields.extend(response.variables);
        }
    }

    let content = MergeFieldsPartial { fields: all_fields };
    let html = content.render_once()?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(html))
}

// API endpoint to fetch dataset variables
#[handler]
async fn email_dataset_variables(
    Path(dataset_name): Path<String>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let request = common::message::email::GetDatasetVariablesRequest { dataset_name };

    match common::service::request::<common::message::email::GetDatasetVariablesResponse>(
        "emailer.datasets.variables",
        request,
    )
    .await
    {
        Ok(response) => {
            let json = serde_json::to_string(&response.variables)?;
            Ok(poem::Response::builder()
                .content_type("application/json")
                .body(json))
        }
        Err(e) => Err(anyhow::anyhow!("Failed to fetch dataset variables: {}", e)),
    }
}

// ============================================================================
// Address Sets
// ============================================================================

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/address_sets.stpl.html")]
pub struct AddressSetsContent {
    pub address_sets: Vec<common::message::user::AddressSetInfo>,
}

#[handler]
async fn address_sets_list(
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let request = common::message::user::AddressSetListRequest {};

    let response: common::message::user::AddressSetListResponse =
        common::service::request("user.address_sets.list", request)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to list address sets: {}", e))?;

    let content = AddressSetsContent {
        address_sets: response.address_sets,
    };

    AdminLayout::new("Address Sets", content)
        .with_breadcrumb("Address Sets", None)
        .with_sidebar_active("email-address-sets")
        .into_response(user_info)
        .await
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/pages/new_address_set.stpl.html")]
pub struct NewAddressSetPage {
    pub error_message: Option<String>,
}

#[handler]
async fn address_sets_new(
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let content = NewAddressSetPage {
        error_message: None,
    };

    AdminLayout::new("New Address Set", content)
        .with_breadcrumb("Address Sets", Some("/-admin/email/address-sets"))
        .with_breadcrumb("New", None)
        .with_sidebar_active("email-address-sets")
        .into_response(user_info)
        .await
}

#[derive(Debug, Deserialize)]
pub struct CreateAddressSetForm {
    slug: String,
    name: String,
}

#[handler]
async fn address_sets_create(
    Form(form): Form<CreateAddressSetForm>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let request = common::message::user::AddressSetCreateRequest {
        slug: form.slug,
        name: form.name,
    };

    common::service::request::<common::message::user::AddressSetResponse>(
        "user.address_sets.create",
        request,
    )
    .await
    .map_err(|e| anyhow::anyhow!("Failed to create address set: {}", e))?;

    Ok(poem::Response::builder()
        .status(poem::http::StatusCode::SEE_OTHER)
        .header("Location", "/-admin/email/address-sets")
        .finish())
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/pages/edit_address_set.stpl.html")]
pub struct EditAddressSetPage {
    pub slug: String,
    pub name: String,
    pub addresses: Vec<String>,
    pub error_message: Option<String>,
}

#[handler]
async fn address_sets_edit(
    Path(slug): Path<String>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let request = common::message::user::AddressSetGetRequest {
        slug: slug.clone(),
        page: None,
        limit: None,
    };

    match common::service::request::<common::message::user::AddressSetGetResponse>(
        "user.address_sets.get",
        request,
    )
    .await
    {
        Ok(response) => {
            let content = EditAddressSetPage {
                slug: response.slug,
                name: response.name,
                addresses: response.addresses,
                error_message: None,
            };
            AdminLayout::new("Edit Address Set", content)
                .with_breadcrumb("Address Sets", Some("/-admin/email/address-sets"))
                .with_breadcrumb("Edit", None)
                .with_sidebar_active("email-address-sets")
                .into_response(user_info)
                .await
        }
        Err(e) => Err(anyhow::anyhow!("Failed to load address set: {}", e)),
    }
}

#[derive(Debug, Deserialize)]
pub struct UpdateAddressSetForm {
    name: String,
}

#[handler]
async fn address_sets_update(
    Path(slug): Path<String>,
    Form(form): Form<UpdateAddressSetForm>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let request = common::message::user::AddressSetUpdateRequest {
        slug: slug.clone(),
        name: form.name,
    };

    common::service::request::<common::message::user::AddressSetResponse>(
        "user.address_sets.update",
        request,
    )
    .await
    .map_err(|e| anyhow::anyhow!("Failed to update address set: {}", e))?;

    Ok(poem::Response::builder()
        .status(poem::http::StatusCode::SEE_OTHER)
        .header("Location", format!("/-admin/email/address-sets/{}", slug))
        .finish())
}

#[handler]
async fn address_sets_delete(
    Path(slug): Path<String>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let request = common::message::user::AddressSetDeleteRequest { slug };

    common::service::request::<common::message::user::AddressSetResponse>(
        "user.address_sets.delete",
        request,
    )
    .await
    .map_err(|e| anyhow::anyhow!("Failed to delete address set: {}", e))?;

    Ok(poem::Response::builder()
        .status(poem::http::StatusCode::OK)
        .header("HX-Redirect", "/-admin/email/address-sets")
        .finish())
}

#[derive(Debug, Deserialize)]
pub struct AddMemberForm {
    email_address: String,
}

#[handler]
async fn address_sets_add_member(
    Path(slug): Path<String>,
    Form(form): Form<AddMemberForm>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let email_address = form.email_address.clone();
    let request = common::message::user::AddressSetAddMemberRequest {
        slug: slug.clone(),
        email_address: form.email_address,
    };

    common::service::request::<common::message::user::AddressSetMemberResponse>(
        "user.address_sets.add_member",
        request,
    )
    .await
    .map_err(|e| anyhow::anyhow!("Failed to add member: {}", e))?;

    let html = format!(
        r#"<li class="py-3 flex justify-between items-center">
    <span class="text-sm text-gray-900">{}</span>
    <button
        hx-delete="/-admin/email/address-sets/{}/members/{}"
        hx-confirm="Remove {} from this address set?"
        hx-target="closest li"
        hx-swap="outerHTML swap:500ms"
        class="text-sm text-red-600 hover:text-red-900"
    >
        Remove
    </button>
</li>
<div id="empty-state" hx-swap-oob="delete"></div>"#,
        email_address, slug, email_address, email_address
    );

    Ok(poem::Response::builder()
        .status(poem::http::StatusCode::OK)
        .content_type("text/html")
        .body(html))
}

#[handler]
async fn address_sets_remove_member(
    Path((slug, email)): Path<(String, String)>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let request = common::message::user::AddressSetRemoveMemberRequest {
        slug: slug.clone(),
        email_address: email,
    };

    common::service::request::<common::message::user::AddressSetMemberResponse>(
        "user.address_sets.remove_member",
        request,
    )
    .await
    .map_err(|e| anyhow::anyhow!("Failed to remove member: {}", e))?;

    Ok(poem::Response::builder()
        .status(poem::http::StatusCode::OK)
        .finish())
}

// ============================================================================
// User Sets
// ============================================================================

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/user_sets.stpl.html")]
pub struct UserSetsContent {
    pub user_sets: Vec<common::message::user::UserSetInfo>,
}

#[handler]
async fn user_sets_list(
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let request = common::message::user::UserSetListRequest {};

    let response: common::message::user::UserSetListResponse =
        common::service::request("user.user_sets.list", request)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to list user sets: {}", e))?;

    let content = UserSetsContent {
        user_sets: response.user_sets,
    };

    AdminLayout::new("User Sets", content)
        .with_breadcrumb("User Sets", None)
        .with_sidebar_active("email-user-sets")
        .into_response(user_info)
        .await
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/pages/new_user_set.stpl.html")]
pub struct NewUserSetPage {
    pub error_message: Option<String>,
}

#[handler]
async fn user_sets_new(
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let content = NewUserSetPage {
        error_message: None,
    };

    AdminLayout::new("New User Set", content)
        .with_breadcrumb("User Sets", Some("/-admin/email/user-sets"))
        .with_breadcrumb("New", None)
        .with_sidebar_active("email-user-sets")
        .into_response(user_info)
        .await
}

#[derive(Debug, Deserialize)]
pub struct CreateUserSetForm {
    slug: String,
    name: String,
}

#[handler]
async fn user_sets_create(
    Form(form): Form<CreateUserSetForm>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let request = common::message::user::UserSetCreateRequest {
        slug: form.slug,
        name: form.name,
    };

    common::service::request::<common::message::user::UserSetResponse>(
        "user.user_sets.create",
        request,
    )
    .await
    .map_err(|e| anyhow::anyhow!("Failed to create user set: {}", e))?;

    Ok(poem::Response::builder()
        .status(poem::http::StatusCode::SEE_OTHER)
        .header("Location", "/-admin/email/user-sets")
        .finish())
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/pages/edit_user_set.stpl.html")]
pub struct EditUserSetPage {
    pub slug: String,
    pub name: String,
    pub criteria: Vec<common::message::user::UserSetCriterionWithId>,
    pub project_names: std::collections::HashMap<Ulid, String>,
    pub user_emails: std::collections::HashMap<Ulid, String>,
    pub error_message: Option<String>,
}

#[handler]
async fn user_sets_edit(
    Path(slug): Path<String>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let request = common::message::user::UserSetGetRequest {
        slug: slug.clone(),
        page: None,
        limit: None,
        evaluation_id: None,
    };

    match common::service::request::<common::message::user::UserSetDetailResponse>(
        "user.user_sets.get_detail",
        request,
    )
    .await
    {
        Ok(response) => {
            // Extract all project IDs and user IDs from criteria
            let mut project_ids = Vec::new();
            let mut user_ids = Vec::new();

            for criterion in &response.criteria {
                match &criterion.criterion {
                    common::message::user::UserSetCriterion::AffiliateParticipation {
                        affiliate_id,
                        ..
                    } => {
                        project_ids.push(*affiliate_id);
                    }
                    common::message::user::UserSetCriterion::ExplicitList { user_ids: ids } => {
                        user_ids.extend(ids.iter().copied());
                    }
                    _ => {}
                }
            }

            // Look up project names and user emails
            let project_name_list = if !project_ids.is_empty() {
                autocomplete::resolve_project_names(project_ids)
                    .await
                    .unwrap_or_default()
            } else {
                vec![]
            };

            let user_email_list = if !user_ids.is_empty() {
                autocomplete::resolve_user_names(user_ids)
                    .await
                    .unwrap_or_default()
            } else {
                vec![]
            };

            // Convert to HashMaps for easy lookup
            let project_names: std::collections::HashMap<Ulid, String> =
                project_name_list.into_iter().collect();
            let user_emails: std::collections::HashMap<Ulid, String> =
                user_email_list.into_iter().collect();

            let content = EditUserSetPage {
                slug: response.slug,
                name: response.name,
                criteria: response.criteria,
                project_names,
                user_emails,
                error_message: None,
            };
            AdminLayout::new("Edit User Set", content)
                .with_breadcrumb("User Sets", Some("/-admin/email/user-sets"))
                .with_breadcrumb("Edit", None)
                .with_sidebar_active("email-user-sets")
                .into_response(user_info)
                .await
        }
        Err(e) => Err(anyhow::anyhow!("Failed to load user set: {}", e)),
    }
}

#[derive(Debug, Deserialize)]
pub struct UpdateUserSetForm {
    name: String,
}

// Criterion field templates
#[derive(TemplateOnce)]
#[template(path = "admin/partials/criterion_fields/has_tag.stpl.html")]
pub struct HasTagCriterionFields;

#[derive(TemplateOnce)]
#[template(path = "admin/partials/criterion_fields/has_point_in_region.stpl.html")]
pub struct HasPointInRegionCriterionFields;

#[derive(TemplateOnce)]
#[template(path = "admin/partials/criterion_fields/affiliate_participation.stpl.html")]
pub struct AffiliateParticipationCriterionFields {
    widget: autocomplete::EntityAutocomplete,
}

#[derive(TemplateOnce)]
#[template(path = "admin/partials/criterion_fields/is_page_manager.stpl.html")]
pub struct IsPageManagerCriterionFields;

#[derive(TemplateOnce)]
#[template(path = "admin/partials/criterion_fields/is_staff.stpl.html")]
pub struct IsStaffCriterionFields;

#[derive(TemplateOnce)]
#[template(path = "admin/partials/criterion_fields/is_verified.stpl.html")]
pub struct IsVerifiedCriterionFields;

#[derive(TemplateOnce)]
#[template(path = "admin/partials/explicit_list_widget.stpl.html")]
pub struct ExplicitListWidget {
    user_names: Vec<(Ulid, String)>,
    widget: autocomplete::EntityAutocomplete,
}

#[derive(TemplateOnce)]
#[template(path = "admin/partials/criterion_fields/explicit_list.stpl.html")]
pub struct ExplicitListCriterionFields {
    inner: ExplicitListWidget,
}

#[derive(TemplateOnce)]
#[template(path = "admin/partials/user_set_criteria_list.stpl.html")]
pub struct UserSetCriteriaList {
    slug: String,
    criteria: Vec<common::message::user::UserSetCriterionWithId>,
    project_names: std::collections::HashMap<Ulid, String>,
    user_emails: std::collections::HashMap<Ulid, String>,
}

#[handler]
async fn user_sets_update(
    Path(slug): Path<String>,
    Form(form): Form<UpdateUserSetForm>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let request = common::message::user::UserSetUpdateRequest {
        slug: slug.clone(),
        name: form.name,
    };

    common::service::request::<common::message::user::UserSetResponse>(
        "user.user_sets.update",
        request,
    )
    .await
    .map_err(|e| anyhow::anyhow!("Failed to update user set: {}", e))?;

    Ok(poem::Response::builder()
        .status(poem::http::StatusCode::SEE_OTHER)
        .header("Location", format!("/-admin/email/user-sets/{}", slug))
        .finish())
}

#[handler]
async fn user_sets_delete(
    Path(slug): Path<String>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let request = common::message::user::UserSetDeleteRequest { slug };

    common::service::request::<common::message::user::UserSetResponse>(
        "user.user_sets.delete",
        request,
    )
    .await
    .map_err(|e| anyhow::anyhow!("Failed to delete user set: {}", e))?;

    Ok(poem::Response::builder()
        .status(poem::http::StatusCode::OK)
        .header("HX-Redirect", "/-admin/email/user-sets")
        .finish())
}

#[derive(Debug, Deserialize)]
struct CriterionFieldsQuery {
    criterion_type: String,
}

#[handler]
async fn user_sets_criterion_fields(
    Query(query): Query<CriterionFieldsQuery>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    use common::message::user::UserSetCriterionType;
    use std::str::FromStr;

    let criterion_type = UserSetCriterionType::from_str(&query.criterion_type)
        .map_err(|_| anyhow::anyhow!("Invalid criterion type: {}", query.criterion_type))?;

    let html = match criterion_type {
        UserSetCriterionType::HasTag => HasTagCriterionFields.render_once()?,
        UserSetCriterionType::HasPointInRegion => HasPointInRegionCriterionFields.render_once()?,
        UserSetCriterionType::AffiliateParticipation => {
            let widget = autocomplete::EntityAutocomplete::new_project_selector(
                "affiliate_id",
                "Affiliate Project",
                "Search for project...",
            );
            AffiliateParticipationCriterionFields { widget }.render_once()?
        }
        UserSetCriterionType::IsPageManager => IsPageManagerCriterionFields.render_once()?,
        UserSetCriterionType::IsStaff => IsStaffCriterionFields.render_once()?,
        UserSetCriterionType::IsVerified => IsVerifiedCriterionFields.render_once()?,
        UserSetCriterionType::ExplicitList => {
            let widget = autocomplete::EntityAutocomplete::new_user_selector(
                "new_user_id",
                "",
                "Search for user to add...",
            );
            let inner = ExplicitListWidget {
                user_names: vec![],
                widget,
            };
            ExplicitListCriterionFields { inner }.render_once()?
        }
    };

    Ok(poem::Response::builder()
        .status(poem::http::StatusCode::OK)
        .content_type("text/html")
        .body(html))
}

async fn render_explicit_list_widget(user_ids: Vec<Ulid>) -> Result<String, anyhow::Error> {
    // Resolve user names
    let user_names = if !user_ids.is_empty() {
        autocomplete::resolve_user_names(user_ids.clone()).await?
    } else {
        vec![]
    };

    let widget = autocomplete::EntityAutocomplete::new_user_selector(
        "new_user_id",
        "",
        "Search for user to add...",
    );

    let template = ExplicitListWidget { user_names, widget };

    Ok(template.render_once()?)
}

#[derive(Debug, Deserialize)]
pub struct ExplicitListAddForm {
    new_user_id: Option<String>,
    #[serde(default)]
    user_ids: Vec<String>,
}

#[handler]
async fn user_sets_explicit_list_add_user(
    QsForm(form): QsForm<ExplicitListAddForm>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let mut user_ids: Vec<Ulid> = form
        .user_ids
        .iter()
        .filter_map(|s| s.parse().ok())
        .collect();

    if let Some(new_id_str) = form.new_user_id {
        if let Ok(new_id) = new_id_str.parse::<Ulid>() {
            if !user_ids.contains(&new_id) {
                user_ids.push(new_id);
            }
        }
    }

    let html = render_explicit_list_widget(user_ids).await?;

    Ok(poem::Response::builder()
        .status(poem::http::StatusCode::OK)
        .content_type("text/html")
        .body(html))
}

#[derive(Debug, Deserialize)]
pub struct ExplicitListRemoveForm {
    #[serde(default)]
    user_ids: Vec<String>,
}

#[handler]
async fn user_sets_explicit_list_remove_user(
    Path(user_id_to_remove): Path<String>,
    QsForm(form): QsForm<ExplicitListRemoveForm>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let remove_id = user_id_to_remove
        .parse::<Ulid>()
        .map_err(|_| anyhow::anyhow!("Invalid user ID"))?;

    let user_ids: Vec<Ulid> = form
        .user_ids
        .iter()
        .filter_map(|s| s.parse().ok())
        .filter(|id| *id != remove_id)
        .collect();

    let html = render_explicit_list_widget(user_ids).await?;

    Ok(poem::Response::builder()
        .status(poem::http::StatusCode::OK)
        .content_type("text/html")
        .body(html))
}

#[derive(Debug, Deserialize)]
pub struct AddCriterionForm {
    criterion_type: String,
    operation: Option<String>,
    tag: Option<String>,
    geojson: Option<String>,
    affiliate_id: Option<String>,
    start: Option<String>,
    end: Option<String>,
    #[serde(default)]
    user_ids: Vec<String>,
}

#[handler]
async fn user_sets_add_criterion(
    Path(slug): Path<String>,
    QsForm(form): QsForm<AddCriterionForm>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    use common::message::user::UserSetCriterionType;
    use std::str::FromStr;

    // Parse the criterion type from string
    let criterion_type = UserSetCriterionType::from_str(&form.criterion_type)
        .map_err(|_| anyhow::anyhow!("Invalid criterion type: {}", form.criterion_type))?;

    // Parse criterion data based on type
    let criterion = match criterion_type {
        UserSetCriterionType::HasTag => {
            let tag = form.tag.ok_or_else(|| anyhow::anyhow!("Missing tag"))?;
            common::message::user::UserSetCriterion::HasTag { tag }
        }
        UserSetCriterionType::HasPointInRegion => {
            let geojson = form
                .geojson
                .ok_or_else(|| anyhow::anyhow!("Missing geojson"))?;
            common::message::user::UserSetCriterion::HasPointInRegion { geojson }
        }
        UserSetCriterionType::AffiliateParticipation => {
            let affiliate_id_str = form
                .affiliate_id
                .ok_or_else(|| anyhow::anyhow!("Missing affiliate_id"))?;
            let affiliate_id = affiliate_id_str
                .parse::<Ulid>()
                .map_err(|e| anyhow::anyhow!("Invalid affiliate_id: {}", e))?;

            let start_str = form
                .start
                .ok_or_else(|| anyhow::anyhow!("Missing start date"))?;
            let start = TimeString::try_from(start_str)
                .map_err(|e| anyhow::anyhow!("Invalid start time: {}", e))?;

            let end_str = form
                .end
                .ok_or_else(|| anyhow::anyhow!("Missing end date"))?;
            let end = TimeString::try_from(end_str)
                .map_err(|e| anyhow::anyhow!("Invalid end time: {}", e))?;

            common::message::user::UserSetCriterion::AffiliateParticipation {
                affiliate_id,
                start,
                end,
            }
        }
        UserSetCriterionType::IsPageManager => {
            common::message::user::UserSetCriterion::IsPageManager
        }
        UserSetCriterionType::IsStaff => common::message::user::UserSetCriterion::IsStaff,
        UserSetCriterionType::IsVerified => common::message::user::UserSetCriterion::IsVerified,
        UserSetCriterionType::ExplicitList => {
            if form.user_ids.is_empty() {
                return Err(anyhow::anyhow!("Missing user IDs"));
            }
            let parsed_ids: Result<Vec<Ulid>, _> =
                form.user_ids.iter().map(|s| s.parse::<Ulid>()).collect();
            let user_ids = parsed_ids.map_err(|e| anyhow::anyhow!("Invalid ULID: {}", e))?;

            common::message::user::UserSetCriterion::ExplicitList { user_ids }
        }
    };

    // Parse the operation type
    let operation = form.operation.as_deref().unwrap_or("union");
    let operation =
        common::message::user::CriterionOperation::from_str(operation).map_err(|_| {
            anyhow::anyhow!(
                "Invalid operation: {}",
                form.operation.as_deref().unwrap_or("unknown")
            )
        })?;

    let request = common::message::user::UserSetAddCriterionRequest {
        slug: slug.clone(),
        criterion,
        operation,
    };

    common::service::request::<common::message::user::UserSetCriterionResponse>(
        "user.user_sets.add_criterion",
        request,
    )
    .await
    .map_err(|e| anyhow::anyhow!("Failed to add criterion: {}", e))?;

    // Fetch updated criteria list
    let get_request = common::message::user::UserSetGetRequest {
        slug: slug.clone(),
        page: None,
        limit: None,
        evaluation_id: None,
    };
    let response = common::service::request::<common::message::user::UserSetDetailResponse>(
        "user.user_sets.get_detail",
        get_request,
    )
    .await
    .map_err(|e| anyhow::anyhow!("Failed to fetch updated user set: {}", e))?;

    // Extract all project IDs and user IDs from criteria
    let mut project_ids = Vec::new();
    let mut user_ids = Vec::new();

    for criterion in &response.criteria {
        match &criterion.criterion {
            common::message::user::UserSetCriterion::AffiliateParticipation {
                affiliate_id,
                ..
            } => {
                project_ids.push(*affiliate_id);
            }
            common::message::user::UserSetCriterion::ExplicitList { user_ids: ids } => {
                user_ids.extend(ids.iter().copied());
            }
            _ => {}
        }
    }

    // Look up project names and user emails
    let project_name_list = if !project_ids.is_empty() {
        autocomplete::resolve_project_names(project_ids)
            .await
            .unwrap_or_default()
    } else {
        vec![]
    };

    let user_email_list = if !user_ids.is_empty() {
        autocomplete::resolve_user_names(user_ids)
            .await
            .unwrap_or_default()
    } else {
        vec![]
    };

    // Convert to HashMaps for easy lookup
    let project_names: std::collections::HashMap<Ulid, String> =
        project_name_list.into_iter().collect();
    let user_emails: std::collections::HashMap<Ulid, String> =
        user_email_list.into_iter().collect();

    // Render criteria list
    let criteria_list = UserSetCriteriaList {
        slug: slug.clone(),
        criteria: response.criteria,
        project_names,
        user_emails,
    };
    let list_html = criteria_list.render_once()?;

    // Render fresh criterion fields for the same type (OOB swap)
    let fields_html = match criterion_type {
        UserSetCriterionType::HasTag => HasTagCriterionFields.render_once()?,
        UserSetCriterionType::HasPointInRegion => HasPointInRegionCriterionFields.render_once()?,
        UserSetCriterionType::AffiliateParticipation => {
            let widget = autocomplete::EntityAutocomplete::new_project_selector(
                "affiliate_id",
                "Affiliate Project",
                "Search for project...",
            );
            AffiliateParticipationCriterionFields { widget }.render_once()?
        }
        UserSetCriterionType::IsPageManager => IsPageManagerCriterionFields.render_once()?,
        UserSetCriterionType::IsStaff => IsStaffCriterionFields.render_once()?,
        UserSetCriterionType::IsVerified => IsVerifiedCriterionFields.render_once()?,
        UserSetCriterionType::ExplicitList => {
            let widget = autocomplete::EntityAutocomplete::new_user_selector(
                "new_user_id",
                "",
                "Search for user to add...",
            );
            let inner = ExplicitListWidget {
                user_names: vec![],
                widget,
            };
            ExplicitListCriterionFields { inner }.render_once()?
        }
    };

    // Combine with OOB swap
    let response_html = format!(
        "{}\n<div id=\"criterion-fields-container\" hx-swap-oob=\"true\">{}</div>",
        list_html, fields_html
    );

    Ok(poem::Response::builder()
        .status(poem::http::StatusCode::OK)
        .content_type("text/html")
        .body(response_html))
}

#[handler]
async fn user_sets_remove_criterion(
    Path((slug, id)): Path<(String, String)>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let criterion_id = id
        .parse::<i64>()
        .map_err(|_| anyhow::anyhow!("Invalid criterion ID"))?;

    let request = common::message::user::UserSetRemoveCriterionRequest {
        slug: slug.clone(),
        criterion_id,
    };

    common::service::request::<common::message::user::UserSetCriterionResponse>(
        "user.user_sets.remove_criterion",
        request,
    )
    .await
    .map_err(|e| anyhow::anyhow!("Failed to remove criterion: {}", e))?;

    Ok(poem::Response::builder()
        .status(poem::http::StatusCode::OK)
        .finish())
}

// Recipient Search API Query Structs
#[derive(Debug, Deserialize)]
struct RecipientSearchQuery {
    q: String,
    #[serde(default)]
    filter: Option<RecipientFilterQuery>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum RecipientFilterQuery {
    Users,
}

// Recipient Search API Handlers
#[handler]
async fn api_recipient_search(
    Query(query): Query<RecipientSearchQuery>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    if query.q.trim().len() < 2 {
        return Ok(poem::Response::builder().content_type("text/html").body(""));
    }

    let request = common::message::user::RecipientSearchRequest {
        query: query.q,
        limit: Some(10),
        filter: query.filter.map(|f| match f {
            RecipientFilterQuery::Users => common::message::user::RecipientFilter::Users,
        }),
    };

    let response: common::message::user::RecipientSearchResponse =
        common::service::request("user.search.recipients", request).await?;

    use common::message::user::RecipientSearchResult;

    #[derive(TemplateOnce)]
    #[template(path = "admin/partials/recipient_search_results.stpl.html")]
    struct RecipientSearchResults {
        results: Vec<RecipientSearchResult>,
    }

    let html = RecipientSearchResults {
        results: response.results,
    }
    .render_once()?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(html))
}

#[derive(Debug, Deserialize)]
struct RecipientSelectForm {
    q: String,
    badgeclass: String,
    #[serde(default)]
    recipient_types: Vec<String>,
    #[serde(default)]
    recipient_values: Vec<String>,
}

#[handler]
async fn api_recipient_select(
    Form(form): Form<RecipientSelectForm>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let badge = common::badge::Badge::from_badgeclass(&form.badgeclass)
        .ok_or_else(|| anyhow::anyhow!("Invalid badge class"))?;

    // Build current recipients list
    let mut recipients = Vec::new();
    for (idx, (recipient_type, recipient_value)) in form
        .recipient_types
        .iter()
        .zip(form.recipient_values.iter())
        .enumerate()
    {
        // Look up display name for existing recipients
        let display = if recipient_type == "User" {
            if let Ok(user_id) = recipient_value.parse::<ulid::Ulid>() {
                match common::service::request::<common::message::user::UserEmailInfo>(
                    "user.info.email_info",
                    common::message::user::UserEmailInfoRequest { user_id },
                )
                .await
                {
                    Ok(user) => {
                        if !user.first_name.is_empty() || !user.last_name.is_empty() {
                            format!("{} {}", user.first_name, user.last_name)
                                .trim()
                                .to_string()
                        } else {
                            user.handle
                        }
                    }
                    Err(_) => recipient_value.clone(),
                }
            } else {
                recipient_value.clone()
            }
        } else {
            recipient_value.clone()
        };

        recipients.push(RecipientInfo {
            id: idx.to_string(),
            display,
            recipient_type: recipient_type.clone(),
            recipient_value: recipient_value.clone(),
        });
    }

    // Add new recipient
    let (display, recipient_type, recipient_value) = if let Ok(user_id) =
        form.q.parse::<ulid::Ulid>()
    {
        // It's a user ID - look up the user
        match common::service::request::<common::message::user::UserEmailInfo>(
            "user.info.email_info",
            common::message::user::UserEmailInfoRequest { user_id },
        )
        .await
        {
            Ok(user) => {
                let display_name = if !user.first_name.is_empty() || !user.last_name.is_empty() {
                    format!("{} {}", user.first_name, user.last_name)
                        .trim()
                        .to_string()
                } else {
                    user.handle
                };
                (display_name, "User", user_id.to_string())
            }
            Err(_) => {
                return Ok(poem::Response::builder()
                    .status(poem::http::StatusCode::BAD_REQUEST)
                    .body("User not found"));
            }
        }
    } else {
        return Ok(poem::Response::builder()
            .status(poem::http::StatusCode::BAD_REQUEST)
            .body("Invalid user ID"));
    };

    recipients.push(RecipientInfo {
        id: recipients.len().to_string(),
        display,
        recipient_type: recipient_type.to_string(),
        recipient_value,
    });

    // Return the entire form
    let form_html = GrantBadgeForm { badge, recipients }.render_once()?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(format!(r#"<div id="grant-badge-form-container" class="px-6 py-4 border-b border-gray-200" hx-swap-oob="true">{}</div>"#, form_html)))
}

#[derive(Debug, Deserialize)]
struct RecipientRemoveForm {
    remove_id: String,
    badgeclass: String,
    #[serde(default)]
    recipient_types: Vec<String>,
    #[serde(default)]
    recipient_values: Vec<String>,
}

#[handler]
async fn api_recipient_remove(
    Form(form): Form<RecipientRemoveForm>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let badge = common::badge::Badge::from_badgeclass(&form.badgeclass)
        .ok_or_else(|| anyhow::anyhow!("Invalid badge class"))?;

    // Build recipients list, filtering out all recipients at the position matching remove_id
    let mut recipients = Vec::new();
    let remove_idx = form.remove_id.parse::<usize>().unwrap_or(usize::MAX);

    for (idx, (recipient_type, recipient_value)) in form
        .recipient_types
        .iter()
        .zip(form.recipient_values.iter())
        .enumerate()
    {
        // Skip the one being removed
        if idx == remove_idx {
            continue;
        }

        // Look up display name
        let display = if recipient_type == "User" {
            if let Ok(user_id) = recipient_value.parse::<ulid::Ulid>() {
                match common::service::request::<common::message::user::UserEmailInfo>(
                    "user.info.email_info",
                    common::message::user::UserEmailInfoRequest { user_id },
                )
                .await
                {
                    Ok(user) => {
                        if !user.first_name.is_empty() || !user.last_name.is_empty() {
                            format!("{} {}", user.first_name, user.last_name)
                                .trim()
                                .to_string()
                        } else {
                            user.handle
                        }
                    }
                    Err(_) => recipient_value.clone(),
                }
            } else {
                recipient_value.clone()
            }
        } else {
            recipient_value.clone()
        };

        recipients.push(RecipientInfo {
            id: idx.to_string(),
            display,
            recipient_type: recipient_type.clone(),
            recipient_value: recipient_value.clone(),
        });
    }

    // Return the entire form
    let form_html = GrantBadgeForm { badge, recipients }.render_once()?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(format!(r#"<div id="grant-badge-form-container" class="px-6 py-4 border-b border-gray-200" hx-swap-oob="true">{}</div>"#, form_html)))
}

// Badge Management
#[derive(TemplateOnce)]
#[template(path = "admin/partials/badge_holders_content.stpl.html")]
struct BadgeHoldersContentPartial {
    badge: common::badge::Badge,
    search_term: String,
    holders: Vec<common::message::user::BadgeHolderInfo>,
    current_page: u32,
    total_count: u32,
    has_previous: bool,
    has_next: bool,
    per_page: u32,
    start_item: u32,
    end_item: u32,
}

#[derive(Clone)]
struct RecipientInfo {
    id: String,
    display: String,
    recipient_type: String,
    recipient_value: String,
}

#[derive(TemplateOnce)]
#[template(path = "admin/partials/grant_badge_form.stpl.html")]
struct GrantBadgeForm {
    badge: common::badge::Badge,
    recipients: Vec<RecipientInfo>,
}

#[derive(TemplateOnce)]
#[template(path = "admin/partials/recipient_badge.stpl.html")]
struct RecipientBadge {
    display: String,
    id: String,
}

#[handler]
async fn exchanges_list(
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    // Fetch exchanges from database
    let rows = sqlx::query!(
        "SELECT exchange_id, slug, name, description, logo_url, created, updated FROM exchanges ORDER BY created DESC"
    )
    .fetch_all(db())
    .await?;

    let exchanges: Vec<ExchangeListItem> = rows
        .into_iter()
        .map(|row| ExchangeListItem {
            exchange_id: row.exchange_id,
            slug: row.slug,
            name: row.name,
            description: row.description,
            logo_url: row.logo_url,
            created: row.created,
        })
        .collect();

    let content = ExchangesListPage { exchanges };

    let prepared = crate::component::AdminLayout::new("Exchange Management", content)
        .with_sidebar_active("exchanges")
        .prepare(user_info)
        .await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(prepared.render_once()?))
}

#[derive(Debug, serde::Deserialize)]
struct ExchangeCreateForm {
    exchange_id: i64,
    slug: String,
    name: String,
    description: String,
    #[serde(default)]
    logo_url: String,
}

#[handler]
async fn exchange_create_page(
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let content = ExchangeCreatePage {};

    let prepared = crate::component::AdminLayout::new("Create Exchange", content)
        .with_sidebar_active("exchanges")
        .prepare(user_info)
        .await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(prepared.render_once()?))
}

#[handler]
async fn exchange_create(
    Authenticated(_user_info): Authenticated,
    form: poem::web::Form<ExchangeCreateForm>,
) -> Result<poem::Response, anyhow::Error> {
    use crate::exchange::Exchange;

    // Create new exchange with default settings
    let exchange = Exchange {
        exchange_id: form.exchange_id,
        slug: form.slug.clone(),
        name: form.name.clone(),
        description: form.description.clone(),
        logo_url: if form.logo_url.is_empty() {
            None
        } else {
            Some(form.logo_url.clone())
        },
        settings: crate::exchange::ExchangeSettings {
            allow_project_voting: true,
            allow_project_reviews: true,
            require_approval: false,
            restricted_add: false,
            custom_css: None,
        },
        predefined_searches: Vec::new(),
        hidden_projects: std::collections::HashSet::new(),
        created: common::timestring::TimeString::now(),
        updated: common::timestring::TimeString::now(),
    };

    // Use LoadStorePeered to save and broadcast
    exchange.store().await?;

    // Redirect to exchanges list
    Ok(poem::web::Redirect::see_other("/-admin/exchanges").into_response())
}

#[handler]
async fn exchange_edit_page(
    Path(exchange_id): Path<i64>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    use crate::exchange::Exchange;

    // Load exchange via LoadStorePeered
    let exchange = Exchange::load(exchange_id).await?;

    let content = ExchangeEditPage {
        exchange_id: exchange.exchange_id,
        slug: exchange.slug,
        name: exchange.name,
        description: exchange.description,
        logo_url: exchange.logo_url.unwrap_or_default(),
    };

    let prepared = crate::component::AdminLayout::new("Edit Exchange", content)
        .with_sidebar_active("exchanges")
        .prepare(user_info)
        .await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(prepared.render_once()?))
}

#[derive(Debug, serde::Deserialize)]
struct ExchangeUpdateForm {
    slug: String,
    name: String,
    description: String,
    #[serde(default)]
    logo_url: String,
}

#[handler]
async fn exchange_update(
    Path(exchange_id): Path<i64>,
    Authenticated(_user_info): Authenticated,
    form: poem::web::Form<ExchangeUpdateForm>,
) -> Result<poem::Response, anyhow::Error> {
    use crate::exchange::Exchange;

    // Load existing exchange
    let mut exchange = Exchange::load(exchange_id).await?;

    // Update fields
    exchange.slug = form.slug.clone();
    exchange.name = form.name.clone();
    exchange.description = form.description.clone();
    exchange.logo_url = if form.logo_url.is_empty() {
        None
    } else {
        Some(form.logo_url.clone())
    };
    exchange.updated = common::timestring::TimeString::now();

    // Use LoadStorePeered to save and broadcast
    exchange.store().await?;

    // Redirect to exchanges list
    Ok(poem::web::Redirect::see_other("/-admin/exchanges").into_response())
}

#[handler]
async fn exchange_delete(
    Path(exchange_id): Path<i64>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    use crate::exchange::Exchange;
    use crate::peering::LoadStorePeered;

    // Use LoadStorePeered to delete and broadcast to peers
    Exchange::delete(exchange_id).await?;

    // Return success message for HTMX
    Ok(Html("<div class=\"text-green-600\">Exchange deleted successfully</div>").into_response())
}

#[handler]
async fn badges_list(
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let content = BadgesListPage {};

    let prepared = crate::component::AdminLayout::new("Badge Management", content)
        .with_sidebar_active("badges")
        .prepare(user_info)
        .await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(prepared.render_once()?))
}

#[derive(Debug, Deserialize)]
struct BadgesHoldersQuery {
    search: Option<String>,
    page: Option<u32>,
}

#[handler]
async fn badges_holders(
    Path(badgeclass): Path<String>,
    Query(query): Query<BadgesHoldersQuery>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let badge = common::badge::Badge::from_badgeclass(&badgeclass)
        .ok_or_else(|| anyhow::anyhow!("Invalid badge class"))?;

    let page = query.page.unwrap_or(1);
    let limit = 50;

    let request = common::message::user::BadgeHoldersRequest {
        badge,
        search: query.search.clone(),
        page: Some(page),
        limit: Some(limit),
    };

    let response = common::service::request::<common::message::user::BadgeHoldersResponse>(
        "user.badges.holders",
        request,
    )
    .await?;

    let total_pages = (response.total_count + limit - 1) / limit;
    let has_previous = page > 1;
    let has_next = page < total_pages;
    let start_item = if response.holders.is_empty() {
        0
    } else {
        ((page - 1) * limit) + 1
    };
    let end_item = std::cmp::min(page * limit, response.total_count);

    let content = BadgeHoldersPage {
        badge,
        search_term: query.search.clone().unwrap_or_default(),
        holders: response.holders,
        current_page: page,
        total_count: response.total_count,
        has_previous,
        has_next,
        per_page: limit,
        start_item,
        end_item,
    };

    let prepared =
        crate::component::AdminLayout::new(&format!("{} - Badge Holders", badge.name()), content)
            .with_sidebar_active("badges")
            .with_breadcrumb("Badges", Some("/-admin/badges"))
            .with_breadcrumb(badge.name(), None)
            .prepare(user_info)
            .await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(prepared.render_once()?))
}

#[handler]
async fn badges_holders_content(
    Path(badgeclass): Path<String>,
    Query(query): Query<BadgesHoldersQuery>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let badge = common::badge::Badge::from_badgeclass(&badgeclass)
        .ok_or_else(|| anyhow::anyhow!("Invalid badge class"))?;

    let page = query.page.unwrap_or(1);
    let limit = 50;

    let request = common::message::user::BadgeHoldersRequest {
        badge,
        search: query.search.clone(),
        page: Some(page),
        limit: Some(limit),
    };

    let response = common::service::request::<common::message::user::BadgeHoldersResponse>(
        "user.badges.holders",
        request,
    )
    .await?;

    let total_pages = (response.total_count + limit - 1) / limit;
    let has_previous = page > 1;
    let has_next = page < total_pages;
    let start_item = if response.holders.is_empty() {
        0
    } else {
        ((page - 1) * limit) + 1
    };
    let end_item = std::cmp::min(page * limit, response.total_count);

    let partial = BadgeHoldersContentPartial {
        badge,
        search_term: query.search.clone().unwrap_or_default(),
        holders: response.holders,
        current_page: page,
        total_count: response.total_count,
        has_previous,
        has_next,
        per_page: limit,
        start_item,
        end_item,
    };

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(partial.render_once()?))
}

#[derive(Debug, Deserialize)]
struct BadgeGrantForm {
    #[serde(default)]
    recipient_types: Vec<String>,
    #[serde(default)]
    recipient_values: Vec<String>,
}

#[handler]
async fn badges_grant(
    Path(badgeclass): Path<String>,
    QsForm(form): QsForm<BadgeGrantForm>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let badge = common::badge::Badge::from_badgeclass(&badgeclass)
        .ok_or_else(|| anyhow::anyhow!("Invalid badge class"))?;

    // Validate parallel arrays
    if form.recipient_types.len() != form.recipient_values.len() {
        return Err(anyhow::anyhow!("Mismatched recipient data"));
    }

    if form.recipient_types.is_empty() {
        return Err(anyhow::anyhow!("No recipients specified"));
    }

    // Grant badge to all selected users
    for (recipient_type, recipient_value) in form
        .recipient_types
        .iter()
        .zip(form.recipient_values.iter())
    {
        if recipient_type == "User" {
            let user_id = recipient_value
                .parse::<Ulid>()
                .map_err(|_| anyhow::anyhow!("Invalid user ID format"))?;

            let request = common::message::user::UserAndBadge {
                user_id,
                badge: badge.clone(),
            };

            common::service::send("user.badges.grant", request).await?;

            // Log the activity
            if let Some(user_info) = &user_info {
                common::service::log_activity(
                    None,
                    "badge-grant",
                    user_info.user_id,
                    user_id,
                    Some(badgeclass.clone()),
                )
                .await?;
            }
        }
    }

    // Return empty form (cleared state) after successful grant
    let form_html = GrantBadgeForm {
        badge,
        recipients: Vec::new(),
    }
    .render_once()?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(format!(r#"<div id="grant-badge-form-container" class="px-6 py-4 border-b border-gray-200" hx-swap-oob="true">{}</div>"#, form_html)))
}

#[handler]
async fn badges_revoke(
    Path((badgeclass, user_id_str)): Path<(String, String)>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let badge = common::badge::Badge::from_badgeclass(&badgeclass)
        .ok_or_else(|| anyhow::anyhow!("Invalid badge class"))?;

    let user_id = user_id_str
        .parse::<Ulid>()
        .map_err(|_| anyhow::anyhow!("Invalid user ID format"))?;

    let request = common::message::user::UserAndBadge { user_id, badge };

    common::service::send("user.badges.revoke", request).await?;

    // Log the activity
    if let Some(user_info) = &user_info {
        common::service::log_activity(
            None,
            "badge-revoke",
            user_info.user_id,
            user_id,
            Some(badgeclass.clone()),
        )
        .await?;
    }

    // Return empty response to delete the row via HTMX
    Ok(poem::Response::builder()
        .status(poem::http::StatusCode::OK)
        .finish())
}

// Exchange Management Templates
#[derive(Debug, Clone)]
struct ExchangeListItem {
    exchange_id: i64,
    slug: String,
    name: String,
    description: String,
    logo_url: Option<String>,
    created: String,
}

#[derive(TemplateOnce)]
#[template(path = "admin/exchanges.stpl.html")]
struct ExchangesListPage {
    exchanges: Vec<ExchangeListItem>,
}

#[derive(TemplateOnce)]
#[template(path = "admin/exchanges_create.stpl.html")]
struct ExchangeCreatePage {}

#[derive(TemplateOnce)]
#[template(path = "admin/exchanges_edit.stpl.html")]
struct ExchangeEditPage {
    exchange_id: i64,
    slug: String,
    name: String,
    description: String,
    logo_url: String,
}

// Badge Management Templates
#[derive(TemplateOnce)]
#[template(path = "admin/badges.stpl.html")]
struct BadgesListPage {}

#[derive(TemplateOnce)]
#[template(path = "admin/badges_holders.stpl.html")]
struct BadgeHoldersPage {
    badge: common::badge::Badge,
    search_term: String,
    holders: Vec<common::message::user::BadgeHolderInfo>,
    current_page: u32,
    total_count: u32,
    has_previous: bool,
    has_next: bool,
    per_page: u32,
    start_item: u32,
    end_item: u32,
}
