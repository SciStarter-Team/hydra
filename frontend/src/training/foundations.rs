//! Foundations of Citizen Science — the first training built on the shared
//! [`crate::training`] framework. Ported from orrery's `/training-foundations`
//! (now served at `/training/foundations`). State is derived from the user's
//! `foundations_progress` completion timestamps in the user-service.
//!
//! Editable copy lives in [`FoundationsContent`] (admin-managed, peered);
//! structure, gating, and project-completion mappings live in code here.

use std::sync::Arc;

use anyhow::Error;
use common::message::user::{BasicInfo, WorkflowStatusResponse, WorkflowType};
use poem::{Response, handler};
use sailfish::TemplateOnce;

use crate::middleware::authorization::Authenticated;
use crate::peering::LoadStorePeered;
use crate::training::{
    self, ArticulateCard, ClaimBody, ProjectCardView, SidebarBody, SigninBody, StepView, StepsView,
    TrainingChrome, TrainingContent, TutorialBody, linear_states, module_url, open_flags,
    render_page, render_progress_fragment, tutorial_launch_url,
};

const BASE_PATH: &str = "/training/foundations";
const PROGRESS_PATH: &str = "/training/foundations/-progress";
const SLUG: &str = "foundations";

// Canonical project pages and the completion keys they map to. Structural — not
// editable content.
// The tutorial's two builds. Both are older Rise exports that report a
// result without a launch activity id, so they are launched without one --
// but with everything else, or they report nothing at all.
const FOUNDATIONS_EN_URL: &str = "https://media.scistarter.org/foundations/index.html";
const FOUNDATIONS_ES_URL: &str = "https://media.scistarter.org/fundamentos/index.html";

const SQUIRREL_LINK: &str = "/projects/project-squirrel";
const STALL_LINK: &str = "/projects/stall-catchers-by-eyesonalz";

// The EN/ES tutorial card images are fixed design assets (the editable part is
// the Articulate URL each links to).
const ARTICULATE_EN_IMAGE: &str = "https://static.scistarter.org/img/training/foundations-en.png";
const ARTICULATE_ES_IMAGE: &str = "https://static.scistarter.org/img/training/foundations-es.png";

/// Editable text content for the Foundations training. Structure, gating, and
/// project-completion mappings live in code (below); this holds only the copy
/// the Engine Room admin can edit. `Default` carries the live copy, so it
/// doubles as the seed and the page renders unchanged until first edited.
///
/// `#[serde(default)]` is essential: stored JSON is read back through this
/// struct, so when a field is added later, rows saved before it deserialize by
/// filling the new field from `Default` rather than failing (which would
/// silently discard an admin's saved edits). Every training content struct must
/// carry it.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct FoundationsContent {
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
    pub articulate_es: String,
    pub projects_heading: String,
    pub projects_intro: String,
    pub squirrel_title: String,
    pub squirrel_description: String,
    pub squirrel_image: String,
    pub squirrel_time_tag: String,
    pub squirrel_place_tag: String,
    pub stall_title: String,
    pub stall_description: String,
    pub stall_image: String,
    pub stall_time_tag: String,
    pub stall_place_tag: String,
    pub claim_heading: String,
    pub claim_html: String,
    pub sidebar_html: String,
}

impl Default for FoundationsContent {
    fn default() -> Self {
        FoundationsContent {
            title: "Foundations of Citizen Science".to_string(),
            sponsor: "Presented by SciStarter with support from the National Library of Medicine"
                .to_string(),
            intro: "The Foundations of Citizen Science badge is awarded to individuals who have \
                translated their curiosity into impact by demonstrating foundational citizen science \
                skills and contributing to advancing scientific research on the SciStarter platform."
                .to_string(),
            estimated_time: "1 hour".to_string(),
            badge_image: "/-static/img/badges/foundations.png".to_string(),
            skills_intro: "By the end of this tutorial and the required steps to earn this badge, \
                you'll demonstrate all of the following foundational citizen science \
                skills/competencies within the SciStarter platform and obtain a score of 80% or \
                better on the quiz embedded in the tutorial:"
                .to_string(),
            skills: vec![
                "Recognize how citizen science can address real-world challenges and questions."
                    .to_string(),
                "Elaborate on ways that participation in citizen science projects benefits the \
                    scientific community and society."
                    .to_string(),
                "Recognize high-level eligibility requirements for who can contribute to citizen \
                    science projects."
                    .to_string(),
                "Identify, explore, and access one or more citizen science projects.".to_string(),
                "Articulate the purpose of SciStarter and explain how citizen scientists and \
                    research scientists can use the platform to connect curiosity with impact."
                    .to_string(),
                "Successfully register for a SciStarter account.".to_string(),
                "Perform the requisite steps to participate in and contribute to one or more \
                    citizen science projects."
                    .to_string(),
                "Follow the necessary steps to locate and bookmark projects on the SciStarter \
                    platform."
                    .to_string(),
            ],
            skills_footnote: r#"You'll also practice or acquire the following skills which are part of an "open skills taxonomy" used by employers and schools: <a href="https://lightcast.io/open-skills/skills/KS120GV6C72JMSZKMTD7/data-analysis" target="_blank">data analysis</a>, <a href="https://lightcast.io/open-skills/skills/KS122NY60HCYYDXHQGQN/data-entry" target="_blank">data entry</a>, <a href="https://lightcast.io/open-skills/skills/ESE2D5EA264FB06F5C4F/information-gathering" target="_blank">information gathering</a>, and <a href="https://lightcast.io/open-skills/skills/KSG21Y30BQVH1S8XMJHK/questioning-skills" target="_blank">questioning skills</a>."#.to_string(),
            account_heading: "SciStarter Account".to_string(),
            tutorial_heading: "Self-guided tutorial and quiz".to_string(),
            tutorial_intro: r#"<b>Complete this self-guided tutorial and quiz.</b> Score an 80% or higher to move on to the next steps towards claiming your badge! This module opens in a new tab. Return to this page upon completion to finish earning your badge."#.to_string(),
            articulate_en: FOUNDATIONS_EN_URL.to_string(),
            articulate_es: FOUNDATIONS_ES_URL.to_string(),
            projects_heading: "Participate in Two Citizen Science Projects".to_string(),
            projects_intro: r#"<b>Participate in two citizen science projects.</b> It's time to participate in two projects selected because they represent common types of citizen science projects: one where you add data observations to help when there's not enough data; and another where you review and classify data to help when there's too much data. Even if you've completed these projects before, repeat them now to proceed to Step 4 of the training. Important: When you create your Stall Catchers account, be sure to use the same email address you used to create your SciStarter account! And remember, you can find thousands of additional projects anytime on SciStarter.org."#.to_string(),
            squirrel_title: "Project Squirrel".to_string(),
            squirrel_description: "Help scientists better understand tree squirrel ecology. Zero is an important data point too! Participate in this project from anywhere.".to_string(),
            squirrel_image: "https://orrery-media.s3.amazonaws.com/library/project_squirrel-card_NqhYvGZ.jpeg".to_string(),
            squirrel_time_tag: "15–30 minutes".to_string(),
            squirrel_place_tag: "outdoors".to_string(),
            stall_title: "Stall Catchers".to_string(),
            stall_description: "Accelerate Alzheimer's research by playing an online game. Make at least one classification and return here.".to_string(),
            stall_image: "https://orrery-media.s3.amazonaws.com/library/stall_catchers_card_IMAGE_ahutvR6.png".to_string(),
            stall_time_tag: "15–30 minutes".to_string(),
            stall_place_tag: "indoors".to_string(),
            claim_heading: "Claim your badge".to_string(),
            claim_html: r#"<h2>Congratulations!</h2>
<p>You'll find your badge on your <a href="/dashboard/profile">SciStarter profile</a>, and you'll receive an email with instructions for sharing the verified badge and metadata on LinkedIn.</p>"#.to_string(),
            sidebar_html: r#"<img src="https://static.scistarter.org/backport/img/training/nlm_logo.png" alt="National Library of Medicine logo" class="mt-8" />
<div>
    <p class="text-sm">With support from the National Library of Medicine, Arizona State University, North Carolina State University, the Moore Foundation, and the Institute for Museum and Library Services, SciStarter assembled a team with expertise in instructional design, education, inclusive practices, digital design, micro accreditation, and, of course, citizen science to produce a series of online, self-guided training modules and accompanying badges and resources to help people from all walks of life engage in science and bring citizen science to new communities.</p>
    <p class="text-sm">Find ASU Evaluation Outcomes <a class="text-sm" href="https://scistarter.org/go/foundations-training-evaluation-report" rel="noopener noreferrer" target="_blank">here</a>.</p>
    <hr class="my-8 w-[50px] text-primary-light" />
    <p class="text-sm">Learn more about <a class="text-sm" href="https://scistarter.org/go/badging-brochure-2025">SciStarter Badges</a>.</p>
    <a href="https://scistarter.org/go/badging-brochure-2025" rel="noopener noreferrer" target="_blank"><img class="border" src="https://media.scistarter.org/curated/2025/SciStarter_Badging_Program_Brochure_2025_-_C.jpg" ></a>
    <hr class="my-8 w-[50px] text-primary-light" />
    <p>Once you've completed the tutorial, download the <em>Foundations of Citizen Science</em> <a href="https://scistarter.org/go/Foundations-of-Citizen-Science-facilitator-guide-March-2025" target="_blank">facilitator guide</a> and <a href="https://scistarter.org/go/Foundations-of-Citizen-Science-facilitator-slides-March-2025" target="_blank">slide deck</a> to help you introduce citizen science and SciStarter to others.</p>
</div>"#.to_string(),
        }
    }
}

impl LoadStorePeered for FoundationsContent {
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

impl FoundationsContent {
    /// The current editable content, or the built-in defaults if none has been
    /// saved yet.
    pub async fn current() -> FoundationsContent {
        FoundationsContent::load(()).await.unwrap_or_default()
    }
}

/// Full page at `/training/foundations`.
#[handler]
pub async fn page(Authenticated(user_info): Authenticated) -> Result<Response, Error> {
    let content = FoundationsContent::current().await;
    let rendered = build_content(&content, user_info.clone()).await?;
    render_page(BASE_PATH, content.title.clone(), rendered, user_info).await
}

/// SSE-triggered fragment at `/training/foundations/-progress`.
#[handler]
pub async fn progress(Authenticated(user_info): Authenticated) -> Result<Response, Error> {
    let content = FoundationsContent::current().await;
    let steps = build_steps(&content, &user_info).await;
    let count = training::tutorials_completed(WorkflowType::Foundations, None).await;
    render_progress_fragment(steps, count)
}

async fn build_content(
    content: &FoundationsContent,
    user_info: Option<Arc<BasicInfo>>,
) -> Result<TrainingContent, Error> {
    let live_updates = user_info.is_some();
    let steps = build_steps(content, &user_info).await;
    let tutorials_completed = training::tutorials_completed(WorkflowType::Foundations, None).await;
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
        badge_alt: "Foundations of Citizen Science badge".to_string(),
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
        sse_event: WorkflowType::Foundations.slug().to_string(),
        progress_path: PROGRESS_PATH.to_string(),
    })
}

/// Build the four-step accordion from the user's completion timestamps and the
/// editable content.
///
/// Step gating is linear: the SciStarter Account step is complete once the user
/// is signed in; the tutorial completes when `tutorial` is recorded; the
/// projects step completes once any two of the three eligible projects are
/// recorded; the badge step completes when the badge is granted.
async fn build_steps(
    content: &FoundationsContent,
    user_info: &Option<Arc<BasicInfo>>,
) -> StepsView {
    let logged_in = user_info.is_some();
    let status = match user_info {
        Some(user) => {
            training::fetch_status(WorkflowType::Foundations, &user.user_id.to_string()).await
        }
        None => WorkflowStatusResponse {
            current_step: None,
            completed_steps: vec![],
            badge_granted: false,
        },
    };

    let done = |key: &str| status.completed_steps.iter().any(|step| step == key);
    let squirrel_done = done("squirrel");
    let stall_done = done("stall");
    let debris_done = done("debris");
    let projects_done = [squirrel_done, stall_done, debris_done]
        .iter()
        .filter(|done| **done)
        .count()
        >= 2;

    let states = linear_states(&[
        logged_in,
        done("tutorial"),
        projects_done,
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
        cards: vec![
            ArticulateCard {
                href: tutorial_launch_url(
                    module_url(&content.articulate_en, FOUNDATIONS_EN_URL),
                    None,
                    user_info,
                ),
                image: ARTICULATE_EN_IMAGE.to_string(),
            },
            ArticulateCard {
                href: tutorial_launch_url(
                    module_url(&content.articulate_es, FOUNDATIONS_ES_URL),
                    None,
                    user_info,
                ),
                image: ARTICULATE_ES_IMAGE.to_string(),
            },
        ],
        extra: String::new(),
    }
    .render_once()
    .unwrap_or_default();

    let projects_body = ProjectsBody {
        intro: content.projects_intro.clone(),
        squirrel: ProjectCardView {
            image: content.squirrel_image.clone(),
            alt: "squirrel".to_string(),
            title: content.squirrel_title.clone(),
            description: content.squirrel_description.clone(),
            time_tag: content.squirrel_time_tag.clone(),
            place_tag: content.squirrel_place_tag.clone(),
            link: SQUIRREL_LINK.to_string(),
            completed: squirrel_done,
        },
        stall: ProjectCardView {
            image: content.stall_image.clone(),
            alt: "someone playing Stall Catchers".to_string(),
            title: content.stall_title.clone(),
            description: content.stall_description.clone(),
            time_tag: content.stall_time_tag.clone(),
            place_tag: content.stall_place_tag.clone(),
            link: STALL_LINK.to_string(),
            completed: stall_done,
        },
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
            heading: content.projects_heading.clone(),
            state: states[2],
            open: opens[2],
            body: projects_body,
        },
        StepView {
            heading: content.claim_heading.clone(),
            state: states[3],
            open: opens[3],
            body: claim_body,
        },
    ];

    StepsView { steps }
}

#[derive(TemplateOnce)]
#[template(path = "training/foundations/projects.stpl.html")]
struct ProjectsBody {
    intro: String,
    squirrel: ProjectCardView,
    stall: ProjectCardView,
}

#[cfg(test)]
mod tests {
    use super::FoundationsContent;

    /// Stored content is read back through this struct, so a field added in a
    /// future version must deserialize from older rows by filling that one field
    /// from `Default` — without discarding the rest of the admin's saved edits.
    #[test]
    fn missing_field_fills_from_default_without_discarding_edits() {
        let saved = FoundationsContent {
            title: "Edited Title".to_string(),
            ..FoundationsContent::default()
        };

        // Serialize, then drop a field, simulating JSON written before that
        // field existed.
        let mut value = serde_json::to_value(&saved).unwrap();
        value.as_object_mut().unwrap().remove("sponsor");

        let loaded: FoundationsContent = serde_json::from_value(value).unwrap();

        // The edited field survives; the dropped field fills from Default
        // rather than resetting the whole record.
        assert_eq!(loaded.title, "Edited Title");
        assert_eq!(loaded.sponsor, FoundationsContent::default().sponsor);
    }
}
