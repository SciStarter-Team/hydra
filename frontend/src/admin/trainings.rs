//! Admin management of training-page text content (Engine Room).
//!
//! Each training's structure and gating live in code; this page edits only the
//! copy — titles, prose, Articulate URLs, project-card text, sidebar, etc. —
//! held in that training's typed content record (e.g.
//! [`crate::training::foundations::FoundationsContent`]) and peered across
//! frontends on save. Every training's form is rendered from a shared template
//! driven by [`Section`]/[`Field`] descriptors, while the POST still
//! deserializes into the training's typed form struct.

use std::sync::Arc;

use crate::component::{AdminLayout, Component};
use crate::middleware::authorization::Authenticated;
use crate::peering::LoadStorePeered;
use crate::training::champion::ChampionContent;
use crate::training::dataethics::DataEthicsContent;
use crate::training::dataliteracy::DataLiteracyContent;
use crate::training::foundations::FoundationsContent;
use crate::training::highered::HigherEdContent;
use crate::training::libraries::LibrariesContent;
use common::message::user::BasicInfo;
use poem::{Endpoint, Route, get, handler, web::Form};
use sailfish::TemplateOnce;
use serde::Deserialize;

// ==================== Generic form rendering ====================

/// One editable field. `rows == 0` renders a single-line text input; `rows > 0`
/// renders a textarea (`mono` for HTML-source fields).
struct Field {
    name: &'static str,
    label: String,
    value: String,
    rows: u32,
    mono: bool,
}

impl Field {
    fn text(name: &'static str, label: &str, value: &str) -> Field {
        Field {
            name,
            label: label.to_string(),
            value: value.to_string(),
            rows: 0,
            mono: false,
        }
    }

    fn area(name: &'static str, label: &str, value: &str, rows: u32) -> Field {
        Field {
            name,
            label: label.to_string(),
            value: value.to_string(),
            rows,
            mono: false,
        }
    }

    fn html(name: &'static str, label: &str, value: &str, rows: u32) -> Field {
        Field {
            name,
            label: label.to_string(),
            value: value.to_string(),
            rows,
            mono: true,
        }
    }
}

/// A titled group of fields.
struct Section {
    heading: &'static str,
    fields: Vec<Field>,
}

fn section(heading: &'static str, fields: Vec<Field>) -> Section {
    Section { heading, fields }
}

#[derive(TemplateOnce)]
#[template(path = "admin/training_edit.stpl.html")]
struct TrainingEditPage {
    action: String,
    saved: bool,
    sections: Vec<Section>,
}

async fn render_edit(
    user_info: Option<Arc<BasicInfo>>,
    title: &str,
    breadcrumb: &str,
    action: &str,
    sections: Vec<Section>,
    saved: bool,
) -> Result<poem::Response, anyhow::Error> {
    let page = TrainingEditPage {
        action: action.to_string(),
        saved,
        sections,
    };
    let prepared = AdminLayout::new(title, page)
        .with_sidebar_active("engine-room-trainings")
        .with_breadcrumb("Engine Room", None)
        .with_breadcrumb("Trainings", Some("/-admin/trainings"))
        .with_breadcrumb(breadcrumb, None)
        .prepare(user_info)
        .await?;
    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(prepared.render_once()?))
}

/// Skills are edited as one-per-line text.
fn skills_to_text(skills: &[String]) -> String {
    skills.join("\n")
}

fn skills_from_text(text: &str) -> Vec<String> {
    text.lines()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect()
}

// ==================== Listing ====================

struct TrainingRow {
    name: &'static str,
    slug: &'static str,
}

/// The trainings that have an editable content record. Grows as each training
/// is ported.
const TRAININGS: &[TrainingRow] = &[
    TrainingRow {
        name: "Foundations of Citizen Science",
        slug: "foundations",
    },
    TrainingRow {
        name: "Data Ethics for Practitioners",
        slug: "data-ethics",
    },
    TrainingRow {
        name: "Data Literacy",
        slug: "data-literacy",
    },
    TrainingRow {
        name: "Teaching in Higher Education",
        slug: "higher-ed",
    },
    TrainingRow {
        name: "Libraries as Community Hubs",
        slug: "libraries-hubs",
    },
    TrainingRow {
        name: "Citizen Science Champion",
        slug: "champion",
    },
];

#[derive(TemplateOnce)]
#[template(path = "admin/trainings.stpl.html")]
struct TrainingsPage {
    trainings: &'static [TrainingRow],
}

#[handler]
async fn trainings_list(
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let prepared = AdminLayout::new("Trainings", TrainingsPage { trainings: TRAININGS })
        .with_sidebar_active("engine-room-trainings")
        .with_breadcrumb("Engine Room", None)
        .with_breadcrumb("Trainings", None)
        .prepare(user_info)
        .await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(prepared.render_once()?))
}

/// Shared header/skills sections common to every training.
fn header_and_skills(
    title: &str,
    sponsor: &str,
    intro: &str,
    estimated_time: &str,
    badge_image: &str,
    skills_intro: &str,
    skills: &[String],
    skills_footnote: &str,
) -> Vec<Section> {
    vec![
        section(
            "Page header",
            vec![
                Field::text("title", "Title", title),
                Field::text("sponsor", "Sponsor line (blank to hide)", sponsor),
                Field::html("intro", "Intro (HTML)", intro, 5),
                Field::text("estimated_time", "Estimated time", estimated_time),
                Field::text("badge_image", "Badge image URL", badge_image),
            ],
        ),
        section(
            "Skills & competencies",
            vec![
                Field::area("skills_intro", "Intro sentence", skills_intro, 2),
                Field::html(
                    "skills",
                    "Skills (one per line, HTML allowed)",
                    &skills_to_text(skills),
                    6,
                ),
                Field::html("skills_footnote", "Footnote (HTML)", skills_footnote, 3),
            ],
        ),
    ]
}

fn log_edit(user_info: &Option<Arc<BasicInfo>>, slug: &str) -> impl std::future::Future<Output = ()> {
    let user = user_info.clone();
    let slug = slug.to_string();
    async move {
        if let Some(user) = user {
            let _ = common::service::log_activity(
                None,
                "training-content-edit",
                user.user_id,
                None,
                Some(slug),
            )
            .await;
        }
    }
}

// ==================== Foundations ====================

#[derive(Debug, Deserialize)]
struct FoundationsForm {
    title: String,
    sponsor: String,
    intro: String,
    estimated_time: String,
    badge_image: String,
    skills_intro: String,
    skills: String,
    skills_footnote: String,
    account_heading: String,
    tutorial_heading: String,
    tutorial_intro: String,
    articulate_en: String,
    articulate_es: String,
    projects_heading: String,
    projects_intro: String,
    squirrel_title: String,
    squirrel_description: String,
    squirrel_image: String,
    squirrel_time_tag: String,
    squirrel_place_tag: String,
    stall_title: String,
    stall_description: String,
    stall_image: String,
    stall_time_tag: String,
    stall_place_tag: String,
    claim_heading: String,
    claim_html: String,
    sidebar_html: String,
}

fn foundations_sections(c: &FoundationsContent) -> Vec<Section> {
    let mut sections = header_and_skills(
        &c.title,
        &c.sponsor,
        &c.intro,
        &c.estimated_time,
        &c.badge_image,
        &c.skills_intro,
        &c.skills,
        &c.skills_footnote,
    );
    sections.push(section(
        "Step 1 — SciStarter Account",
        vec![Field::text("account_heading", "Step heading", &c.account_heading)],
    ));
    sections.push(section(
        "Step 2 — Tutorial & quiz",
        vec![
            Field::text("tutorial_heading", "Step heading", &c.tutorial_heading),
            Field::html("tutorial_intro", "Intro (HTML)", &c.tutorial_intro, 3),
            Field::text("articulate_en", "Articulate URL (English)", &c.articulate_en),
            Field::text("articulate_es", "Articulate URL (Spanish)", &c.articulate_es),
        ],
    ));
    sections.push(section(
        "Step 3 — Participate in projects",
        vec![
            Field::text("projects_heading", "Step heading", &c.projects_heading),
            Field::html("projects_intro", "Intro (HTML)", &c.projects_intro, 4),
            Field::text("squirrel_title", "Squirrel — title", &c.squirrel_title),
            Field::text("squirrel_image", "Squirrel — image URL", &c.squirrel_image),
            Field::area("squirrel_description", "Squirrel — description", &c.squirrel_description, 2),
            Field::text("squirrel_time_tag", "Squirrel — time tag", &c.squirrel_time_tag),
            Field::text("squirrel_place_tag", "Squirrel — place tag", &c.squirrel_place_tag),
            Field::text("stall_title", "Stall Catchers — title", &c.stall_title),
            Field::text("stall_image", "Stall Catchers — image URL", &c.stall_image),
            Field::area("stall_description", "Stall Catchers — description", &c.stall_description, 2),
            Field::text("stall_time_tag", "Stall Catchers — time tag", &c.stall_time_tag),
            Field::text("stall_place_tag", "Stall Catchers — place tag", &c.stall_place_tag),
        ],
    ));
    sections.push(section(
        "Step 4 — Claim your badge",
        vec![
            Field::text("claim_heading", "Step heading", &c.claim_heading),
            Field::html("claim_html", "Body (HTML)", &c.claim_html, 4),
        ],
    ));
    sections.push(section(
        "Sidebar",
        vec![Field::html("sidebar_html", "Sidebar (HTML, badge rendered separately)", &c.sidebar_html, 12)],
    ));
    sections
}

#[handler]
async fn foundations_edit(
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let content = FoundationsContent::current().await;
    render_edit(
        user_info,
        "Foundations Training",
        "Foundations of Citizen Science",
        "/-admin/trainings/foundations",
        foundations_sections(&content),
        false,
    )
    .await
}

#[handler]
async fn foundations_save(
    Form(form): Form<FoundationsForm>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let content = FoundationsContent {
        title: form.title,
        sponsor: form.sponsor,
        intro: form.intro,
        estimated_time: form.estimated_time,
        badge_image: form.badge_image,
        skills_intro: form.skills_intro,
        skills: skills_from_text(&form.skills),
        skills_footnote: form.skills_footnote,
        account_heading: form.account_heading,
        tutorial_heading: form.tutorial_heading,
        tutorial_intro: form.tutorial_intro,
        articulate_en: form.articulate_en,
        articulate_es: form.articulate_es,
        projects_heading: form.projects_heading,
        projects_intro: form.projects_intro,
        squirrel_title: form.squirrel_title,
        squirrel_description: form.squirrel_description,
        squirrel_image: form.squirrel_image,
        squirrel_time_tag: form.squirrel_time_tag,
        squirrel_place_tag: form.squirrel_place_tag,
        stall_title: form.stall_title,
        stall_description: form.stall_description,
        stall_image: form.stall_image,
        stall_time_tag: form.stall_time_tag,
        stall_place_tag: form.stall_place_tag,
        claim_heading: form.claim_heading,
        claim_html: form.claim_html,
        sidebar_html: form.sidebar_html,
    };
    content.store().await?;
    log_edit(&user_info, "foundations").await;
    render_edit(
        user_info,
        "Foundations Training",
        "Foundations of Citizen Science",
        "/-admin/trainings/foundations",
        foundations_sections(&content),
        true,
    )
    .await
}

// ==================== Data Ethics ====================

#[derive(Debug, Deserialize)]
struct DataEthicsForm {
    title: String,
    sponsor: String,
    intro: String,
    estimated_time: String,
    badge_image: String,
    skills_intro: String,
    skills: String,
    skills_footnote: String,
    account_heading: String,
    tutorial_heading: String,
    tutorial_intro: String,
    articulate_url: String,
    articulate_image: String,
    claim_heading: String,
    claim_html: String,
    sidebar_html: String,
}

fn dataethics_sections(c: &DataEthicsContent) -> Vec<Section> {
    let mut sections = header_and_skills(
        &c.title,
        &c.sponsor,
        &c.intro,
        &c.estimated_time,
        &c.badge_image,
        &c.skills_intro,
        &c.skills,
        &c.skills_footnote,
    );
    sections.push(section(
        "Step 1 — SciStarter Account",
        vec![Field::text("account_heading", "Step heading", &c.account_heading)],
    ));
    sections.push(section(
        "Step 2 — Tutorial & quiz",
        vec![
            Field::text("tutorial_heading", "Step heading", &c.tutorial_heading),
            Field::html("tutorial_intro", "Intro (HTML)", &c.tutorial_intro, 3),
            Field::text("articulate_url", "Articulate URL", &c.articulate_url),
            Field::text("articulate_image", "Tutorial card image URL", &c.articulate_image),
        ],
    ));
    sections.push(section(
        "Step 3 — Claim your badge",
        vec![
            Field::text("claim_heading", "Step heading", &c.claim_heading),
            Field::html("claim_html", "Body (HTML)", &c.claim_html, 4),
        ],
    ));
    sections.push(section(
        "Sidebar",
        vec![Field::html("sidebar_html", "Sidebar (HTML, badge rendered separately)", &c.sidebar_html, 12)],
    ));
    sections
}

#[handler]
async fn dataethics_edit(
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let content = DataEthicsContent::current().await;
    render_edit(
        user_info,
        "Data Ethics Training",
        "Data Ethics for Practitioners",
        "/-admin/trainings/data-ethics",
        dataethics_sections(&content),
        false,
    )
    .await
}

#[handler]
async fn dataethics_save(
    Form(form): Form<DataEthicsForm>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let content = DataEthicsContent {
        title: form.title,
        sponsor: form.sponsor,
        intro: form.intro,
        estimated_time: form.estimated_time,
        badge_image: form.badge_image,
        skills_intro: form.skills_intro,
        skills: skills_from_text(&form.skills),
        skills_footnote: form.skills_footnote,
        account_heading: form.account_heading,
        tutorial_heading: form.tutorial_heading,
        tutorial_intro: form.tutorial_intro,
        articulate_url: form.articulate_url,
        articulate_image: form.articulate_image,
        claim_heading: form.claim_heading,
        claim_html: form.claim_html,
        sidebar_html: form.sidebar_html,
    };
    content.store().await?;
    log_edit(&user_info, "data-ethics").await;
    render_edit(
        user_info,
        "Data Ethics Training",
        "Data Ethics for Practitioners",
        "/-admin/trainings/data-ethics",
        dataethics_sections(&content),
        true,
    )
    .await
}

// ==================== Data Literacy ====================

#[derive(Debug, Deserialize)]
struct DataLiteracyForm {
    title: String,
    sponsor: String,
    intro: String,
    estimated_time: String,
    badge_image: String,
    skills_intro: String,
    skills: String,
    skills_footnote: String,
    account_heading: String,
    tutorial_heading: String,
    tutorial_intro: String,
    articulate_en: String,
    articulate_en_image: String,
    articulate_es: String,
    articulate_es_image: String,
    claim_heading: String,
    claim_html: String,
    sidebar_html: String,
}

fn dataliteracy_sections(c: &DataLiteracyContent) -> Vec<Section> {
    let mut sections = header_and_skills(
        &c.title,
        &c.sponsor,
        &c.intro,
        &c.estimated_time,
        &c.badge_image,
        &c.skills_intro,
        &c.skills,
        &c.skills_footnote,
    );
    sections.push(section(
        "Step 1 — SciStarter Account",
        vec![Field::text("account_heading", "Step heading", &c.account_heading)],
    ));
    sections.push(section(
        "Step 2 — Tutorial & quiz",
        vec![
            Field::text("tutorial_heading", "Step heading", &c.tutorial_heading),
            Field::html("tutorial_intro", "Intro (HTML)", &c.tutorial_intro, 3),
            Field::text("articulate_en", "Articulate URL (English)", &c.articulate_en),
            Field::text("articulate_en_image", "Card image URL (English)", &c.articulate_en_image),
            Field::text("articulate_es", "Articulate URL (Spanish)", &c.articulate_es),
            Field::text("articulate_es_image", "Card image URL (Spanish)", &c.articulate_es_image),
        ],
    ));
    sections.push(section(
        "Step 3 — Claim your badge",
        vec![
            Field::text("claim_heading", "Step heading", &c.claim_heading),
            Field::html("claim_html", "Body (HTML)", &c.claim_html, 5),
        ],
    ));
    sections.push(section(
        "Sidebar",
        vec![Field::html("sidebar_html", "Sidebar (HTML, badge rendered separately)", &c.sidebar_html, 8)],
    ));
    sections
}

#[handler]
async fn dataliteracy_edit(
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let content = DataLiteracyContent::current().await;
    render_edit(
        user_info,
        "Data Literacy Training",
        "Data Literacy",
        "/-admin/trainings/data-literacy",
        dataliteracy_sections(&content),
        false,
    )
    .await
}

#[handler]
async fn dataliteracy_save(
    Form(form): Form<DataLiteracyForm>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let content = DataLiteracyContent {
        title: form.title,
        sponsor: form.sponsor,
        intro: form.intro,
        estimated_time: form.estimated_time,
        badge_image: form.badge_image,
        skills_intro: form.skills_intro,
        skills: skills_from_text(&form.skills),
        skills_footnote: form.skills_footnote,
        account_heading: form.account_heading,
        tutorial_heading: form.tutorial_heading,
        tutorial_intro: form.tutorial_intro,
        articulate_en: form.articulate_en,
        articulate_en_image: form.articulate_en_image,
        articulate_es: form.articulate_es,
        articulate_es_image: form.articulate_es_image,
        claim_heading: form.claim_heading,
        claim_html: form.claim_html,
        sidebar_html: form.sidebar_html,
    };
    content.store().await?;
    log_edit(&user_info, "data-literacy").await;
    render_edit(
        user_info,
        "Data Literacy Training",
        "Data Literacy",
        "/-admin/trainings/data-literacy",
        dataliteracy_sections(&content),
        true,
    )
    .await
}

// ==================== Higher Education ====================

#[derive(Debug, Deserialize)]
struct HigherEdForm {
    title: String,
    sponsor: String,
    intro: String,
    estimated_time: String,
    badge_image: String,
    skills_intro: String,
    skills: String,
    skills_footnote: String,
    account_heading: String,
    tutorial_heading: String,
    tutorial_intro: String,
    articulate_url: String,
    articulate_image: String,
    list_heading: String,
    list_html: String,
    explore_heading: String,
    explore_html: String,
    explore_url: String,
    sidebar_html: String,
}

fn highered_sections(c: &HigherEdContent) -> Vec<Section> {
    let mut sections = header_and_skills(
        &c.title,
        &c.sponsor,
        &c.intro,
        &c.estimated_time,
        &c.badge_image,
        &c.skills_intro,
        &c.skills,
        &c.skills_footnote,
    );
    sections.push(section(
        "Step 1 — SciStarter Account",
        vec![Field::text("account_heading", "Step heading", &c.account_heading)],
    ));
    sections.push(section(
        "Step 2 — Tutorial & quiz",
        vec![
            Field::text("tutorial_heading", "Step heading", &c.tutorial_heading),
            Field::html("tutorial_intro", "Intro (HTML)", &c.tutorial_intro, 3),
            Field::text("articulate_url", "Articulate URL", &c.articulate_url),
            Field::text("articulate_image", "Tutorial card image URL", &c.articulate_image),
        ],
    ));
    sections.push(section(
        "Step 3 — Create a list (tracked)",
        vec![
            Field::text("list_heading", "Step heading", &c.list_heading),
            Field::html("list_html", "Body (HTML)", &c.list_html, 4),
        ],
    ));
    sections.push(section(
        "Step 4 — Explore AAPS (tracked, grants badge)",
        vec![
            Field::text("explore_heading", "Step heading", &c.explore_heading),
            Field::html("explore_html", "Body (HTML) — link to /training/higher-ed/explore", &c.explore_html, 4),
            Field::text("explore_url", "Explore destination URL (where the link redirects)", &c.explore_url),
        ],
    ));
    sections.push(section(
        "Sidebar",
        vec![Field::html("sidebar_html", "Sidebar (HTML, badge rendered separately)", &c.sidebar_html, 8)],
    ));
    sections
}

#[handler]
async fn highered_edit(
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let content = HigherEdContent::current().await;
    render_edit(
        user_info,
        "Higher Education Training",
        "Teaching in Higher Education",
        "/-admin/trainings/higher-ed",
        highered_sections(&content),
        false,
    )
    .await
}

#[handler]
async fn highered_save(
    Form(form): Form<HigherEdForm>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let content = HigherEdContent {
        title: form.title,
        sponsor: form.sponsor,
        intro: form.intro,
        estimated_time: form.estimated_time,
        badge_image: form.badge_image,
        skills_intro: form.skills_intro,
        skills: skills_from_text(&form.skills),
        skills_footnote: form.skills_footnote,
        account_heading: form.account_heading,
        tutorial_heading: form.tutorial_heading,
        tutorial_intro: form.tutorial_intro,
        articulate_url: form.articulate_url,
        articulate_image: form.articulate_image,
        list_heading: form.list_heading,
        list_html: form.list_html,
        explore_heading: form.explore_heading,
        explore_html: form.explore_html,
        explore_url: form.explore_url,
        sidebar_html: form.sidebar_html,
    };
    content.store().await?;
    log_edit(&user_info, "higher-ed").await;
    render_edit(
        user_info,
        "Higher Education Training",
        "Teaching in Higher Education",
        "/-admin/trainings/higher-ed",
        highered_sections(&content),
        true,
    )
    .await
}

// ==================== Libraries & Hubs ====================

#[derive(Debug, Deserialize)]
struct LibrariesForm {
    title: String,
    sponsor: String,
    intro: String,
    estimated_time: String,
    badge_image: String,
    skills_intro: String,
    skills: String,
    skills_footnote: String,
    account_heading: String,
    prereq_heading: String,
    prereq_html: String,
    tutorial_heading: String,
    tutorial_intro: String,
    articulate_url: String,
    articulate_image: String,
    supplemental_heading: String,
    supplemental_html: String,
    claim_heading: String,
    claim_html: String,
    sidebar_html: String,
}

fn libraries_sections(c: &LibrariesContent) -> Vec<Section> {
    let mut sections = header_and_skills(
        &c.title,
        &c.sponsor,
        &c.intro,
        &c.estimated_time,
        &c.badge_image,
        &c.skills_intro,
        &c.skills,
        &c.skills_footnote,
    );
    sections.push(section(
        "Step 1 — SciStarter Account",
        vec![Field::text("account_heading", "Step heading", &c.account_heading)],
    ));
    sections.push(section(
        "Step 2 — Foundations prerequisite",
        vec![
            Field::text("prereq_heading", "Step heading", &c.prereq_heading),
            Field::html("prereq_html", "Body (HTML)", &c.prereq_html, 3),
        ],
    ));
    sections.push(section(
        "Step 3 — Tutorial & quiz",
        vec![
            Field::text("tutorial_heading", "Step heading", &c.tutorial_heading),
            Field::html("tutorial_intro", "Intro (HTML)", &c.tutorial_intro, 3),
            Field::text("articulate_url", "Articulate URL", &c.articulate_url),
            Field::text("articulate_image", "Tutorial card image URL", &c.articulate_image),
        ],
    ));
    sections.push(section(
        "Step 4 — Join the Library Network",
        vec![
            Field::text("supplemental_heading", "Step heading", &c.supplemental_heading),
            Field::html("supplemental_html", "Body (HTML)", &c.supplemental_html, 3),
        ],
    ));
    sections.push(section(
        "Step 5 — Claim your badge",
        vec![
            Field::text("claim_heading", "Step heading", &c.claim_heading),
            Field::html("claim_html", "Body (HTML)", &c.claim_html, 10),
        ],
    ));
    sections.push(section(
        "Sidebar",
        vec![Field::html("sidebar_html", "Sidebar (HTML, badge rendered separately)", &c.sidebar_html, 6)],
    ));
    sections
}

#[handler]
async fn libraries_edit(
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let content = LibrariesContent::current().await;
    render_edit(
        user_info,
        "Libraries & Hubs Training",
        "Libraries as Community Hubs",
        "/-admin/trainings/libraries-hubs",
        libraries_sections(&content),
        false,
    )
    .await
}

#[handler]
async fn libraries_save(
    Form(form): Form<LibrariesForm>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let content = LibrariesContent {
        title: form.title,
        sponsor: form.sponsor,
        intro: form.intro,
        estimated_time: form.estimated_time,
        badge_image: form.badge_image,
        skills_intro: form.skills_intro,
        skills: skills_from_text(&form.skills),
        skills_footnote: form.skills_footnote,
        account_heading: form.account_heading,
        prereq_heading: form.prereq_heading,
        prereq_html: form.prereq_html,
        tutorial_heading: form.tutorial_heading,
        tutorial_intro: form.tutorial_intro,
        articulate_url: form.articulate_url,
        articulate_image: form.articulate_image,
        supplemental_heading: form.supplemental_heading,
        supplemental_html: form.supplemental_html,
        claim_heading: form.claim_heading,
        claim_html: form.claim_html,
        sidebar_html: form.sidebar_html,
    };
    content.store().await?;
    log_edit(&user_info, "libraries-hubs").await;
    render_edit(
        user_info,
        "Libraries & Hubs Training",
        "Libraries as Community Hubs",
        "/-admin/trainings/libraries-hubs",
        libraries_sections(&content),
        true,
    )
    .await
}

// ==================== Champion ====================

#[derive(Debug, Deserialize)]
struct ChampionForm {
    title: String,
    sponsor: String,
    intro: String,
    estimated_time: String,
    badge_image: String,
    skills_intro: String,
    skills: String,
    skills_footnote: String,
    account_heading: String,
    prereq_heading: String,
    prereq_html: String,
    tutorial_heading: String,
    tutorial_intro: String,
    articulate_url: String,
    articulate_image: String,
    claim_heading: String,
    claim_html: String,
    apply_heading: String,
    apply_html: String,
    sidebar_html: String,
}

fn champion_sections(c: &ChampionContent) -> Vec<Section> {
    let mut sections = header_and_skills(
        &c.title,
        &c.sponsor,
        &c.intro,
        &c.estimated_time,
        &c.badge_image,
        &c.skills_intro,
        &c.skills,
        &c.skills_footnote,
    );
    sections.push(section(
        "Step 1 — SciStarter Account",
        vec![Field::text("account_heading", "Step heading", &c.account_heading)],
    ));
    sections.push(section(
        "Step 2 — Foundations prerequisite",
        vec![
            Field::text("prereq_heading", "Step heading", &c.prereq_heading),
            Field::html("prereq_html", "Body (HTML)", &c.prereq_html, 3),
        ],
    ));
    sections.push(section(
        "Step 3 — Tutorial & quiz",
        vec![
            Field::text("tutorial_heading", "Step heading", &c.tutorial_heading),
            Field::html("tutorial_intro", "Intro (HTML)", &c.tutorial_intro, 3),
            Field::text("articulate_url", "Articulate URL", &c.articulate_url),
            Field::text("articulate_image", "Tutorial card image URL", &c.articulate_image),
        ],
    ));
    sections.push(section(
        "Step 4 — Champion claim",
        vec![
            Field::text("claim_heading", "Step heading", &c.claim_heading),
            Field::html("claim_html", "Body (HTML)", &c.claim_html, 10),
        ],
    ));
    sections.push(section(
        "Step 5 — Apply (Certified Ambassador)",
        vec![
            Field::text("apply_heading", "Step heading", &c.apply_heading),
            Field::html("apply_html", "Body (HTML)", &c.apply_html, 8),
        ],
    ));
    sections.push(section(
        "Sidebar",
        vec![Field::html("sidebar_html", "Sidebar (HTML, badge rendered separately)", &c.sidebar_html, 6)],
    ));
    sections
}

#[handler]
async fn champion_edit(
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let content = ChampionContent::current().await;
    render_edit(
        user_info,
        "Champion Training",
        "Citizen Science Champion",
        "/-admin/trainings/champion",
        champion_sections(&content),
        false,
    )
    .await
}

#[handler]
async fn champion_save(
    Form(form): Form<ChampionForm>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let content = ChampionContent {
        title: form.title,
        sponsor: form.sponsor,
        intro: form.intro,
        estimated_time: form.estimated_time,
        badge_image: form.badge_image,
        skills_intro: form.skills_intro,
        skills: skills_from_text(&form.skills),
        skills_footnote: form.skills_footnote,
        account_heading: form.account_heading,
        prereq_heading: form.prereq_heading,
        prereq_html: form.prereq_html,
        tutorial_heading: form.tutorial_heading,
        tutorial_intro: form.tutorial_intro,
        articulate_url: form.articulate_url,
        articulate_image: form.articulate_image,
        claim_heading: form.claim_heading,
        claim_html: form.claim_html,
        apply_heading: form.apply_heading,
        apply_html: form.apply_html,
        sidebar_html: form.sidebar_html,
    };
    content.store().await?;
    log_edit(&user_info, "champion").await;
    render_edit(
        user_info,
        "Champion Training",
        "Citizen Science Champion",
        "/-admin/trainings/champion",
        champion_sections(&content),
        true,
    )
    .await
}

// ==================== Routes ====================

pub fn routes() -> impl Endpoint {
    Route::new()
        .at("/", get(trainings_list))
        .at("/foundations", get(foundations_edit).post(foundations_save))
        .at("/data-ethics", get(dataethics_edit).post(dataethics_save))
        .at(
            "/data-literacy",
            get(dataliteracy_edit).post(dataliteracy_save),
        )
        .at("/higher-ed", get(highered_edit).post(highered_save))
        .at(
            "/libraries-hubs",
            get(libraries_edit).post(libraries_save),
        )
        .at("/champion", get(champion_edit).post(champion_save))
}
