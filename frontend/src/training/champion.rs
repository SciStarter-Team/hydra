//! Citizen Science Champion training (internally the `Ambassador` workflow).
//! Five steps: account → Foundations prerequisite → tutorial → Champion claim →
//! apply (informational; Ambassador applications open periodically). Ported from
//! orrery's `/training-champion`, now at `/training/champion`, with legacy
//! redirects from `/training-champion` and the outdated `/training-ambassador`.
//! State comes from `ambassador_progress` (plus the Foundations badge for the
//! prerequisite).

use std::sync::Arc;

use anyhow::Error;
use common::message::dashboard::AmbassadorApplication;
use common::message::user::{BasicInfo, WorkflowStatusResponse, WorkflowType};
use common::service::request;
use poem::{Response, handler, web::Redirect};
use sailfish::TemplateOnce;

use crate::middleware::authorization::Authenticated;
use crate::peering::LoadStorePeered;
use crate::training::{
    self, ArticulateCard, SidebarBody, SigninBody, StepState, StepView, StepsView, TrainingChrome,
    TrainingContent, TutorialBody, linear_states, module_url, open_flags, render_page,
    render_progress_fragment, tutorial_launch_url,
};

const BASE_PATH: &str = "/training/champion";
const PROGRESS_PATH: &str = "/training/champion/-progress";
const SLUG: &str = "champion";

const CHAMPION_URL: &str =
    "https://media.scistarter.org/champion-training/scormdriver/indexAPI.html";

/// The Storyline package's own xAPI activity id, as its `tincan.xml` spells
/// it. A SCORM Driver export reports no result without it.
const CHAMPION_ACTIVITY_ID: &str = "http://cD2GSPUMolo6VyXCZLI9dnAvy4skHGNE_rise";

/// Editable copy for the Champion training. `#[serde(default)]` required (see
/// [`crate::training::foundations::FoundationsContent`]).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ChampionContent {
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
    pub apply_heading: String,
    pub apply_html: String,
    pub received_heading: String,
    pub received_html: String,
    pub sidebar_html: String,
}

impl Default for ChampionContent {
    fn default() -> Self {
        ChampionContent {
            title: "Citizen Science Champion Training".to_string(),
            sponsor: "Presented by SciStarter".to_string(),
            intro: r#"<p>After completing the <a href="/training/foundations">Foundations of Citizen Science Tutorial</a>, you're eligible to become a Citizen Science Champion. By completing the Citizen Science Champion training, you'll be prepared to help others discover opportunities to participate in real scientific research. Becoming a Champion also makes you eligible to apply to become a SciStarter Ambassador.</p>
<p><strong>Citizen Science Champion Benefits</strong></p>
<ul class="list-disc px-8">
  <li>Earn a certificate. <strong>Tip:</strong> Add your full name to your SciStarter profile if you'd like your certificate to be personalized.</li>
  <li>Build skills in community engagement, facilitation, and science communication</li>
  <li>Connect with others interested in citizen science</li>
  <li>Beta test new citizen science projects and resources</li>
  <li>Receive notifications of new local projects and events near you</li>
  <li>Access professionally designed, field-tested presentation materials with speakers' notes, outreach materials, planning guides, and other materials to help you introduce citizen science to others</li>
</ul>
<details class="mb-8">
  <summary class="cursor-pointer font-bold">Become a Certified SciStarter Ambassador</summary>
  <p>Champions (18+ and based in the US) are eligible to apply to become a SciStarter Ambassador! Ambassadors receive all the benefits of Champions. However, Ambassadors are included in a public, searchable directory so they may be contacted and invited to host or support events that introduce citizen science and SciStarter to new audiences. Upon successful completion of an application and third-party background screening, among other requirements, Ambassadors receive a digital badge with personalized metadata, access to free event materials, and are required to host or facilitate events at least two times a year.</p>
  <p>Ambassador applications are open periodically throughout the year. Everyone who completes the Citizen Science Champion training will be notified when applications reopen.</p>
  <p><a href="/ambassadors-apply" class="btn btn-accent">Find out more about the SciStarter Ambassador Program here.</a></p>
</details>"#.to_string(),
            estimated_time: "1 hour".to_string(),
            badge_image: "https://media.scistarter.org/curated/2026/Citizen_Science_Champion_certificate.png".to_string(),
            skills_intro: "This training will help you become confident introducing citizen science to others. In this training you will:".to_string(),
            skills: vec![
                "<strong>Learn</strong> how to start conversations, answer common questions, and help people discover opportunities that fit their interests and availability.".to_string(),
                "<strong>Become</strong> familiar with SciStarter's resources so you can guide people to projects, events, and activities that are meaningful to them.".to_string(),
                "<strong>Develop</strong> practical facilitation skills that can help you lead conversations, introduce citizen science to groups you're already part of, and create welcoming experiences for people who may be discovering citizen science for the first time.".to_string(),
            ],
            skills_footnote: r#"You will practice or acquire the following skills which are part of an "open skills taxonomy" used by employers and schools: <a href="https://lightcast.io/open-skills/skills/BGS6F9F4F7DBABC95CE3/community-outreach" target="_blank">Community Outreach</a>, <a href="https://lightcast.io/open-skills/skills/KS122CZ71K19596XHQ92/event-planning" target="_blank">Event Planning</a>, <a href="https://lightcast.io/open-skills/skills/KS440BS61162WZN04SVD/science-communication" target="_blank">Science Communication</a>, <a href="https://lightcast.io/open-skills/skills/KS124JB619VXG6RQ810C/leadership" target="_blank">Leadership</a>, <a href="https://lightcast.io/open-skills/skills/ESA420F05EBBD34B3C2A/curiosity" target="_blank">Curiosity</a>, and <a href="https://lightcast.io/open-skills/skills/KS4401D6PYRMFQC4HR62/resource-management" target="_blank">Resource Management</a>."#.to_string(),
            account_heading: "SciStarter Account".to_string(),
            prereq_heading: "Foundations of Citizen Science".to_string(),
            prereq_html: r#"<p>Complete the <a href="/training/foundations">Foundations of Citizen Science</a> training module and earn the badge to access the SciStarter Ambassador Tutorial.</p>"#.to_string(),
            tutorial_heading: "Ambassador training and quiz".to_string(),
            tutorial_intro: "Complete the Citizen Science Champion training module.".to_string(),
            // Base URL of the Champion Storyline package. It is wrapped per-user
            // with the LRS launch parameters at render time (see `build_steps`).
            articulate_url: CHAMPION_URL.to_string(),
            articulate_image: "https://orrery-media.s3.amazonaws.com/attr/2026-07/Citizen_Science_Champion_nP0OiMQ.png".to_string(),
            claim_heading: "Congratulations, Champion!".to_string(),
            claim_html: r#"<p>🎉 <strong>Congratulations, you are a Citizen Science Champion!</strong> 🎉</p>
<p>You've taken an exciting step in advancing citizen science in your community.<br>
Your <strong>Champion Certificate</strong> is now available on your <a href="/dashboard/profile">SciStarter profile</a>.</p>
<p>You've also unlocked access to the <strong>downloadable Champion Handbook</strong>:<br>
👉 <a href="https://scistarter.org/go/champion-handbook" target="_blank">Download the Guide</a></p>
<p>This handbook is packed with tips, talking points, free templates, suggested activities, and other resources to help you introduce citizen science and SciStarter to others with confidence!</p>
<p><strong>What to Do Next:</strong></p>
<ul>
<li class="mb-2">✅ <strong>Join the Facebook Group</strong> to connect with fellow Champions and get inspired: <a href="https://www.facebook.com/share/g/18rEgXA9yA/" target="_blank">Champions group</a></li>
<li class="mb-2">✅ <strong>Reach Out to Local Partners</strong> (libraries, schools, nature centers!) to explore collaboration. <a href="https://scistarter.org/library-locations">Explore the locations in our Citizen and Community Science Library Network</a>.</li>
<li class="mb-2">✅ <strong>Add Your Events</strong> to the <a href="/add-event" target="_blank">SciStarter Event Calendar</a> to promote and track your impact.</li>
</ul>"#.to_string(),
            apply_heading: "Apply to be a Certified Ambassador".to_string(),
            apply_html: r#"<p><strong>You will be notified when applications open.</strong></p>
<details class="mb-8">
  <summary class="cursor-pointer font-bold">What Happens After You Apply?</summary>
  <p>After you submit your application:</p>
  <ul class="list-disc px-8">
    <li class="mb-2">The SciStarter team reviews your application</li>
    <li class="mb-2">A third-party service completes a background screening</li>
    <li class="mb-2">The process may take a few weeks</li>
  </ul>
  <p>You may be contacted if additional information is needed. Once approved, you'll receive an email with your Certified SciStarter Ambassador badge, and it will appear on your SciStarter profile and in the public Ambassador directory.</p>
</details>"#.to_string(),
            received_heading: "Application Received".to_string(),
            received_html: r#"<p>Your application is under review and a third-party background screening is in progress. This process may take a few weeks. We'll email you if we need anything else—and again when your Certified Ambassador badge is ready.</p>"#.to_string(),
            sidebar_html: String::new(),
        }
    }
}

impl LoadStorePeered for ChampionContent {
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

impl ChampionContent {
    pub async fn current() -> ChampionContent {
        ChampionContent::load(()).await.unwrap_or_default()
    }
}

#[handler]
pub async fn page(Authenticated(user_info): Authenticated) -> Result<Response, Error> {
    let content = ChampionContent::current().await;
    let rendered = build_content(&content, user_info.clone()).await?;
    render_page(BASE_PATH, content.title.clone(), rendered, user_info).await
}

#[handler]
pub async fn progress(Authenticated(user_info): Authenticated) -> Result<Response, Error> {
    let content = ChampionContent::current().await;
    let steps = build_steps(&content, &user_info).await;
    let count = training::tutorials_completed(WorkflowType::Ambassador).await;
    render_progress_fragment(steps, count)
}

/// 301 redirect to the canonical page. Mounted for both the legacy
/// `/training-champion` and the outdated `/training-ambassador`.
#[handler]
pub async fn legacy_redirect() -> Redirect {
    Redirect::moved_permanent(BASE_PATH)
}

async fn build_content(
    content: &ChampionContent,
    user_info: Option<Arc<BasicInfo>>,
) -> Result<TrainingContent, Error> {
    let live_updates = user_info.is_some();
    let steps = build_steps(content, &user_info).await;
    let tutorials_completed = training::tutorials_completed(WorkflowType::Ambassador).await;
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
        badge_alt: "Citizen Science Champion certificate".to_string(),
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
        sse_event: WorkflowType::Ambassador.slug().to_string(),
        progress_path: PROGRESS_PATH.to_string(),
    })
}

/// Five steps. Account → Foundations prerequisite → tutorial → Champion claim
/// are linear and tracked; the final "apply" step is informational (Ambassador
/// applications open periodically, so it never completes) — it surfaces as the
/// open terminal step once the Champion badge is earned.
async fn build_steps(content: &ChampionContent, user_info: &Option<Arc<BasicInfo>>) -> StepsView {
    let logged_in = user_info.is_some();
    let (status, foundations_done, applied) = match user_info {
        Some(user) => {
            let id = user.user_id.to_string();
            let status = training::fetch_status(WorkflowType::Ambassador, &id).await;
            let foundations = training::fetch_status(WorkflowType::Foundations, &id).await;
            // Has the user submitted (not just drafted) an ambassador application?
            let applied = request::<Option<AmbassadorApplication>>("user.ambassador.get", id)
                .await
                .ok()
                .flatten()
                .map(|a| !a.draft && a.submitted.is_some())
                .unwrap_or(false);
            (status, foundations.badge_granted, applied)
        }
        None => (
            WorkflowStatusResponse {
                current_step: None,
                completed_steps: vec![],
                badge_granted: false,
            },
            false,
            false,
        ),
    };

    let tutorial_done = status.completed_steps.iter().any(|step| step == "tutorial");
    // Becoming a Champion is earned by completing the tutorial (orrery grants the
    // Champion certificate on tutorial_enter); it is NOT the Certified Ambassador
    // badge (`status.badge_granted`), which comes later via the application. So
    // the "Congratulations, Champion!" celebration opens once the tutorial is
    // done (carries `false` — it is a celebration, not a completable task). The
    // "apply" step completes once an application is submitted; a follow-on
    // "Application Received" step is appended while the application is pending.
    let states = linear_states(&[logged_in, foundations_done, tutorial_done, false, applied]);
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
        module_url(&content.articulate_url, CHAMPION_URL),
        Some(CHAMPION_ACTIVITY_ID),
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

    let mut steps = vec![
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
        StepView {
            heading: content.apply_heading.clone(),
            state: states[4],
            open: opens[4],
            body: content.apply_html.clone(),
        },
    ];

    // Once an application is submitted, surface the pending "Application
    // Received" step as the open terminal step (hidden until then, mirroring
    // orrery's champion page).
    if applied {
        steps.push(StepView {
            heading: content.received_heading.clone(),
            state: StepState::Current,
            open: true,
            body: content.received_html.clone(),
        });
    }

    StepsView { steps }
}
