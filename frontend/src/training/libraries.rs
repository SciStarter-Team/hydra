//! Libraries as Community Hubs for Citizen Science training. Five linear steps:
//! account → Foundations prerequisite (cross-training) → tutorial → join the
//! Library Network (tracked "supplemental") → claim. Ported from orrery's
//! `/training-library`, now at `/training/libraries-hubs`. State comes from
//! `libraries_hubs_progress` (plus the user's Foundations badge for the
//! prerequisite step).

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

const BASE_PATH: &str = "/training/libraries-hubs";
const PROGRESS_PATH: &str = "/training/libraries-hubs/-progress";
const SLUG: &str = "libraries-hubs";

// An older Rise export: it reports its result without a launch activity id,
// but not without the rest of the launch context.
const LIBRARIES_URL: &str = "https://media.scistarter.org/libraries-hubs-v2/index.html";

/// Editable copy for the Libraries & Hubs training. `#[serde(default)]` required
/// (see [`crate::training::foundations::FoundationsContent`]).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct LibrariesContent {
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
    pub supplemental_heading: String,
    pub supplemental_html: String,
    pub claim_heading: String,
    pub claim_html: String,
    pub sidebar_html: String,
}

impl Default for LibrariesContent {
    fn default() -> Self {
        LibrariesContent {
            title: "Libraries as Community Hubs for Citizen Science Training".to_string(),
            sponsor: "Presented by SciStarter".to_string(),
            intro: r#"<p>This training will empower your library to become a vibrant community hub for citizen science by providing the knowledge, skills, and resources needed to engage your community in real scientific research. Participants will explore the power of citizen science, learning why it is an impactful approach to engaging diverse audiences and how it differs from other science engagement opportunities.</p>
<p>The training will also highlight the unique role libraries can play in citizen science engagement, drawing on successful examples from the Citizen and Community Science Library Network. Attendees will discover how to leverage SciStarter resources to plan, promote, and implement citizen science events and programs in a library setting. Finally, the training will guide participants in developing strategies to launch and sustain citizen science initiatives, ensuring long-term success within their communities.</p>"#.to_string(),
            estimated_time: "1 hour".to_string(),
            badge_image: "/-static/img/badges/badge-libraries-hubs.png".to_string(),
            skills_intro: "By the end of this course, you will be able to:".to_string(),
            skills: vec![
                "<strong>Recognize</strong> why engaging the public in citizen science is a critical component of library lifelong learning efforts.".to_string(),
                "<strong>Identify</strong> the benefits of turning your library into a community hub for citizen science.".to_string(),
                "<strong>Identify</strong> strategies and tools for selecting and aligning the right citizen science programs with the interests or concerns of your community.".to_string(),
                "<strong>Identify</strong> resources (including health and medical resources) that will help you introduce, supplement, support, and facilitate awareness of and engagement in citizen science at your library.".to_string(),
                "<strong>Identify</strong> resources and strategies that will help you train staff to recruit and sustain patron engagement in citizen science.".to_string(),
            ],
            skills_footnote: r#"You will practice or acquire the following skills which are part of an "open skills taxonomy" used by employers and schools: <a href="https://skills.lightcast.io/skills/KS123987840L8J17862K" target="_blank">evidence-based practice</a>, <a href="https://skills.lightcast.io/skills/KS125RC6CTWJ5S7841WG" target="_blank">library and information services</a>, <a href="https://skills.lightcast.io/skills/KS125TG66T8TXK7B2VT3" target="_blank">library services</a>, <a href="https://skills.lightcast.io/skills/KS122556LMQ829GZCCRV" target="_blank">communication</a>, <a href="https://skills.lightcast.io/skills/KS1254G6X37NS306Y596" target="_blank">information literacy</a>, <a href="https://skills.lightcast.io/skills/KS125856JH9VGVNY0TBP" target="_blank">interdisciplinary teaching</a>, <a href="https://skills.lightcast.io/skills/KS12100638C964SSQDJD" target="_blank">library instruction</a>, <a href="https://skills.lightcast.io/skills/KS125TX75XTZZXVJL8B5" target="_blank">lifelong learning</a>, <a href="https://skills.lightcast.io/skills/KS120PQ6XD1JWMFQVJNW" target="_blank">teaching</a>, and <a href="https://skills.lightcast.io/skills/KS122F96DJHT3ZXB8FCZ" target="_blank">training and development</a>."#.to_string(),
            account_heading: "Create or Log in to your SciStarter Account".to_string(),
            prereq_heading: "Complete the Foundations of Citizen Science Training".to_string(),
            prereq_html: r#"<p>Complete the <a href="/training/foundations">Foundations of Citizen Science</a> training module and earn the badge to access the rest of this training.</p>"#.to_string(),
            tutorial_heading: "Complete the Libraries as Community Hubs for Citizen Science Tutorial and Quiz".to_string(),
            tutorial_intro: r#"Click <q>Start</q> to begin the self-guided tutorial."#.to_string(),
            articulate_url: LIBRARIES_URL.to_string(),
            articulate_image: "https://static.scistarter.org/img/training/libraries.png".to_string(),
            supplemental_heading: "Join the Citizen and Community Science Library Network".to_string(),
            supplemental_html: r#"<p>The Library Network provides free resources, peer-to-peer learning and more to support you and your library. Complete the brief form <a href="https://scistarter.org/go/library-network?next=/training/libraries-hubs">here</a>.</p>"#.to_string(),
            claim_heading: "Congratulations!".to_string(),
            claim_html: r#"<p>🎉 <strong>Congratulations on completing the Libraries as Community Hubs for Citizen Science training.</strong> 🎉</p>
<p>You've taken an exciting step in bringing citizen science to your library.<br>
Your <strong>badge</strong> is now available on your <a href="/dashboard/profile">SciStarter profile</a>.</p>
<p>You've also unlocked access to the downloadable <strong>Library Resource Guide</strong>:<br>
👉 <a href="https://docs.google.com/document/d/1n1di73boTPznvmgP3ZOFjNaPIOD9XkNpiFDw-yvh3Ho/edit" target="_blank">Download the Guide</a></p>
<p>This handbook is packed with tips, talking points, free templates, suggested activities, and other resources to help you introduce citizen science and SciStarter to others with confidence!</p>
<p><strong>What to Do Next:</strong><br>
✅ <strong>Join the Library Network Facebook Group</strong> to connect with others and get inspired: <a href="https://www.facebook.com/groups/788879025402547" target="_blank">Library Network group</a><br>
✅ <strong>Reach out to a <a href="/training/champion">Citizen Science Champion</a></strong> for support planning and facilitating programs.<br>
✅ <strong><a href="/add-event">Add your events</a></strong> to the SciStarter Event Calendar to promote and track your impact.<br>
✅ These <strong>resources</strong> and more can be found on <a href="https://scistarter.org/library">SciStarter.org/library</a>.<br>
✅ Look for more resources and updates in our monthly newsletter!</p>"#.to_string(),
            sidebar_html: r#"<p class="mt-4">With support from Arizona State University, the National Library of Medicine, and others, SciStarter assembled a team with expertise in instructional design, education, libraries, digital design, micro accreditation, and, of course, citizen science to produce a series of tutorials, trainings, and accompanying modules.</p>
<p>This page provides a training resource designed to help facilitators, particularly librarians, across a spectrum of capacities, integrate SciStarter and participatory science into their communities.</p>
<p>This work is licensed under a Creative Commons Attribution-ShareAlike 4.0 International License.</p>"#.to_string(),
        }
    }
}

impl LoadStorePeered for LibrariesContent {
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

impl LibrariesContent {
    pub async fn current() -> LibrariesContent {
        LibrariesContent::load(()).await.unwrap_or_default()
    }
}

#[handler]
pub async fn page(Authenticated(user_info): Authenticated) -> Result<Response, Error> {
    let content = LibrariesContent::current().await;
    let rendered = build_content(&content, user_info.clone()).await?;
    render_page(BASE_PATH, content.title.clone(), rendered, user_info).await
}

#[handler]
pub async fn progress(Authenticated(user_info): Authenticated) -> Result<Response, Error> {
    let content = LibrariesContent::current().await;
    let steps = build_steps(&content, &user_info).await;
    let count = training::tutorials_completed(WorkflowType::LibrariesHubs).await;
    render_progress_fragment(steps, count)
}

async fn build_content(
    content: &LibrariesContent,
    user_info: Option<Arc<BasicInfo>>,
) -> Result<TrainingContent, Error> {
    let live_updates = user_info.is_some();
    let steps = build_steps(content, &user_info).await;
    let tutorials_completed = training::tutorials_completed(WorkflowType::LibrariesHubs).await;
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
        badge_alt: "Libraries as Community Hubs badge".to_string(),
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
        sse_event: WorkflowType::LibrariesHubs.slug().to_string(),
        progress_path: PROGRESS_PATH.to_string(),
    })
}

/// Five linear steps. The Foundations prerequisite step is gated by the user's
/// Foundations badge (a separate workflow); tutorial and the Library Network
/// signup are tracked on this workflow.
async fn build_steps(content: &LibrariesContent, user_info: &Option<Arc<BasicInfo>>) -> StepsView {
    let logged_in = user_info.is_some();
    let (status, foundations_done) = match user_info {
        Some(user) => {
            let id = user.user_id.to_string();
            let status = training::fetch_status(WorkflowType::LibrariesHubs, &id).await;
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

    let tutorial_done = status.completed_steps.iter().any(|step| step == "tutorial");
    let supplemental_done = status
        .completed_steps
        .iter()
        .any(|step| step == "supplemental");
    let states = linear_states(&[
        logged_in,
        foundations_done,
        tutorial_done,
        supplemental_done,
        status.badge_granted,
    ]);
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
                module_url(&content.articulate_url, LIBRARIES_URL),
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
            heading: content.supplemental_heading.clone(),
            state: states[3],
            open: opens[3],
            body: content.supplemental_html.clone(),
        },
        StepView {
            heading: content.claim_heading.clone(),
            state: states[4],
            open: opens[4],
            body: content.claim_html.clone(),
        },
    ];

    StepsView { steps }
}
