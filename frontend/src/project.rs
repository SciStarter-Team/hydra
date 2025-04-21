use common::{shared::nats, timestring::TimeString};
use futures::StreamExt;
use ulid::Ulid;

use crate::{contact::Contact, SERVER_ID_STR};

#[derive(Default, sqlx::Type, serde::Serialize, serde::Deserialize)]
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

#[derive(Default, sqlx::Type, serde::Serialize, serde::Deserialize)]
pub enum ProjectPromotionTime {
    #[default]
    #[sqlx(rename = "Promote at any time")]
    PromoteAtAnyTime,
    #[sqlx(rename = "Do not promote")]
    DoNotPromote,
    #[sqlx(rename = "Prefer promotion on the target date")]
    PreferOnTargetDate,
}

#[derive(Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Project {
    project_id: Ulid,
    legacy_v1_project_id: Option<i64>,
    legacy_v2_project_id: Option<i64>,
    foreign_id: Option<String>,

    // section Admin
    publish: bool,
    published_at: Option<TimeString>,
    duplicate: bool,
    flagged: bool,
    is_test: bool,
    status: ProjectStatus,
    promotion_time: ProjectPromotionTime,
    promotion_target: Option<TimeString>,
    admin_notes: String,
    created: TimeString,
    wild_hope: bool,
    exchange: Option<String>,
    origin: String,
    suborigin: String,
    guid: String,
    opengraph_description: String,
    is_affiliate: bool,
    is_inaturalist: bool,
    inat_project_id: Option<i32>,
    welcomed: bool,
    page_creator: String, // NB was "owner"
    page_managers: Box<[String]>,
    extra: String,
    project_last_editor: Option<Ulid>,
    project_last_edited: Option<TimeString>,
    scistarter_last_editor: Option<Ulid>,
    scistarter_last_edited: Option<TimeString>,

    // section Project
    draft: bool,
    changed_draft: TimeString,

    name: String,
    changed_name: TimeString,

    tags: Box<[String]>,
    changed_tags: TimeString,

    participation_degree: String,
    changed_participation_degree: TimeString,

    participant_instructions: String,
    changed_participant_instructions: TimeString,

    participant_instructions_email: String,
    changed_participant_instructions_email: TimeString,

    participant_files: Box<[String]>,
    changed_participant_files: TimeString,

    us_federally_funded: bool,
    changed_us_federally_funded: TimeString,

    presenter: String,
    changed_presenter: TimeString,

    goal: String,
    changed_goal: TimeString,

    task: String,
    changed_task: TimeString,

    description: String,
    changed_description: TimeString,

    image: String,
    changed_image: TimeString,

    image_credit: String,
    changed_image_credit: TimeString,

    connect_opaque_url: String,
    changed_connect_opaque_url: TimeString,

    connect_opaque_url1: String,
    changed_connect_opaque_url1: TimeString,

    connect_opaque_url2: String,
    changed_connect_opaque_url2: TimeString,

    // section Contact
    contacts: Box<[Contact]>,

    // section Web
    url: String,
    changed_url: TimeString,

    video_url: String,
    changed_video_url: TimeString,

    blog_url: String,
    changed_blog_url: TimeString,

    twitter_name: String,
    changed_twitter_name: TimeString,

    facebook_page: String,
    changed_facebook_page: TimeString,

    instagram_page: String,
    changed_instagram_page: TimeString,

    // section Categorization
    todo: String,
}

pub async fn receive_updates() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let client = nats();
    let mut updates = client.subscribe("frontend.project.save").await?;
    while let Some(msg) = updates.next().await {
        if let Some(headers) = msg.headers {
            if headers
                .get("Source")
                .is_some_and(|v| v.as_str() != SERVER_ID_STR.as_str())
            {
                let project: Project = match flexbuffers::from_slice(&msg.payload) {
                    Ok(proj) => proj,
                    Err(err) => {
                        eprintln!("Unable to decode message payload to project record in receive_updates: {}", err);
                        continue;
                    }
                };
            }
        }
    }

    Ok(())
}
