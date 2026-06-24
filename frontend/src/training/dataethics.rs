//! Data Ethics for Practitioners training — a clean three-step training
//! (Account → Tutorial → Claim) on the shared [`crate::training`] framework.
//! Ported from orrery's `/training-dataethics`, now at `/training/data-ethics`.
//! State comes from the user's `data_ethics_progress` timestamps.

use std::sync::Arc;

use anyhow::Error;
use common::message::user::{BasicInfo, WorkflowStatusResponse, WorkflowType};
use poem::{Response, handler, web::Redirect};
use sailfish::TemplateOnce;

use crate::middleware::authorization::Authenticated;
use crate::peering::LoadStorePeered;
use crate::training::{
    self, ArticulateCard, ClaimBody, SidebarBody, SigninBody, StepView, StepsView, TrainingContent,
    TutorialBody, linear_states, open_flags, render_page, render_progress_fragment,
};

const BASE_PATH: &str = "/training/data-ethics";
const PROGRESS_PATH: &str = "/training/data-ethics/-progress";
const SLUG: &str = "data-ethics";

/// Editable copy for the Data Ethics training. See
/// [`crate::training::foundations::FoundationsContent`] for the pattern;
/// `#[serde(default)]` is required so added fields fill from `Default` instead
/// of discarding saved edits.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct DataEthicsContent {
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
    pub articulate_url: String,
    pub articulate_image: String,
    pub claim_heading: String,
    pub claim_html: String,
    pub sidebar_html: String,
}

impl Default for DataEthicsContent {
    fn default() -> Self {
        DataEthicsContent {
            title: "Data Ethics for Practitioners".to_string(),
            sponsor: String::new(),
            intro: r#"The "Data Ethics" tutorial and badge are designed for practitioners and leaders of participatory science projects, including citizen science and community science projects. The tutorial will help you “think like an ethicist” by identifying relevant ethical obligations, tensions, and issues that may arise in your project, and use them to make decisions. The tutorial is intended to help you understand the unique obligations for collecting, stewarding, and disseminating project data in ways that foster trust between practitioners, participants, and partners.
<br>To maximize your engagement with and implementation of this material, we encourage you to become a member of the <a href="https://participatorysciences.org/" target="_blank">Association for Advancing Participatory Sciences</a> for access to more experiences and expertise across this diverse community of practice. AAPS membership is not required to access this tutorial or receive the badge, but one goal of this resource is to support the CSA’s capacity to create and maintain a culture of ethical data practices in the participatory sciences."#.to_string(),
            estimated_time: "2 hours".to_string(),
            badge_image: "https://media.scistarter.org/curated/2023/Data_Ethics_with_border.png"
                .to_string(),
            skills_intro: "By the end of this tutorial, you will be able to:".to_string(),
            skills: vec![
                "<strong>Distinguish</strong> roles and ethical obligations of project leaders, participants, and partners in participatory science projects.".to_string(),
                "<strong>Identify</strong> how ethical tensions arise in participatory science projects.".to_string(),
                "<strong>Recognize</strong> the obligations associated with ethical principles in the context of participatory science projects.".to_string(),
                "<strong>Analyze</strong> ethical tensions related to data integrity, report-outs, recompense, and report-backs in participatory science projects.".to_string(),
                "<strong>Recognize</strong> how data governance structures can concentrate or diffuse decision-making power.".to_string(),
            ],
            skills_footnote: r#"You will practice or acquire the following skills, which are part of an Open Skills Taxonomy used by employers and schools: <a href="https://skills.lightcast.io/skills/KS122P86NZFH1GP38G15" target="_blank">data integrity</a>, <a href="https://skills.lightcast.io/skills/KS122P378DGNVX4NKQKN" target="_blank">data governance</a>."#.to_string(),
            account_heading: "SciStarter Account".to_string(),
            tutorial_heading: "Self-guided tutorial and quiz".to_string(),
            tutorial_intro: r#"<b>Complete this self-guided tutorial and quiz.</b> Score an 80% or higher to move onto the next steps towards claiming your badge!"#.to_string(),
            articulate_url: "#".to_string(),
            articulate_image: "https://orrery-media.s3.amazonaws.com/uploads/froala_editor/images/Data%20Ethics%20Website%20Image_a3PGAki.png".to_string(),
            claim_heading: "Claim your badge".to_string(),
            claim_html: r#"<h2>Congratulations!</h2>
<p>You'll find your badge on your <a href="/dashboard/profile">SciStarter profile</a>, and you'll receive an email with instructions for sharing the verified badge and metadata on LinkedIn.</p>"#.to_string(),
            sidebar_html: r#"<div>
    <p class="text-sm">This material is based upon input from over 100 individuals so that it reflects both areas of broad agreement and areas of divergent priorities and values. Resources were developed and field tested by a team with expertise in instructional design, education, libraries, inclusive practices, digital design, micro accreditation and, of course, citizen science to produce a series of tutorials, trainings, and accompanying modules to help people from all walks of life discover and engage in authentic science.</p>
    <hr class="my-8 w-[50px] text-primary-light" />
    <p class="text-sm">Learn more about <a class="text-sm" href="https://scistarter.org/go/badging-brochure-2025" target="_blank">SciStarter Badges</a>.</p>
    <a href="https://scistarter.org/go/badging-brochure-2025" rel="noopener noreferrer" target="_blank"><img class="border" src="https://media.scistarter.org/curated/2025/SciStarter_Badging_Program_Brochure_2025_-_C.jpg" ></a>
    <hr class="my-8 w-[50px] text-primary-light" />
    <p>Optional: download the <em>PDF version</em> of the <a href="https://scistarter.org/go/data-ethics-toolkit" target="_blank">Data Ethics for Practitioners Toolkit</a>.</p>
    <a href="https://scistarter.org/go/data-ethics-toolkit" rel="noopener noreferrer" target="_blank"><img class="border" src="https://orrery-media.s3.amazonaws.com/uploads/froala_editor/images/Data%20Ethics%20Toolkit%20thumbnail.png" ></a>
</div>"#.to_string(),
        }
    }
}

impl LoadStorePeered for DataEthicsContent {
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

impl DataEthicsContent {
    /// Current editable content, or built-in defaults if none saved yet.
    pub async fn current() -> DataEthicsContent {
        DataEthicsContent::load(()).await.unwrap_or_default()
    }
}

/// Full page at `/training/data-ethics`.
#[handler]
pub async fn page(Authenticated(user_info): Authenticated) -> Result<Response, Error> {
    let content = DataEthicsContent::current().await;
    let rendered = build_content(&content, user_info.clone()).await?;
    render_page(BASE_PATH, content.title.clone(), rendered, user_info).await
}

/// SSE-triggered fragment at `/training/data-ethics/-progress`.
#[handler]
pub async fn progress(Authenticated(user_info): Authenticated) -> Result<Response, Error> {
    let content = DataEthicsContent::current().await;
    let steps = build_steps(&content, &user_info).await;
    let count = training::tutorials_completed(WorkflowType::DataEthics).await;
    render_progress_fragment(steps, count)
}

/// 301 redirect from the legacy `/training-dataethics` URL.
#[handler]
pub async fn legacy_redirect() -> Redirect {
    Redirect::moved_permanent(BASE_PATH)
}

async fn build_content(
    content: &DataEthicsContent,
    user_info: Option<Arc<BasicInfo>>,
) -> Result<TrainingContent, Error> {
    let live_updates = user_info.is_some();
    let steps = build_steps(content, &user_info).await;
    let tutorials_completed = training::tutorials_completed(WorkflowType::DataEthics).await;
    let sidebar_html = SidebarBody {
        badge_image: content.badge_image.clone(),
        html: content.sidebar_html.clone(),
    }
    .render_once()
    .unwrap_or_default();

    Ok(TrainingContent {
        breadcrumb_label: content.title.clone(),
        badge_image: content.badge_image.clone(),
        badge_alt: "Data Ethics for Practitioners badge".to_string(),
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
        sse_event: WorkflowType::DataEthics.slug().to_string(),
        progress_path: PROGRESS_PATH.to_string(),
    })
}

/// Three linear steps: account (signed in) → tutorial (recorded) → badge.
async fn build_steps(content: &DataEthicsContent, user_info: &Option<Arc<BasicInfo>>) -> StepsView {
    let logged_in = user_info.is_some();
    let status = match user_info {
        Some(user) => {
            training::fetch_status(WorkflowType::DataEthics, &user.user_id.to_string()).await
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
    }
    .render_once()
    .unwrap_or_default();

    let tutorial_body = TutorialBody {
        intro: content.tutorial_intro.clone(),
        cards: vec![ArticulateCard {
            href: content.articulate_url.clone(),
            image: content.articulate_image.clone(),
        }],
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
