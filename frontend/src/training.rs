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

use crate::articulate::{ArticulateParams, LRS_URL, standalone_url, with_query};
use crate::component::{Component, Head, StandardLayout};

pub mod champion;
pub mod dataethics;
pub mod dataliteracy;
pub mod foundations;
pub mod highered;
pub mod k12;
pub mod libraries;

/// Display state of a single accordion step, derived from the user's
/// completion timestamps. Each variant maps to the CSS class the existing
/// accordion stylesheet/JS already understands.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StepState {
    /// Finished — collapsed, shows a checkmark.
    Completed,
    /// The step the user is working on now — expanded.
    Current,
    /// Locked behind earlier steps — dimmed and not expandable.
    Blocked,
    /// Not where the user is, but not locked either: a step they may do now
    /// even though something before it is unfinished. Reads as an ordinary
    /// collapsed step — the point is the absence of `blocked disabled`.
    Available,
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
            StepState::Available => "",
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
    linear_states_ungating(done, &[])
}

/// [`linear_states`], with an escape from the gate.
///
/// `ungated` runs parallel to `done`: a step marked there is `Available`
/// rather than `Blocked` when it would otherwise be locked behind an
/// unfinished step. A short slice leaves the remaining steps gated, so
/// passing `&[]` is the plain linear case.
///
/// Data Ethics is the one training that needs this: its tutorial is open to
/// anyone, signed in or not, and only the badge at the end requires an
/// account. Orrery expresses the same exemption as `data-unblocked` in the
/// page markup, and the two must agree -- a learner offered the tutorial on
/// one site and refused it on the other is a difference nobody chose.
pub fn linear_states_ungating(done: &[bool], ungated: &[bool]) -> Vec<StepState> {
    let mut states = Vec::with_capacity(done.len());
    let mut current_assigned = false;
    for (index, &is_done) in done.iter().enumerate() {
        if is_done {
            states.push(StepState::Completed);
        } else if !current_assigned {
            states.push(StepState::Current);
            current_assigned = true;
        } else if ungated.get(index).copied().unwrap_or(false) {
            states.push(StepState::Available);
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

/// The fixed labels the shared chrome renders around a training's own copy.
///
/// A translated training is a page of its own at a URL of its own, rather than
/// one page run through a message catalogue at render time: the translated
/// Data Ethics pages have lived at their own orrery URLs since July 2026 and
/// those have to keep working. So the wording travels with the page. English
/// by default; [`TrainingChrome::spanish`] and [`TrainingChrome::portuguese`]
/// carry the wording of orrery's translated pages verbatim, so a learner
/// arriving from a bookmark reads what they read before.
#[derive(Debug, Clone)]
pub struct TrainingChrome {
    pub breadcrumb_root: String,
    pub start_now: String,
    pub estimated_time_label: String,
    pub tutorials_completed_label: String,
    pub skills_heading: String,
    pub complete_heading: String,
    /// Account-step copy once signed in. `{handle}` is substituted.
    pub signed_in_html: String,
    /// Account-step copy when signed out. `{next}` is substituted with the
    /// path to return to, so the login link comes back to this page.
    pub signed_out_html: String,
}

impl Default for TrainingChrome {
    fn default() -> Self {
        TrainingChrome {
            breadcrumb_root: "Training".to_string(),
            start_now: "Start Now".to_string(),
            estimated_time_label: "Estimated time to complete".to_string(),
            tutorials_completed_label: "Tutorials Completed to Date".to_string(),
            skills_heading: "Skills and Competencies".to_string(),
            complete_heading: "Complete the Training".to_string(),
            signed_in_html: "You're signed in as <b>{handle}</b>. Your SciStarter account is all set &mdash; continue with the steps below to earn your badge.".to_string(),
            signed_out_html: r#"You'll need a free SciStarter account to track your progress and claim your badge. <a href="/login?next={next}">Log in</a> or <a href="/login?next={next}">create an account</a> to get started, then return to this page."#.to_string(),
        }
    }
}

impl TrainingChrome {
    /// The wording of orrery's `training-dataethics-es` page.
    pub fn spanish() -> Self {
        TrainingChrome {
            breadcrumb_root: "Formación".to_string(),
            start_now: "Comenzar ahora".to_string(),
            estimated_time_label: "Tiempo estimado para completar el curso".to_string(),
            tutorials_completed_label: "Tutoriales completados hasta la fecha".to_string(),
            skills_heading: "Habilidades y competencias".to_string(),
            complete_heading: "Completar la formación".to_string(),
            signed_in_html: "<strong>¡Muy bien!</strong> Has iniciado sesión como <b>{handle}</b> y tu progreso se guardará.".to_string(),
            signed_out_html: r#"<strong><a href="/login-es?next={next}">Crea una cuenta</a> de SciStarter o <a href="/login-es?next={next}">inicia sesión</a>.</strong> Si no completas este paso, no podrás solicitar/obtener tu insignia."#.to_string(),
        }
    }

    /// The wording of orrery's `training-dataethics-pt` page.
    pub fn portuguese() -> Self {
        TrainingChrome {
            breadcrumb_root: "Formação".to_string(),
            start_now: "Começar agora".to_string(),
            estimated_time_label: "Tempo estimado para conclusão".to_string(),
            tutorials_completed_label: "Tutoriais concluídos até o momento".to_string(),
            skills_heading: "Habilidades e competências".to_string(),
            complete_heading: "Concluir a formação".to_string(),
            signed_in_html: "<strong>Ótimo!</strong> Você iniciou sessão como <b>{handle}</b> e seu progresso será salvo.".to_string(),
            signed_out_html: r#"<strong><a href="/login-pt?next={next}">Crie uma conta</a> SciStarter ou <a href="/login-pt?next={next}">faça login</a></strong>. Sem concluir esta etapa, você não poderá solicitar seu selo."#.to_string(),
        }
    }
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
    pub chrome: TrainingChrome,
}

impl SigninBody {
    /// The step's copy, with the account handle and the return path filled in.
    pub fn html(&self) -> String {
        if self.logged_in {
            self.chrome.signed_in_html.replace("{handle}", &self.handle)
        } else {
            self.chrome
                .signed_out_html
                .replace("{next}", &self.next_path)
        }
    }
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

/// The module URL to launch: what the content record holds, or what the
/// training ships with when the record holds nothing usable.
///
/// Several of these records were seeded with `"#"` before anyone knew where
/// the module lived, and a saved record wins over built-in copy -- so a
/// training whose record still carries the placeholder would go on linking
/// nowhere however the code around it is fixed. An empty or placeholder value
/// means "not set", not "link to this".
pub fn module_url<'a>(stored: &'a str, built_in: &'a str) -> &'a str {
    match stored.trim() {
        "" | "#" => built_in,
        url => url,
    }
}

/// Wrap a tutorial module's base URL with this user's LRS launch parameters.
///
/// An Articulate package reports its completion to the LRS the launch query
/// names, as the actor it names, authenticated by the JWT it carries. Linked
/// bare, the module has nowhere to report to: the learner finishes it, the
/// callback never fires, and nothing is recorded. Five of these pages linked
/// bare until September 2026, which is why their first step could never
/// complete on hydra.
///
/// `activity_id` is the module's own xAPI activity IRI, spelled as its
/// `tincan.xml` spells it. Only content that withholds its assessment result
/// without the full launch context needs one -- the SCORM Driver exports. The
/// older Rise exports report without it and pass `None`, but they still need
/// everything else.
///
/// A signed-out visitor gets the module's LMS-free entry point (see
/// [`standalone_url`]): there is no actor to name, and the SCORM Driver
/// wrapper would show them a blank page rather than the course.
pub fn tutorial_launch_url(
    base_url: &str,
    activity_id: Option<&str>,
    user: &Option<Arc<BasicInfo>>,
) -> String {
    let Some(user) = user else {
        return standalone_url(base_url).into_owned();
    };

    let mut params = ArticulateParams::new(LRS_URL, user.clone());

    if let Some(activity_id) = activity_id {
        params = params.with_activity_id(activity_id);
    }

    with_query(base_url, &params.to_string())
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
    pub chrome: TrainingChrome,
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
pub async fn read_content_json<T: serde::de::DeserializeOwned>(workflow: &str) -> Result<T, Error> {
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
    crate::number::thousands(response.count)
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The placeholder several content records shipped with is not a URL,
    /// and must not be launched as one.
    #[test]
    fn a_placeholder_module_url_falls_back_to_the_built_in_one() {
        assert_eq!(
            module_url("#", "https://media/x.html"),
            "https://media/x.html"
        );
        assert_eq!(
            module_url("  ", "https://media/x.html"),
            "https://media/x.html"
        );
        assert_eq!(
            module_url("https://media/edited.html", "https://media/x.html"),
            "https://media/edited.html"
        );
    }

    /// Nobody to launch as, so no launch context -- and a SCORM Driver
    /// wrapper handed no launch context renders a blank page forever. The
    /// signed-out link has to be the package's LMS-free entry point instead.
    #[test]
    fn a_signed_out_visitor_gets_an_entry_point_that_works() {
        assert_eq!(
            tutorial_launch_url(
                "https://media.scistarter.org/dataethics-es/scormdriver/indexAPI.html",
                Some("http://4xI0y6kwB69QeGbWDC8I-KkmgEYmLbuL_rise"),
                &None
            ),
            "https://media.scistarter.org/dataethics-es/scormcontent/index.html"
        );
    }

    /// A Rise export runs with or without an LMS, so a signed-out visitor
    /// gets exactly the URL the training names.
    #[test]
    fn a_signed_out_visitor_gets_a_rise_export_unchanged() {
        assert_eq!(
            tutorial_launch_url(
                "https://media.scistarter.org/foundations/index.html",
                None,
                &None
            ),
            "https://media.scistarter.org/foundations/index.html"
        );
    }

    /// The plain linear case: one step in progress, everything after it shut.
    #[test]
    fn a_gated_progression_blocks_everything_past_the_current_step() {
        assert_eq!(
            linear_states(&[true, false, false]),
            vec![StepState::Completed, StepState::Current, StepState::Blocked],
        );
    }

    /// An ungated step is reachable while an earlier one is unfinished --
    /// Data Ethics' tutorial, which anyone may work through -- without
    /// unlocking the steps around it. `Available` is the whole point: it
    /// renders without `blocked disabled`, which is what the accordion JS
    /// checks before it will expand a step.
    #[test]
    fn an_ungated_step_stays_reachable_while_its_neighbours_are_locked() {
        let states = linear_states_ungating(&[false, false, false], &[false, true, false]);

        assert_eq!(
            states,
            vec![StepState::Current, StepState::Available, StepState::Blocked],
        );
        assert_eq!(states[1].css_class(), "");
        assert!(states[2].css_class().contains("disabled"));
    }

    /// The activity id is what a SCORM Driver export needs before it will
    /// report a result at all, so it must survive into the query -- and it
    /// must survive percent-encoded, because it is a URL inside a URL.
    #[test]
    fn an_activity_id_is_encoded_into_the_query() {
        let params = ArticulateParams::new(LRS_URL, test_user()).with_activity_id("http://x_rise");

        assert!(
            params
                .to_string()
                .contains("activity_id=http%3A%2F%2Fx_rise")
        );
    }

    /// Older Rise exports report without one. Passing `None` must leave the
    /// rest of the launch context in place -- that is the whole difference
    /// between a module that records completions and one that cannot.
    #[test]
    fn a_launch_without_an_activity_id_still_carries_the_rest() {
        let params = ArticulateParams::new(LRS_URL, test_user()).to_string();

        assert!(!params.contains("activity_id"));
        assert!(params.contains("endpoint="));
        assert!(params.contains("auth=Bearer"));
        assert!(params.contains("actor="));
    }

    fn test_user() -> Arc<BasicInfo> {
        Arc::new(BasicInfo {
            user_id: ulid::Ulid::from_string("01ARZ3NDEKTSV4RRFFQ69G5FAV").expect("a ulid"),
            handle: "ada".to_string(),
            is_staff: false,
            is_superuser: false,
            public: true,
            protected: false,
            needs_point: false,
        })
    }
}
