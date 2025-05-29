use anyhow::Error;
use common::timestring::TimeString;
use ulid::Ulid;

use crate::{contact::Contact, peering::LoadStorePeered};

use super::{ProjectParticipationDegree, ProjectPromotionTime, ProjectStatus};

/// Container for all project fields.
///
/// This structure contains a representation of all project data. Its
/// primary purpose is for receiving data synchronizations. It should
/// not be used casually for other purposes, since there are more
/// efficient options in most cases.
#[derive(Default, Debug, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ProjectAll {
    pub project_id: Ulid,
    pub legacy_v1_project_id: Option<i64>,
    pub legacy_v2_project_id: Option<i64>,
    pub foreign_id: Option<String>,

    // section Admin
    pub publish: bool,
    pub duplicate: bool,
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
    pub tags: Box<[String]>,
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
    type DatabaseKey = Ulid;

    const GROUP: &str = "project";
    const SPECIFIC: &str = "all";

    async fn save_to_database(&self) -> Result<(), Error> {
        let db = common::shared::db();

        let project_id = self.project_id.to_string();
        let page_creator_id = self.page_creator_id.to_string();
        let part_of = self.part_of.map(|u| u.to_string());

        sqlx::query!(
            r#"insert or replace into projects (
          "project_id",
          "legacy_v1_project_id",
          "legacy_v2_project_id",
          "foreign_id",
          "publish",
          "duplicate",
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
          case
            when ?111 is null then
              null
            when substr(?111, 1, 1) == '{' and substr(?111, -1, 1) == '}' then
              GeomFromGeoJSON(?111)
            else
              GeomFromEWKT(?111)
          end,
          case
            when ?112 is null then
              null
            when substr(?112, 1, 1) == '{' and substr(?112, -1, 1) == '}' then
              GeomFromGeoJSON(?112)
            else
              GeomFromEWKT(?112)
          end
        )"#,
            project_id,
            self.legacy_v1_project_id,
            self.legacy_v2_project_id,
            self.foreign_id,
            self.publish,
            self.duplicate,
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
        .execute(db)
        .await?;

        // The following all have to be fully written out, because there's
        // no way to parameterize the table and field names in a
        // compile-time checked SQLx query. We can't even use a macro to
        // manage it, because the query! macro requires a literal token
        // for the query, and we can't construct those at compile
        // time. Revisit this later.

        // "page_editor_ids",
        let tx = db.begin().await?;
        sqlx::query!(
            r#"delete from project_page_editors where project_id = ?"#,
            project_id
        )
        .execute(db)
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
            .execute(db)
            .await?;
        }
        tx.commit().await?;

        // "project_last_editor",
        // "project_last_edited",
        if let Some(last_editor) = self.project_last_editor {
            common::service::send(
                "log.project.user-edit",
                (last_editor, self.project_last_edited.to_owned(), "all"),
            )
            .await?;
        }

        // "scistarter_last_editor",
        // "scistarter_last_edited",
        if let Some(last_editor) = self.scistarter_last_editor {
            common::service::send(
                "log.project.staff-edit",
                (last_editor, self.scistarter_last_edited.to_owned(), "all"),
            )
            .await?;
        }

        // "tags",
        let tx = db.begin().await?;
        sqlx::query!(
            r#"delete from project_tags where project_id = ?"#,
            project_id
        )
        .execute(db)
        .await?;
        for val in self.tags.iter() {
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
            .execute(db)
            .await?;
        }
        tx.commit().await?;

        // "participant_files",
        sqlx::query!(r#""#).execute(db).await?;
        let tx = db.begin().await?;
        sqlx::query!(
            r#"delete from project_participant_files where project_id = ?"#,
            project_id
        )
        .execute(db)
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
            .execute(db)
            .await?;
        }
        tx.commit().await?;

        // "contacts",
        let tx = db.begin().await?;
        sqlx::query!(
            r#"delete from project_contacts where project_id = ?"#,
            project_id
        )
        .execute(db)
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
            .execute(db)
            .await?;
        }
        tx.commit().await?;

        // "topics",
        let tx = db.begin().await?;
        sqlx::query!(
            r#"delete from project_topics where project_id = ?"#,
            project_id
        )
        .execute(db)
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
            .execute(db)
            .await?;
        }
        tx.commit().await?;

        // "fields_of_science",
        let tx = db.begin().await?;
        sqlx::query!(
            r#"delete from project_fields_of_science where project_id = ?"#,
            project_id
        )
        .execute(db)
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
            .execute(db)
            .await?;
        }
        tx.commit().await?;

        // "practices",
        let tx = db.begin().await?;
        sqlx::query!(
            r#"delete from project_practices where project_id = ?"#,
            project_id
        )
        .execute(db)
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
            .execute(db)
            .await?;
        }
        tx.commit().await?;

        // "audience",
        let tx = db.begin().await?;
        sqlx::query!(
            r#"delete from project_audiences where project_id = ?"#,
            project_id
        )
        .execute(db)
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
            .execute(db)
            .await?;
        }
        tx.commit().await?;

        // "associated_projects",
        let tx = db.begin().await?;
        sqlx::query!(
            r#"delete from project_associated_projects where project_id = ?"#,
            project_id
        )
        .execute(db)
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
            .execute(db)
            .await?;
        }
        tx.commit().await?;

        // "geographic_scope",
        let tx = db.begin().await?;
        sqlx::query!(
            r#"delete from project_geographic_scopes where project_id = ?"#,
            project_id
        )
        .execute(db)
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
            .execute(db)
            .await?;
        }
        tx.commit().await?;

        // "participation_tasks",
        let tx = db.begin().await?;
        sqlx::query!(
            r#"delete from project_participation_tasks where project_id = ?"#,
            project_id
        )
        .execute(db)
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
            .execute(db)
            .await?;
        }
        tx.commit().await?;

        // "activities",
        let tx = db.begin().await?;
        sqlx::query!(
            r#"delete from project_activities where project_id = ?"#,
            project_id
        )
        .execute(db)
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
            .execute(db)
            .await?;
        }
        tx.commit().await?;

        // "data_aggregation",
        let tx = db.begin().await?;
        sqlx::query!(
            r#"delete from project_data_aggregations where project_id = ?"#,
            project_id
        )
        .execute(db)
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
            .execute(db)
            .await?;
        }
        tx.commit().await?;

        // "data_consent",
        let tx = db.begin().await?;
        sqlx::query!(
            r#"delete from project_data_consents where project_id = ?"#,
            project_id
        )
        .execute(db)
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
            .execute(db)
            .await?;
        }
        tx.commit().await?;

        // "data_access",
        let tx = db.begin().await?;
        sqlx::query!(
            r#"delete from project_data_consents where project_id = ?"#,
            project_id
        )
        .execute(db)
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
            .execute(db)
            .await?;
        }
        tx.commit().await?;

        // "intended_outcomes",
        let tx = db.begin().await?;
        sqlx::query!(
            r#"delete from project_intended_outcomes where project_id = ?"#,
            project_id
        )
        .execute(db)
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
            .execute(db)
            .await?;
        }
        tx.commit().await?;

        // "sponsors",
        let tx = db.begin().await?;
        sqlx::query!(
            r#"delete from project_sponsors where project_id = ?"#,
            project_id
        )
        .execute(db)
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
            .execute(db)
            .await?;
        }
        tx.commit().await?;

        // "stakeholders",
        let tx = db.begin().await?;
        sqlx::query!(
            r#"delete from project_stakeholders where project_id = ?"#,
            project_id
        )
        .execute(db)
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
            .execute(db)
            .await?;
        }

        tx.commit().await?;

        Ok(())
    }

    async fn read_from_database(_key: &Self::DatabaseKey) -> Result<Self, Error> {
        unimplemented!(
            "Loading ProjectAll from the database has been deferred until a need is demonstrated"
        )
    }
}
