//! EvolveMe Integration
//!
//! Integration with ASA's (American Student Assistance) learning platform.
//! Implements multi-step workflows with HMAC-signed API callbacks.

use anyhow::Error;
use hmac::{Hmac, Mac};
use poem::{Route, handler, web::Query};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::sync::LazyLock;
use ulid::Ulid;
use uuid::Uuid;

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
const EVOLVEME_HOST: &'static str = "evolveme-api.asa.org";
// const EVOLVEME_HOST: &'static str = "evolveme-api-stage.asa.org";  // For staging

// ASA API secret from environment
const EVOLVEME_SECRET: &'static str = env!("EVOLVEME_SECRET");

pub fn routes() -> Route {
    Route::new()
        .at("/events", poem::get(evolveme_events))
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

// Placeholder handlers - to be implemented
#[handler]
async fn evolveme_events() -> Result<poem::Response, Error> {
    todo!("Implement SSE event stream")
}

#[handler]
async fn search_and_save_signup() -> Result<poem::Response, Error> {
    todo!("Implement search-and-save signup")
}

#[handler]
async fn search_and_save_login() -> Result<poem::Response, Error> {
    todo!("Implement search-and-save login")
}

#[handler]
async fn search_and_save_search() -> Result<poem::Response, Error> {
    todo!("Implement search-and-save search")
}

#[handler]
async fn search_and_save_project() -> Result<poem::Response, Error> {
    todo!("Implement search-and-save project")
}

#[handler]
async fn search_and_save_done() -> Result<poem::Response, Error> {
    todo!("Implement search-and-save done")
}

#[handler]
async fn lesson_quiz_signup() -> Result<poem::Response, Error> {
    todo!("Implement lesson-quiz signup")
}

#[handler]
async fn lesson_quiz_login() -> Result<poem::Response, Error> {
    todo!("Implement lesson-quiz login")
}

#[handler]
async fn lesson_quiz_lesson() -> Result<poem::Response, Error> {
    todo!("Implement lesson-quiz lesson")
}

#[handler]
async fn lesson_quiz_done() -> Result<poem::Response, Error> {
    todo!("Implement lesson-quiz done")
}

#[handler]
async fn project_squirrel_signup() -> Result<poem::Response, Error> {
    todo!("Implement project-squirrel signup")
}

#[handler]
async fn project_squirrel_login() -> Result<poem::Response, Error> {
    todo!("Implement project-squirrel login")
}

#[handler]
async fn project_squirrel_project() -> Result<poem::Response, Error> {
    todo!("Implement project-squirrel project")
}

#[handler]
async fn project_squirrel_done() -> Result<poem::Response, Error> {
    todo!("Implement project-squirrel done")
}

#[handler]
async fn great_sunflower_signup() -> Result<poem::Response, Error> {
    todo!("Implement great-sunflower signup")
}

#[handler]
async fn great_sunflower_login() -> Result<poem::Response, Error> {
    todo!("Implement great-sunflower login")
}

#[handler]
async fn great_sunflower_project() -> Result<poem::Response, Error> {
    todo!("Implement great-sunflower project")
}

#[handler]
async fn great_sunflower_done() -> Result<poem::Response, Error> {
    todo!("Implement great-sunflower done")
}

#[handler]
async fn stallcatchers_signup() -> Result<poem::Response, Error> {
    todo!("Implement stallcatchers signup")
}

#[handler]
async fn stallcatchers_login() -> Result<poem::Response, Error> {
    todo!("Implement stallcatchers login")
}

#[handler]
async fn stallcatchers_project() -> Result<poem::Response, Error> {
    todo!("Implement stallcatchers project")
}

#[handler]
async fn stallcatchers_done() -> Result<poem::Response, Error> {
    todo!("Implement stallcatchers done")
}

#[handler]
async fn instant_wild_signup() -> Result<poem::Response, Error> {
    todo!("Implement instant-wild signup")
}

#[handler]
async fn instant_wild_login() -> Result<poem::Response, Error> {
    todo!("Implement instant-wild login")
}

#[handler]
async fn instant_wild_project() -> Result<poem::Response, Error> {
    todo!("Implement instant-wild project")
}

#[handler]
async fn instant_wild_done() -> Result<poem::Response, Error> {
    todo!("Implement instant-wild done")
}
