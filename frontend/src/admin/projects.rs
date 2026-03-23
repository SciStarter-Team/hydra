use crate::component::{AdminLayout, Component, admin_layout::AdminLayoutPrepared};
use crate::contact::Contact;
use crate::entity::{Entity, EntityKind, EntityTarget};
use crate::middleware::authorization::Authenticated;
use crate::peering::LoadStorePeered;
use crate::project::all::ProjectAll;
use crate::project::field::{
    ACTIVITIES_VOCABULARY, AUDIENCE_VOCABULARY, DATA_ACCESS_VOCABULARY,
    DATA_AGGREGATION_VOCABULARY, DATA_CONSENT_VOCABULARY, DIFFICULTY_VOCABULARY,
    FIELDS_OF_SCIENCE_VOCABULARY, GEOGRAPHIC_SCOPE_VOCABULARY, INTENDED_OUTCOMES_VOCABULARY,
    PARTICIPATION_TASKS_VOCABULARY, PRACTICES_VOCABULARY, ProjectField, ProjectFieldType,
    ProjectFieldValue, SPONSORS_VOCABULARY, STAKEHOLDERS_VOCABULARY,
    SUSTAINABLE_DEVELOPMENT_GOALS_VOCABULARY, TOPICS_VOCABULARY,
};
use common::insensitivestring::InsensitiveString;
use common::shared::db;
use common::timestring::TimeString;
use poem::{
    Route, delete, get, handler, post, put,
    web::{Form, Path, Query},
};
use sailfish::TemplateOnce;
use serde::Deserialize;
use std::collections::HashSet;
use std::sync::Arc;
use ulid::Ulid;

use super::types::lookup_user_email;
use common::message::user::BasicInfo;
use common::service::{TriggerEvent, trigger_email};

pub fn routes() -> Route {
    Route::new()
        .at("/", get(projects).post(projects_create))
        .at("/search", get(projects_search))
        .at("/new", get(projects_new))
        .at("/:id", get(projects_edit))
        .at("/:id/publish", post(projects_publish))
        .at("/:id/unpublish", post(projects_unpublish))
        .at("/:id/field/:field_type", put(projects_update_field))
        .at(
            "/:id/multiselect/:field_type",
            put(projects_update_multiselect_field),
        )
        .at(
            "/:id/array/:field_type/items",
            get(projects_get_array_items).post(projects_add_array_item),
        )
        .at(
            "/:id/array/:field_type/items/:index",
            put(projects_update_array_item).delete(projects_remove_array_item),
        )
        .at("/:id/tags", post(projects_add_tag))
        .at("/:id/tags/:tag", delete(projects_remove_tag))
        .at(
            "/:id/contacts/items",
            get(projects_get_contacts).post(projects_add_contact),
        )
        .at(
            "/:id/contacts/items/:contact_id",
            put(projects_update_contact).delete(projects_remove_contact),
        )
}

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
    pub reviewed: bool,
    pub published: bool,
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
    pub published: bool,
}

impl AdminProjectFull {
    /// Convert a ProjectAll to AdminProjectFull, performing user lookups for editor fields
    pub async fn from_project_all(project: ProjectAll) -> Result<Self, anyhow::Error> {
        // Check if project is published by looking for entity
        let project_id_str = project.project_id.to_string();
        let published = sqlx::query!(
            r#"SELECT COALESCE(publish, 0) as "published!: bool" FROM entities WHERE kind = 'Project' AND target = ?"#,
            project_id_str
        )
        .fetch_optional(db())
        .await?
        .map(|r| r.published)
        .unwrap_or(false);

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
            published,
        })
    }
}

pub async fn fetch_projects_page(
    search: Option<&str>,
    status_filter: Option<&str>,
    flagged_filter: Option<&str>,
    reviewed_filter: Option<&str>,
    spatial_overlap: Option<&str>, // GeoJSON geometry for overlap search
    spatial_point: Option<(f64, f64, f64)>, // (lat, lng, radius_meters) circle for intersection search
    proximity_sort: Option<(f64, f64)>,     // (lat, lng) for proximity sorting
    page: Option<u32>,
    per_page: Option<u32>,
) -> Result<Vec<AdminProject>, anyhow::Error> {
    use sqlx::QueryBuilder;

    let page = page.unwrap_or(1);
    let per_page = per_page.unwrap_or(25);
    let offset = (page - 1) * per_page;

    let mut query = QueryBuilder::new("");

    if let Some(search_term) = search.filter(|s| !s.trim().is_empty()) {
        query.push(
            "SELECT p.project_id, p.name, p.status, p.created, p.presenter,
                    p.origin, p.flagged, p.duplicate, p.is_test, p.goal, p.task, p.image,
                    p.url, p.admin_notes, p.draft, p.reviewed, p.description,
                    COALESCE(ent.publish, 0) as published
             FROM projects p
             JOIN projects_fts f ON p.project_id = f.project_id
             LEFT JOIN entities ent ON ent.target = p.project_id AND ent.kind = 'Project'
             WHERE projects_fts MATCH ",
        );
        // With porter stemming enabled, use simple term matching for better flexibility
        query.push_bind(search_term);
    } else {
        query.push(
            "SELECT p.project_id, p.name, p.status, p.created, p.presenter,
                    p.origin, p.flagged, p.duplicate, p.is_test, p.goal, p.task, p.image,
                    p.url, p.admin_notes, p.draft, p.reviewed, p.description,
                    COALESCE(ent.publish, 0) as published
             FROM projects p
             LEFT JOIN entities ent ON ent.target = p.project_id AND ent.kind = 'Project'
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

    if let Some(reviewed) = reviewed_filter.filter(|s| !s.is_empty()) {
        let reviewed_bool = reviewed == "true";
        query.push(" AND p.reviewed = ");
        query.push_bind(reviewed_bool);
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
        reviewed: bool,
        published: bool,
        description: String,
    }

    let project_rows = query.build_query_as::<ProjectRow>().fetch_all(db()).await?;

    let admin_projects = project_rows
        .into_iter()
        .map(|row| AdminProject {
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
            reviewed: row.reviewed,
            published: row.published,
            tags: Vec::new(), // Tags not needed for project list
        })
        .collect();

    Ok(admin_projects)
}

pub async fn count_projects(
    search: Option<&str>,
    status_filter: Option<&str>,
    flagged_filter: Option<&str>,
    reviewed_filter: Option<&str>,
    spatial_overlap: Option<&str>,
    spatial_point: Option<(f64, f64, f64)>,
) -> Result<u64, anyhow::Error> {
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

    if let Some(reviewed) = reviewed_filter.filter(|s| !s.is_empty()) {
        let reviewed_bool = reviewed == "true";
        query.push(" AND p.reviewed = ");
        query.push_bind(reviewed_bool);
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

#[derive(Debug)]
pub struct ProjectsPage {
    page: Option<u32>,
    search: Option<String>,
    status: Option<String>,
    flagged: Option<String>,
    reviewed: Option<String>,
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
    reviewed: String,
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
        let reviewed_ref = self.reviewed.as_deref();
        let projects_list = match fetch_projects_page(
            search_ref,
            status_ref,
            flagged_ref,
            reviewed_ref,
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
            reviewed_ref,
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
                "[name='search'], [name='status'], [name='flagged'], [name='reviewed']".to_string(),
            search_term: self.search.unwrap_or_default(),
            selected_status: self.status.unwrap_or_default(),
            flagged: self.flagged.unwrap_or_default(),
            reviewed: self.reviewed.unwrap_or_default(),
        };

        AdminLayout::new("Projects", projects_content)
            .with_breadcrumb("Projects", None)
            .with_sidebar_active("projects")
            .prepare(param)
            .await
    }
}

#[derive(Debug, Deserialize)]
struct ProjectPageQuery {
    page: Option<u32>,
    search: Option<String>,
    status: Option<String>,
    flagged: Option<String>,
    reviewed: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
struct ProjectSearchQuery {
    search: Option<String>,
    status: Option<String>,
    flagged: Option<String>,
    reviewed: Option<String>,
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
    pub reviewed: String,
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/partials/project_row.stpl.html")]
pub struct ProjectRow {
    pub project: AdminProject,
    pub oob: bool,
    pub oob_swap: String,
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/partials/publish_status.stpl.html")]
pub struct PublishStatus {
    pub project_id: String,
    pub published: bool,
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
            .with_breadcrumb_short(&title, "Project", None)
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

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/pages/add_project.stpl.html")]
struct AddProjectPage {
    error_message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UpdateProjectFieldForm {
    value: Option<String>,
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
pub struct TagsDisplay {
    pub project_id: String,
    pub tags: Vec<String>,
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
pub struct ContactWidget {
    pub project_id: Ulid,
    pub contact: Option<Contact>,
}

impl ContactWidget {
    pub fn new_editable(project_id: Ulid, contact: Option<Contact>) -> Self {
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
        query.reviewed.as_deref(),
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
        query.reviewed.as_deref(),
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
        pagination_include: "[name='search'], [name='status'], [name='flagged'], [name='reviewed']"
            .to_string(),
        search_term: query.search.unwrap_or_default(),
        selected_status: query.status.unwrap_or_default(),
        flagged: query.flagged.unwrap_or_default(),
        reviewed: query.reviewed.unwrap_or_default(),
    })
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
async fn projects(
    Query(query): Query<ProjectPageQuery>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    // If any search parameters are provided, perform a search instead of the default list
    if query.search.is_some()
        || query.status.is_some()
        || query.flagged.is_some()
        || query.reviewed.is_some()
    {
        let search_query = ProjectSearchQuery {
            search: query.search,
            status: query.status,
            flagged: query.flagged,
            reviewed: query.reviewed,
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
        reviewed: query.reviewed,
    }
    .into_response(user_info)
    .await
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
        reviewed: query.reviewed.clone().unwrap_or_default(),
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
    if let Some(reviewed) = &query.reviewed {
        if !reviewed.trim().is_empty() {
            url_params.push(format!("reviewed={}", urlencoding::encode(reviewed)));
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
async fn projects_publish(
    Path(project_id): Path<String>,
    Query(query): Query<std::collections::HashMap<String, String>>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let project_ulid = project_id
        .parse::<Ulid>()
        .map_err(|_| anyhow::anyhow!("Invalid project ID format"))?;

    // Try to load existing entity
    let project_id_str = project_ulid.to_string();
    let existing = sqlx::query!(
        r#"SELECT slug, created FROM entities WHERE kind = 'Project' AND target = ?"#,
        project_id_str
    )
    .fetch_optional(db())
    .await?;

    if let Some(existing) = existing {
        // Entity exists, update it with publish=true
        let entity = Entity::new_with_created(
            existing.slug,
            EntityKind::Project,
            EntityTarget::Ulid(project_ulid),
            true, // publish = true
            common::timestring::TimeString::try_from(existing.created)?,
        );
        entity.store().await?;
    } else {
        // No entity exists, create new one with publish=true
        let project = sqlx::query!(
            "SELECT name FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;

        Entity::new_by_name(
            &project.name,
            EntityKind::Project,
            EntityTarget::Ulid(project_ulid),
        )
        .await?;
    }

    // Fire publish trigger to the project creator
    if let Ok(project_info) = sqlx::query!(
        "SELECT name, page_creator_id FROM projects WHERE project_id = ?",
        project_id_str
    )
    .fetch_one(db())
    .await
    {
        if let Ok(creator_id) = project_info.page_creator_id.parse::<Ulid>() {
            trigger_email(
                TriggerEvent::ProjectPublished,
                creator_id,
                Some(&[
                    ("project_name".to_string(), project_info.name),
                    ("project_id".to_string(), project_id_str.clone()),
                ]),
            )
            .await
            .ok();
        }
    }

    // Check if this is from the edit page
    let is_edit_context = query.get("context").map(|s| s.as_str()) == Some("edit");

    if is_edit_context {
        // Return just the publish status section
        let publish_status = PublishStatus {
            project_id: project_id_str,
            published: true,
        };

        Ok(poem::Response::builder()
            .content_type("text/html")
            .body(publish_status.render_once()?))
    } else {
        // Return the full project row for list view
        let row = sqlx::query!(
            r#"SELECT p.project_id, p.name, p.status, p.created, p.presenter,
                      p.origin, p.flagged, p.duplicate, p.is_test, p.goal, p.task, p.image,
                      p.url, p.admin_notes, p.draft, p.reviewed, p.description,
                      COALESCE(ent.publish, 0) as "published!: bool"
               FROM projects p
               LEFT JOIN entities ent ON ent.target = p.project_id AND ent.kind = 'Project'
               WHERE p.project_id = ?"#,
            project_id_str
        )
        .fetch_one(db())
        .await?;

        let project = AdminProject {
            id: row.project_id,
            title: row.name,
            status: row.status,
            created_date: row.created,
            page_creator_id: String::new(),
            page_creator_email: None,
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
            reviewed: row.reviewed,
            published: row.published,
            tags: Vec::new(),
        };

        let project_row = ProjectRow {
            project,
            oob: false,
            oob_swap: String::new(),
        };

        Ok(poem::Response::builder()
            .content_type("text/html")
            .body(project_row.render_once()?))
    }
}

#[handler]
async fn projects_unpublish(
    Path(project_id): Path<String>,
    Query(query): Query<std::collections::HashMap<String, String>>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let project_ulid = project_id
        .parse::<Ulid>()
        .map_err(|_| anyhow::anyhow!("Invalid project ID format"))?;

    // Load existing entity
    let project_id_str = project_ulid.to_string();
    let existing = sqlx::query!(
        r#"SELECT slug, created FROM entities WHERE kind = 'Project' AND target = ?"#,
        project_id_str
    )
    .fetch_optional(db())
    .await?;

    if let Some(existing) = existing {
        // Entity exists, update it with publish=false
        let entity = Entity::new_with_created(
            existing.slug,
            EntityKind::Project,
            EntityTarget::Ulid(project_ulid),
            false, // publish = false
            common::timestring::TimeString::try_from(existing.created)?,
        );
        entity.store().await?;
    }
    // If no entity exists, it's already unpublished (no action needed)

    // Check if this is from the edit page
    let is_edit_context = query.get("context").map(|s| s.as_str()) == Some("edit");

    if is_edit_context {
        // Return just the publish status section
        let publish_status = PublishStatus {
            project_id: project_id_str,
            published: false,
        };

        Ok(poem::Response::builder()
            .content_type("text/html")
            .body(publish_status.render_once()?))
    } else {
        // Return the full project row for list view
        let row = sqlx::query!(
            r#"SELECT p.project_id, p.name, p.status, p.created, p.presenter,
                      p.origin, p.flagged, p.duplicate, p.is_test, p.goal, p.task, p.image,
                      p.url, p.admin_notes, p.draft, p.reviewed, p.description,
                      COALESCE(ent.publish, 0) as "published!: bool"
               FROM projects p
               LEFT JOIN entities ent ON ent.target = p.project_id AND ent.kind = 'Project'
               WHERE p.project_id = ?"#,
            project_id_str
        )
        .fetch_one(db())
        .await?;

        let project = AdminProject {
            id: row.project_id,
            title: row.name,
            status: row.status,
            created_date: row.created,
            page_creator_id: String::new(),
            page_creator_email: None,
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
            reviewed: row.reviewed,
            published: row.published,
            tags: Vec::new(),
        };

        let project_row = ProjectRow {
            project,
            oob: false,
            oob_swap: String::new(),
        };

        Ok(poem::Response::builder()
            .content_type("text/html")
            .body(project_row.render_once()?))
    }
}

#[handler]
async fn projects_update_field(
    Path((project_id, field_type)): Path<(String, String)>,
    req: &poem::Request,
    Form(form): Form<UpdateProjectFieldForm>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    // Check if this is from the list view by looking at HX-Target header
    let hx_target = req
        .headers()
        .get("HX-Target")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let is_list_view = hx_target.starts_with("#project-row-");

    // Parse project ID
    let parsed_project_id = project_id
        .parse::<ulid::Ulid>()
        .map_err(|_| anyhow::anyhow!("Invalid project ID format"))?;
    let project_id_str = parsed_project_id.to_string();

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
        "reviewed" => crate::project::field::ProjectFieldType::Reviewed,
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
    // For checkboxes, an unchecked box sends no value, so treat None/empty as empty string
    let form_value = form.value.unwrap_or_default();

    let project_field = crate::project::field::ProjectField::new(
        parsed_project_id,
        match field_type_enum {
            // Core project fields
            crate::project::field::ProjectFieldType::ProjectId => {
                let ulid_value = form_value
                    .parse::<Ulid>()
                    .map_err(|e| anyhow::anyhow!("Invalid ULID format: {}", e))?;
                crate::project::field::ProjectFieldValue::ProjectId(ulid_value)
            }
            crate::project::field::ProjectFieldType::LegacyId => {
                let option_value = if form_value.is_empty() {
                    None
                } else {
                    Some(
                        form_value
                            .parse::<i64>()
                            .map_err(|e| anyhow::anyhow!("Invalid integer format: {}", e))?,
                    )
                };
                crate::project::field::ProjectFieldValue::LegacyId(option_value)
            }
            crate::project::field::ProjectFieldType::LegacyV1ProjectId => {
                let option_value = if form_value.is_empty() {
                    None
                } else {
                    Some(
                        form_value
                            .parse::<i64>()
                            .map_err(|e| anyhow::anyhow!("Invalid integer format: {}", e))?,
                    )
                };
                crate::project::field::ProjectFieldValue::LegacyV1ProjectId(option_value)
            }
            crate::project::field::ProjectFieldType::LegacyV2ProjectId => {
                let option_value = if form_value.is_empty() {
                    None
                } else {
                    Some(
                        form_value
                            .parse::<i64>()
                            .map_err(|e| anyhow::anyhow!("Invalid integer format: {}", e))?,
                    )
                };
                crate::project::field::ProjectFieldValue::LegacyV2ProjectId(option_value)
            }
            crate::project::field::ProjectFieldType::ForeignId => {
                let option_value = if form_value.is_empty() {
                    None
                } else {
                    Some(form_value)
                };
                crate::project::field::ProjectFieldValue::ForeignId(option_value)
            }
            crate::project::field::ProjectFieldType::Duplicate => {
                let bool_value = form_value.parse::<bool>().unwrap_or_else(|_| {
                    form_value.to_lowercase() == "true"
                        || form_value == "1"
                        || form_value.to_lowercase() == "on"
                });
                crate::project::field::ProjectFieldValue::Duplicate(bool_value)
            }
            crate::project::field::ProjectFieldType::Authoritative => {
                let bool_value = form_value.parse::<bool>().unwrap_or_else(|_| {
                    form_value.to_lowercase() == "true"
                        || form_value == "1"
                        || form_value.to_lowercase() == "on"
                });
                crate::project::field::ProjectFieldValue::Authoritative(bool_value)
            }
            crate::project::field::ProjectFieldType::Flagged => {
                let bool_value = form_value.parse::<bool>().unwrap_or_else(|_| {
                    form_value.to_lowercase() == "true"
                        || form_value == "1"
                        || form_value.to_lowercase() == "on"
                });
                crate::project::field::ProjectFieldValue::Flagged(bool_value)
            }
            crate::project::field::ProjectFieldType::IsTest => {
                let bool_value = form_value.parse::<bool>().unwrap_or_else(|_| {
                    form_value.to_lowercase() == "true"
                        || form_value == "1"
                        || form_value.to_lowercase() == "on"
                });
                crate::project::field::ProjectFieldValue::IsTest(bool_value)
            }
            crate::project::field::ProjectFieldType::Status => {
                let status_enum = match form_value.as_str() {
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
                let promotion_time = match form_value.as_str() {
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
                let option_value = if form_value.is_empty() {
                    None
                } else {
                    Some(
                        TimeString::try_from(form_value)
                            .map_err(|e| anyhow::anyhow!("Invalid time format: {}", e))?,
                    )
                };
                crate::project::field::ProjectFieldValue::PromotionTarget(option_value)
            }
            crate::project::field::ProjectFieldType::AdminNotes => {
                crate::project::field::ProjectFieldValue::AdminNotes(form_value)
            }
            crate::project::field::ProjectFieldType::Created => {
                let time_string = TimeString::try_from(form_value)
                    .map_err(|e| anyhow::anyhow!("Invalid time format: {}", e))?;
                crate::project::field::ProjectFieldValue::Created(time_string)
            }
            crate::project::field::ProjectFieldType::WildHope => {
                let bool_value = form_value.parse::<bool>().unwrap_or_else(|_| {
                    form_value.to_lowercase() == "true"
                        || form_value == "1"
                        || form_value.to_lowercase() == "on"
                });
                crate::project::field::ProjectFieldValue::WildHope(bool_value)
            }
            crate::project::field::ProjectFieldType::Exchange => {
                let option_value = if form_value.is_empty() {
                    None
                } else {
                    Some(form_value)
                };
                crate::project::field::ProjectFieldValue::Exchange(option_value)
            }
            crate::project::field::ProjectFieldType::Origin => {
                crate::project::field::ProjectFieldValue::Origin(form_value)
            }
            crate::project::field::ProjectFieldType::Suborigin => {
                crate::project::field::ProjectFieldValue::Suborigin(form_value)
            }
            crate::project::field::ProjectFieldType::Guid => {
                crate::project::field::ProjectFieldValue::Guid(form_value)
            }
            crate::project::field::ProjectFieldType::OpengraphDescription => {
                crate::project::field::ProjectFieldValue::OpengraphDescription(form_value)
            }
            crate::project::field::ProjectFieldType::IsAffiliate => {
                let bool_value = form_value.parse::<bool>().unwrap_or_else(|_| {
                    form_value.to_lowercase() == "true"
                        || form_value == "1"
                        || form_value.to_lowercase() == "on"
                });
                crate::project::field::ProjectFieldValue::IsAffiliate(bool_value)
            }
            crate::project::field::ProjectFieldType::IsInaturalist => {
                let bool_value = form_value.parse::<bool>().unwrap_or_else(|_| {
                    form_value.to_lowercase() == "true"
                        || form_value == "1"
                        || form_value.to_lowercase() == "on"
                });
                crate::project::field::ProjectFieldValue::IsInaturalist(bool_value)
            }
            crate::project::field::ProjectFieldType::InatProjectId => {
                let option_value = if form_value.is_empty() {
                    None
                } else {
                    Some(
                        form_value
                            .parse::<i32>()
                            .map_err(|e| anyhow::anyhow!("Invalid integer format: {}", e))?,
                    )
                };
                crate::project::field::ProjectFieldValue::InatProjectId(option_value)
            }
            crate::project::field::ProjectFieldType::Welcomed => {
                let bool_value = form_value.parse::<bool>().unwrap_or_else(|_| {
                    form_value.to_lowercase() == "true"
                        || form_value == "1"
                        || form_value.to_lowercase() == "on"
                });
                crate::project::field::ProjectFieldValue::Welcomed(bool_value)
            }
            crate::project::field::ProjectFieldType::PageCreatorId => {
                let ulid_value = form_value
                    .parse::<Ulid>()
                    .map_err(|e| anyhow::anyhow!("Invalid ULID format: {}", e))?;
                crate::project::field::ProjectFieldValue::PageCreatorId(ulid_value)
            }
            crate::project::field::ProjectFieldType::PageEditorIds => {
                let ulids: Vec<Ulid> = if form_value.is_empty() {
                    vec![]
                } else {
                    serde_json::from_str(&form_value)
                        .map_err(|e| anyhow::anyhow!("Invalid JSON format for ULID array: {}", e))?
                };
                crate::project::field::ProjectFieldValue::PageEditorIds(ulids.into_boxed_slice())
            }
            crate::project::field::ProjectFieldType::Extra => {
                crate::project::field::ProjectFieldValue::Extra(form_value)
            }
            crate::project::field::ProjectFieldType::Reviewed => {
                let bool_value = form_value.parse::<bool>().unwrap_or_else(|_| {
                    form_value.to_lowercase() == "true"
                        || form_value == "1"
                        || form_value.to_lowercase() == "on"
                });
                crate::project::field::ProjectFieldValue::Reviewed(bool_value)
            }
            crate::project::field::ProjectFieldType::ProjectLastEditor => {
                let option_value = if form_value.is_empty() {
                    None
                } else {
                    Some(
                        form_value
                            .parse::<Ulid>()
                            .map_err(|e| anyhow::anyhow!("Invalid ULID format: {}", e))?,
                    )
                };
                crate::project::field::ProjectFieldValue::ProjectLastEditor(option_value)
            }
            crate::project::field::ProjectFieldType::ProjectLastEdited => {
                let option_value = if form_value.is_empty() {
                    None
                } else {
                    Some(
                        TimeString::try_from(form_value)
                            .map_err(|e| anyhow::anyhow!("Invalid time format: {}", e))?,
                    )
                };
                crate::project::field::ProjectFieldValue::ProjectLastEdited(option_value)
            }
            crate::project::field::ProjectFieldType::ScistarterLastEditor => {
                let option_value = if form_value.is_empty() {
                    None
                } else {
                    Some(
                        form_value
                            .parse::<Ulid>()
                            .map_err(|e| anyhow::anyhow!("Invalid ULID format: {}", e))?,
                    )
                };
                crate::project::field::ProjectFieldValue::ScistarterLastEditor(option_value)
            }
            crate::project::field::ProjectFieldType::ScistarterLastEdited => {
                let option_value = if form_value.is_empty() {
                    None
                } else {
                    Some(
                        TimeString::try_from(form_value)
                            .map_err(|e| anyhow::anyhow!("Invalid time format: {}", e))?,
                    )
                };
                crate::project::field::ProjectFieldValue::ScistarterLastEdited(option_value)
            }
            crate::project::field::ProjectFieldType::Draft => {
                let bool_value = form_value.parse::<bool>().unwrap_or_else(|_| {
                    form_value.to_lowercase() == "true"
                        || form_value == "1"
                        || form_value.to_lowercase() == "on"
                });
                crate::project::field::ProjectFieldValue::Draft(bool_value)
            }
            // Content fields
            crate::project::field::ProjectFieldType::Name => {
                crate::project::field::ProjectFieldValue::Name(form_value)
            }
            crate::project::field::ProjectFieldType::Tags => {
                let tags: HashSet<InsensitiveString> = if form_value.is_empty() {
                    HashSet::new()
                } else {
                    serde_json::from_str(&form_value)
                        .map_err(|e| anyhow::anyhow!("Invalid JSON format for tags: {}", e))?
                };
                crate::project::field::ProjectFieldValue::Tags(tags)
            }
            crate::project::field::ProjectFieldType::ParticipationDegree => {
                let participation_degree = match form_value.as_str() {
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
                crate::project::field::ProjectFieldValue::ParticipantInstructions(form_value)
            }
            crate::project::field::ProjectFieldType::ParticipantInstructionsEmail => {
                crate::project::field::ProjectFieldValue::ParticipantInstructionsEmail(form_value)
            }
            crate::project::field::ProjectFieldType::ParticipantFiles => {
                let files: Vec<String> = if form_value.is_empty() {
                    vec![]
                } else {
                    serde_json::from_str(&form_value).map_err(|e| {
                        anyhow::anyhow!("Invalid JSON format for files array: {}", e)
                    })?
                };
                crate::project::field::ProjectFieldValue::ParticipantFiles(files.into_boxed_slice())
            }
            crate::project::field::ProjectFieldType::UsFederallyFunded => {
                let bool_value = form_value.parse::<bool>().unwrap_or_else(|_| {
                    form_value.to_lowercase() == "true"
                        || form_value == "1"
                        || form_value.to_lowercase() == "on"
                });
                crate::project::field::ProjectFieldValue::UsFederallyFunded(bool_value)
            }
            crate::project::field::ProjectFieldType::Presenter => {
                crate::project::field::ProjectFieldValue::Presenter(form_value)
            }
            crate::project::field::ProjectFieldType::Goal => {
                crate::project::field::ProjectFieldValue::Goal(form_value)
            }
            crate::project::field::ProjectFieldType::Task => {
                crate::project::field::ProjectFieldValue::Task(form_value)
            }
            crate::project::field::ProjectFieldType::Description => {
                crate::project::field::ProjectFieldValue::Description(form_value)
            }
            crate::project::field::ProjectFieldType::Image => {
                crate::project::field::ProjectFieldValue::Image(form_value)
            }
            crate::project::field::ProjectFieldType::ImageCredit => {
                crate::project::field::ProjectFieldValue::ImageCredit(form_value)
            }
            crate::project::field::ProjectFieldType::ConnectOpaqueUrl => {
                crate::project::field::ProjectFieldValue::ConnectOpaqueUrl(form_value)
            }
            crate::project::field::ProjectFieldType::ConnectOpaqueUrl1 => {
                crate::project::field::ProjectFieldValue::ConnectOpaqueUrl1(form_value)
            }
            crate::project::field::ProjectFieldType::ConnectOpaqueUrl2 => {
                crate::project::field::ProjectFieldValue::ConnectOpaqueUrl2(form_value)
            }
            crate::project::field::ProjectFieldType::Contacts => {
                let contacts: Vec<Contact> = if form_value.is_empty() {
                    vec![]
                } else {
                    serde_json::from_str(&form_value).map_err(|e| {
                        anyhow::anyhow!("Invalid JSON format for contacts array: {}", e)
                    })?
                };
                crate::project::field::ProjectFieldValue::Contacts(contacts.into_boxed_slice())
            }
            crate::project::field::ProjectFieldType::Url => {
                crate::project::field::ProjectFieldValue::Url(form_value)
            }
            crate::project::field::ProjectFieldType::VideoUrl => {
                crate::project::field::ProjectFieldValue::VideoUrl(form_value)
            }
            crate::project::field::ProjectFieldType::BlogUrl => {
                crate::project::field::ProjectFieldValue::BlogUrl(form_value)
            }
            crate::project::field::ProjectFieldType::TwitterName => {
                crate::project::field::ProjectFieldValue::TwitterName(form_value)
            }
            crate::project::field::ProjectFieldType::FacebookPage => {
                crate::project::field::ProjectFieldValue::FacebookPage(form_value)
            }
            crate::project::field::ProjectFieldType::InstagramPage => {
                crate::project::field::ProjectFieldValue::InstagramPage(form_value)
            }
            crate::project::field::ProjectFieldType::Topics => {
                let topics: Vec<String> = if form_value.is_empty() {
                    vec![]
                } else {
                    serde_json::from_str(&form_value).map_err(|e| {
                        anyhow::anyhow!("Invalid JSON format for topics array: {}", e)
                    })?
                };
                crate::project::field::ProjectFieldValue::Topics(topics.into_boxed_slice())
            }
            crate::project::field::ProjectFieldType::FieldsOfScience => {
                let fields: Vec<String> = if form_value.is_empty() {
                    vec![]
                } else {
                    serde_json::from_str(&form_value).map_err(|e| {
                        anyhow::anyhow!("Invalid JSON format for fields array: {}", e)
                    })?
                };
                crate::project::field::ProjectFieldValue::FieldsOfScience(fields.into_boxed_slice())
            }
            crate::project::field::ProjectFieldType::Practices => {
                let practices: Vec<String> = if form_value.is_empty() {
                    vec![]
                } else {
                    serde_json::from_str(&form_value).map_err(|e| {
                        anyhow::anyhow!("Invalid JSON format for practices array: {}", e)
                    })?
                };
                crate::project::field::ProjectFieldValue::Practices(practices.into_boxed_slice())
            }
            crate::project::field::ProjectFieldType::PracticesHow => {
                crate::project::field::ProjectFieldValue::PracticesHow(form_value)
            }
            crate::project::field::ProjectFieldType::SearchTerms => {
                crate::project::field::ProjectFieldValue::SearchTerms(form_value)
            }
            crate::project::field::ProjectFieldType::Difficulty => {
                crate::project::field::ProjectFieldValue::Difficulty(form_value)
            }
            crate::project::field::ProjectFieldType::Diy => {
                let bool_value = form_value.parse::<bool>().unwrap_or_else(|_| {
                    form_value.to_lowercase() == "true"
                        || form_value == "1"
                        || form_value.to_lowercase() == "on"
                });
                crate::project::field::ProjectFieldValue::Diy(bool_value)
            }
            crate::project::field::ProjectFieldType::Audience => {
                let audience: Vec<String> = if form_value.is_empty() {
                    vec![]
                } else {
                    serde_json::from_str(&form_value).map_err(|e| {
                        anyhow::anyhow!("Invalid JSON format for audience array: {}", e)
                    })?
                };
                crate::project::field::ProjectFieldValue::Audience(audience.into_boxed_slice())
            }
            crate::project::field::ProjectFieldType::SignupUrl => {
                crate::project::field::ProjectFieldValue::SignupUrl(form_value)
            }
            crate::project::field::ProjectFieldType::PrefixHowToLoggedIn => {
                crate::project::field::ProjectFieldValue::PrefixHowToLoggedIn(form_value)
            }
            crate::project::field::ProjectFieldType::PrefixHowToLoggedOut => {
                crate::project::field::ProjectFieldValue::PrefixHowToLoggedOut(form_value)
            }
            crate::project::field::ProjectFieldType::HowToJoin => {
                crate::project::field::ProjectFieldValue::HowToJoin(form_value)
            }
            crate::project::field::ProjectFieldType::HowtoVideo => {
                crate::project::field::ProjectFieldValue::HowtoVideo(form_value)
            }
            crate::project::field::ProjectFieldType::OutcomesLabel => {
                crate::project::field::ProjectFieldValue::OutcomesLabel(form_value)
            }
            crate::project::field::ProjectFieldType::OutcomesLink => {
                crate::project::field::ProjectFieldValue::OutcomesLink(form_value)
            }
            crate::project::field::ProjectFieldType::TimeRequired => {
                crate::project::field::ProjectFieldValue::TimeRequired(form_value)
            }
            crate::project::field::ProjectFieldType::TrainingMaterials => {
                crate::project::field::ProjectFieldValue::TrainingMaterials(form_value)
            }
            crate::project::field::ProjectFieldType::ClassroomMaterials => {
                crate::project::field::ProjectFieldValue::ClassroomMaterials(form_value)
            }
            crate::project::field::ProjectFieldType::ConsentStatement => {
                crate::project::field::ProjectFieldValue::ConsentStatement(form_value)
            }
            crate::project::field::ProjectFieldType::ConsentStatementDate => {
                let option_value = if form_value.is_empty() {
                    None
                } else {
                    Some(
                        TimeString::try_from(form_value)
                            .map_err(|e| anyhow::anyhow!("Invalid time format: {}", e))?,
                    )
                };
                crate::project::field::ProjectFieldValue::ConsentStatementDate(option_value)
            }
            crate::project::field::ProjectFieldType::ParticipationTools => {
                let bool_value = form_value.parse::<bool>().unwrap_or_else(|_| {
                    form_value.to_lowercase() == "true"
                        || form_value == "1"
                        || form_value.to_lowercase() == "on"
                });
                crate::project::field::ProjectFieldValue::ParticipationTools(bool_value)
            }
            crate::project::field::ProjectFieldType::ParticipationPrompt => {
                crate::project::field::ProjectFieldValue::ParticipationPrompt(form_value)
            }
            crate::project::field::ProjectFieldType::ParticipationConfirmationMessage => {
                crate::project::field::ProjectFieldValue::ParticipationConfirmationMessage(
                    form_value,
                )
            }
            crate::project::field::ProjectFieldType::ParticipationNotificationEmail => {
                crate::project::field::ProjectFieldValue::ParticipationNotificationEmail(form_value)
            }
            crate::project::field::ProjectFieldType::ParticipationUsage => {
                crate::project::field::ProjectFieldValue::ParticipationUsage(form_value)
            }
            crate::project::field::ProjectFieldType::PartOf => {
                let option_value = if form_value.is_empty() {
                    None
                } else {
                    Some(
                        form_value
                            .parse::<Ulid>()
                            .map_err(|e| anyhow::anyhow!("Invalid ULID format: {}", e))?,
                    )
                };
                crate::project::field::ProjectFieldValue::PartOf(option_value)
            }
            crate::project::field::ProjectFieldType::AssociatedProjects => {
                let associated_projects: Vec<Ulid> = if form_value.is_empty() {
                    vec![]
                } else {
                    serde_json::from_str(&form_value)
                        .map_err(|e| anyhow::anyhow!("Invalid JSON format for ULID array: {}", e))?
                };
                crate::project::field::ProjectFieldValue::AssociatedProjects(
                    associated_projects.into_boxed_slice(),
                )
            }
            crate::project::field::ProjectFieldType::TotalExpense => {
                let option_value = if form_value.is_empty() {
                    None
                } else {
                    Some(
                        form_value
                            .parse::<f32>()
                            .map_err(|e| anyhow::anyhow!("Invalid float format: {}", e))?,
                    )
                };
                crate::project::field::ProjectFieldValue::TotalExpense(option_value)
            }
            crate::project::field::ProjectFieldType::Materials => {
                crate::project::field::ProjectFieldValue::Materials(form_value)
            }
            crate::project::field::ProjectFieldType::Gear => {
                crate::project::field::ProjectFieldValue::Gear(form_value)
            }
            crate::project::field::ProjectFieldType::MobileAppUrlAndroid => {
                crate::project::field::ProjectFieldValue::MobileAppUrlAndroid(form_value)
            }
            crate::project::field::ProjectFieldType::MobileAppUrlIos => {
                crate::project::field::ProjectFieldValue::MobileAppUrlIos(form_value)
            }
            crate::project::field::ProjectFieldType::MobileAppUrl => {
                crate::project::field::ProjectFieldValue::MobileAppUrl(form_value)
            }
            // Location fields
            crate::project::field::ProjectFieldType::LocationType => {
                crate::project::field::ProjectFieldValue::LocationType(form_value)
            }
            crate::project::field::ProjectFieldType::EntirelyOnline => {
                let bool_value = form_value.parse::<bool>().unwrap_or_else(|_| {
                    form_value.to_lowercase() == "true"
                        || form_value == "1"
                        || form_value.to_lowercase() == "on"
                });
                crate::project::field::ProjectFieldValue::EntirelyOnline(bool_value)
            }
            crate::project::field::ProjectFieldType::OnDemand => {
                let bool_value = form_value.parse::<bool>().unwrap_or_else(|_| {
                    form_value.to_lowercase() == "true"
                        || form_value == "1"
                        || form_value.to_lowercase() == "on"
                });
                crate::project::field::ProjectFieldValue::OnDemand(bool_value)
            }
            crate::project::field::ProjectFieldType::InPerson => {
                let bool_value = form_value.parse::<bool>().unwrap_or_else(|_| {
                    form_value.to_lowercase() == "true"
                        || form_value == "1"
                        || form_value.to_lowercase() == "on"
                });
                crate::project::field::ProjectFieldValue::InPerson(bool_value)
            }
            crate::project::field::ProjectFieldType::Indoors => {
                let bool_value = form_value.parse::<bool>().unwrap_or_else(|_| {
                    form_value.to_lowercase() == "true"
                        || form_value == "1"
                        || form_value.to_lowercase() == "on"
                });
                crate::project::field::ProjectFieldValue::Indoors(bool_value)
            }
            crate::project::field::ProjectFieldType::Outdoors => {
                let bool_value = form_value.parse::<bool>().unwrap_or_else(|_| {
                    form_value.to_lowercase() == "true"
                        || form_value == "1"
                        || form_value.to_lowercase() == "on"
                });
                crate::project::field::ProjectFieldValue::Outdoors(bool_value)
            }
            crate::project::field::ProjectFieldType::Regions => {
                let option_value = if form_value.is_empty() {
                    None
                } else {
                    Some(form_value)
                };
                crate::project::field::ProjectFieldValue::Regions(option_value)
            }
            crate::project::field::ProjectFieldType::RegionsDescription => {
                crate::project::field::ProjectFieldValue::RegionsDescription(form_value)
            }
            crate::project::field::ProjectFieldType::Point => {
                let option_value = if form_value.is_empty() {
                    None
                } else {
                    Some(form_value)
                };
                crate::project::field::ProjectFieldValue::Point(option_value)
            }
            crate::project::field::ProjectFieldType::Address => {
                crate::project::field::ProjectFieldValue::Address(form_value)
            }
            crate::project::field::ProjectFieldType::City => {
                crate::project::field::ProjectFieldValue::City(form_value)
            }
            crate::project::field::ProjectFieldType::State => {
                crate::project::field::ProjectFieldValue::State(form_value)
            }
            crate::project::field::ProjectFieldType::Zip => {
                crate::project::field::ProjectFieldValue::Zip(form_value)
            }
            crate::project::field::ProjectFieldType::Country => {
                crate::project::field::ProjectFieldValue::Country(form_value)
            }
            crate::project::field::ProjectFieldType::Other => {
                crate::project::field::ProjectFieldValue::Other(form_value)
            }
            crate::project::field::ProjectFieldType::LocationName => {
                crate::project::field::ProjectFieldValue::LocationName(form_value)
            }
            crate::project::field::ProjectFieldType::LocationLanduse => {
                crate::project::field::ProjectFieldValue::LocationLanduse(form_value)
            }
            crate::project::field::ProjectFieldType::LocationDescription => {
                crate::project::field::ProjectFieldValue::LocationDescription(form_value)
            }
            crate::project::field::ProjectFieldType::GeographicScope => {
                let scope: Vec<String> = if form_value.is_empty() {
                    vec![]
                } else {
                    serde_json::from_str(&form_value).map_err(|e| {
                        anyhow::anyhow!("Invalid JSON format for scope array: {}", e)
                    })?
                };
                crate::project::field::ProjectFieldValue::GeographicScope(scope.into_boxed_slice())
            }
            crate::project::field::ProjectFieldType::UnitedNationsRegion => {
                crate::project::field::ProjectFieldValue::UnitedNationsRegion(form_value)
            }
            // Timing fields
            crate::project::field::ProjectFieldType::Begin => {
                let option_value = if form_value.is_empty() {
                    None
                } else {
                    Some(
                        TimeString::try_from(form_value)
                            .map_err(|e| anyhow::anyhow!("Invalid time format: {}", e))?,
                    )
                };
                crate::project::field::ProjectFieldValue::Begin(option_value)
            }
            crate::project::field::ProjectFieldType::End => {
                let option_value = if form_value.is_empty() {
                    None
                } else {
                    Some(
                        TimeString::try_from(form_value)
                            .map_err(|e| anyhow::anyhow!("Invalid time format: {}", e))?,
                    )
                };
                crate::project::field::ProjectFieldValue::End(option_value)
            }
            crate::project::field::ProjectFieldType::ProjectTiming => {
                crate::project::field::ProjectFieldValue::ProjectTiming(form_value)
            }
            crate::project::field::ProjectFieldType::ParticipationSchedule => {
                crate::project::field::ProjectFieldValue::ParticipationSchedule(form_value)
            }
            crate::project::field::ProjectFieldType::TimePerParticipation => {
                crate::project::field::ProjectFieldValue::TimePerParticipation(form_value)
            }
            crate::project::field::ProjectFieldType::YearStarted => {
                let option_value = if form_value.is_empty() {
                    None
                } else {
                    Some(
                        form_value
                            .parse::<i32>()
                            .map_err(|e| anyhow::anyhow!("Invalid integer format: {}", e))?,
                    )
                };
                crate::project::field::ProjectFieldValue::YearStarted(option_value)
            }
            crate::project::field::ProjectFieldType::YearCompleted => {
                let option_value = if form_value.is_empty() {
                    None
                } else {
                    Some(
                        form_value
                            .parse::<i32>()
                            .map_err(|e| anyhow::anyhow!("Invalid integer format: {}", e))?,
                    )
                };
                crate::project::field::ProjectFieldValue::YearCompleted(option_value)
            }
            crate::project::field::ProjectFieldType::MonthCompleted => {
                let option_value = if form_value.is_empty() {
                    None
                } else {
                    Some(
                        form_value
                            .parse::<i32>()
                            .map_err(|e| anyhow::anyhow!("Invalid integer format: {}", e))?,
                    )
                };
                crate::project::field::ProjectFieldValue::MonthCompleted(option_value)
            }
            crate::project::field::ProjectFieldType::ParticipationTasks => {
                let tasks: Vec<String> = if form_value.is_empty() {
                    vec![]
                } else {
                    serde_json::from_str(&form_value).map_err(|e| {
                        anyhow::anyhow!("Invalid JSON format for tasks array: {}", e)
                    })?
                };
                crate::project::field::ProjectFieldValue::ParticipationTasks(
                    tasks.into_boxed_slice(),
                )
            }
            crate::project::field::ProjectFieldType::Activities => {
                let activities: Vec<String> = if form_value.is_empty() {
                    vec![]
                } else {
                    serde_json::from_str(&form_value).map_err(|e| {
                        anyhow::anyhow!("Invalid JSON format for activities array: {}", e)
                    })?
                };
                crate::project::field::ProjectFieldValue::Activities(activities.into_boxed_slice())
            }
            crate::project::field::ProjectFieldType::SpecialSkills => {
                crate::project::field::ProjectFieldValue::SpecialSkills(form_value)
            }
            // Data fields
            crate::project::field::ProjectFieldType::DataResults => {
                crate::project::field::ProjectFieldValue::DataResults(form_value)
            }
            crate::project::field::ProjectFieldType::DataAggregation => {
                let aggregation: Vec<String> = if form_value.is_empty() {
                    vec![]
                } else {
                    serde_json::from_str(&form_value).map_err(|e| {
                        anyhow::anyhow!("Invalid JSON format for aggregation array: {}", e)
                    })?
                };
                crate::project::field::ProjectFieldValue::DataAggregation(
                    aggregation.into_boxed_slice(),
                )
            }
            crate::project::field::ProjectFieldType::DataConsent => {
                let consent: Vec<String> = if form_value.is_empty() {
                    vec![]
                } else {
                    serde_json::from_str(&form_value).map_err(|e| {
                        anyhow::anyhow!("Invalid JSON format for consent array: {}", e)
                    })?
                };
                crate::project::field::ProjectFieldValue::DataConsent(consent.into_boxed_slice())
            }
            crate::project::field::ProjectFieldType::DataVisualization => {
                let bool_value = form_value.parse::<bool>().unwrap_or_else(|_| {
                    form_value.to_lowercase() == "true"
                        || form_value == "1"
                        || form_value.to_lowercase() == "on"
                });
                crate::project::field::ProjectFieldValue::DataVisualization(bool_value)
            }
            crate::project::field::ProjectFieldType::DataAccess => {
                let access: Vec<String> = if form_value.is_empty() {
                    vec![]
                } else {
                    serde_json::from_str(&form_value).map_err(|e| {
                        anyhow::anyhow!("Invalid JSON format for access array: {}", e)
                    })?
                };
                crate::project::field::ProjectFieldValue::DataAccess(access.into_boxed_slice())
            }
            crate::project::field::ProjectFieldType::DataQuality => {
                let bool_value = form_value.parse::<bool>().unwrap_or_else(|_| {
                    form_value.to_lowercase() == "true"
                        || form_value == "1"
                        || form_value.to_lowercase() == "on"
                });
                crate::project::field::ProjectFieldValue::DataQuality(bool_value)
            }
            crate::project::field::ProjectFieldType::MetadataPublicationUrl => {
                crate::project::field::ProjectFieldValue::MetadataPublicationUrl(form_value)
            }
            crate::project::field::ProjectFieldType::DataPublicationUrl => {
                crate::project::field::ProjectFieldValue::DataPublicationUrl(form_value)
            }
            crate::project::field::ProjectFieldType::DataLicense => {
                crate::project::field::ProjectFieldValue::DataLicense(form_value)
            }
            crate::project::field::ProjectFieldType::IntendedOutcomes => {
                let outcomes: Vec<String> = if form_value.is_empty() {
                    vec![]
                } else {
                    serde_json::from_str(&form_value).map_err(|e| {
                        anyhow::anyhow!("Invalid JSON format for outcomes array: {}", e)
                    })?
                };
                crate::project::field::ProjectFieldValue::IntendedOutcomes(
                    outcomes.into_boxed_slice(),
                )
            }
            crate::project::field::ProjectFieldType::OutcomeOther => {
                crate::project::field::ProjectFieldValue::OutcomeOther(form_value)
            }
            crate::project::field::ProjectFieldType::UniversityAffiliation => {
                crate::project::field::ProjectFieldValue::UniversityAffiliation(form_value)
            }
            crate::project::field::ProjectFieldType::Sponsors => {
                let sponsors: Vec<String> = if form_value.is_empty() {
                    vec![]
                } else {
                    serde_json::from_str(&form_value).map_err(|e| {
                        anyhow::anyhow!("Invalid JSON format for sponsors array: {}", e)
                    })?
                };
                crate::project::field::ProjectFieldValue::Sponsors(sponsors.into_boxed_slice())
            }
            crate::project::field::ProjectFieldType::Stakeholders => {
                let stakeholders: Vec<String> = if form_value.is_empty() {
                    vec![]
                } else {
                    serde_json::from_str(&form_value).map_err(|e| {
                        anyhow::anyhow!("Invalid JSON format for stakeholders array: {}", e)
                    })?
                };
                crate::project::field::ProjectFieldValue::Stakeholders(
                    stakeholders.into_boxed_slice(),
                )
            }
            crate::project::field::ProjectFieldType::CircuitId => {
                crate::project::field::ProjectFieldValue::CircuitId(form_value)
            }
            crate::project::field::ProjectFieldType::SustainableDevelopmentGoals => {
                crate::project::field::ProjectFieldValue::SustainableDevelopmentGoals(form_value)
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

            // If this is from the list view (HX-Target starts with "project-row-"), return the full row
            if is_list_view {
                // Return the full project row for list view
                let row = sqlx::query!(
                    r#"SELECT p.project_id, p.name, p.status, p.created, p.presenter,
                              p.origin, p.flagged, p.duplicate, p.is_test, p.goal, p.task, p.image,
                              p.url, p.admin_notes, p.draft, p.reviewed, p.description,
                              COALESCE(ent.publish, 0) as "published!: bool"
                       FROM projects p
                       LEFT JOIN entities ent ON ent.target = p.project_id AND ent.kind = 'Project'
                       WHERE p.project_id = ?"#,
                    project_id_str
                )
                .fetch_one(db())
                .await?;

                let project = AdminProject {
                    id: row.project_id,
                    title: row.name,
                    status: row.status,
                    created_date: row.created,
                    page_creator_id: String::new(),
                    page_creator_email: None,
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
                    reviewed: row.reviewed,
                    published: row.published,
                    tags: Vec::new(),
                };

                let project_row = ProjectRow {
                    project,
                    oob: false,
                    oob_swap: String::new(),
                };

                return Ok(poem::Response::builder()
                    .content_type("text/html")
                    .body(project_row.render_once()?));
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
            let entity_with_publish = crate::entity::Entity::new_with_publish(
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

            // Fire project submission triggers
            trigger_email(
                TriggerEvent::ProjectSubmitted,
                user_info.user_id,
                Some(&[
                    ("project_name".to_string(), form.name.clone()),
                    ("project_id".to_string(), project_id.to_string()),
                ]),
            )
            .await
            .ok();

            trigger_email(
                TriggerEvent::ProjectCreatedStaff,
                user_info.user_id,
                Some(&[
                    ("project_name".to_string(), form.name.clone()),
                    ("project_id".to_string(), project_id.to_string()),
                ]),
            )
            .await
            .ok();

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
