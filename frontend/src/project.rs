use anyhow::Error;
use async_nats::Message;
use common::{nats_headers, service::send_with_headers, shared::nats, timestring::TimeString};
use futures::StreamExt;
use ulid::Ulid;

use crate::{SERVER_ID_STR, contact::Contact};

pub mod field;

#[derive(Clone, Copy, Default, Debug, sqlx::Type, serde::Serialize, serde::Deserialize)]
pub enum ProjectStatus {
    Pending,
    #[default]
    Active,
    #[sqlx(rename = "Periodically Active")]
    PeriodicallyActive,
    Hiatus,
    Complete,
    Hidden,
}

#[derive(Clone, Copy, Default, Debug, sqlx::Type, serde::Serialize, serde::Deserialize)]
pub enum ProjectPromotionTime {
    #[default]
    #[sqlx(rename = "Promote at any time")]
    PromoteAtAnyTime,
    #[sqlx(rename = "Do not promote")]
    DoNotPromote,
    #[sqlx(rename = "Prefer promotion on the target date")]
    PreferOnTargetDate,
}

#[derive(Clone, Copy, Default, Debug, sqlx::Type, serde::Serialize, serde::Deserialize)]
pub enum ProjectParticipationDegree {
    #[default]
    Collegial,
    #[sqlx(rename = "Co-Created")]
    CoCreated,
    Collaborative,
    Contributory,
    Contractual,
}

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

impl ProjectAll {
    pub async fn save(&self) -> Result<(), Error> {
        save_local(self).await?;
        send_with_headers(
            "frontend.project.save.all",
            nats_headers! {
                "Source": SERVER_ID_STR.as_str(),
            },
            self,
        )
        .await?;
        Ok(())
    }

    pub async fn load(project_id: Ulid) -> Result<ProjectAll, Error> {
        let _db = common::shared::db();
        todo!()
    }
}

async fn save_local(all: &ProjectAll) -> Result<(), Error> {
    let db = common::shared::db();

    let project_id = all.project_id.to_string();
    let page_creator_id = all.page_creator_id.to_string();
    let part_of = all.part_of.map(|u| u.to_string());

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
        all.legacy_v1_project_id,
        all.legacy_v2_project_id,
        all.foreign_id,
        all.publish,
        all.duplicate,
        all.flagged,
        all.is_test,
        all.status,
        all.promotion_time,
        all.promotion_target,
        all.admin_notes,
        all.created,
        all.wild_hope,
        all.exchange,
        all.origin,
        all.suborigin,
        all.guid,
        all.opengraph_description,
        all.is_affiliate,
        all.is_inaturalist,
        all.inat_project_id,
        all.welcomed,
        page_creator_id,
        all.extra,
        all.draft,
        all.name,
        all.participation_degree,
        all.participant_instructions,
        all.participant_instructions_email,
        all.us_federally_funded,
        all.presenter,
        all.goal,
        all.task,
        all.description,
        all.image,
        all.image_credit,
        all.connect_opaque_url,
        all.connect_opaque_url1,
        all.connect_opaque_url2,
        all.url,
        all.video_url,
        all.blog_url,
        all.twitter_name,
        all.facebook_page,
        all.instagram_page,
        all.practices_how,
        all.search_terms,
        all.difficulty,
        all.diy,
        all.signup_url,
        all.prefix_how_to_logged_in,
        all.prefix_how_to_logged_out,
        all.how_to_join,
        all.howto_video,
        all.outcomes_label,
        all.outcomes_link,
        all.time_required,
        all.training_materials,
        all.classroom_materials,
        all.consent_statement,
        all.consent_statement_date,
        all.participation_tools,
        all.participation_prompt,
        all.participation_confirmation_message,
        all.participation_notification_email,
        all.participation_usage,
        part_of,
        all.total_expense,
        all.materials,
        all.gear,
        all.mobile_app_url_android,
        all.mobile_app_url_ios,
        all.mobile_app_url,
        all.location_type,
        all.entirely_online,
        all.on_demand,
        all.in_person,
        all.indoors,
        all.outdoors,
        all.regions_description,
        all.address,
        all.city,
        all.state,
        all.zip,
        all.country,
        all.other,
        all.location_name,
        all.location_landuse,
        all.location_description,
        all.united_nations_region,
        all.begin,
        all.end,
        all.project_timing,
        all.participation_schedule,
        all.time_per_participation,
        all.year_started,
        all.year_completed,
        all.month_completed,
        all.special_skills,
        all.data_results,
        all.data_visualization,
        all.data_quality,
        all.metadata_publication_url,
        all.data_publication_url,
        all.data_license,
        all.outcome_other,
        all.university_affiliation,
        all.circuit_id,
        all.sustainable_development_goals,
        all.regions,
        all.point,
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
    for user_id in all.page_editor_ids.iter() {
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
    sqlx::query!(r#""#).execute(db).await?;

    // "project_last_edited",
    sqlx::query!(r#""#).execute(db).await?;

    // "scistarter_last_editor",
    sqlx::query!(r#""#).execute(db).await?;

    // "scistarter_last_edited",
    sqlx::query!(r#""#).execute(db).await?;

    // "tags",
    let tx = db.begin().await?;
    sqlx::query!(
        r#"delete from project_tags where project_id = ?"#,
        project_id
    )
    .execute(db)
    .await?;
    for val in all.tags.iter() {
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
    for val in all.participant_files.iter() {
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
    for contact in all.contacts.iter() {
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
    for val in all.topics.iter() {
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
    for val in all.fields_of_science.iter() {
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
    for val in all.practices.iter() {
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
    for val in all.audience.iter() {
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
    for assoc_id in all.associated_projects.iter() {
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
    for val in all.geographic_scope.iter() {
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
    for val in all.participation_tasks.iter() {
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
    for val in all.activities.iter() {
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
    for val in all.data_aggregation.iter() {
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
    for val in all.data_consent.iter() {
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
    for val in all.data_consent.iter() {
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
    for val in all.intended_outcomes.iter() {
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
    for val in all.sponsors.iter() {
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
    for val in all.stakeholders.iter() {
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

async fn project_all_save(msg: Message) -> Result<(), Error> {
    if let Some(headers) = msg.headers {
        if headers
            .get("Source")
            .is_some_and(|v| v.as_str() != SERVER_ID_STR.as_str())
        {
            let project: ProjectAll = match flexbuffers::from_slice(&msg.payload) {
                Ok(proj) => proj,
                Err(err) => {
                    eprintln!(
                        "Unable to decode message payload to project record in project_all_save: {}",
                        err
                    );
                    return Ok(());
                }
            };

            save_local(&project).await?;
        }
    }

    Ok(())
}

/// Launch as a separate task to receive and process project updates over NATS
pub async fn receive_project_updates() -> Result<(), Error> {
    let client = nats();

    let mut updates = client.subscribe("frontend.project.>").await?;

    while let Some(msg) = updates.next().await {
        match msg.subject.as_str() {
            "frontend.project.save.all" => project_all_save(msg).await?,
            "frontend.project.save.field" => field::project_field_save(msg).await?,
            subj => eprintln!("Unrecognized frontend.project.> subject {}", subj),
        }
    }

    Ok(())
}
