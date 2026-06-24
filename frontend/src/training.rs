//! Shared framework for per-user, stateful training pages.
//!
//! Every SciStarter training (Foundations, Libraries & Hubs, Data Literacy,
//! ...) renders the same chrome — a header with badge/title/estimated-time/
//! tutorials-completed count, a skills section, an accordion of steps, and a
//! partner sidebar — and derives each step's display state from the user's
//! list of completion timestamps in the user-service. What differs per
//! training is the workflow it reads, its copy, and its step bodies. This
//! module owns the generic parts; each training module (e.g.
//! [`foundations`]) supplies the content.

use std::sync::Arc;

use anyhow::Error;
use common::message::user::{
    BasicInfo, WorkflowStatusRequest, WorkflowStatusResponse, WorkflowTutorialCountRequest,
    WorkflowTutorialCountResponse, WorkflowType,
};
use common::shared::db;
use poem::{IntoResponse, Response, web::Html};
use sailfish::TemplateOnce;

use crate::component::{Component, Head, StandardLayout};

pub mod champion;
pub mod dataethics;
pub mod dataliteracy;
pub mod foundations;
pub mod highered;
pub mod libraries;

/// Display state of a single accordion step, derived from the user's
/// completion timestamps. Each variant maps to the CSS class the existing
/// accordion stylesheet/JS already understands.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum StepState {
    /// Finished — collapsed, shows a checkmark.
    Completed,
    /// The step the user is working on now — expanded.
    Current,
    /// Locked behind earlier steps — dimmed and not expandable.
    Blocked,
}

impl StepState {
    /// Classes appended to `ss-training-step ss-accordion`. Whether the panel
    /// starts open is a separate concern (see [`open_flags`]) — `blocked
    /// disabled` dims it and makes the shared accordion JS skip its toggle.
    pub fn css_class(self) -> &'static str {
        match self {
            StepState::Completed => "completed",
            StepState::Current => "current",
            StepState::Blocked => "blocked disabled",
        }
    }

    pub fn is_completed(self) -> bool {
        matches!(self, StepState::Completed)
    }
}

/// Map an ordered list of "is this step done?" flags onto display states: a
/// done step is `Completed`, the first not-done step is `Current`, and every
/// step after it is `Blocked`. This is the common linear-progression case;
/// trainings whose gating differs can compute states themselves.
pub fn linear_states(done: &[bool]) -> Vec<StepState> {
    let mut states = Vec::with_capacity(done.len());
    let mut current_assigned = false;
    for &is_done in done {
        if is_done {
            states.push(StepState::Completed);
        } else if !current_assigned {
            states.push(StepState::Current);
            current_assigned = true;
        } else {
            states.push(StepState::Blocked);
        }
    }
    states
}

/// One accordion step. `body` is the training-specific inner HTML, already
/// rendered by the training module (so each training keeps its markup in its
/// own templates while sharing this wrapper). `open` controls whether the panel
/// starts expanded (see [`open_flags`]).
pub struct StepView {
    pub heading: String,
    pub state: StepState,
    pub open: bool,
    pub body: String,
}

/// Which steps should start expanded: the `Current` step (the one in progress),
/// or — when every step is complete and there is no current step — the last
/// step, so a finished training doesn't render entirely collapsed.
pub fn open_flags(states: &[StepState]) -> Vec<bool> {
    let all_completed =
        !states.is_empty() && states.iter().all(|s| matches!(s, StepState::Completed));
    let last = states.len().saturating_sub(1);
    states
        .iter()
        .enumerate()
        .map(|(i, s)| matches!(s, StepState::Current) || (all_completed && i == last))
        .collect()
}

/// Generic body for the "SciStarter Account" step: a greeting once signed in,
/// or a login/sign-up prompt that returns to `next_path` afterward. Reused by
/// every training.
#[derive(TemplateOnce)]
#[template(path = "training/signin.stpl.html")]
pub struct SigninBody {
    pub logged_in: bool,
    pub handle: String,
    /// Local path the login/signup flow returns to after success.
    pub next_path: String,
}

/// Generic, reusable project card for any training's "participate in projects"
/// step. Links to the project's canonical page and shows a completion badge.
#[derive(TemplateOnce)]
#[template(path = "training/project_card.stpl.html")]
pub struct ProjectCardView {
    pub image: String,
    pub alt: String,
    pub title: String,
    pub description: String,
    pub time_tag: String,
    pub place_tag: String,
    pub link: String,
    pub completed: bool,
}

/// Generic body for the final "Claim your badge" step; `html` is the editable
/// congratulations copy.
#[derive(TemplateOnce)]
#[template(path = "training/claim.stpl.html")]
pub struct ClaimBody {
    pub html: String,
}

/// One Articulate tutorial card — a linked image. A tutorial step has one or
/// more (e.g. English / Spanish).
pub struct ArticulateCard {
    pub href: String,
    pub image: String,
}

/// Generic "self-guided tutorial and quiz" step body: intro copy, the Articulate
/// cards, and optional `extra` HTML for follow-up activities (empty for most
/// trainings).
#[derive(TemplateOnce)]
#[template(path = "training/tutorial.stpl.html")]
pub struct TutorialBody {
    pub intro: String,
    pub cards: Vec<ArticulateCard>,
    pub extra: String,
}

/// Generic partner/sponsor sidebar: the badge image followed by an editable
/// HTML block (acknowledgments, brochure, downloads).
#[derive(TemplateOnce)]
#[template(path = "training/sidebar.stpl.html")]
pub struct SidebarBody {
    pub badge_image: String,
    pub html: String,
}

/// The accordion region — the part re-rendered live when the user's progress
/// changes. Shared by the full page and the SSE-triggered fragment so there is
/// a single source of truth for step markup.
#[derive(TemplateOnce)]
#[template(path = "training/steps.stpl.html")]
pub struct StepsView {
    pub steps: Vec<StepView>,
}

/// The full training page's body, rendered inside [`StandardLayout`]. Trainings
/// populate every field; the structural markup is shared.
#[derive(TemplateOnce)]
#[template(path = "training/page_content.stpl.html")]
pub struct TrainingContent {
    pub breadcrumb_label: String,
    pub badge_image: String,
    pub badge_alt: String,
    pub sponsor: String,
    pub title: String,
    pub intro: String,
    pub estimated_time: String,
    pub tutorials_completed: String,
    pub skills_intro: String,
    pub skills: Vec<String>,
    pub skills_footnote: String,
    pub steps: StepsView,
    pub sidebar_html: String,
    /// When true (the user is signed in), the steps region subscribes to the
    /// per-user SSE channel and re-fetches `progress_path` on a matching event.
    pub live_updates: bool,
    /// Per-user SSE channel to connect to (`/-channel/me/{sse_channel}`).
    pub sse_channel: String,
    /// SSE event name that signals this training's progress changed.
    pub sse_event: String,
    /// Endpoint returning the freshly-stated steps fragment.
    pub progress_path: String,
}

#[derive(TemplateOnce)]
#[template(path = "training/page.stpl.html")]
struct TrainingPage {
    head: <Head as Component>::Prepared,
    layout: <StandardLayout<TrainingContent> as Component>::Prepared,
    title: String,
}

/// HTMX response to "refresh my progress": the freshly-stated accordion plus an
/// out-of-band update of the header's tutorials-completed figure.
#[derive(TemplateOnce)]
#[template(path = "training/steps_fragment.stpl.html")]
struct StepsFragment {
    steps: StepsView,
    tutorials_completed: String,
}

/// Persist a training's typed content struct as JSON in the shared
/// `training_content` table. Each training's `LoadStorePeered` impl delegates
/// its `save_to_database` here; `store()` then peers the change to all
/// frontends.
pub async fn save_content_json<T: serde::Serialize>(
    workflow: &str,
    value: &T,
) -> Result<(), Error> {
    let json = serde_json::to_string(value)?;
    sqlx::query!(
        r#"INSERT OR REPLACE INTO "training_content" ("workflow", "content") VALUES (?, ?)"#,
        workflow,
        json,
    )
    .execute(db())
    .await?;
    Ok(())
}

/// Read a training's typed content struct from the shared `training_content`
/// table. A missing row is an error; callers fall back to `Default` (the live
/// copy) so a training renders unchanged until its content is first edited.
pub async fn read_content_json<T: serde::de::DeserializeOwned>(
    workflow: &str,
) -> Result<T, Error> {
    let row = sqlx::query!(
        r#"SELECT "content" FROM "training_content" WHERE "workflow" = ?"#,
        workflow,
    )
    .fetch_one(db())
    .await?;
    Ok(serde_json::from_str(&row.content)?)
}

/// Fetch the user's completion-timestamp status for a workflow, defaulting to
/// an empty (not-started) status if the service is unreachable.
pub async fn fetch_status(workflow: WorkflowType, user_id: &str) -> WorkflowStatusResponse {
    common::service::request(
        "user.workflow.status",
        WorkflowStatusRequest {
            user_id: user_id.to_string(),
            workflow,
        },
    )
    .await
    .unwrap_or(WorkflowStatusResponse {
        current_step: None,
        completed_steps: vec![],
        badge_granted: false,
    })
}

/// Platform-wide count of completed tutorials for a workflow (the "Tutorials
/// Completed to Date" figure), formatted with thousands separators.
pub async fn tutorials_completed(workflow: WorkflowType) -> String {
    let response: WorkflowTutorialCountResponse = common::service::request(
        "user.workflow.tutorial_count",
        WorkflowTutorialCountRequest { workflow },
    )
    .await
    .unwrap_or(WorkflowTutorialCountResponse { count: 0 });
    commafy(response.count)
}

/// Render a complete training page within the standard site layout.
pub async fn render_page(
    path: &str,
    title: String,
    content: TrainingContent,
    user: Option<Arc<BasicInfo>>,
) -> Result<Response, Error> {
    let layout = StandardLayout::new(path.to_string(), content)
        .prepare(user.clone())
        .await?;

    TrainingPage {
        head: Head::default().with_sse().prepare(None).await?,
        layout,
        title,
    }
    .into_response(None)
    .await
}

/// Render the HTMX progress-refresh fragment for a training.
pub fn render_progress_fragment(
    steps: StepsView,
    tutorials_completed: String,
) -> Result<Response, Error> {
    let html = StepsFragment {
        steps,
        tutorials_completed,
    }
    .render_once()?;
    Ok(Html(html).into_response())
}

/// Format an integer with comma thousands separators (e.g. `58221` → `58,221`).
fn commafy(n: i64) -> String {
    let negative = n < 0;
    let digits = n.unsigned_abs().to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3 + 1);
    let bytes = digits.as_bytes();
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 && (bytes.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(*b as char);
    }
    if negative {
        format!("-{out}")
    } else {
        out
    }
}
