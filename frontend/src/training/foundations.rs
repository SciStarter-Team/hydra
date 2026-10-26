//! Foundations of Citizen Science — the first training built on the shared
//! [`crate::training`] framework. Ported from orrery's `/training-foundations`
//! (served here at `/training/foundations`). State is derived from the user's
//! `foundations_progress` completion timestamps in the user-service.
//!
//! The training is the tutorial alone: account, tutorial, badge. A visitor
//! may work through the tutorial without an account, and its last page links
//! back here through go links that hold the completion in their session
//! until they sign up or sign in.
//!
//! Editable copy lives in [`FoundationsContent`] (admin-managed, peered);
//! structure and gating live in code here.

use std::sync::Arc;

use anyhow::Error;
use common::badge::Badge;
use common::message::user::{BasicInfo, WorkflowStatusResponse, WorkflowType};
use poem::{Response, handler, session::Session};
use sailfish::TemplateOnce;

use crate::middleware::authorization::Authenticated;
use crate::training::content::TrainingCopy;
use crate::training::{
    self, ArticulateCard, SidebarBody, SigninBody, StepState, StepView, StepsView, TrainingChrome,
    TrainingContent, TutorialBody, linear_states_ungating, module_url, open_flags, render_page,
    render_progress_fragment, tutorial_launch_url, tutorial_pending,
};

pub const BASE_PATH: &str = "/training/foundations";
const PROGRESS_PATH: &str = "/training/foundations/-progress";

/// Where the page's own log-in links return to: the steps, which is where a
/// visitor coming back from signing in has something to see.
const RETURN_PATH: &str = "/training/foundations%23start";

/// The key [`FoundationsContent`] is stored under. It is the copy of the
/// tutorial-only page; copy saved for the four-step page that preceded it,
/// if any was, sits under `foundations` and is not read.
const CONTENT_KEY: &str = "foundations-tutorial";

// The tutorial's two builds. The English one is the October 2026 SCORM
// Driver rebuild, which reports its quiz result only when launched with its
// activity id. The Spanish one is an older Rise export that reports without
// one, so it is launched without one -- but with everything else, or it
// reports nothing at all.
const FOUNDATIONS_EN_URL: &str =
    "https://media.scistarter.org/foundations-v2/scormdriver/indexAPI.html";
const FOUNDATIONS_EN_ACTIVITY_ID: &str = "http://FoundationsStandardized";
const FOUNDATIONS_ES_URL: &str = "https://media.scistarter.org/fundamentos/index.html";

// The EN/ES tutorial card images are fixed design assets (the editable part is
// the Articulate URL each links to).
const ARTICULATE_EN_IMAGE: &str = "https://static.scistarter.org/img/training/foundations-en.png";
const ARTICULATE_ES_IMAGE: &str = "https://static.scistarter.org/img/training/foundations-es.png";

/// Editable text content for the Foundations training. Structure and gating
/// live in code (below); this holds only the copy the Engine Room admin can
/// edit, as [`crate::training::content`] describes.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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
    /// The account step's copy for a signed-out visitor who has not done the
    /// tutorial. `{next}` is substituted with the path to return to.
    pub account_signed_out_html: String,
    pub tutorial_heading: String,
    pub tutorial_intro: String,
    /// Shown above the tutorial's intro to a signed-out visitor who has
    /// finished it.
    pub tutorial_done_html: String,
    pub articulate_en: String,
    pub articulate_es: String,
    pub claim_heading: String,
    /// The header's button for a signed-out visitor who has finished the
    /// tutorial, in place of "Start Now". It leads to the badge step.
    pub claim_button: String,
    /// The badge step's congratulations, above the earner's own badge links.
    pub claim_html: String,
    /// The badge step's copy below the earner's badge links.
    pub next_steps_html: String,
    /// The badge step's copy for a signed-out visitor who has finished the
    /// tutorial: the badge waits on an account, and this is their last
    /// chance to make one.
    pub last_chance_html: String,
    pub sidebar_html: String,
}

impl TrainingCopy for FoundationsContent {
    type Record = ();

    fn row(_: ()) -> &'static str {
        CONTENT_KEY
    }

    fn built_in(_: ()) -> FoundationsContent {
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
            skills_intro: "By the end of this tutorial, you'll demonstrate all of the following \
                foundational citizen science skills/competencies within the SciStarter platform \
                and obtain a score of 80% or better on the quiz embedded in the tutorial:"
                .to_string(),
            skills: vec![
                "<strong>Recognize</strong> what citizen science is and how it can be used to \
                    address real-world questions in collaboration with others."
                    .to_string(),
                "<strong>Articulate</strong> how participating in citizen science benefits science \
                    and society."
                    .to_string(),
                "<strong>Recognize</strong> who can participate in different citizen science \
                    projects."
                    .to_string(),
                "<strong>Recall</strong> the types of citizen science projects you can participate \
                    in, the skills you can practice or acquire, and the ways you can advance \
                    real-world research."
                    .to_string(),
                "<strong>Recall</strong> what SciStarter is and how it can connect you to citizen \
                    science projects and help you track your participation."
                    .to_string(),
                "<strong>Complete</strong> the steps to register for an account on SciStarter, \
                    participate in contributory citizen science projects, and find and bookmark \
                    projects on SciStarter."
                    .to_string(),
            ],
            skills_footnote: r#"You'll also practice or acquire the following skills which are part of an "open skills taxonomy" used by employers and schools: <a href="https://lightcast.io/open-skills/skills/KS120GV6C72JMSZKMTD7/data-analysis" target="_blank">data analysis</a>, <a href="https://lightcast.io/open-skills/skills/KS122NY60HCYYDXHQGQN/data-entry" target="_blank">data entry</a>, <a href="https://lightcast.io/open-skills/skills/ESE2D5EA264FB06F5C4F/information-gathering" target="_blank">information gathering</a>, and <a href="https://lightcast.io/open-skills/skills/KSG21Y30BQVH1S8XMJHK/questioning-skills" target="_blank">questioning skills</a>."#.to_string(),
            account_heading: "SciStarter Account".to_string(),
            account_signed_out_html: r#"<b><a href="/login?next={next}">Create</a> or <a href="/login?next={next}">log in</a> to your SciStarter account</b> before you start, and your badge will be added to your profile as soon as you finish the tutorial. You can also create an account or log in after you finish."#.to_string(),
            tutorial_heading: "Self-guided tutorial and quiz".to_string(),
            tutorial_intro: r#"<b>Complete this self-guided tutorial and quiz.</b> Score 80% or higher on the quiz to earn your badge. The tutorial opens in a new tab, and its last page brings you back here. If you're logged in to SciStarter when you start, your badge is added to your profile as soon as you finish; if not, you can log in or create an account when you return to claim it.
<br><small>This tutorial was created using Articulate; for Safari users, Articulate requires Safari 17.4 or a newer version.</small>"#.to_string(),
            tutorial_done_html: "<b>You've completed the tutorial.</b> Log in or create an account to receive your badge.".to_string(),
            articulate_en: FOUNDATIONS_EN_URL.to_string(),
            articulate_es: FOUNDATIONS_ES_URL.to_string(),
            claim_heading: "Congratulations!".to_string(),
            claim_button: "Claim your badge".to_string(),
            claim_html: "<p>🎉 <b>You've completed the Foundations of Citizen Science training!</b> 🎉</p>".to_string(),
            next_steps_html: r#"<p><b>What's next?</b> Put what you've learned into action by finding a project that matches your interests, location, or available time.</p>
<p><a href="/finder" class="btn btn-primary">Find a project</a></p>
<p>Or keep learning! Explore additional SciStarter trainings and opportunities to earn more badges and certificates.</p>
<p><a href="/training" class="btn btn-primary">Explore more trainings</a></p>"#.to_string(),
            last_chance_html: r#"<p><b>You completed the Foundations of Citizen Science training.</b> One last step to receive your digital badge.</p>
<p>Log in to SciStarter or create a free account now so we can connect you with your completed training and award your <b>Foundations of Citizen Science digital badge</b>.</p>
<p><b>This is your final opportunity to claim your badge.</b> Once you leave this page, we will no longer be able to associate you with your completed Foundations training.</p>
<p>Your badge will appear on your SciStarter profile, where you can share it on LinkedIn and other platforms.</p>
<div class="flex flex-col md:flex-row gap-4 mb-4">
  <a href="/login?next=/training/foundations%23start" class="btn btn-accent text-center" style="white-space: normal">Log in or create an account &amp; get my badge</a>
  <a href="/training" class="btn btn-secondary text-center" style="white-space: normal">Find more trainings</a>
</div>
<p class="text-sm">After you log in or sign up, you'll be brought back to this page, and your badge will appear on your SciStarter profile.</p>"#.to_string(),
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

/// Full page at `/training/foundations`.
#[handler]
pub async fn page(
    session: &Session,
    Authenticated(user_info): Authenticated,
) -> Result<Response, Error> {
    let content = FoundationsContent::current(()).await;
    // The tutorial's last page links back here through go links, and a
    // visitor who finished it signed out arrives with the step held.
    let pending = tutorial_pending(session, WorkflowType::Foundations, &user_info);
    let rendered = build_content(&content, user_info.clone(), pending).await?;
    render_page(BASE_PATH, content.title.clone(), rendered, user_info).await
}

/// SSE-triggered fragment at `/training/foundations/-progress`.
#[handler]
pub async fn progress(Authenticated(user_info): Authenticated) -> Result<Response, Error> {
    let content = FoundationsContent::current(()).await;
    // Only a signed-in page subscribes to progress, and nothing is held for
    // a signed-in visitor.
    let steps = build_steps(&content, &user_info, false).await;
    let count = training::tutorials_completed(WorkflowType::Foundations).await;
    render_progress_fragment(steps, count)
}

async fn build_content(
    content: &FoundationsContent,
    user_info: Option<Arc<BasicInfo>>,
    tutorial_pending: bool,
) -> Result<TrainingContent, Error> {
    let live_updates = user_info.is_some();
    let steps = build_steps(content, &user_info, tutorial_pending).await;
    let tutorials_completed = training::tutorials_completed(WorkflowType::Foundations).await;
    let sidebar_html = SidebarBody {
        badge_image: content.badge_image.clone(),
        html: content.sidebar_html.clone(),
    }
    .render_once()
    .unwrap_or_default();

    // A visitor back from the tutorial with no account is sent down to the
    // badge step, which tells them how to claim the badge, rather than
    // invited to start.
    let mut chrome = TrainingChrome::default();
    if tutorial_pending {
        chrome.start_now = content.claim_button.clone();
    }

    Ok(TrainingContent {
        chrome,
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

/// The display state of the account, tutorial and badge steps.
///
/// The tutorial counts as done once the badge is granted, since finishing it
/// is what grants the badge: a learner short of the badge is still on the
/// tutorial, whatever their recorded steps say. The tutorial is open to
/// everyone -- a visitor with no account can work through it, they just
/// cannot be credited for it, which orrery's page says with
/// `data-unblocked`.
///
/// A signed-out visitor who has finished it is past the order the steps are
/// laid out in: the tutorial is done, the account is still to do, and the
/// badge step is where they are told how to claim the badge. Orrery's
/// status reports the same position as step 3 with step 1 outstanding.
fn step_states(logged_in: bool, badge_granted: bool, tutorial_pending: bool) -> Vec<StepState> {
    if tutorial_pending {
        return vec![
            StepState::Available,
            StepState::Completed,
            StepState::Current,
        ];
    }

    linear_states_ungating(
        &[logged_in, badge_granted, badge_granted],
        &[false, true, false],
    )
}

/// Which steps start open: the current one, as for every training, and the
/// tutorial whenever it can be worked on -- orrery's `data-open` -- so a
/// signed-out visitor sees it beside the invitation to make an account.
fn step_opens(states: &[StepState]) -> Vec<bool> {
    let mut opens = open_flags(states);

    if states[1] == StepState::Available {
        opens[1] = true;
    }

    opens
}

/// Build the three-step accordion from the user's completion timestamps and
/// the editable content.
async fn build_steps(
    content: &FoundationsContent,
    user_info: &Option<Arc<BasicInfo>>,
    tutorial_pending: bool,
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

    let states = step_states(logged_in, status.badge_granted, tutorial_pending);
    let opens = step_opens(&states);

    let account_body = SigninBody {
        logged_in,
        tutorial_pending,
        handle: user_info
            .as_ref()
            .map(|user| user.handle.clone())
            .unwrap_or_default(),
        next_path: RETURN_PATH.to_string(),
        chrome: TrainingChrome {
            signed_out_html: content.account_signed_out_html.clone(),
            ..TrainingChrome::default()
        },
    }
    .render_once()
    .unwrap_or_default();

    let tutorial_body = TutorialBody {
        notice: if tutorial_pending {
            content.tutorial_done_html.clone()
        } else {
            String::new()
        },
        intro: content.tutorial_intro.clone(),
        cards: vec![
            ArticulateCard {
                href: tutorial_launch_url(
                    module_url(&content.articulate_en, FOUNDATIONS_EN_URL),
                    Some(FOUNDATIONS_EN_ACTIVITY_ID),
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

    let badge_body = BadgeBody {
        tutorial_pending,
        claim_html: content.claim_html.clone(),
        earned_html: crate::badge::earned_html(user_info, Badge::FoundationsOfCitizenScience).await,
        next_steps_html: content.next_steps_html.clone(),
        last_chance_html: content.last_chance_html.clone(),
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
            body: badge_body,
        },
    ];

    StepsView { steps }
}

/// The badge step: congratulations and the earner's own badge links, or --
/// for a signed-out visitor who has finished the tutorial -- the last chance
/// to make the account the badge waits on.
#[derive(TemplateOnce)]
#[template(path = "training/foundations/badge.stpl.html")]
struct BadgeBody {
    tutorial_pending: bool,
    claim_html: String,
    earned_html: String,
    next_steps_html: String,
    last_chance_html: String,
}

#[cfg(test)]
mod tests {
    use super::{BadgeBody, FoundationsContent, step_opens, step_states};
    use crate::training::StepState::{Available, Blocked, Completed, Current};
    use crate::training::content::TrainingCopy;
    use sailfish::TemplateOnce;

    /// A signed-out visitor is invited to make an account, and offered the
    /// tutorial beside it; the badge waits.
    #[test]
    fn a_signed_out_visitor_is_offered_the_account_and_the_tutorial() {
        let states = step_states(false, false, false);

        assert_eq!(states, vec![Current, Available, Blocked]);
        assert_eq!(step_opens(&states), vec![true, true, false]);
    }

    /// Short of the badge, an account is on the tutorial, whatever steps of
    /// the retired four-step training it has recorded.
    #[test]
    fn an_account_short_of_the_badge_is_on_the_tutorial() {
        let states = step_states(true, false, false);

        assert_eq!(states, vec![Completed, Current, Blocked]);
        assert_eq!(step_opens(&states), vec![false, true, false]);
    }

    /// The badge holder's view of the badge step is the one left open.
    #[test]
    fn the_badge_holder_sees_the_badge_step() {
        let states = step_states(true, true, false);

        assert_eq!(states, vec![Completed, Completed, Completed]);
        assert_eq!(step_opens(&states), vec![false, false, true]);
    }

    /// Back from the tutorial with no account: the tutorial is done, the
    /// account is still to do but not pressed open, and the badge step --
    /// where the last chance to claim the badge is -- is open.
    #[test]
    fn a_visitor_back_from_the_tutorial_lands_on_the_badge_step() {
        let states = step_states(false, false, true);

        assert_eq!(states, vec![Available, Completed, Current]);
        assert_eq!(step_opens(&states), vec![false, false, true]);
    }

    /// The badge step tells a visitor holding a finished tutorial how to
    /// claim the badge, and tells anyone else -- the earner -- that they
    /// have it.
    #[test]
    fn the_badge_step_says_last_chance_or_congratulations() {
        let content = FoundationsContent::built_in(());
        let body = |tutorial_pending| {
            BadgeBody {
                tutorial_pending,
                claim_html: content.claim_html.clone(),
                earned_html: "EARNED".to_string(),
                next_steps_html: content.next_steps_html.clone(),
                last_chance_html: content.last_chance_html.clone(),
            }
            .render_once()
            .unwrap()
        };

        let pending = body(true);
        assert!(pending.contains("final opportunity"));
        assert!(pending.contains("next=/training/foundations%23start"));
        assert!(!pending.contains("EARNED"));

        let earned = body(false);
        assert!(earned.contains("You've completed the Foundations"));
        assert!(earned.contains("EARNED"));
        assert!(!earned.contains("final opportunity"));
    }
}
