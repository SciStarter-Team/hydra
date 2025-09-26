use std::collections::HashSet;

use anyhow::Error;
use common::{
    insensitivestring::InsensitiveString, shared::db, timestring::TimeString,
    transaction::Immediate,
};
use ulid::Ulid;

use crate::{
    contact::{Contact, ContactRole},
    peering::LoadStorePeered,
};

use super::{ProjectParticipationDegree, ProjectPromotionTime, ProjectStatus};

/// Container for all project fields.
///
/// This structure contains a representation of all project data. Its
/// primary purpose is for receiving data synchronizations. It should
/// not be used casually for other purposes, since there are more
/// efficient options in most cases.
///
/// Note: The `tags_text` database column is NOT included here as it's
/// a computed field for FTS search optimization, not a real project field.
/// Tags are managed separately via the project_tags table.
#[derive(Default, Debug, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ProjectAll {
    pub project_id: Ulid,
    pub legacy_id: Option<i64>,
    pub legacy_v1_project_id: Option<i64>,
    pub legacy_v2_project_id: Option<i64>,
    pub foreign_id: Option<String>,

    // section Admin
    pub publish: bool,
    pub duplicate: bool,
    pub authoritative: bool,
    pub flagged: bool,
    pub is_test: bool,
    pub status: ProjectStatus,
    pub promotion_time: ProjectPromotionTime,
    pub promotion_target: Option<TimeString>,
    pub admin_notes: String,
    pub created: TimeString,
    pub wild_hope: bool,
    pub exchange: Option<String>,
    pub origin: String,
    pub suborigin: String,
    pub guid: String,
    pub opengraph_description: String,
    pub is_affiliate: bool,
    pub is_inaturalist: bool,
    pub inat_project_id: Option<i32>,
    pub welcomed: bool,
    pub page_creator_id: Ulid,        // NB was "owner"
    pub page_editor_ids: Box<[Ulid]>, // NB was "managers"
    pub extra: String,
    pub project_last_editor: Option<Ulid>,
    pub project_last_edited: Option<TimeString>,
    pub scistarter_last_editor: Option<Ulid>,
    pub scistarter_last_edited: Option<TimeString>,

    // section Project
    pub draft: bool,
    pub name: String,
    pub tags: HashSet<InsensitiveString>,
    pub participation_degree: ProjectParticipationDegree,
    pub participant_instructions: String,
    pub participant_instructions_email: String,
    pub participant_files: Box<[String]>,
    pub us_federally_funded: bool,
    pub presenter: String,
    pub goal: String,
    pub task: String,
    pub description: String,
    pub image: String,
    pub image_credit: String,
    pub connect_opaque_url: String,
    pub connect_opaque_url1: String,
    pub connect_opaque_url2: String,

    // section Contact
    pub contacts: Box<[Contact]>,

    // section Web
    pub url: String,
    pub video_url: String,
    pub blog_url: String,
    pub twitter_name: String,
    pub facebook_page: String,
    pub instagram_page: String,

    // section Categorization
    pub topics: Box<[String]>,
    pub fields_of_science: Box<[String]>,
    pub practices: Box<[String]>,
    pub practices_how: String,
    pub search_terms: String,
    pub difficulty: String, // choices = [[1, "Easy"], [2, "medium"], [3, "Difficult"]]
    pub diy: bool,

    // section Joining
    pub audience: Box<[String]>,
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
    pub consent_statement_date: Option<TimeString>,
    pub participation_tools: bool, // deprecated, maybe don't implement
    pub participation_prompt: String, // deprecated, maybe don't implement
    pub participation_confirmation_message: String, // deprecated, maybe don't implement
    pub participation_notification_email: String, // deprecated, maybe don't implement
    pub participation_usage: String, // deprecated, maybe don't implement

    // section Relations
    pub part_of: Option<Ulid>,
    pub associated_projects: Box<[Ulid]>,

    // section Participants Need
    pub total_expense: Option<f32>,
    pub materials: String,
    pub gear: String,
    pub mobile_app_url_android: String,
    pub mobile_app_url_ios: String,
    pub mobile_app_url: String,

    // section Where
    pub location_type: String, // choices = ["Global, anywhere on the planet", "In a geographic location or range", "At a specific address"]
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
    pub geographic_scope: Box<[String]>,
    pub united_nations_region: String,

    // section When
    pub begin: Option<TimeString>,
    pub end: Option<TimeString>,
    pub project_timing: String,
    pub participation_schedule: String,
    pub time_per_participation: String,
    pub year_started: Option<i32>,
    pub year_completed: Option<i32>,
    pub month_completed: Option<i32>,

    // section Skills
    pub participation_tasks: Box<[String]>,
    pub activities: Box<[String]>,
    pub special_skills: String,

    // section Data
    pub data_results: String,
    pub data_aggregation: Box<[String]>,
    pub data_consent: Box<[String]>,
    pub data_visualization: bool,
    pub data_access: Box<[String]>,
    pub data_quality: bool,
    pub metadata_publication_url: String,
    pub data_publication_url: String,
    pub data_license: String,

    // section Outcomes
    pub intended_outcomes: Box<[String]>,
    pub outcome_other: String,

    // section Sponsors
    pub university_affiliation: String,
    pub sponsors: Box<[String]>,
    pub stakeholders: Box<[String]>,
    pub circuit_id: String,
    pub sustainable_development_goals: String,
}

impl LoadStorePeered for ProjectAll {
    //type DatabaseKey<'k> = Ulid;
    type DatabaseKey = Ulid;

    const GROUP: &str = "project";
    const SPECIFIC: &str = "all";

    #[tracing::instrument]
    async fn save_to_database(&self) -> Result<(), Error> {
        let mut tx = Immediate::begin_with_retry(db(), None, None).await?;

        //let mut tx = db().begin().await?;

        let project_id = self.project_id.to_string();
        let page_creator_id = self.page_creator_id.to_string();
        let part_of = self.part_of.map(|u| u.to_string());

        sqlx::query!(
            r#"insert or replace into projects (
          "project_id",
          "legacy_id",
          "legacy_v1_project_id",
          "legacy_v2_project_id",
          "foreign_id",
          "publish",
          "duplicate",
          "authoritative",
          "flagged",
          "is_test",
          "status",
          "promotion_time",
          "promotion_target",
          "admin_notes",
          "created",
          "wild_hope",
          "exchange",
          "origin",
          "suborigin",
          "guid",
          "opengraph_description",
          "is_affiliate",
          "is_inaturalist",
          "inat_project_id",
          "welcomed",
          "page_creator_id",
          "extra",
          "draft",
          "name",
          "participation_degree",
          "participant_instructions",
          "participant_instructions_email",
          "us_federally_funded",
          "presenter",
          "goal",
          "task",
          "description",
          "image",
          "image_credit",
          "connect_opaque_url",
          "connect_opaque_url1",
          "connect_opaque_url2",
          "url",
          "video_url",
          "blog_url",
          "twitter_name",
          "facebook_page",
          "instagram_page",
          "practices_how",
          "search_terms",
          "difficulty",
          "diy",
          "signup_url",
          "prefix_how_to_logged_in",
          "prefix_how_to_logged_out",
          "how_to_join",
          "howto_video",
          "outcomes_label",
          "outcomes_link",
          "time_required",
          "training_materials",
          "classroom_materials",
          "consent_statement",
          "consent_statement_date",
          "participation_tools",
          "participation_prompt",
          "participation_confirmation_message",
          "participation_notification_email",
          "participation_usage",
          "part_of",
          "total_expense",
          "materials",
          "gear",
          "mobile_app_url_android",
          "mobile_app_url_ios",
          "mobile_app_url",
          "location_type",
          "entirely_online",
          "on_demand",
          "in_person",
          "indoors",
          "outdoors",
          "regions_description",
          "address",
          "city",
          "state",
          "zip",
          "country",
          "other",
          "location_name",
          "location_landuse",
          "location_description",
          "united_nations_region",
          "begin",
          "end",
          "project_timing",
          "participation_schedule",
          "time_per_participation",
          "year_started",
          "year_completed",
          "month_completed",
          "special_skills",
          "data_results",
          "data_visualization",
          "data_quality",
          "metadata_publication_url",
          "data_publication_url",
          "data_license",
          "outcome_other",
          "university_affiliation",
          "circuit_id",
          "sustainable_development_goals",
          "regions",
          "point"
        ) values (
          ?1,
          ?2,
          ?2,
          ?3,
          ?4,
          ?5,
          ?6,
          ?7,
          ?8,
          ?9,
          ?10,
          ?11,
          ?12,
          ?13,
          ?14,
          ?15,
          ?16,
          ?17,
          ?18,
          ?19,
          ?20,
          ?21,
          ?22,
          ?23,
          ?24,
          ?25,
          ?26,
          ?27,
          ?28,
          ?29,
          ?30,
          ?31,
          ?32,
          ?33,
          ?34,
          ?35,
          ?36,
          ?37,
          ?38,
          ?39,
          ?40,
          ?41,
          ?42,
          ?43,
          ?44,
          ?45,
          ?46,
          ?47,
          ?48,
          ?49,
          ?50,
          ?51,
          ?52,
          ?53,
          ?54,
          ?55,
          ?56,
          ?57,
          ?58,
          ?59,
          ?60,
          ?61,
          ?62,
          ?63,
          ?64,
          ?65,
          ?66,
          ?67,
          ?68,
          ?69,
          ?70,
          ?71,
          ?72,
          ?73,
          ?74,
          ?75,
          ?76,
          ?77,
          ?78,
          ?79,
          ?80,
          ?81,
          ?82,
          ?83,
          ?84,
          ?85,
          ?86,
          ?87,
          ?88,
          ?89,
          ?90,
          ?91,
          ?92,
          ?93,
          ?94,
          ?95,
          ?96,
          ?97,
          ?98,
          ?99,
          ?100,
          ?101,
          ?102,
          ?103,
          ?104,
          ?105,
          ?106,
          ?107,
          ?108,
          ?109,
          ?110,
          ?111,
          case
            when ?112 is null then
              null
            when substr(?112, 1, 1) == '{' and substr(?112, -1, 1) == '}' then
              GeomFromGeoJSON(?112)
            else
              GeomFromEWKT(?112)
          end,
          case
            when ?113 is null then
              null
            when substr(?113, 1, 1) == '{' and substr(?113, -1, 1) == '}' then
              GeomFromGeoJSON(?113)
            else
              GeomFromEWKT(?113)
          end
        )"#,
            project_id,
            self.legacy_v1_project_id,
            self.legacy_v2_project_id,
            self.foreign_id,
            self.publish,
            self.duplicate,
            self.authoritative,
            self.flagged,
            self.is_test,
            self.status,
            self.promotion_time,
            self.promotion_target,
            self.admin_notes,
            self.created,
            self.wild_hope,
            self.exchange,
            self.origin,
            self.suborigin,
            self.guid,
            self.opengraph_description,
            self.is_affiliate,
            self.is_inaturalist,
            self.inat_project_id,
            self.welcomed,
            page_creator_id,
            self.extra,
            self.draft,
            self.name,
            self.participation_degree,
            self.participant_instructions,
            self.participant_instructions_email,
            self.us_federally_funded,
            self.presenter,
            self.goal,
            self.task,
            self.description,
            self.image,
            self.image_credit,
            self.connect_opaque_url,
            self.connect_opaque_url1,
            self.connect_opaque_url2,
            self.url,
            self.video_url,
            self.blog_url,
            self.twitter_name,
            self.facebook_page,
            self.instagram_page,
            self.practices_how,
            self.search_terms,
            self.difficulty,
            self.diy,
            self.signup_url,
            self.prefix_how_to_logged_in,
            self.prefix_how_to_logged_out,
            self.how_to_join,
            self.howto_video,
            self.outcomes_label,
            self.outcomes_link,
            self.time_required,
            self.training_materials,
            self.classroom_materials,
            self.consent_statement,
            self.consent_statement_date,
            self.participation_tools,
            self.participation_prompt,
            self.participation_confirmation_message,
            self.participation_notification_email,
            self.participation_usage,
            part_of,
            self.total_expense,
            self.materials,
            self.gear,
            self.mobile_app_url_android,
            self.mobile_app_url_ios,
            self.mobile_app_url,
            self.location_type,
            self.entirely_online,
            self.on_demand,
            self.in_person,
            self.indoors,
            self.outdoors,
            self.regions_description,
            self.address,
            self.city,
            self.state,
            self.zip,
            self.country,
            self.other,
            self.location_name,
            self.location_landuse,
            self.location_description,
            self.united_nations_region,
            self.begin,
            self.end,
            self.project_timing,
            self.participation_schedule,
            self.time_per_participation,
            self.year_started,
            self.year_completed,
            self.month_completed,
            self.special_skills,
            self.data_results,
            self.data_visualization,
            self.data_quality,
            self.metadata_publication_url,
            self.data_publication_url,
            self.data_license,
            self.outcome_other,
            self.university_affiliation,
            self.circuit_id,
            self.sustainable_development_goals,
            self.regions,
            self.point,
        )
        .execute(&mut tx)
        .await?;

        // The following all have to be fully written out, because there's
        // no way to parameterize the table and field names in a
        // compile-time checked SQLx query. We can't even use a macro to
        // manage it, because the query! macro requires a literal token
        // for the query, and we can't construct those at compile
        // time. Revisit this later.

        // "page_editor_ids",
        sqlx::query!(
            r#"delete from project_page_editors where project_id = ?"#,
            project_id
        )
        .execute(&mut tx)
        .await?;

        for user_id in self.page_editor_ids.iter() {
            let user_id = user_id.to_string();
            sqlx::query!(
                r#"insert into project_page_editors (
              "project_id",
              "user_id"
            )
            values (
              ?,
              ?
            )"#,
                project_id,
                user_id,
            )
            .execute(&mut tx)
            .await?;
        }

        // "project_last_editor",
        // "project_last_edited",
        if let Some(last_editor) = self.project_last_editor {
            common::service::log_activity(
                self.project_last_edited.clone(),
                "user-edit",
                last_editor,
                self.project_id,
                Some("all".to_string()),
            )
            .await?;
        }

        // "scistarter_last_editor",
        // "scistarter_last_edited",
        if let Some(last_editor) = self.scistarter_last_editor {
            common::service::log_activity(
                self.scistarter_last_edited.clone(),
                "staff-edit",
                last_editor,
                self.project_id,
                Some("all".to_string()),
            )
            .await?;
        }

        // "tags",
        sqlx::query!(
            r#"delete from project_tags where project_id = ?"#,
            project_id
        )
        .execute(&mut tx)
        .await?;

        for val in self.tags.iter() {
            let val = val.as_ref();

            sqlx::query!(
                r#"insert into project_tags (
              "project_id",
              "tag"
            )
            values (
              ?,
              ?
            )"#,
                project_id,
                val,
            )
            .execute(&mut tx)
            .await?;
        }

        // "participant_files",
        sqlx::query!(
            r#"delete from project_participant_files where project_id = ?"#,
            project_id
        )
        .execute(&mut tx)
        .await?;

        for val in self.participant_files.iter() {
            sqlx::query!(
                r#"insert into project_participant_files (
              "project_id",
              "url"
            )
            values (
              ?,
              ?
            )"#,
                project_id,
                val,
            )
            .execute(&mut tx)
            .await?;
        }

        // "contacts",
        sqlx::query!(
            r#"delete from project_contacts where project_id = ?"#,
            project_id
        )
        .execute(&mut tx)
        .await?;

        for contact in self.contacts.iter() {
            let contact_id = contact.contact_id.to_string();
            sqlx::query!(
                r#"insert into project_contacts (
              "contact_id",
              "project_id",
              "role",
              "name",
              "affiliation",
              "email",
              "phone",
              "address"
            )
            values (
              ?,
              ?,
              ?,
              ?,
              ?,
              ?,
              ?,
              ?
            )"#,
                contact_id,
                project_id,
                contact.role,
                contact.name,
                contact.affiliation,
                contact.email,
                contact.phone,
                contact.address,
            )
            .execute(&mut tx)
            .await?;
        }

        // "topics",
        sqlx::query!(
            r#"delete from project_topics where project_id = ?"#,
            project_id
        )
        .execute(&mut tx)
        .await?;

        for val in self.topics.iter() {
            sqlx::query!(
                r#"insert into project_topics (
              "project_id",
              "topic"
            )
            values (
              ?,
              ?
            )"#,
                project_id,
                val,
            )
            .execute(&mut tx)
            .await?;
        }

        // "fields_of_science",
        sqlx::query!(
            r#"delete from project_fields_of_science where project_id = ?"#,
            project_id
        )
        .execute(&mut tx)
        .await?;

        for val in self.fields_of_science.iter() {
            sqlx::query!(
                r#"insert into project_fields_of_science (
              "project_id",
              "field_of_science"
            )
            values (
              ?,
              ?
            )"#,
                project_id,
                val,
            )
            .execute(&mut tx)
            .await?;
        }

        // "practices",
        sqlx::query!(
            r#"delete from project_practices where project_id = ?"#,
            project_id
        )
        .execute(&mut tx)
        .await?;

        for val in self.practices.iter() {
            sqlx::query!(
                r#"insert into project_practices (
              "project_id",
              "practice"
            )
            values (
              ?,
              ?
            )"#,
                project_id,
                val,
            )
            .execute(&mut tx)
            .await?;
        }

        // "audience",
        sqlx::query!(
            r#"delete from project_audiences where project_id = ?"#,
            project_id
        )
        .execute(&mut tx)
        .await?;

        for val in self.audience.iter() {
            sqlx::query!(
                r#"insert into project_audiences (
              "project_id",
              "audience"
            )
            values (
              ?,
              ?
            )"#,
                project_id,
                val,
            )
            .execute(&mut tx)
            .await?;
        }

        // "associated_projects",
        sqlx::query!(
            r#"delete from project_associated_projects where project_id = ?"#,
            project_id
        )
        .execute(&mut tx)
        .await?;

        for assoc_id in self.associated_projects.iter() {
            let assoc_id = assoc_id.to_string();
            sqlx::query!(
                r#"insert into project_associated_projects (
              "project_id",
              "associated_project_id"
            )
            values (
              ?,
              ?
            )"#,
                project_id,
                assoc_id,
            )
            .execute(&mut tx)
            .await?;
        }

        // "geographic_scope",
        sqlx::query!(
            r#"delete from project_geographic_scopes where project_id = ?"#,
            project_id
        )
        .execute(&mut tx)
        .await?;

        for val in self.geographic_scope.iter() {
            sqlx::query!(
                r#"insert into project_geographic_scopes (
              "project_id",
              "geographic_scope"
            )
            values (
              ?,
              ?
            )"#,
                project_id,
                val,
            )
            .execute(&mut tx)
            .await?;
        }

        // "participation_tasks",
        sqlx::query!(
            r#"delete from project_participation_tasks where project_id = ?"#,
            project_id
        )
        .execute(&mut tx)
        .await?;

        for val in self.participation_tasks.iter() {
            sqlx::query!(
                r#"insert into project_participation_tasks (
              "project_id",
              "participation_task"
            )
            values (
              ?,
              ?
            )"#,
                project_id,
                val,
            )
            .execute(&mut tx)
            .await?;
        }

        // "activities",
        sqlx::query!(
            r#"delete from project_activities where project_id = ?"#,
            project_id
        )
        .execute(&mut tx)
        .await?;

        for val in self.activities.iter() {
            sqlx::query!(
                r#"insert into project_activities (
              "project_id",
              "activity"
            )
            values (
              ?,
              ?
            )"#,
                project_id,
                val,
            )
            .execute(&mut tx)
            .await?;
        }

        // "data_aggregation",
        sqlx::query!(
            r#"delete from project_data_aggregations where project_id = ?"#,
            project_id
        )
        .execute(&mut tx)
        .await?;

        for val in self.data_aggregation.iter() {
            sqlx::query!(
                r#"insert into project_data_aggregations (
              "project_id",
              "data_aggregation"
            )
            values (
              ?,
              ?
            )"#,
                project_id,
                val,
            )
            .execute(&mut tx)
            .await?;
        }

        // "data_consent",
        sqlx::query!(
            r#"delete from project_data_consents where project_id = ?"#,
            project_id
        )
        .execute(&mut tx)
        .await?;

        for val in self.data_consent.iter() {
            sqlx::query!(
                r#"insert into project_data_consents (
              "project_id",
              "data_consent"
            )
            values (
              ?,
              ?
            )"#,
                project_id,
                val,
            )
            .execute(&mut tx)
            .await?;
        }

        // "data_access",
        sqlx::query!(
            r#"delete from project_data_access where project_id = ?"#,
            project_id
        )
        .execute(&mut tx)
        .await?;

        for val in self.data_access.iter() {
            sqlx::query!(
                r#"insert into project_data_access (
              "project_id",
              "data_access"
            )
            values (
              ?,
              ?
            )"#,
                project_id,
                val,
            )
            .execute(&mut tx)
            .await?;
        }

        // "intended_outcomes",
        sqlx::query!(
            r#"delete from project_intended_outcomes where project_id = ?"#,
            project_id
        )
        .execute(&mut tx)
        .await?;

        for val in self.intended_outcomes.iter() {
            sqlx::query!(
                r#"insert into project_intended_outcomes (
              "project_id",
              "intended_outcome"
            )
            values (
              ?,
              ?
            )"#,
                project_id,
                val,
            )
            .execute(&mut tx)
            .await?;
        }

        // "sponsors",
        sqlx::query!(
            r#"delete from project_sponsors where project_id = ?"#,
            project_id
        )
        .execute(&mut tx)
        .await?;

        for val in self.sponsors.iter() {
            sqlx::query!(
                r#"insert into project_sponsors (
              "project_id",
              "sponsor"
            )
            values (
              ?,
              ?
            )"#,
                project_id,
                val,
            )
            .execute(&mut tx)
            .await?;
        }

        // "stakeholders",
        sqlx::query!(
            r#"delete from project_stakeholders where project_id = ?"#,
            project_id
        )
        .execute(&mut tx)
        .await?;

        for val in self.stakeholders.iter() {
            sqlx::query!(
                r#"insert into project_stakeholders (
              "project_id",
              "stakeholder"
            )
            values (
              ?,
              ?
            )"#,
                project_id,
                val,
            )
            .execute(&mut tx)
            .await?;
        }

        tx.commit().await?;

        Ok(())
    }

    #[tracing::instrument]
    //async fn read_from_database<'k>(_key: Self::DatabaseKey<'k>) -> Result<Self, Error> {
    async fn read_from_database(project_id: Self::DatabaseKey) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();

        // Read main project data from projects table
        let project_row = sqlx::query!(
            r#"SELECT
                project_id, legacy_id, legacy_v1_project_id, legacy_v2_project_id, foreign_id,
                publish, duplicate, authoritative, flagged, is_test, status,
                promotion_time, promotion_target, admin_notes, created, wild_hope,
                exchange, origin, suborigin, guid, opengraph_description,
                is_affiliate, is_inaturalist, inat_project_id, welcomed,
                page_creator_id, extra, draft, name, participation_degree,
                participant_instructions, participant_instructions_email,
                us_federally_funded, presenter, goal, task, description,
                image, image_credit, connect_opaque_url, connect_opaque_url1,
                connect_opaque_url2, url, video_url, blog_url, twitter_name,
                facebook_page, instagram_page, practices_how, search_terms,
                difficulty, diy, signup_url, prefix_how_to_logged_in,
                prefix_how_to_logged_out, how_to_join, howto_video,
                outcomes_label, outcomes_link, time_required, training_materials,
                classroom_materials, consent_statement, consent_statement_date,
                participation_tools, participation_prompt,
                participation_confirmation_message, participation_notification_email,
                participation_usage, part_of, total_expense, materials, gear,
                mobile_app_url_android, mobile_app_url_ios, mobile_app_url,
                location_type, entirely_online, on_demand, in_person,
                indoors, outdoors, regions_description, address, city, state,
                zip, country, other, location_name, location_landuse,
                location_description, united_nations_region, begin, end,
                project_timing, participation_schedule, time_per_participation,
                year_started, year_completed, month_completed, special_skills,
                data_results, data_visualization, data_quality,
                metadata_publication_url, data_publication_url, data_license,
                outcome_other, university_affiliation, circuit_id,
                sustainable_development_goals,
                AsGeoJSON(regions) as "regions: Option<String>", AsGeoJSON(point) as "point: Option<String>"
            FROM projects WHERE project_id = ?"#,
            project_id_str
        )
        .fetch_one(db())
        .await?;

        // Read array fields from separate tables
        let topics = sqlx::query!(
            "SELECT topic FROM project_topics WHERE project_id = ? ORDER BY topic",
            project_id_str
        )
        .fetch_all(db())
        .await?
        .into_iter()
        .map(|row| row.topic)
        .collect::<Vec<_>>()
        .into_boxed_slice();

        let fields_of_science = sqlx::query!(
            "SELECT field_of_science FROM project_fields_of_science WHERE project_id = ? ORDER BY field_of_science",
            project_id_str
        )
        .fetch_all(db())
        .await?
        .into_iter()
        .map(|row| row.field_of_science)
        .collect::<Vec<_>>()
        .into_boxed_slice();

        let practices = sqlx::query!(
            "SELECT practice FROM project_practices WHERE project_id = ? ORDER BY practice",
            project_id_str
        )
        .fetch_all(db())
        .await?
        .into_iter()
        .map(|row| row.practice)
        .collect::<Vec<_>>()
        .into_boxed_slice();

        let audience = sqlx::query!(
            "SELECT audience FROM project_audiences WHERE project_id = ? ORDER BY audience",
            project_id_str
        )
        .fetch_all(db())
        .await?
        .into_iter()
        .map(|row| row.audience)
        .collect::<Vec<_>>()
        .into_boxed_slice();

        // Read contacts
        let contact_rows = sqlx::query!(
            "SELECT contact_id, role, name, affiliation, email, phone, address FROM project_contacts WHERE project_id = ?",
            project_id_str
        )
        .fetch_all(db())
        .await?;

        let mut contacts = Vec::new();
        for contact_row in contact_rows {
            let contact_id = contact_row
                .contact_id
                .parse::<Ulid>()
                .map_err(|e| anyhow::anyhow!("Invalid contact ULID: {}", e))?;
            let role = match contact_row.role.as_str() {
                "Manager" => ContactRole::Manager,
                "ContactPerson" => ContactRole::ContactPerson,
                "Technical" => ContactRole::Technical,
                "Notification" => ContactRole::Notification,
                _ => ContactRole::ContactPerson,
            };
            contacts.push(Contact {
                contact_id,
                role,
                name: contact_row.name,
                affiliation: contact_row.affiliation,
                email: contact_row.email,
                phone: contact_row.phone,
                address: contact_row.address,
            });
        }

        // Read tags
        let tags = sqlx::query!(
            "SELECT tag FROM project_tags WHERE project_id = ?",
            project_id_str
        )
        .fetch_all(db())
        .await?
        .into_iter()
        .map(|row| InsensitiveString::from(row.tag))
        .collect::<HashSet<_>>();

        // Read participant files
        let participant_files = sqlx::query!(
            "SELECT url FROM project_participant_files WHERE project_id = ?",
            project_id_str
        )
        .fetch_all(db())
        .await?
        .into_iter()
        .map(|row| row.url)
        .collect::<Vec<_>>()
        .into_boxed_slice();

        // Parse main project fields
        let status = match project_row.status.as_str() {
            "Pending" => ProjectStatus::Pending,
            "Active" => ProjectStatus::Active,
            "Periodically Active" => ProjectStatus::PeriodicallyActive,
            "Hiatus" => ProjectStatus::Hiatus,
            "Complete" => ProjectStatus::Complete,
            "Hidden" => ProjectStatus::Hidden,
            "Expired" => ProjectStatus::Expired,
            _ => ProjectStatus::Active,
        };

        let promotion_time = match project_row.promotion_time.as_str() {
            "Promote at any time" => ProjectPromotionTime::PromoteAtAnyTime,
            "Do not promote" => ProjectPromotionTime::DoNotPromote,
            "Prefer promotion on the target date" => ProjectPromotionTime::PreferOnTargetDate,
            _ => ProjectPromotionTime::PromoteAtAnyTime,
        };

        let participation_degree = match project_row.participation_degree.as_str() {
            "Collegial" => ProjectParticipationDegree::Collegial,
            "Co-Created" => ProjectParticipationDegree::CoCreated,
            "Collaborative" => ProjectParticipationDegree::Collaborative,
            "Contributory" => ProjectParticipationDegree::Contributory,
            "Contractual" => ProjectParticipationDegree::Contractual,
            _ => ProjectParticipationDegree::Collegial,
        };

        Ok(ProjectAll {
            project_id,
            legacy_id: project_row.legacy_id,
            legacy_v1_project_id: project_row.legacy_v1_project_id,
            legacy_v2_project_id: project_row.legacy_v2_project_id,
            foreign_id: project_row.foreign_id,

            // Admin section
            publish: project_row.publish.unwrap_or(false),
            duplicate: project_row.duplicate,
            authoritative: project_row.authoritative,
            flagged: project_row.flagged,
            is_test: project_row.is_test,
            status,
            promotion_time,
            promotion_target: project_row
                .promotion_target
                .map(|s| TimeString::try_from(s))
                .transpose()
                .map_err(|e| anyhow::anyhow!("Invalid promotion_target TimeString: {}", e))?,
            admin_notes: project_row.admin_notes,
            created: TimeString::try_from(project_row.created)
                .map_err(|e| anyhow::anyhow!("Invalid created TimeString: {}", e))?,
            wild_hope: project_row.wild_hope,
            exchange: project_row.exchange,
            origin: project_row.origin,
            suborigin: project_row.suborigin,
            guid: project_row.guid,
            opengraph_description: project_row.opengraph_description,
            is_affiliate: project_row.is_affiliate,
            is_inaturalist: project_row.is_inaturalist,
            inat_project_id: project_row.inat_project_id.map(|i| i as i32),
            welcomed: project_row.welcomed,
            page_creator_id: project_row
                .page_creator_id
                .parse::<Ulid>()
                .map_err(|e| anyhow::anyhow!("Invalid page_creator_id ULID: {}", e))?,
            page_editor_ids: {
                let editor_rows = sqlx::query!(
                    "SELECT user_id FROM project_page_editors WHERE project_id = ?",
                    project_id_str
                )
                .fetch_all(db())
                .await?;

                let mut editor_ids = Vec::new();
                for row in editor_rows {
                    let editor_id = row
                        .user_id
                        .parse::<Ulid>()
                        .map_err(|e| anyhow::anyhow!("Invalid editor ULID: {}", e))?;
                    editor_ids.push(editor_id);
                }
                editor_ids.into_boxed_slice()
            },
            extra: project_row.extra,
            // Editor tracking is managed via log entries, not database columns
            project_last_editor: None,
            project_last_edited: None,
            scistarter_last_editor: None,
            scistarter_last_edited: None,

            // Project section
            draft: project_row.draft,
            name: project_row.name,
            tags,
            participation_degree,
            participant_instructions: project_row.participant_instructions,
            participant_instructions_email: project_row.participant_instructions_email,
            participant_files,
            us_federally_funded: project_row.us_federally_funded,
            presenter: project_row.presenter,
            goal: project_row.goal,
            task: project_row.task,
            description: project_row.description,
            image: project_row.image,
            image_credit: project_row.image_credit,
            connect_opaque_url: project_row.connect_opaque_url,
            connect_opaque_url1: project_row.connect_opaque_url1,
            connect_opaque_url2: project_row.connect_opaque_url2,

            // Contact section
            contacts: contacts.into_boxed_slice(),

            // Web section
            url: project_row.url,
            video_url: project_row.video_url,
            blog_url: project_row.blog_url,
            twitter_name: project_row.twitter_name,
            facebook_page: project_row.facebook_page,
            instagram_page: project_row.instagram_page,

            // Categorization section
            topics,
            fields_of_science,
            practices,
            practices_how: project_row.practices_how,
            search_terms: project_row.search_terms,
            difficulty: project_row.difficulty,
            diy: project_row.diy,

            // Joining section
            audience,
            signup_url: project_row.signup_url,
            prefix_how_to_logged_in: project_row.prefix_how_to_logged_in,
            prefix_how_to_logged_out: project_row.prefix_how_to_logged_out,
            how_to_join: project_row.how_to_join,
            howto_video: project_row.howto_video,
            outcomes_label: project_row.outcomes_label,
            outcomes_link: project_row.outcomes_link,
            time_required: project_row.time_required,
            training_materials: project_row.training_materials,
            classroom_materials: project_row.classroom_materials,
            consent_statement: project_row.consent_statement,
            consent_statement_date: project_row
                .consent_statement_date
                .map(|s| TimeString::try_from(s))
                .transpose()
                .map_err(|e| anyhow::anyhow!("Invalid consent_statement_date TimeString: {}", e))?,
            participation_tools: project_row.participation_tools,
            participation_prompt: project_row.participation_prompt,
            participation_confirmation_message: project_row.participation_confirmation_message,
            participation_notification_email: project_row.participation_notification_email,
            participation_usage: project_row.participation_usage,

            // Relations section
            part_of: project_row
                .part_of
                .map(|s| s.parse::<Ulid>())
                .transpose()
                .map_err(|e| anyhow::anyhow!("Invalid part_of ULID: {}", e))?,
            associated_projects: {
                let associated = sqlx::query!(
                    "SELECT associated_project_id FROM project_associated_projects WHERE project_id = ?",
                    project_id_str
                )
                .fetch_all(db())
                .await?;

                let mut project_ids = Vec::new();
                for row in associated {
                    let associated_id = row.associated_project_id.parse::<Ulid>().map_err(|e| {
                        anyhow::anyhow!("Invalid associated_project_id ULID: {}", e)
                    })?;
                    project_ids.push(associated_id);
                }
                project_ids.into_boxed_slice()
            },

            // Participants Need section
            total_expense: project_row.total_expense.map(|f| f as f32),
            materials: project_row.materials,
            gear: project_row.gear,
            mobile_app_url_android: project_row.mobile_app_url_android,
            mobile_app_url_ios: project_row.mobile_app_url_ios,
            mobile_app_url: project_row.mobile_app_url,

            // Where section
            location_type: project_row.location_type,
            entirely_online: project_row.entirely_online,
            on_demand: project_row.on_demand,
            in_person: project_row.in_person,
            indoors: project_row.indoors,
            outdoors: project_row.outdoors,
            regions_description: project_row.regions_description,
            address: project_row.address,
            city: project_row.city,
            state: project_row.state,
            zip: project_row.zip,
            country: project_row.country,
            other: project_row.other,
            location_name: project_row.location_name,
            location_landuse: project_row.location_landuse,
            location_description: project_row.location_description,
            united_nations_region: project_row.united_nations_region,
            point: project_row.point.flatten(),
            regions: project_row.regions.flatten(),

            // When section
            begin: project_row
                .begin
                .map(|s| TimeString::try_from(s))
                .transpose()
                .map_err(|e| anyhow::anyhow!("Invalid begin TimeString: {}", e))?,
            end: project_row
                .end
                .map(|s| TimeString::try_from(s))
                .transpose()
                .map_err(|e| anyhow::anyhow!("Invalid end TimeString: {}", e))?,
            project_timing: project_row.project_timing,
            participation_schedule: project_row.participation_schedule,
            time_per_participation: project_row.time_per_participation,
            year_started: project_row.year_started.map(|i| i as i32),
            year_completed: project_row.year_completed.map(|i| i as i32),
            month_completed: project_row.month_completed.map(|i| i as i32),

            // Skills section
            special_skills: project_row.special_skills,

            // Data section
            data_results: project_row.data_results,
            data_visualization: project_row.data_visualization,
            data_quality: project_row.data_quality,
            metadata_publication_url: project_row.metadata_publication_url,
            data_publication_url: project_row.data_publication_url,
            data_license: project_row.data_license,

            // Read remaining array fields
            participation_tasks: {
                let tasks = sqlx::query!(
                    "SELECT participation_task FROM project_participation_tasks WHERE project_id = ?",
                    project_id_str
                )
                .fetch_all(db())
                .await?
                .into_iter()
                .map(|row| row.participation_task)
                .collect::<Vec<_>>();
                tasks.into_boxed_slice()
            },
            activities: {
                let activities = sqlx::query!(
                    "SELECT activity FROM project_activities WHERE project_id = ?",
                    project_id_str
                )
                .fetch_all(db())
                .await?
                .into_iter()
                .map(|row| row.activity)
                .collect::<Vec<_>>();
                activities.into_boxed_slice()
            },
            geographic_scope: {
                let scopes = sqlx::query!(
                    "SELECT geographic_scope FROM project_geographic_scopes WHERE project_id = ?",
                    project_id_str
                )
                .fetch_all(db())
                .await?
                .into_iter()
                .map(|row| row.geographic_scope)
                .collect::<Vec<_>>();
                scopes.into_boxed_slice()
            },
            data_aggregation: {
                let aggregations = sqlx::query!(
                    "SELECT data_aggregation FROM project_data_aggregations WHERE project_id = ?",
                    project_id_str
                )
                .fetch_all(db())
                .await?
                .into_iter()
                .map(|row| row.data_aggregation)
                .collect::<Vec<_>>();
                aggregations.into_boxed_slice()
            },
            data_consent: {
                let consents = sqlx::query!(
                    "SELECT data_consent FROM project_data_consents WHERE project_id = ?",
                    project_id_str
                )
                .fetch_all(db())
                .await?
                .into_iter()
                .map(|row| row.data_consent)
                .collect::<Vec<_>>();
                consents.into_boxed_slice()
            },
            data_access: {
                let access = sqlx::query!(
                    "SELECT data_access FROM project_data_access WHERE project_id = ?",
                    project_id_str
                )
                .fetch_all(db())
                .await?
                .into_iter()
                .map(|row| row.data_access)
                .collect::<Vec<_>>();
                access.into_boxed_slice()
            },
            intended_outcomes: {
                let outcomes = sqlx::query!(
                    "SELECT intended_outcome FROM project_intended_outcomes WHERE project_id = ?",
                    project_id_str
                )
                .fetch_all(db())
                .await?
                .into_iter()
                .map(|row| row.intended_outcome)
                .collect::<Vec<_>>();
                outcomes.into_boxed_slice()
            },
            sponsors: {
                let sponsors = sqlx::query!(
                    "SELECT sponsor FROM project_sponsors WHERE project_id = ?",
                    project_id_str
                )
                .fetch_all(db())
                .await?
                .into_iter()
                .map(|row| row.sponsor)
                .collect::<Vec<_>>();
                sponsors.into_boxed_slice()
            },
            stakeholders: {
                let stakeholders = sqlx::query!(
                    "SELECT stakeholder FROM project_stakeholders WHERE project_id = ?",
                    project_id_str
                )
                .fetch_all(db())
                .await?
                .into_iter()
                .map(|row| row.stakeholder)
                .collect::<Vec<_>>();
                stakeholders.into_boxed_slice()
            },

            // Outcomes section
            outcome_other: project_row.outcome_other,

            // Sponsors section (additional fields)
            university_affiliation: project_row.university_affiliation,
            circuit_id: project_row.circuit_id,
            sustainable_development_goals: project_row.sustainable_development_goals,
        })
    }
}
