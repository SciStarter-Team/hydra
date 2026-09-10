//! Citizen Science for K-12 Educators training. Four steps mirroring orrery's
//! `/training-k12`: account → Foundations prerequisite → tutorial (which grants
//! the badge) → congratulations. Tracked state comes from `k12_progress`.

use std::sync::Arc;

use anyhow::Error;
use common::message::user::{BasicInfo, WorkflowStatusResponse, WorkflowType};
use poem::{Response, handler};
use sailfish::TemplateOnce;

use crate::middleware::authorization::Authenticated;
use crate::peering::LoadStorePeered;
use crate::training::{
    self, ArticulateCard, SidebarBody, SigninBody, StepView, StepsView, TrainingChrome,
    TrainingContent, TutorialBody, linear_states, module_url, open_flags, render_page,
    render_progress_fragment, tutorial_launch_url,
};

const BASE_PATH: &str = "/training/k12";
const PROGRESS_PATH: &str = "/training/k12/-progress";
const SLUG: &str = "k12";

const K12_URL: &str = "https://media.scistarter.org/k12/scormdriver/indexAPI.html";

/// The Rise package's own xAPI activity id, as its `tincan.xml` spells it.
const K12_ACTIVITY_ID: &str = "http://Fr0w9cmLH3BLsC9yKP_zbzhERPeLDG6t_rise";

/// Editable copy for the K-12 Educators training. `#[serde(default)]` required
/// (see [`crate::training::foundations::FoundationsContent`]).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct K12Content {
    pub title: String,
    pub sponsor: String,
    pub intro: String,
    pub estimated_time: String,
    pub badge_image: String,
    pub skills_intro: String,
    pub skills: Vec<String>,
    pub skills_footnote: String,
    pub account_heading: String,
    pub prereq_heading: String,
    pub prereq_html: String,
    pub tutorial_heading: String,
    pub tutorial_intro: String,
    pub articulate_url: String,
    pub articulate_image: String,
    pub claim_heading: String,
    pub claim_html: String,
    pub sidebar_html: String,
}

impl Default for K12Content {
    fn default() -> Self {
        K12Content {
            title: "Citizen Science for K-12 Educators".to_string(),
            sponsor: "Presented by SciStarter".to_string(),
            intro: "<p>The Citizen Science for K-12 Educators badge is awarded to individuals who have translated their curiosity into impact by demonstrating foundational citizen science skills and learning how to bring citizen science into their own K-12 classrooms.</p>".to_string(),
            estimated_time: "1 hour".to_string(),
            badge_image: "https://static.scistarter.org/img/badge-k12.png".to_string(),
            skills_intro: "By the end of this tutorial and the required steps to earn this badge, you'll be able to:".to_string(),
            skills: vec![
                "<strong>Explain</strong> the role of citizen science in advancing scientific research and public engagement.".to_string(),
                "<strong>Apply</strong> citizen science practices to strengthen teaching and learning across educational settings.".to_string(),
                "<strong>Use</strong> the SciStarter Project Finder to identify citizen science projects aligned to classroom goals, grade levels, student interests, and local contexts.".to_string(),
                "<strong>Identify</strong> which citizen science experiences are appropriate to different grade levels and developmental stages.".to_string(),
                "<strong>Integrate</strong> citizen science into multidisciplinary learning experiences across content areas.".to_string(),
                "<strong>Create</strong> and organize a classroom-ready collection of citizen science projects using SciStarter Lists to support student participation and investigation.".to_string(),
                "<strong>Facilitate</strong> student participation in citizen science through SciStarter accounts and dashboards to support reflection, documentation, and sustained scientific engagement.".to_string(),
                "<strong>Transform</strong> schoolyards and community spaces into living laboratories for observation, investigation, and discovery.".to_string(),
                "<strong>Participate</strong> in two citizen science projects and reflect on classroom applications.".to_string(),
            ],
            skills_footnote: r#"You will practice or acquire the following skills, which are part of an “open skills taxonomy” used by employers and schools: <a href="https://lightcast.io/open-skills/skills/KS122LF6587VF0NZFZJW/curriculum-development" target="_blank">Curriculum Development</a>, <a href="https://lightcast.io/open-skills/skills/KS122LF709JLK7D9PL8J/curriculum-mapping" target="_blank">Curriculum Mapping</a>, <a href="https://lightcast.io/open-skills/skills/KS1256C6DBYG39V53J4V/instructional-design" target="_blank">Instructional Design</a>, <a href="https://lightcast.io/open-skills/skills/KS120PQ6XD1JWMFQVJNW/teaching" target="_blank">Teaching</a>, <a href="https://lightcast.io/open-skills/skills/KS125SS6Z01PYP99SN5G/lesson-planning" target="_blank">Lesson Planning</a>, <a href="https://lightcast.io/open-skills/skills/BGS4866E4D4C7E1F272F/k-12-education" target="_blank">K-12 Education</a>, <a href="https://lightcast.io/open-skills/skills/KS125856JH9VGVNY0TBP/interdisciplinary-teaching" target="_blank">Interdisciplinary Teaching</a>, and <a href="https://lightcast.io/open-skills/skills/KS123R966Z5R6D81XZK4/experiential-learning" target="_blank">Experiential Learning</a>."#.to_string(),
            account_heading: "SciStarter Account".to_string(),
            prereq_heading: "Complete the Foundations of Citizen Science Training".to_string(),
            prereq_html: r#"<p>Complete the <a href="/training/foundations">Foundations of Citizen Science</a> training module and earn the badge to access the Citizen Science for K-12 Educators Tutorial.</p>"#.to_string(),
            tutorial_heading: "K-12 training and quiz".to_string(),
            tutorial_intro: "Complete the Citizen Science for K-12 Educators Training Module."
                .to_string(),
            // Base URL of the K-12 Rise package. It is wrapped per-user with the
            // LRS launch parameters at render time (see `build_steps`).
            articulate_url: K12_URL.to_string(),
            articulate_image: "https://orrery-media.s3.amazonaws.com/attr/2026-07/Citizen_Science_in_K-12_Education.png".to_string(),
            claim_heading: "Congratulations!".to_string(),
            claim_html: r#"<p>🎉 Congratulations on completing the Citizen Science for K-12 Educators Training. 🎉
<br>You’ve taken an exciting step in bringing citizen science into your classroom.
<br>Your <strong>badge</strong> is now available on your <a href="/dashboard/profile">SciStarter profile</a>.</p>
<p>You will also receive an email with instructions for sharing the verified badge and metadata on LinkedIn.</p>
<p><strong>Coming Soon:</strong> Project-specific modules and certificates!</p>"#.to_string(),
            sidebar_html: String::new(),
        }
    }
}

impl LoadStorePeered for K12Content {
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

impl K12Content {
    pub async fn current() -> K12Content {
        K12Content::load(()).await.unwrap_or_default()
    }
}

#[handler]
pub async fn page(Authenticated(user_info): Authenticated) -> Result<Response, Error> {
    let content = K12Content::current().await;
    let rendered = build_content(&content, user_info.clone()).await?;
    render_page(BASE_PATH, content.title.clone(), rendered, user_info).await
}

#[handler]
pub async fn progress(Authenticated(user_info): Authenticated) -> Result<Response, Error> {
    let content = K12Content::current().await;
    let steps = build_steps(&content, &user_info).await;
    let count = training::tutorials_completed(WorkflowType::K12, None).await;
    render_progress_fragment(steps, count)
}

async fn build_content(
    content: &K12Content,
    user_info: Option<Arc<BasicInfo>>,
) -> Result<TrainingContent, Error> {
    let live_updates = user_info.is_some();
    let steps = build_steps(content, &user_info).await;
    let tutorials_completed = training::tutorials_completed(WorkflowType::K12, None).await;
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
        badge_alt: "Citizen Science for K-12 Educators badge".to_string(),
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
        sse_event: WorkflowType::K12.slug().to_string(),
        progress_path: PROGRESS_PATH.to_string(),
    })
}

/// Four linear steps. The congratulations step is a celebration and never
/// completes, so it stays `Current` once the tutorial is done.
async fn build_steps(content: &K12Content, user_info: &Option<Arc<BasicInfo>>) -> StepsView {
    let logged_in = user_info.is_some();
    let (status, foundations_done) = match user_info {
        Some(user) => {
            let id = user.user_id.to_string();
            let status = training::fetch_status(WorkflowType::K12, &id).await;
            let foundations = training::fetch_status(WorkflowType::Foundations, &id).await;
            (status, foundations.badge_granted)
        }
        None => (
            WorkflowStatusResponse {
                current_step: None,
                completed_steps: vec![],
                badge_granted: false,
            },
            false,
        ),
    };

    let tutorial_done = status.completed_steps.iter().any(|s| s == "tutorial");

    let states = linear_states(&[logged_in, foundations_done, tutorial_done, false]);
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

    let tutorial_href = tutorial_launch_url(
        module_url(&content.articulate_url, K12_URL),
        Some(K12_ACTIVITY_ID),
        user_info,
    );

    let tutorial_body = TutorialBody {
        intro: content.tutorial_intro.clone(),
        cards: vec![ArticulateCard {
            href: tutorial_href,
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
            heading: content.prereq_heading.clone(),
            state: states[1],
            open: opens[1],
            body: content.prereq_html.clone(),
        },
        StepView {
            heading: content.tutorial_heading.clone(),
            state: states[2],
            open: opens[2],
            body: tutorial_body,
        },
        StepView {
            heading: content.claim_heading.clone(),
            state: states[3],
            open: opens[3],
            body: content.claim_html.clone(),
        },
    ];

    StepsView { steps }
}
