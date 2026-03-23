//! EvolveMe Integration
//!
//! Integration with ASA's (American Student Assistance) learning platform.
//! Implements multi-step workflows with HMAC-signed API callbacks.
//!
//! ## Server-Sent Events (SSE)
//!
//! EvolveMe pages use the existing channel.rs SSE mechanism for real-time updates.
//!
//! To enable SSE on EvolveMe pages:
//! 1. Include SSE support in the page head: `Head::default().with_sse()`
//! 2. Connect to the channel using HTMX SSE extension in templates:
//!    ```html
//!    <div hx-ext="sse" sse-connect="/channel/evolveme" sse-swap="event-name">
//!      Content to update
//!    </div>
//!    ```
//!
//! System components can send events to EvolveMe pages:
//! ```rust
//! channel::send(&session_subject, "evolveme",
//!     channel::Event::new("step_complete", data)).await?;
//! ```

use anyhow::Error;
use common::{
    message::{
        log::{
            ArticulateCompletionRequest, ArticulateCompletionResponse, ProjectParticipationRequest,
            ProjectParticipationResponse,
        },
        user::{BookmarkCreateRequest, BookmarkCreateResponse},
    },
    service::request,
    shared::db,
};
use poem::{
    IntoResponse, Route, handler,
    web::{Html, Query, Redirect},
};
use sailfish::TemplateOnce;
use serde::{Deserialize, Serialize};

#[cfg(not(debug_assertions))]
use hmac::{Hmac, Mac};
#[cfg(not(debug_assertions))]
use sha2::Sha256;
use std::sync::LazyLock;
use ulid::Ulid;
use uuid::Uuid;

use crate::middleware::authorization::Authenticated;
use common::message::user::BasicInfo;
use poem::session::Session;
use std::sync::Arc;

// Project IDs for specific workflows
static PROJECT_SQUIRREL: LazyLock<Ulid> =
    LazyLock::new(|| Ulid::from_string("01JKXMGGB6RNNZXYZN2BCZP532").unwrap());
static GREAT_SUNFLOWER: LazyLock<Ulid> =
    LazyLock::new(|| Ulid::from_string("01JKXMGBR27B4213KMNSSPH70Y").unwrap());
static STALLCATCHERS: LazyLock<Ulid> =
    LazyLock::new(|| Ulid::from_string("01JKXMGGJ39R326PJVD94J2X4N").unwrap());
static INSTANT_WILD: LazyLock<Ulid> =
    LazyLock::new(|| Ulid::from_string("01JKXMG2RATVH68HPEMMAA4X0M").unwrap());

// ASA API host configuration
#[cfg(not(debug_assertions))]
const EVOLVEME_HOST: &'static str = "evolveme-api.asa.org";
// const EVOLVEME_HOST: &'static str = "evolveme-api-stage.asa.org";  // For staging

// ASA API secret from environment
#[cfg(not(debug_assertions))]
const EVOLVEME_SECRET: &'static str = env!("EVOLVEME_SECRET");

pub fn routes() -> Route {
    Route::new()
        // Note: SSE events are handled by /channel/evolveme (see module docs)
        .at("/search-and-save/signup", poem::get(search_and_save_signup))
        .at("/search-and-save/login", poem::get(search_and_save_login))
        .at("/search-and-save/search", poem::get(search_and_save_search))
        .at(
            "/search-and-save/project",
            poem::get(search_and_save_project),
        )
        .at("/search-and-save/done", poem::post(search_and_save_done))
        .at("/lesson-quiz/signup", poem::get(lesson_quiz_signup))
        .at("/lesson-quiz/login", poem::get(lesson_quiz_login))
        .at("/lesson-quiz/lesson", poem::get(lesson_quiz_lesson))
        .at("/lesson-quiz/done", poem::get(lesson_quiz_done))
        .at(
            "/project-squirrel/signup",
            poem::get(project_squirrel_signup),
        )
        .at("/project-squirrel/login", poem::get(project_squirrel_login))
        .at(
            "/project-squirrel/project",
            poem::get(project_squirrel_project),
        )
        .at("/project-squirrel/done", poem::get(project_squirrel_done))
        .at("/great-sunflower/signup", poem::get(great_sunflower_signup))
        .at("/great-sunflower/login", poem::get(great_sunflower_login))
        .at(
            "/great-sunflower/project",
            poem::get(great_sunflower_project),
        )
        .at("/great-sunflower/done", poem::get(great_sunflower_done))
        .at("/stallcatchers/signup", poem::get(stallcatchers_signup))
        .at("/stallcatchers/login", poem::get(stallcatchers_login))
        .at("/stallcatchers/project", poem::get(stallcatchers_project))
        .at("/stallcatchers/done", poem::get(stallcatchers_done))
        .at("/instant-wild/signup", poem::get(instant_wild_signup))
        .at("/instant-wild/login", poem::get(instant_wild_login))
        .at("/instant-wild/project", poem::get(instant_wild_project))
        .at("/instant-wild/done", poem::get(instant_wild_done))
}

/// Parameters for EvolveMe workflow state
#[derive(Serialize, Deserialize, Debug, Default, Clone)]
#[serde(default)]
struct EvolveMeParams {
    user_id: i32,
    unique_task_key: Uuid,
    partner_task_reference: Option<i32>,
    step_number: i32, // 1-based
    partner_task_step_reference: Option<i32>,
}

impl EvolveMeParams {
    /// Complete a step in the ASA workflow
    async fn complete_step(&self, step: i32) -> Result<(), Error> {
        // In debug mode, use dummy implementation instead of actual API calls
        #[cfg(debug_assertions)]
        {
            tracing::info!(
                "EvolveMe DEBUG: Would complete step {} for user {} (task: {})",
                step,
                self.user_id,
                self.unique_task_key
            );
            return Ok(());
        }

        #[cfg(not(debug_assertions))]
        {
            let body = serde_json::to_vec(&serde_json::json!({
                "stepNumber": step,
                "uniqueTaskKey": self.unique_task_key,
                "userId": self.user_id
            }))?;

            // Sign with HMAC-SHA256
            let mut mac: Hmac<Sha256> = Hmac::new_from_slice(EVOLVEME_SECRET.as_bytes())?;
            mac.update(&body);
            let sig = hex::encode(mac.finalize().into_bytes());

            let client = reqwest::Client::new();
            let response = client
                .post(format!("https://{}/user/tasks/steps", EVOLVEME_HOST))
                .header("X-EM-Signature", sig)
                .body(body)
                .send()
                .await?;

            let response_text = response.text().await?;
            tracing::info!("EvolveMe response: {}", response_text);

            Ok(())
        }
    }
}

// Search parameter structures
#[derive(Serialize, Deserialize, Debug, Default, Clone)]
struct SearchParams {
    #[serde(default)]
    phrase: String,
    #[serde(
        default,
        deserialize_with = "common::serde_utils::deserialize_empty_string_as_none"
    )]
    topic: Option<i32>,
    #[serde(default)]
    online: bool,
}

impl SearchParams {
    fn phrase(&self, default: &'static str) -> &str {
        if self.phrase.is_empty() {
            default
        } else {
            &self.phrase
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Default)]
struct SearchAndSaveSearchParams {
    page: Option<i32>,
    #[serde(default)]
    advanced: bool,
    #[serde(flatten)]
    search: SearchParams,
}

impl SearchAndSaveSearchParams {
    pub fn with_page(&self, page: i32) -> Self {
        SearchAndSaveSearchParams {
            page: Some(page),
            advanced: self.advanced,
            search: self.search.clone(),
        }
    }

    pub fn url(&self) -> String {
        format!(
            "?{}",
            serde_qs::to_string(self).unwrap_or_else(|_| String::new())
        )
    }

    pub fn phrase(&self, default: &'static str) -> &str {
        self.search.phrase(default)
    }
}

// Project preview for search results
#[derive(Debug)]
struct ProjectPreview {
    id: String,
    name: String,
    goal: String,
    task: String,
    image: String,
}

// Project detail for project page
#[derive(Debug)]
struct ProjectDetail {
    id: String,
    name: String,
    presenter: String,
    image: String,
    likes: i32,
    goal: String,
    task: String,
    description: String,
    url: String,
    topics: Vec<String>,
    audience: Vec<String>,
    tags: Vec<String>,
}

// Template structures
#[derive(TemplateOnce, Debug)]
#[template(path = "evolveme/header.stpl.html")]
struct Header;

#[derive(TemplateOnce, Debug)]
#[template(path = "evolveme/footer.stpl.html")]
struct Footer;

#[derive(TemplateOnce, Debug)]
#[template(path = "evolveme/signup.stpl.html")]
struct SignupPage {
    csrftoken: String,
    task: &'static str,
    next: &'static str,
    steps: Vec<(String, String, Option<String>)>,
}

#[derive(TemplateOnce, Debug)]
#[template(path = "evolveme/login.stpl.html")]
struct LoginPage {
    csrftoken: String,
    task: &'static str,
    next: &'static str,
    steps: Vec<(String, String, Option<String>)>,
}

#[derive(TemplateOnce, Debug)]
#[template(path = "evolveme/search.stpl.html")]
struct SearchAndSaveSearchPage {
    advanced: bool,
    results: Option<Vec<ProjectPreview>>,
    offset: usize,
    limit: usize,
    total: usize,
    page: i32,
    max_page: i32,
    params: SearchAndSaveSearchParams,
}

#[derive(TemplateOnce, Debug)]
#[template(path = "evolveme/card.stpl.html")]
struct ProjectCard<'a> {
    project: &'a ProjectPreview,
}

#[derive(TemplateOnce, Debug)]
#[template(path = "evolveme/project.stpl.html")]
struct SearchAndSaveProjectPage {
    project: ProjectDetail,
}

#[derive(TemplateOnce, Debug)]
#[template(path = "evolveme/done.stpl.html")]
struct DonePage {
    links: Vec<(&'static str, &'static str, &'static str, &'static str)>,
}

#[derive(TemplateOnce, Debug)]
#[template(path = "evolveme/lesson.stpl.html")]
struct LessonPage {
    user: Arc<BasicInfo>,
}

/// Parameters for Articulate Storyline LRS integration
#[derive(Debug)]
struct ArticulateParams {
    lrs_url: String,
    user: Arc<BasicInfo>,
    activity: Option<String>,
    jwt: String,
}

impl ArticulateParams {
    fn new(lrs_url: &str, user: Arc<BasicInfo>) -> Self {
        use jsonwebtoken::{EncodingKey, Header, encode};
        use serde::Serialize;

        #[derive(Serialize)]
        struct ArticulateJwtClaims {
            email: String,
        }

        // Generate JWT with email field containing ULID@scistarter.org
        let claims = ArticulateJwtClaims {
            email: format!("{}@scistarter.org", user.user_id),
        };

        let jwt_secret = env!("SCISTARTER_JWT_SECRET");
        let jwt = encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(jwt_secret.as_bytes()),
        )
        .unwrap_or_else(|_| String::from(""));

        Self {
            lrs_url: lrs_url.to_string(),
            user,
            activity: None,
            jwt,
        }
    }

    fn with_activity(mut self, activity: &str) -> Self {
        self.activity = Some(activity.to_string());
        self
    }
}

impl std::fmt::Display for ArticulateParams {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Build query string for Articulate Storyline
        let mut params = vec![
            format!("endpoint={}", urlencoding::encode(&self.lrs_url)),
            format!(
                "actor={}",
                urlencoding::encode(
                    &serde_json::json!({
                        "objectType": "Agent",
                        "name": self.user.handle,
                        "mbox": format!("mailto:{}@scistarter.org", self.user.user_id),
                    })
                    .to_string()
                )
            ),
            format!(
                "auth={}",
                urlencoding::encode(&format!("Bearer {}", self.jwt))
            ),
        ];

        if let Some(activity) = &self.activity {
            params.push(format!("activity={}", urlencoding::encode(activity)));
        }

        params.push("tincan=true".to_string());

        write!(f, "{}", params.join("&"))
    }
}

#[derive(TemplateOnce, Debug)]
#[template(path = "evolveme/waiting.stpl.html")]
struct WaitingPage {
    name: String,
    url: String,
}

// Placeholder handlers - to be implemented
// Note: CSRF tokens not needed (modern SameSite cookies). Use session for EvolveMeParams.

#[handler]
async fn search_and_save_signup(
    session: &Session,
    Query(params): Query<EvolveMeParams>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, Error> {
    // Store params in session if provided (coming from ASA)
    // Check if params has meaningful values (not all defaults)
    if params.unique_task_key != Uuid::nil() {
        session.set("evolveme_params", &params);
    }

    // If already authenticated, redirect to next step
    if user_info.is_some() {
        return Ok(Redirect::see_other("/evolveme/search-and-save/search").into_response());
    }

    // Render signup page
    Ok(Html(
        SignupPage {
            csrftoken: String::new(), // Not needed - using SameSite cookies
            task: "search-and-save",
            next: "/evolveme/search-and-save/search",
            steps: vec![
                (
                    "2".into(),
                    "Find<br />a Citizen Science Project".into(),
                    None,
                ),
                (
                    "3".into(),
                    "Save<br />a Citizen Science Project".into(),
                    Some("Last Step".into()),
                ),
            ],
        }
        .render_once()?,
    )
    .into_response())
}

#[handler]
async fn search_and_save_login(
    session: &Session,
    Query(params): Query<EvolveMeParams>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, Error> {
    // Store params in session if provided (coming from ASA)
    if params.unique_task_key != Uuid::nil() {
        session.set("evolveme_params", &params);
    }

    // If already authenticated, redirect to next step
    if user_info.is_some() {
        return Ok(Redirect::see_other("/evolveme/search-and-save/search").into_response());
    }

    // Render login page
    Ok(Html(
        LoginPage {
            csrftoken: String::new(),
            task: "search-and-save",
            next: "/evolveme/search-and-save/search",
            steps: vec![
                (
                    "2".into(),
                    "Find<br />a Citizen Science Project".into(),
                    None,
                ),
                (
                    "3".into(),
                    "Save<br />a Citizen Science Project".into(),
                    Some("Last Step".into()),
                ),
            ],
        }
        .render_once()?,
    )
    .into_response())
}

#[handler]
async fn search_and_save_search(
    session: &Session,
    Query(search): Query<SearchAndSaveSearchParams>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, Error> {
    // Require authentication
    if user_info.is_none() {
        return Ok(Redirect::see_other("/evolveme/search-and-save/signup").into_response());
    }

    // Get EvolveMeParams from session
    let evolveme: EvolveMeParams = session.get("evolveme_params").unwrap_or_default();

    if let Some(page) = search.page {
        // Show search results
        let offset = ((page - 1) * 10) as usize;
        let limit = 10usize;
        let limit_i64 = limit as i64;
        let offset_i64 = offset as i64;

        let projects = if !search.search.phrase.is_empty() {
            // FTS search with phrase
            sqlx::query_as!(
                ProjectPreview,
                r#"
                SELECT
                    p.project_id as "id!",
                    p.name as "name!",
                    p.goal as "goal!",
                    p.task as "task!",
                    p.image as "image!"
                FROM projects p
                JOIN projects_fts fts ON p.project_id = fts.project_id
                WHERE fts.projects_fts MATCH ?
                AND p.status = 'Active'
                ORDER BY fts.rank
                LIMIT ? OFFSET ?
                "#,
                search.search.phrase,
                limit_i64,
                offset_i64
            )
            .fetch_all(db())
            .await?
        } else {
            // No phrase - show all active projects
            sqlx::query_as!(
                ProjectPreview,
                r#"
                SELECT
                    project_id as "id!",
                    name as "name!",
                    goal as "goal!",
                    task as "task!",
                    image as "image!"
                FROM projects
                WHERE status = 'Active'
                ORDER BY name
                LIMIT ? OFFSET ?
                "#,
                limit_i64,
                offset_i64
            )
            .fetch_all(db())
            .await?
        };

        // Get total count
        let total = if !search.search.phrase.is_empty() {
            sqlx::query_scalar!(
                r#"
                SELECT COUNT(*) as "count!"
                FROM projects p
                JOIN projects_fts fts ON p.project_id = fts.project_id
                WHERE fts.projects_fts MATCH ?
                AND p.status = 'Active'
                "#,
                search.search.phrase
            )
            .fetch_one(db())
            .await? as usize
        } else {
            sqlx::query_scalar!(
                r#"
                SELECT COUNT(*) as "count!"
                FROM projects
                WHERE status = 'Active'
                "#
            )
            .fetch_one(db())
            .await? as usize
        };

        // Complete step 2 (showed results)
        evolveme.complete_step(2).await?;

        let max_page = if total == 0 {
            1
        } else {
            ((total - 1) / limit) as i32 + 1
        };

        Ok(Html(
            SearchAndSaveSearchPage {
                advanced: search.advanced,
                results: Some(projects),
                offset,
                limit,
                total,
                page,
                max_page,
                params: search,
            }
            .render_once()?,
        )
        .into_response())
    } else {
        // Show search form (first view)
        evolveme.complete_step(1).await?;

        Ok(Html(
            SearchAndSaveSearchPage {
                advanced: search.advanced,
                results: None,
                offset: 0,
                limit: 0,
                total: 0,
                page: 0,
                max_page: 0,
                params: Default::default(),
            }
            .render_once()?,
        )
        .into_response())
    }
}

#[derive(Deserialize, Debug)]
struct SearchAndSaveProjectParams {
    id: String,
}

#[handler]
async fn search_and_save_project(
    Query(params): Query<SearchAndSaveProjectParams>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, Error> {
    // Require authentication
    if user_info.is_none() {
        return Ok(Redirect::see_other("/evolveme/search-and-save/signup").into_response());
    }

    // Load project details
    let project_basic = sqlx::query!(
        r#"
        SELECT
            project_id,
            name,
            presenter,
            image,
            goal,
            task,
            description,
            url
        FROM projects
        WHERE project_id = ?
        "#,
        params.id
    )
    .fetch_one(db())
    .await?;

    // Load topics
    let topics = sqlx::query_scalar!(
        r#"SELECT topic FROM project_topics WHERE project_id = ?"#,
        params.id
    )
    .fetch_all(db())
    .await?;

    // Load audiences
    let audience = sqlx::query_scalar!(
        r#"SELECT audience FROM project_audiences WHERE project_id = ?"#,
        params.id
    )
    .fetch_all(db())
    .await?;

    // Load tags
    let tags = sqlx::query_scalar!(
        r#"SELECT tag FROM project_tags WHERE project_id = ?"#,
        params.id
    )
    .fetch_all(db())
    .await?;

    let project = ProjectDetail {
        id: project_basic.project_id,
        name: project_basic.name,
        presenter: project_basic.presenter,
        image: project_basic.image,
        likes: 0, // TODO: Implement likes system
        goal: project_basic.goal,
        task: project_basic.task,
        description: project_basic.description,
        url: project_basic.url,
        topics,
        audience,
        tags,
    };

    Ok(Html(SearchAndSaveProjectPage { project }.render_once()?).into_response())
}

#[derive(Deserialize, Debug)]
struct SearchAndSaveDoneParams {
    id: String,
}

#[handler]
async fn search_and_save_done(
    session: &Session,
    poem::web::Form(params): poem::web::Form<SearchAndSaveDoneParams>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, Error> {
    // Require authentication
    if user_info.is_none() {
        return Ok(Redirect::see_other("/evolveme/search-and-save/signup").into_response());
    }

    // Get EvolveMeParams from session
    let evolveme: EvolveMeParams = session.get("evolveme_params").unwrap_or_default();

    // Complete step 3 (saved project)
    evolveme.complete_step(3).await?;

    // Create bookmark in user's dashboard
    if let Some(user) = user_info {
        let bookmark_request = BookmarkCreateRequest {
            user_id: user.user_id,
            project_id: params.id.clone(),
        };

        let _response: BookmarkCreateResponse =
            request("user.bookmarks.create", &bookmark_request).await?;
    }

    // Render done page with links to other EvolveMe tasks
    Ok(Html(
        DonePage {
            links: vec![
                (
                    "Foundations of Citizen Science",
                    "Train to be a citizen scientist by following a tutorial and taking part in two beginner citizen science projects.",
                    "foundations.jpg",
                    "https://evolveme.asa.org/task/1349?deeplink=true",
                ),
                (
                    "Do Citizen Science: Instant Wild",
                    "ID the species in the camera trap images.",
                    "stream.jpg",
                    "https://evolveme.asa.org/task/1348?deeplink=true",
                ),
                (
                    "Do Citizen Science: Project Squirrel",
                    "Learn about squirrel behavior.",
                    "squirrel.jpg",
                    "https://evolveme.asa.org/task/1363?deeplink=true",
                ),
            ],
        }
        .render_once()?,
    )
    .into_response())
}

#[handler]
async fn lesson_quiz_signup(
    session: &Session,
    Query(params): Query<EvolveMeParams>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, Error> {
    // Store params in session if provided (coming from ASA)
    if params.unique_task_key != Uuid::nil() {
        session.set("evolveme_params", &params);
    }

    // If already authenticated, redirect to next step
    if user_info.is_some() {
        return Ok(Redirect::see_other("/evolveme/lesson-quiz/lesson").into_response());
    }

    // Render signup page
    Ok(Html(
        SignupPage {
            csrftoken: String::new(),
            task: "lesson-quiz",
            next: "/evolveme/lesson-quiz/lesson",
            steps: vec![
                (
                    "2".into(),
                    "Follow the Tutorial &amp;<br />Take the Quiz".into(),
                    None,
                ),
                ("3".into(), "Task Complete".into(), Some("Last Step".into())),
            ],
        }
        .render_once()?,
    )
    .into_response())
}

#[handler]
async fn lesson_quiz_login(
    session: &Session,
    Query(params): Query<EvolveMeParams>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, Error> {
    // Store params in session if provided (coming from ASA)
    if params.unique_task_key != Uuid::nil() {
        session.set("evolveme_params", &params);
    }

    // If already authenticated, redirect to next step
    if user_info.is_some() {
        return Ok(Redirect::see_other("/evolveme/lesson-quiz/lesson").into_response());
    }

    // Render login page
    Ok(Html(
        LoginPage {
            csrftoken: String::new(),
            task: "lesson-quiz",
            next: "/evolveme/lesson-quiz/lesson",
            steps: vec![
                (
                    "2".into(),
                    "Follow the Tutorial &amp;<br />Take the Quiz".into(),
                    None,
                ),
                ("3".into(), "Task Complete".into(), Some("Last Step".into())),
            ],
        }
        .render_once()?,
    )
    .into_response())
}

#[handler]
async fn lesson_quiz_lesson(
    session: &Session,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, Error> {
    // Require authentication
    let user = match user_info {
        Some(user) => user,
        None => return Ok(Redirect::see_other("/evolveme/lesson-quiz/signup").into_response()),
    };

    // Check if quiz has already been completed
    // Try both English and Spanish activities
    for activity in ["foundations-evolveme", "fundamentos-evolveme"] {
        let check_request = ArticulateCompletionRequest {
            user_id: user.user_id,
            activity: activity.to_string(),
        };
        let response: ArticulateCompletionResponse =
            request("log.articulate_completion", &check_request).await?;

        if response.completed {
            // Already complete - redirect to done page
            return Ok(Redirect::see_other("/evolveme/lesson-quiz/done").into_response());
        }
    }

    // Get EvolveMeParams from session
    let evolveme: EvolveMeParams = session.get("evolveme_params").unwrap_or_default();

    // Complete step 1 (viewed lesson page)
    evolveme.complete_step(1).await?;

    // Render lesson page - template will use SSE to wait for completion
    Ok(Html(LessonPage { user }.render_once()?).into_response())
}

#[handler]
async fn lesson_quiz_done(
    session: &Session,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, Error> {
    // Require authentication
    let user = match user_info {
        Some(user) => user,
        None => return Ok(Redirect::see_other("/evolveme/lesson-quiz/signup").into_response()),
    };

    // Check if quiz has actually been completed
    let mut completed = false;
    for activity in ["foundations-evolveme", "fundamentos-evolveme"] {
        let check_request = ArticulateCompletionRequest {
            user_id: user.user_id,
            activity: activity.to_string(),
        };
        let response: ArticulateCompletionResponse =
            request("log.articulate_completion", &check_request).await?;

        if response.completed {
            completed = true;
            break;
        }
    }

    if !completed {
        // Quiz not completed - redirect back to lesson page
        return Ok(Redirect::see_other("/evolveme/lesson-quiz/lesson").into_response());
    }

    // Get EvolveMeParams from session
    let evolveme: EvolveMeParams = session.get("evolveme_params").unwrap_or_default();

    // Complete step 2 (completed quiz)
    evolveme.complete_step(2).await?;

    // Render done page with links to other EvolveMe tasks
    Ok(Html(
        DonePage {
            links: vec![
                (
                    "Do Citizen Science: Great Sunflower Project",
                    "Find out which pollinators visit your garden.",
                    "sunflower.jpg",
                    "https://evolveme.asa.org/task/1351?deeplink=true",
                ),
                (
                    "Do Citizen Science: Stall Catchers",
                    "Help find a cure for Alzheimer's disease.",
                    "stall.jpg",
                    "https://evolveme.asa.org/task/1352?deeplink=true",
                ),
                (
                    "Search and Save a Citizen Science Project",
                    "Search for a project and save it to your dashboard.",
                    "search.jpg",
                    "https://evolveme.asa.org/task/1350?deeplink=true",
                ),
            ],
        }
        .render_once()?,
    )
    .into_response())
}

#[handler]
async fn project_squirrel_signup(
    session: &Session,
    Query(params): Query<EvolveMeParams>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, Error> {
    // Store params in session if provided (coming from ASA)
    if params.unique_task_key != Uuid::nil() {
        session.set("evolveme_params", &params);
    }

    // If already authenticated, redirect to next step
    if user_info.is_some() {
        return Ok(Redirect::see_other("/evolveme/project-squirrel/project").into_response());
    }

    // Render signup page
    Ok(Html(
        SignupPage {
            csrftoken: String::new(),
            task: "project-squirrel",
            next: "/evolveme/project-squirrel/project",
            steps: vec![
                (
                    "2".into(),
                    "Participate in<br />Project Squirrel".into(),
                    None,
                ),
                ("3".into(), "Task Complete".into(), Some("Last Step".into())),
            ],
        }
        .render_once()?,
    )
    .into_response())
}

#[handler]
async fn project_squirrel_login(
    session: &Session,
    Query(params): Query<EvolveMeParams>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, Error> {
    // Store params in session if provided (coming from ASA)
    if params.unique_task_key != Uuid::nil() {
        session.set("evolveme_params", &params);
    }

    // If already authenticated, redirect to next step
    if user_info.is_some() {
        return Ok(Redirect::see_other("/evolveme/project-squirrel/project").into_response());
    }

    // Render login page
    Ok(Html(
        LoginPage {
            csrftoken: String::new(),
            task: "project-squirrel",
            next: "/evolveme/project-squirrel/project",
            steps: vec![
                (
                    "2".into(),
                    "Participate in<br />Project Squirrel".into(),
                    None,
                ),
                ("3".into(), "Task Complete".into(), Some("Last Step".into())),
            ],
        }
        .render_once()?,
    )
    .into_response())
}

#[handler]
async fn project_squirrel_project(
    session: &Session,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, Error> {
    // Require authentication
    let user = match user_info {
        Some(user) => user,
        None => return Ok(Redirect::see_other("/evolveme/project-squirrel/signup").into_response()),
    };

    // Check if user has already participated in this project
    let project_id = PROJECT_SQUIRREL.to_string();
    let check_request = ProjectParticipationRequest {
        user_id: user.user_id,
        project_id: project_id.clone(),
    };
    let response: ProjectParticipationResponse =
        request("log.project_participation", &check_request).await?;

    if response.participated {
        // Already participated - redirect to done page
        return Ok(Redirect::see_other("/evolveme/project-squirrel/done").into_response());
    }

    // Get EvolveMeParams from session
    let evolveme: EvolveMeParams = session.get("evolveme_params").unwrap_or_default();

    // Complete step 1 (viewed project page)
    evolveme.complete_step(1).await?;

    // Get project details
    let project = sqlx::query!(
        r#"
        SELECT name, url
        FROM projects
        WHERE project_id = ?
        "#,
        project_id
    )
    .fetch_one(db())
    .await?;

    // Render waiting page - template will use SSE to wait for participation
    Ok(Html(
        WaitingPage {
            name: project.name,
            url: project.url,
        }
        .render_once()?,
    )
    .into_response())
}

#[handler]
async fn project_squirrel_done(
    session: &Session,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, Error> {
    // Require authentication
    let user = match user_info {
        Some(user) => user,
        None => return Ok(Redirect::see_other("/evolveme/project-squirrel/signup").into_response()),
    };

    // Check if user has actually participated in this project
    let project_id = PROJECT_SQUIRREL.to_string();
    let check_request = ProjectParticipationRequest {
        user_id: user.user_id,
        project_id: project_id.clone(),
    };
    let response: ProjectParticipationResponse =
        request("log.project_participation", &check_request).await?;

    if !response.participated {
        // Not participated - redirect back to project page
        return Ok(Redirect::see_other("/evolveme/project-squirrel/project").into_response());
    }

    // Get EvolveMeParams from session
    let evolveme: EvolveMeParams = session.get("evolveme_params").unwrap_or_default();

    // Complete step 2 (participated in project)
    evolveme.complete_step(2).await?;

    // Render done page with links to other EvolveMe tasks
    Ok(Html(
        DonePage {
            links: vec![
                (
                    "Foundations of Citizen Science",
                    "Train to be a citizen scientist by following a tutorial and taking part in two beginner citizen science projects.",
                    "foundations.jpg",
                    "https://evolveme.asa.org/task/1349?deeplink=true",
                ),
                (
                    "Do Citizen Science: Great Sunflower Project",
                    "Find out which pollinators visit your garden.",
                    "sunflower.jpg",
                    "https://evolveme.asa.org/task/1351?deeplink=true",
                ),
                (
                    "Search and Save a Citizen Science Project",
                    "Search for a project and save it to your dashboard.",
                    "search.jpg",
                    "https://evolveme.asa.org/task/1350?deeplink=true",
                ),
            ],
        }
        .render_once()?,
    )
    .into_response())
}

#[handler]
async fn great_sunflower_signup(
    session: &Session,
    Query(params): Query<EvolveMeParams>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, Error> {
    // Store params in session if provided (coming from ASA)
    if params.unique_task_key != Uuid::nil() {
        session.set("evolveme_params", &params);
    }

    // If already authenticated, redirect to next step
    if user_info.is_some() {
        return Ok(Redirect::see_other("/evolveme/great-sunflower/project").into_response());
    }

    // Render signup page
    Ok(Html(
        SignupPage {
            csrftoken: String::new(),
            task: "great-sunflower",
            next: "/evolveme/great-sunflower/project",
            steps: vec![
                (
                    "2".into(),
                    "Participate in<br />Great Sunflower Project".into(),
                    None,
                ),
                ("3".into(), "Task Complete".into(), Some("Last Step".into())),
            ],
        }
        .render_once()?,
    )
    .into_response())
}

#[handler]
async fn great_sunflower_login(
    session: &Session,
    Query(params): Query<EvolveMeParams>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, Error> {
    // Store params in session if provided (coming from ASA)
    if params.unique_task_key != Uuid::nil() {
        session.set("evolveme_params", &params);
    }

    // If already authenticated, redirect to next step
    if user_info.is_some() {
        return Ok(Redirect::see_other("/evolveme/great-sunflower/project").into_response());
    }

    // Render login page
    Ok(Html(
        LoginPage {
            csrftoken: String::new(),
            task: "great-sunflower",
            next: "/evolveme/great-sunflower/project",
            steps: vec![
                (
                    "2".into(),
                    "Participate in<br />Great Sunflower Project".into(),
                    None,
                ),
                ("3".into(), "Task Complete".into(), Some("Last Step".into())),
            ],
        }
        .render_once()?,
    )
    .into_response())
}

#[handler]
async fn great_sunflower_project(
    session: &Session,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, Error> {
    // Require authentication
    let user = match user_info {
        Some(user) => user,
        None => return Ok(Redirect::see_other("/evolveme/great-sunflower/signup").into_response()),
    };

    // Check if user has already participated in this project
    let project_id = GREAT_SUNFLOWER.to_string();
    let check_request = ProjectParticipationRequest {
        user_id: user.user_id,
        project_id: project_id.clone(),
    };
    let response: ProjectParticipationResponse =
        request("log.project_participation", &check_request).await?;

    if response.participated {
        // Already participated - redirect to done page
        return Ok(Redirect::see_other("/evolveme/great-sunflower/done").into_response());
    }

    // Get EvolveMeParams from session
    let evolveme: EvolveMeParams = session.get("evolveme_params").unwrap_or_default();

    // Complete step 1 (viewed project page)
    evolveme.complete_step(1).await?;

    // Get project details
    let project = sqlx::query!(
        r#"
        SELECT name, url
        FROM projects
        WHERE project_id = ?
        "#,
        project_id
    )
    .fetch_one(db())
    .await?;

    // Render waiting page - template will use SSE to wait for participation
    Ok(Html(
        WaitingPage {
            name: project.name,
            url: project.url,
        }
        .render_once()?,
    )
    .into_response())
}

#[handler]
async fn great_sunflower_done(
    session: &Session,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, Error> {
    // Require authentication
    let user = match user_info {
        Some(user) => user,
        None => return Ok(Redirect::see_other("/evolveme/great-sunflower/signup").into_response()),
    };

    // Check if user has actually participated in this project
    let project_id = GREAT_SUNFLOWER.to_string();
    let check_request = ProjectParticipationRequest {
        user_id: user.user_id,
        project_id: project_id.clone(),
    };
    let response: ProjectParticipationResponse =
        request("log.project_participation", &check_request).await?;

    if !response.participated {
        // Not participated - redirect back to project page
        return Ok(Redirect::see_other("/evolveme/great-sunflower/project").into_response());
    }

    // Get EvolveMeParams from session
    let evolveme: EvolveMeParams = session.get("evolveme_params").unwrap_or_default();

    // Complete step 2 (participated in project)
    evolveme.complete_step(2).await?;

    // Render done page with links to other EvolveMe tasks
    Ok(Html(
        DonePage {
            links: vec![
                (
                    "Foundations of Citizen Science",
                    "Train to be a citizen scientist by following a tutorial and taking part in two beginner citizen science projects.",
                    "foundations.jpg",
                    "https://evolveme.asa.org/task/1349?deeplink=true",
                ),
                (
                    "Do Citizen Science: Stall Catchers",
                    "Help find a cure for Alzheimer's disease.",
                    "stall.jpg",
                    "https://evolveme.asa.org/task/1352?deeplink=true",
                ),
                (
                    "Search and Save a Citizen Science Project",
                    "Search for a project and save it to your dashboard.",
                    "search.jpg",
                    "https://evolveme.asa.org/task/1350?deeplink=true",
                ),
            ],
        }
        .render_once()?,
    )
    .into_response())
}

#[handler]
async fn stallcatchers_signup(
    session: &Session,
    Query(params): Query<EvolveMeParams>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, Error> {
    // Store params in session if provided (coming from ASA)
    if params.unique_task_key != Uuid::nil() {
        session.set("evolveme_params", &params);
    }

    // If already authenticated, redirect to next step
    if user_info.is_some() {
        return Ok(Redirect::see_other("/evolveme/stallcatchers/project").into_response());
    }

    // Render signup page
    Ok(Html(
        SignupPage {
            csrftoken: String::new(),
            task: "stallcatchers",
            next: "/evolveme/stallcatchers/project",
            steps: vec![
                (
                    "2".into(),
                    "Participate in<br />Stall Catchers".into(),
                    None,
                ),
                ("3".into(), "Task Complete".into(), Some("Last Step".into())),
            ],
        }
        .render_once()?,
    )
    .into_response())
}

#[handler]
async fn stallcatchers_login(
    session: &Session,
    Query(params): Query<EvolveMeParams>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, Error> {
    // Store params in session if provided (coming from ASA)
    if params.unique_task_key != Uuid::nil() {
        session.set("evolveme_params", &params);
    }

    // If already authenticated, redirect to next step
    if user_info.is_some() {
        return Ok(Redirect::see_other("/evolveme/stallcatchers/project").into_response());
    }

    // Render login page
    Ok(Html(
        LoginPage {
            csrftoken: String::new(),
            task: "stallcatchers",
            next: "/evolveme/stallcatchers/project",
            steps: vec![
                (
                    "2".into(),
                    "Participate in<br />Stall Catchers".into(),
                    None,
                ),
                ("3".into(), "Task Complete".into(), Some("Last Step".into())),
            ],
        }
        .render_once()?,
    )
    .into_response())
}

#[handler]
async fn stallcatchers_project(
    session: &Session,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, Error> {
    // Require authentication
    let user = match user_info {
        Some(user) => user,
        None => return Ok(Redirect::see_other("/evolveme/stallcatchers/signup").into_response()),
    };

    // Check if user has already participated in this project
    let project_id = STALLCATCHERS.to_string();
    let check_request = ProjectParticipationRequest {
        user_id: user.user_id,
        project_id: project_id.clone(),
    };
    let response: ProjectParticipationResponse =
        request("log.project_participation", &check_request).await?;

    if response.participated {
        // Already participated - redirect to done page
        return Ok(Redirect::see_other("/evolveme/stallcatchers/done").into_response());
    }

    // Get EvolveMeParams from session
    let evolveme: EvolveMeParams = session.get("evolveme_params").unwrap_or_default();

    // Complete step 1 (viewed project page)
    evolveme.complete_step(1).await?;

    // Get project details
    let project = sqlx::query!(
        r#"
        SELECT name, url
        FROM projects
        WHERE project_id = ?
        "#,
        project_id
    )
    .fetch_one(db())
    .await?;

    // Render waiting page - template will use SSE to wait for participation
    Ok(Html(
        WaitingPage {
            name: project.name,
            url: project.url,
        }
        .render_once()?,
    )
    .into_response())
}

#[handler]
async fn stallcatchers_done(
    session: &Session,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, Error> {
    // Require authentication
    let user = match user_info {
        Some(user) => user,
        None => return Ok(Redirect::see_other("/evolveme/stallcatchers/signup").into_response()),
    };

    // Check if user has actually participated in this project
    let project_id = STALLCATCHERS.to_string();
    let check_request = ProjectParticipationRequest {
        user_id: user.user_id,
        project_id: project_id.clone(),
    };
    let response: ProjectParticipationResponse =
        request("log.project_participation", &check_request).await?;

    if !response.participated {
        // Not participated - redirect back to project page
        return Ok(Redirect::see_other("/evolveme/stallcatchers/project").into_response());
    }

    // Get EvolveMeParams from session
    let evolveme: EvolveMeParams = session.get("evolveme_params").unwrap_or_default();

    // Complete step 2 (participated in project)
    evolveme.complete_step(2).await?;

    // Render done page with links to other EvolveMe tasks
    Ok(Html(
        DonePage {
            links: vec![
                (
                    "Foundations of Citizen Science",
                    "Train to be a citizen scientist by following a tutorial and taking part in two beginner citizen science projects.",
                    "foundations.jpg",
                    "https://evolveme.asa.org/task/1349?deeplink=true",
                ),
                (
                    "Do Citizen Science: Great Sunflower Project",
                    "Find out which pollinators visit your garden.",
                    "sunflower.jpg",
                    "https://evolveme.asa.org/task/1351?deeplink=true",
                ),
                (
                    "Search and Save a Citizen Science Project",
                    "Search for a project and save it to your dashboard.",
                    "search.jpg",
                    "https://evolveme.asa.org/task/1350?deeplink=true",
                ),
            ],
        }
        .render_once()?,
    )
    .into_response())
}

#[handler]
async fn instant_wild_signup(
    session: &Session,
    Query(params): Query<EvolveMeParams>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, Error> {
    // Store params in session if provided (coming from ASA)
    if params.unique_task_key != Uuid::nil() {
        session.set("evolveme_params", &params);
    }

    // If already authenticated, redirect to next step
    if user_info.is_some() {
        return Ok(Redirect::see_other("/evolveme/instant-wild/project").into_response());
    }

    // Render signup page
    Ok(Html(
        SignupPage {
            csrftoken: String::new(),
            task: "instant-wild",
            next: "/evolveme/instant-wild/project",
            steps: vec![
                ("2".into(), "Participate in<br />Instant Wild".into(), None),
                ("3".into(), "Task Complete".into(), Some("Last Step".into())),
            ],
        }
        .render_once()?,
    )
    .into_response())
}

#[handler]
async fn instant_wild_login(
    session: &Session,
    Query(params): Query<EvolveMeParams>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, Error> {
    // Store params in session if provided (coming from ASA)
    if params.unique_task_key != Uuid::nil() {
        session.set("evolveme_params", &params);
    }

    // If already authenticated, redirect to next step
    if user_info.is_some() {
        return Ok(Redirect::see_other("/evolveme/instant-wild/project").into_response());
    }

    // Render login page
    Ok(Html(
        LoginPage {
            csrftoken: String::new(),
            task: "instant-wild",
            next: "/evolveme/instant-wild/project",
            steps: vec![
                ("2".into(), "Participate in<br />Instant Wild".into(), None),
                ("3".into(), "Task Complete".into(), Some("Last Step".into())),
            ],
        }
        .render_once()?,
    )
    .into_response())
}

#[handler]
async fn instant_wild_project(
    session: &Session,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, Error> {
    // Require authentication
    let user = match user_info {
        Some(user) => user,
        None => return Ok(Redirect::see_other("/evolveme/instant-wild/signup").into_response()),
    };

    // Check if user has already participated in this project
    let project_id = INSTANT_WILD.to_string();
    let check_request = ProjectParticipationRequest {
        user_id: user.user_id,
        project_id: project_id.clone(),
    };
    let response: ProjectParticipationResponse =
        request("log.project_participation", &check_request).await?;

    if response.participated {
        // Already participated - redirect to done page
        return Ok(Redirect::see_other("/evolveme/instant-wild/done").into_response());
    }

    // Get EvolveMeParams from session
    let evolveme: EvolveMeParams = session.get("evolveme_params").unwrap_or_default();

    // Complete step 1 (viewed project page)
    evolveme.complete_step(1).await?;

    // Get project details
    let project = sqlx::query!(
        r#"
        SELECT name, url
        FROM projects
        WHERE project_id = ?
        "#,
        project_id
    )
    .fetch_one(db())
    .await?;

    // Render waiting page - template will use SSE to wait for participation
    Ok(Html(
        WaitingPage {
            name: project.name,
            url: project.url,
        }
        .render_once()?,
    )
    .into_response())
}

#[handler]
async fn instant_wild_done(
    session: &Session,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, Error> {
    // Require authentication
    let user = match user_info {
        Some(user) => user,
        None => return Ok(Redirect::see_other("/evolveme/instant-wild/signup").into_response()),
    };

    // Check if user has actually participated in this project
    let project_id = INSTANT_WILD.to_string();
    let check_request = ProjectParticipationRequest {
        user_id: user.user_id,
        project_id: project_id.clone(),
    };
    let response: ProjectParticipationResponse =
        request("log.project_participation", &check_request).await?;

    if !response.participated {
        // Not participated - redirect back to project page
        return Ok(Redirect::see_other("/evolveme/instant-wild/project").into_response());
    }

    // Get EvolveMeParams from session
    let evolveme: EvolveMeParams = session.get("evolveme_params").unwrap_or_default();

    // Complete step 2 (participated in project)
    evolveme.complete_step(2).await?;

    // Render done page with links to other EvolveMe tasks
    Ok(Html(
        DonePage {
            links: vec![
                (
                    "Foundations of Citizen Science",
                    "Train to be a citizen scientist by following a tutorial and taking part in two beginner citizen science projects.",
                    "foundations.jpg",
                    "https://evolveme.asa.org/task/1349?deeplink=true",
                ),
                (
                    "Do Citizen Science: Stall Catchers",
                    "Help find a cure for Alzheimer's disease.",
                    "stall.jpg",
                    "https://evolveme.asa.org/task/1352?deeplink=true",
                ),
                (
                    "Search and Save a Citizen Science Project",
                    "Search for a project and save it to your dashboard.",
                    "search.jpg",
                    "https://evolveme.asa.org/task/1350?deeplink=true",
                ),
            ],
        }
        .render_once()?,
    )
    .into_response())
}
