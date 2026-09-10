//! Teaching in Higher Education with Citizen Science training. Four sequential
//! tracked steps matching orrery's HigherEdState: account → tutorial → create a
//! list → explore the AAPS site (which grants the badge). The "list" step is
//! recorded when the user creates or adds to a list (see `dashboard::lists`);
//! the "explore" step is recorded by the [`explore`] record-and-redirect
//! endpoint. State comes from `higher_ed_progress`.

use std::sync::Arc;

use anyhow::Error;
use common::message::user::{
    BasicInfo, WorkflowProgressRequest, WorkflowProgressResponse, WorkflowStatusResponse,
    WorkflowType,
};
use poem::{Response, handler, web::Redirect};
use sailfish::TemplateOnce;

use crate::middleware::authorization::Authenticated;
use crate::peering::LoadStorePeered;
use crate::training::{
    self, ArticulateCard, SidebarBody, SigninBody, StepView, StepsView, TrainingChrome,
    TrainingContent, TutorialBody, linear_states, module_url, open_flags, render_page,
    render_progress_fragment, tutorial_launch_url,
};

const BASE_PATH: &str = "/training/higher-ed";
const PROGRESS_PATH: &str = "/training/higher-ed/-progress";
/// Records the "explore" step then redirects to the AAPS site.
pub const EXPLORE_PATH: &str = "/training/higher-ed/explore";
const SLUG: &str = "higher-ed";

// An older Rise export: no launch activity id, but the rest of the context
// still has to be there.
const HIGHERED_URL: &str = "https://media.scistarter.org/highered/index.html";

/// Records the Higher Education "list" step for a user (fired when they create
/// or add to a list). Fire-and-forget; it is a no-op unless the user has
/// completed the tutorial. Lives here so the list subsystem has a single,
/// named entry point rather than open-coding the workflow call.
pub async fn record_list_step(user_id: &str) {
    let _: Result<WorkflowProgressResponse, _> = common::service::request(
        "user.workflow.progress",
        WorkflowProgressRequest {
            user_id: user_id.to_string(),
            workflow: WorkflowType::HigherEd,
            step: "list".to_string(),
        },
    )
    .await;
}

/// Editable copy for the Higher Education training. `#[serde(default)]` required
/// (see [`crate::training::foundations::FoundationsContent`]).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct HigherEdContent {
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
    pub list_heading: String,
    pub list_html: String,
    pub explore_heading: String,
    pub explore_html: String,
    /// Where the explore step's record-and-redirect endpoint sends the user.
    pub explore_url: String,
    pub sidebar_html: String,
}

impl Default for HigherEdContent {
    fn default() -> Self {
        HigherEdContent {
            title: "Teaching in Higher Education with Citizen Science".to_string(),
            sponsor: String::new(),
            intro: r#"<em>Teaching in Higher Education with Citizen Science</em> is an online, self-guided module that will provide you with information on some of the benefits of including citizen science in your undergraduate courses, practical strategies for doing so, and opportunities for networking and professional development. Upon completion of this module and related activities, you will receive details on how to claim your badge."#.to_string(),
            estimated_time: "2 hours".to_string(),
            badge_image: "/-static/img/badges/badge-tcshe.png".to_string(),
            skills_intro: "By the end of this tutorial, you will be able to:".to_string(),
            skills: vec![
                "Identify the benefits of including citizen science experiences in your courses.".to_string(),
                "Identify the utility of citizen science from the perspective of multiple learning theories and effective pedagogical approaches to undergraduate learning.".to_string(),
                "Select citizen science projects that align with your course learning objectives.".to_string(),
                "Select assessment approaches and tools (for teaching and research) that align with your chosen citizen science projects and course learning objectives.".to_string(),
                "Recognize the importance of alignment between pedagogy, learning objectives, assignment activities/projects, and assessment of learning outcomes.".to_string(),
                "Identify resources and strategies that will help prepare students in citizen science.".to_string(),
            ],
            skills_footnote: r#"You will practice or acquire the following skills, which are part of an Open Skills Taxonomy used by employers and schools: <a href="https://skills.lightcast.io/skills/KSLWYCD49AGA2P0SY8UE" target="_blank">constructivism</a>, <a href="https://skills.lightcast.io/skills/KS123R966Z5R6D81XZK4" target="_blank">experiential learning</a>, <a href="https://skills.lightcast.io/skills/KS123R9698P8RCDZJ7H8" target="_blank">experiential education</a>, <a href="https://skills.lightcast.io/skills/ESAEA6B36CACC973C466" target="_blank">formative and summative assessments</a>."#.to_string(),
            account_heading: "SciStarter Account".to_string(),
            tutorial_heading: "Complete the tutorial and quiz".to_string(),
            tutorial_intro: r#"<b>Complete this self-guided tutorial and quiz.</b> Score an 80% or higher to move on to the next steps towards claiming your badge."#.to_string(),
            articulate_url: HIGHERED_URL.to_string(),
            articulate_image: "https://media.scistarter.org/highered/High+Ed+Landing+Page+Image.png".to_string(),
            list_heading: "Create a List on SciStarter".to_string(),
            list_html: r#"<p><strong>Create a List on SciStarter</strong> by visiting <a href="/finder" target="_blank">the project finder</a>, selecting a project, and using the "Add to my lists" link. Select the list you want to add the project to from the drop-down menu, or create a new list by typing in a new list name, then click "Add to list". View the <a href="https://www.youtube.com/watch?v=umVbYNkbq2g" target="_blank">How To Create a SciStarter List video</a>. This step completes automatically once you've created or added to a list.</p>"#.to_string(),
            explore_heading: "Explore the Association for Advancing Participatory Sciences".to_string(),
            explore_html: r#"<p>Click through to the <a href="/training/higher-ed/explore" target="_blank">Association for Advancing Participatory Sciences site</a>. You'll find news, events, a journal, and more, all about citizen science. Visiting completes this step and earns your badge. Also, click <a href="https://scistarter.org/go/badging-highered-additional" target="_blank">here</a> to access additional resources and learn more about the role of citizen science in higher education classes.</p>"#.to_string(),
            explore_url: "https://participatorysciences.org/".to_string(),
            sidebar_html: String::new(),
        }
    }
}

impl LoadStorePeered for HigherEdContent {
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

impl HigherEdContent {
    pub async fn current() -> HigherEdContent {
        HigherEdContent::load(()).await.unwrap_or_default()
    }
}

#[handler]
pub async fn page(Authenticated(user_info): Authenticated) -> Result<Response, Error> {
    let content = HigherEdContent::current().await;
    let rendered = build_content(&content, user_info.clone()).await?;
    render_page(BASE_PATH, content.title.clone(), rendered, user_info).await
}

#[handler]
pub async fn progress(Authenticated(user_info): Authenticated) -> Result<Response, Error> {
    let content = HigherEdContent::current().await;
    let steps = build_steps(&content, &user_info).await;
    let count = training::tutorials_completed(WorkflowType::HigherEd, None).await;
    render_progress_fragment(steps, count)
}

/// Records the "explore" step (if signed in) and redirects to the AAPS site —
/// the hydra equivalent of orrery's state-trigger link.
#[handler]
pub async fn explore(Authenticated(user_info): Authenticated) -> Redirect {
    let content = HigherEdContent::current().await;
    if let Some(user) = user_info {
        let _: Result<WorkflowProgressResponse, _> = common::service::request(
            "user.workflow.progress",
            WorkflowProgressRequest {
                user_id: user.user_id.to_string(),
                workflow: WorkflowType::HigherEd,
                step: "explore".to_string(),
            },
        )
        .await;
    }
    Redirect::see_other(content.explore_url)
}

async fn build_content(
    content: &HigherEdContent,
    user_info: Option<Arc<BasicInfo>>,
) -> Result<TrainingContent, Error> {
    let live_updates = user_info.is_some();
    let steps = build_steps(content, &user_info).await;
    let tutorials_completed = training::tutorials_completed(WorkflowType::HigherEd, None).await;
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
        badge_alt: "Teaching in Higher Education badge".to_string(),
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
        sse_event: WorkflowType::HigherEd.slug().to_string(),
        progress_path: PROGRESS_PATH.to_string(),
    })
}

/// Four sequential tracked steps: account → tutorial → list → explore. Reaching
/// explore grants the badge, so there is no separate claim step.
async fn build_steps(content: &HigherEdContent, user_info: &Option<Arc<BasicInfo>>) -> StepsView {
    let logged_in = user_info.is_some();
    let status = match user_info {
        Some(user) => {
            training::fetch_status(WorkflowType::HigherEd, &user.user_id.to_string()).await
        }
        None => WorkflowStatusResponse {
            current_step: None,
            completed_steps: vec![],
            badge_granted: false,
        },
    };

    let done = |key: &str| status.completed_steps.iter().any(|step| step == key);
    let states = linear_states(&[logged_in, done("tutorial"), done("list"), done("explore")]);
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
        cards: vec![ArticulateCard {
            href: tutorial_launch_url(
                module_url(&content.articulate_url, HIGHERED_URL),
                None,
                user_info,
            ),
            image: content.articulate_image.clone(),
        }],
        extra: String::new(),
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
            heading: content.list_heading.clone(),
            state: states[2],
            open: opens[2],
            body: content.list_html.clone(),
        },
        StepView {
            heading: content.explore_heading.clone(),
            state: states[3],
            open: opens[3],
            body: content.explore_html.clone(),
        },
    ];

    StepsView { steps }
}
