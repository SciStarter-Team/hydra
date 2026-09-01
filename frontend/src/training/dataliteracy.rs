//! Data Literacy training — a three-step training (Account → Tutorial → Claim)
//! with a bilingual (EN/ES) tutorial. Ported from orrery's legacy
//! `/training-data-literacy` into the shared layout, now at
//! `/training/data-literacy`. State comes from `data_literacy_progress`.

use std::sync::Arc;

use anyhow::Error;
use common::message::user::{BasicInfo, WorkflowStatusResponse, WorkflowType};
use poem::{Response, handler, web::Redirect};
use sailfish::TemplateOnce;

use crate::middleware::authorization::Authenticated;
use crate::peering::LoadStorePeered;
use crate::training::{
    self, ArticulateCard, ClaimBody, SidebarBody, SigninBody, StepView, StepsView, TrainingChrome,
    TrainingContent, TutorialBody, linear_states, module_url, open_flags, render_page,
    render_progress_fragment, tutorial_launch_url,
};

const BASE_PATH: &str = "/training/data-literacy";
const PROGRESS_PATH: &str = "/training/data-literacy/-progress";
const SLUG: &str = "data-literacy";

// The tutorial's two builds, both older Rise exports that report without a
// launch activity id.
const DATALITERACY_EN_URL: &str = "https://media.scistarter.org/dataliteracy/index.html";
const DATALITERACY_ES_URL: &str = "https://media.scistarter.org/dataliteracy/es/index.html";

/// Editable copy for the Data Literacy training. `#[serde(default)]` is required
/// (see [`crate::training::foundations::FoundationsContent`]).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct DataLiteracyContent {
    pub title: String,
    pub sponsor: String,
    pub intro: String,
    pub estimated_time: String,
    pub badge_image: String,
    pub skills_intro: String,
    pub skills: Vec<String>,
    pub skills_footnote: String,
    pub account_heading: String,
    pub tutorial_heading: String,
    pub tutorial_intro: String,
    pub articulate_en: String,
    pub articulate_en_image: String,
    pub articulate_es: String,
    pub articulate_es_image: String,
    pub claim_heading: String,
    pub claim_html: String,
    pub sidebar_html: String,
}

impl Default for DataLiteracyContent {
    fn default() -> Self {
        DataLiteracyContent {
            title: "Data Literacy".to_string(),
            sponsor: String::new(),
            intro: r#"Before starting this module, you'll need to complete the prerequisite <a href="/training/foundations">Foundations of Citizen Science</a> training module and badge. If you have already completed it, you may need to <a href="/login?next=/training/data-literacy">log in</a> to SciStarter before proceeding.
<br>This module opens in a new tab. Return to this page to claim your badge upon completion."#.to_string(),
            estimated_time: "30 minutes".to_string(),
            badge_image: "/-static/img/badges/badge-data-literacy.png".to_string(),
            skills_intro: "By the end of this tutorial, you will be able to:".to_string(),
            skills: vec![
                "Articulate how citizen science benefits science and society.".to_string(),
                "Describe data literacy and how it can help you make better decisions.".to_string(),
                "Differentiate the types of data collected and used by scientists and citizen science participants.".to_string(),
                "Interpret data-rich graphs and charts.".to_string(),
                "Explain how data quality impacts the credibility of scientific conclusions.".to_string(),
            ],
            skills_footnote: r#"You will practice or acquire the following skills, which are part of the Open Skills Taxonomy: <a href="https://skills.lightcast.io/skills/ES476D9219E938F5CB96" target="_blank">data literacy</a>, <a href="https://skills.lightcast.io/skills/ESF14A354DDD69B5FBD4" target="_blank">data-driven decision-making</a>, <a href="https://skills.lightcast.io/skills/KS122J56PZMMFL1SVY1F" target="_blank">critical thinking</a>, and <a href="https://skills.lightcast.io/skills/KS120GV6C72JMSZKMTD7" target="_blank">data analysis</a>."#.to_string(),
            account_heading: "SciStarter Account".to_string(),
            tutorial_heading: "Self-guided tutorial and quiz".to_string(),
            tutorial_intro: r#"<b>Complete this self-guided tutorial and quiz to claim your badge.</b>"#.to_string(),
            articulate_en: DATALITERACY_EN_URL.to_string(),
            articulate_en_image: "https://static.scistarter.org/img/training/data-literacy.png".to_string(),
            articulate_es: DATALITERACY_ES_URL.to_string(),
            articulate_es_image: "https://static.scistarter.org/img/training/data-literacy-spanish.png".to_string(),
            claim_heading: "Claim your badge".to_string(),
            claim_html: r#"<h2>Congratulations! It's time to claim your badge.</h2>
<p>You'll find your badge on your <a href="/dashboard/profile">SciStarter profile</a>! You'll receive an email with instructions for sharing the verified badge and metadata on LinkedIn.</p>
<p>(Optional) Please complete <a href="https://forms.gle/baqkAZGbYp5fCYLDA" target="_blank">this survey</a>.</p>"#.to_string(),
            sidebar_html: String::new(),
        }
    }
}

impl LoadStorePeered for DataLiteracyContent {
    type DatabaseKey = ();

    const GROUP: &str = "training_content";
    const SPECIFIC: &str = SLUG;

    async fn save_to_database(&self) -> Result<(), Error> {
        training::save_content_json(SLUG, self).await
    }

    async fn read_from_database(_key: ()) -> Result<Self, Error> {
        training::read_content_json(SLUG).await
    }
}

impl DataLiteracyContent {
    pub async fn current() -> DataLiteracyContent {
        DataLiteracyContent::load(()).await.unwrap_or_default()
    }
}

#[handler]
pub async fn page(Authenticated(user_info): Authenticated) -> Result<Response, Error> {
    let content = DataLiteracyContent::current().await;
    let rendered = build_content(&content, user_info.clone()).await?;
    render_page(BASE_PATH, content.title.clone(), rendered, user_info).await
}

#[handler]
pub async fn progress(Authenticated(user_info): Authenticated) -> Result<Response, Error> {
    let content = DataLiteracyContent::current().await;
    let steps = build_steps(&content, &user_info).await;
    let count = training::tutorials_completed(WorkflowType::DataLiteracy).await;
    render_progress_fragment(steps, count)
}

/// 301 redirect from the legacy `/training-data-literacy` URL.
#[handler]
pub async fn legacy_redirect() -> Redirect {
    Redirect::moved_permanent(BASE_PATH)
}

async fn build_content(
    content: &DataLiteracyContent,
    user_info: Option<Arc<BasicInfo>>,
) -> Result<TrainingContent, Error> {
    let live_updates = user_info.is_some();
    let steps = build_steps(content, &user_info).await;
    let tutorials_completed = training::tutorials_completed(WorkflowType::DataLiteracy).await;
    let sidebar_html = SidebarBody {
        badge_image: content.badge_image.clone(),
        html: content.sidebar_html.clone(),
    }
    .render_once()
    .unwrap_or_default();

    Ok(TrainingContent {
        chrome: TrainingChrome::default(),
        breadcrumb_label: content.title.clone(),
        badge_image: content.badge_image.clone(),
        badge_alt: "Data Literacy badge".to_string(),
        sponsor: content.sponsor.clone(),
        title: content.title.clone(),
        intro: content.intro.clone(),
        estimated_time: content.estimated_time.clone(),
        tutorials_completed,
        skills_intro: content.skills_intro.clone(),
        skills: content.skills.clone(),
        skills_footnote: content.skills_footnote.clone(),
        steps,
        sidebar_html,
        live_updates,
        sse_channel: "training".to_string(),
        sse_event: WorkflowType::DataLiteracy.slug().to_string(),
        progress_path: PROGRESS_PATH.to_string(),
    })
}

/// Three linear steps: account → tutorial → badge.
async fn build_steps(
    content: &DataLiteracyContent,
    user_info: &Option<Arc<BasicInfo>>,
) -> StepsView {
    let logged_in = user_info.is_some();
    let status = match user_info {
        Some(user) => {
            training::fetch_status(WorkflowType::DataLiteracy, &user.user_id.to_string()).await
        }
        None => WorkflowStatusResponse {
            current_step: None,
            completed_steps: vec![],
            badge_granted: false,
        },
    };

    let tutorial_done = status.completed_steps.iter().any(|step| step == "tutorial");
    let states = linear_states(&[logged_in, tutorial_done, status.badge_granted]);
    let opens = open_flags(&states);

    let account_body = SigninBody {
        logged_in,
        handle: user_info
            .as_ref()
            .map(|user| user.handle.clone())
            .unwrap_or_default(),
        next_path: BASE_PATH.to_string(),
        chrome: TrainingChrome::default(),
    }
    .render_once()
    .unwrap_or_default();

    let tutorial_body = TutorialBody {
        intro: content.tutorial_intro.clone(),
        cards: vec![
            ArticulateCard {
                href: tutorial_launch_url(
                    module_url(&content.articulate_en, DATALITERACY_EN_URL),
                    None,
                    user_info,
                ),
                image: content.articulate_en_image.clone(),
            },
            ArticulateCard {
                href: tutorial_launch_url(
                    module_url(&content.articulate_es, DATALITERACY_ES_URL),
                    None,
                    user_info,
                ),
                image: content.articulate_es_image.clone(),
            },
        ],
        extra: String::new(),
    }
    .render_once()
    .unwrap_or_default();

    let claim_body = ClaimBody {
        html: content.claim_html.clone(),
    }
    .render_once()
    .unwrap_or_default();

    let steps = vec![
        StepView {
            heading: content.account_heading.clone(),
            state: states[0],
            open: opens[0],
            body: account_body,
        },
        StepView {
            heading: content.tutorial_heading.clone(),
            state: states[1],
            open: opens[1],
            body: tutorial_body,
        },
        StepView {
            heading: content.claim_heading.clone(),
            state: states[2],
            open: opens[2],
            body: claim_body,
        },
    ];

    StepsView { steps }
}
