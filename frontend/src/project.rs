use anyhow::Error;
use async_nats::Message;
use common::{nats_headers, service::send_with_headers, shared::nats, timestring::TimeString};
use futures::StreamExt;
use ulid::Ulid;

use crate::{SERVER_ID_STR, contact::Contact};

pub mod field;

#[derive(Default, Debug, sqlx::Type, serde::Serialize, serde::Deserialize)]
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

#[derive(Default, Debug, sqlx::Type, serde::Serialize, serde::Deserialize)]
pub enum ProjectPromotionTime {
    #[default]
    #[sqlx(rename = "Promote at any time")]
    PromoteAtAnyTime,
    #[sqlx(rename = "Do not promote")]
    DoNotPromote,
    #[sqlx(rename = "Prefer promotion on the target date")]
    PreferOnTargetDate,
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
    pub page_creator_id: Ulid, // NB was "owner"
    pub page_manager_ids: Box<[Ulid]>,
    pub extra: String,
    pub project_last_editor: Option<Ulid>,
    pub project_last_edited: Option<TimeString>,
    pub scistarter_last_editor: Option<Ulid>,
    pub scistarter_last_edited: Option<TimeString>,

    // section Project
    pub draft: bool,
    pub name: String,
    pub tags: Box<[String]>,
    pub participation_degree: String,
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
    pub part_of: Option<String>,
    pub associated_projects: Box<[String]>,

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
    pub regions: String,
    pub regions_description: String,
    pub point: String,
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
    println!("save_local({:?})", all);
    // let db = common::shared::db();
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

            println!("Update accepted");
        } else {
            println!("Update rejected, Source header matches server id");
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
