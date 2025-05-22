pub mod channel;
pub mod contact;
pub mod entity;
pub mod project;
pub mod session;
pub mod upload;

use std::sync::LazyLock;

use anyhow::Error;
use common::{message::location::LocationInfo, service::request, shared::db};
use poem::{
    EndpointExt, IntoResponse, Route, Server,
    endpoint::{EmbeddedFileEndpoint, EmbeddedFilesEndpoint},
    get, handler,
    http::HeaderMap,
    listener::TcpListener,
    session::{CookieConfig, ServerSession, Session},
    web::{Html, RealIp, cookie::SameSite},
};
use rust_embed::Embed;
use session::UserServiceSessionStorage;
use ulid::Ulid;

#[cfg(debug_assertions)]
pub const SESSION_COOKIE: &'static str = "session-id";

#[cfg(not(debug_assertions))]
pub const SESSION_COOKIE: &'static str = "__Host-session-id";

pub static SERVER_ID: LazyLock<Ulid> = LazyLock::new(Ulid::new);
pub static SERVER_ID_STR: LazyLock<String> = LazyLock::new(|| SERVER_ID.to_string());

#[cfg(debug_assertions)]
#[derive(Embed)]
#[folder = "static/"]
struct Static;

#[cfg(not(debug_assertions))]
#[derive(Embed)]
#[folder = "dist/"]
struct Static;

type StaticFiles = EmbeddedFilesEndpoint<Static>;
type StaticFile = EmbeddedFileEndpoint<Static>;

/// Ensures that the session contains a unique NATS subject, and
/// returns it
pub fn session_subject(session: &Session) -> String {
    if let Some(subj) = session.get::<String>("subject") {
        subj
    } else {
        let subj = Ulid::new().to_string();
        session.set("subject", &subj);
        subj
    }
}

#[handler]
async fn index(
    session: &Session,
    headers: &HeaderMap,
    RealIp(client_ip): RealIp,
) -> impl IntoResponse {
    // Cloning the session is fine except that changing the clone
    // won't cause the session cookie to be saved. Ensuring that
    // there's a session subject here avoids that problem.
    session_subject(session);

    let session = session.clone();
    tokio::spawn(async move {
        for counter in 0..32 {
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
            channel::send(
                &session,
                "moocow",
                channel::Event::new("test", format!("<p>iteration {}</p>", counter)),
            )
            .await
            .unwrap();
        }

        channel::send(&session, "moocow", channel::Event::new("shutdown", ""))
            .await
            .unwrap();
    });

    let location = if let Some(addr) = client_ip {
        request("location.ip", addr.to_string())
            .await
            .unwrap_or_default()
    } else {
        LocationInfo::default()
    };

    Html(format!(
        r#"<!doctype html>
           <html>
             <head>
               <script src="/-static/htmx.js"></script>
               <script src="/-static/hx-sse.js"></script>
               <script src="/-static/hx-alpine-morph.js"></script>
               <script src="/-static/alpine-morph.js"></script>
               <script src="/-static/alpine.js"></script>
             </head>
             <body hx-ext="sse,alpine-morph">
               <p>Before</p>
               <div sse-connect="/-channel/moocow" sse-swap=test sse-close=shutdown hx-target="find p" hx-swap=morph>
                 <p>No events yet</p>
               </div>
               <p>After</p>
               <p>Headers</p>
               <div>
                 {:?}
               </div>
               <code>{:?}</code>
             </body>
           </html>
        "#,
        headers, location,
    ))
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    common::shared::init("frontend").await;

    sqlx::migrate!("./migrations").run(db()).await?;

    let session_storage = UserServiceSessionStorage;

    let routes = Route::new()
        .nest("/-static", StaticFiles::new())
        .nest("/-upload", upload::routes())
        .nest("/-channel", channel::routes())
        .at("/favicon.ico", StaticFile::new("favicon.ico"))
        .at("/", get(index))
        .at("/index.html", get(index))
        .with(ServerSession::new(
            CookieConfig::default()
                .name(SESSION_COOKIE)
                .http_only(true)
                .secure(true)
                .same_site(SameSite::Lax)
                .path("/")
                .max_age(std::time::Duration::from_secs(60 * 60 * 24 * 365)),
            session_storage,
        ));

    let _updates = tokio::spawn(project::receive_project_updates());

    let _synchronize = tokio::spawn(sync::synchronize());

    Server::new(TcpListener::bind(env!("LISTEN_ON")))
        .run(routes)
        .await?;

    Ok(())
}

mod sync {
    use std::{str::FromStr, time::Duration};

    use common::misc;
    use jiff::Timestamp;
    use tokio::time::sleep;

    use crate::project::{
        ProjectAll, ProjectParticipationDegree, ProjectPromotionTime, ProjectStatus,
    };

    #[derive(Default, Debug, serde::Deserialize)]
    #[serde(default)]
    struct ProjectData {
        legacy_v1_project_id: Option<i32>, // default null, partial index
        legacy_v2_project_id: i32,
        id: String, // Ulid
        slug: String,
        publish: bool,
        foreign_id: String, // default blank, partial index
        created: Timestamp,
        updated: Timestamp,
        last_editor: String,
        updated_staff: Option<Timestamp>,
        last_editor_staff: String,
        name: String,
        tags: String,
        meta_description: String,
        published_at: Option<Timestamp>,
        owner: String,
        managers: String, // ulids joined by ","
        exchange: Option<String>,
        // section SciStarter
        wild_hope: bool,
        duplicate: bool,
        authoritative: bool,
        ignore_authoritative: bool,
        redirect: String,
        admin_notes: String,
        origin: String,
        suborigin: String,
        guid: String,
        opengraph_description: String,
        participation_degree: String, // choices = ["Collegial", "Co-Created", "Collaborative", "Contributory", "Contractual"]
        extra: String,
        is_test: bool,
        is_affiliate: bool,
        is_inaturalist: bool,
        inat_project_id: Option<i32>,
        welcomed: bool,
        participant_instructions: String,
        participant_instructions_email: String,
        participant_files: String, // multiple joined by ||
        connect_opaque_url: String,
        connect_opaque_url1: String,
        connect_opaque_url2: String,
        // section Admin
        flagged: bool,
        draft: bool,
        status: String, // choices = ["Pending", "Active", "Periodically Active", "Hiatus", "Complete", "Hidden"]
        promotion_time: String, // choices = ["Promote at any time", "Do not promote", "Prefer promotion on the target date"]
        promotion_target: Option<Timestamp>,
        // section Contact
        manager_name: String,
        manager_affiliation: String,
        manager_email: String,
        manager_phone: String,
        manager_address: String,
        contact_is_manager: bool,
        contact_name: String,
        contact_affiliation: String,
        contact_email: String,
        contact_phone: String,
        contact_address: String,
        additional_contact_name: String,
        additional_contact_affiliation: String,
        additional_contact_email: String,
        additional_contact_phone: String,
        additional_contact_address: String,
        technical_contact_email: String,
        notification_email: String,
        // section Project
        us_federally_funded: bool,
        presenter: String,
        goal: String,
        task: String,
        description: String,
        image: String,
        image_credit: String,
        // section Web
        url: String,
        video_url: String,
        blog_url: String,
        twitter_name: String,
        facebook_page: String,
        instagram_page: String,
        // section Categorization
        topics: String,            // multiple, joined by ||
        fields_of_science: String, // multiple, joined by ||
        practices: String,         // multiple, joined by ||
        practices_how: String,
        search_terms: String,
        difficulty: String, // choices = [[1, "Easy"], [2, "medium"], [3, "Difficult"]]
        diy: bool,
        // section Joining
        audience: String, // multiple, joined by ||
        signup_url: String,
        prefix_how_to_logged_in: String,
        prefix_how_to_logged_out: String,
        how_to_join: String,
        howto_video: String,
        outcomes_label: String,
        outcomes_link: String,
        time_required: String,
        training_materials: String,
        classroom_materials: String,
        consent_statement: String,
        consent_statement_date: Option<Timestamp>,
        participation_tools: bool,    // deprecated, maybe don't implement
        participation_prompt: String, // deprecated, maybe don't implement
        participation_confirmation_message: String, // deprecated, maybe don't implement
        participation_notification_email: String, // deprecated, maybe don't implement
        participation_usage: String,  // deprecated, maybe don't implement
        // section Relations
        part_of: String,
        associated_projects: String, // multiple, joined by ,
        associated_tools: String,    // multiple, joined by , but also deprecated
        // section Participants Need
        total_expense: Option<f32>,
        materials: String,
        gear: String,
        mobile_app_url_android: String,
        mobile_app_url_ios: String,
        mobile_app_url: String,
        // section Where
        location_type: String, // choices = ["Global, anywhere on the planet", "In a geographic location or range", "At a specific address"]
        entirely_online: bool,
        on_demand: bool,
        in_person: bool,
        regions: String,
        regions_description: String,
        point: String,
        address: String,
        city: String,
        state: String,
        zip: String,
        country: String,
        other: String,
        location_name: String,
        location_landuse: String,
        location_description: String,
        geographic_scope: String,      // multiple, joined by ||
        united_nations_region: String, // multiple, joined by ||
        // section When
        begin: Option<Timestamp>,
        end: Option<Timestamp>,
        project_timing: String,
        participation_schedule: String,
        time_per_participation: String,
        year_started: Option<i32>,
        year_completed: Option<i32>,
        month_completed: Option<i32>,
        // section Skills
        participation_tasks: String, // multiple, joined by ||
        activities: String,          // multiple, joined by ||
        special_skills: String,
        indoors: bool,
        outdoors: bool,
        // section Data
        data_results: String,
        data_aggregation: String, // multiple, joined by ||
        data_consent: String,     // multiple, joined by ||
        data_visualization: bool,
        data_access: String, // multiple, joined by ||
        data_quality: bool,
        metadata_publication_url: String,
        data_publication_url: String,
        data_license: String,
        // section Outcomes
        intended_outcomes: String, // multiple, joined by ||
        outcome_other: String,
        // section Sponsors
        university_affiliation: String,
        sponsors: String,     // multiple, joined by ||
        stakeholders: String, // multiple, joined by ||
        circuit_id: String,
        sustainable_development_goals: String,
    }

    pub async fn synchronize() -> Result<(), anyhow::Error> {
        trait EmptyAsNone: Sized {
            fn empty_as_none(self) -> Option<Self>;
        }

        impl EmptyAsNone for String {
            fn empty_as_none(self) -> Option<Self> {
                if self.is_empty() { None } else { Some(self) }
            }
        }

        loop {
            let last = misc::get("project-sync-bookmark")
                .await
                .expect("Retrieving sync bookmark failed")
                .unwrap_or_else(|| String::from("2000-01-01 00:00:00+00"));

            let client = reqwest::Client::new();

            let projects: Vec<ProjectData> = client
                .get("https://scistarter.org/staff/sync/projects")
                .bearer_auth(env!("SYNC_TOKEN"))
                .query(&[("since", &last)])
                .send()
                .await
                .expect("Retrieving session data failed")
                .error_for_status()
                .expect("Retrieving session data produced an error")
                .json()
                .await
                .expect("JSON decoding of session data failed");

            let mut bookmark = last.clone();

            let got_projects = projects.len();

            for data in projects {
                let updated = data.updated.to_string();

                let mut proj = ProjectAll::default();

                proj.project_id = match data.id.parse() {
                    Ok(ulid) => ulid,
                    Err(err) => {
                        eprintln!("Error parsing ULID {:?}: {}", data.id, err);
                        continue;
                    }
                };

                proj.legacy_v1_project_id = data.legacy_v1_project_id.map(|x| x as i64);
                proj.legacy_v2_project_id = Some(data.legacy_v2_project_id as i64);
                proj.foreign_id = data.foreign_id.empty_as_none();
                proj.publish = data.publish;
                proj.duplicate = data.duplicate;
                proj.flagged = data.flagged;
                proj.is_test = data.is_test;
                proj.status = match data.status.as_str() {
                    "Pending" => ProjectStatus::Pending,
                    "Active" => ProjectStatus::Active,
                    "Periodically Active" => ProjectStatus::PeriodicallyActive,
                    "Hiatus" => ProjectStatus::Hiatus,
                    "Complete" => ProjectStatus::Complete,
                    "Hidden" => ProjectStatus::Hidden,
                    s => {
                        eprintln!("Unknown project status `{}`, settingto Hidden", s);
                        ProjectStatus::Hidden
                    }
                };
                proj.promotion_time = match data.promotion_time.as_str() {
                    "Promote at any time" => ProjectPromotionTime::PromoteAtAnyTime,
                    "Do not promote" => ProjectPromotionTime::DoNotPromote,
                    "Prefer promotion on the target date" => {
                        ProjectPromotionTime::PreferOnTargetDate
                    }
                    s => {
                        eprintln!(
                            "Unknown project protion time `{}`, settingto PromoteAtAnyTime ",
                            s
                        );
                        ProjectPromotionTime::PromoteAtAnyTime
                    }
                };
                proj.promotion_target = data.promotion_target.map(|ts| ts.into());
                proj.admin_notes = data.admin_notes;
                proj.created = data.created.into();
                proj.wild_hope = data.wild_hope;
                proj.exchange = data.exchange;
                proj.origin = data.origin;
                proj.suborigin = data.suborigin;
                proj.guid = data.guid;
                proj.opengraph_description = data.opengraph_description;
                proj.is_affiliate = data.is_affiliate;
                proj.is_inaturalist = data.is_inaturalist;
                proj.inat_project_id = data.inat_project_id;
                proj.welcomed = data.welcomed;
                proj.page_creator_id = data.owner.parse().unwrap_or_default();
                proj.page_manager_ids = data
                    .managers
                    .split(',')
                    .filter_map(|u| u.parse().ok())
                    .collect::<Vec<_>>()
                    .into_boxed_slice();
                proj.extra = data.extra;
                proj.project_last_editor = data
                    .last_editor
                    .empty_as_none()
                    .and_then(|s| s.parse().ok());
                proj.project_last_edited = Some(data.updated.into());
                proj.scistarter_last_editor = data
                    .last_editor_staff
                    .empty_as_none()
                    .and_then(|s| s.parse().ok());
                proj.scistarter_last_edited = data.updated_staff.map(|ts| ts.into());
                proj.draft = data.draft;
                proj.name = data.name;
                proj.tags = data
                    .tags
                    .split("||")
                    .filter_map(|s| s.trim_matches('|').trim().to_string().empty_as_none())
                    .collect::<Vec<_>>()
                    .into_boxed_slice();
                proj.participation_degree = match data.participation_degree.as_str() {
                    "Collegial" => ProjectParticipationDegree::Collegial,
                    "Co-Created" => ProjectParticipationDegree::CoCreated,
                    "Collaborative" => ProjectParticipationDegree::Collaborative,
                    "Contributory" => ProjectParticipationDegree::Contributory,
                    "Contractual" => ProjectParticipationDegree::Contractual,
                    s => {
                        eprintln!("Unknown participation degree `{s}`, defaulting to Collegial");
                        ProjectParticipationDegree::Collegial
                    }
                };
                proj.participant_instructions = data.participant_instructions;
                proj.participant_instructions_email = data.participant_instructions_email;
                proj.participant_files = data
                    .participant_files
                    .split("||")
                    .filter_map(|s| s.trim_matches('|').trim().to_string().empty_as_none())
                    .collect::<Vec<_>>()
                    .into_boxed_slice();
                proj.us_federally_funded = data.us_federally_funded;
                proj.presenter = data.presenter;
                proj.goal = data.goal;
                proj.task = data.task;
                proj.description = data.description;
                proj.image = data.image;
                proj.image_credit = data.image_credit;
                proj.connect_opaque_url = data.connect_opaque_url;
                proj.connect_opaque_url1 = data.connect_opaque_url1;
                proj.connect_opaque_url2 = data.connect_opaque_url2;

                // manager_name: String,
                // manager_affiliation: String,
                // manager_email: String,
                // manager_phone: String,
                // manager_address: String,
                // contact_is_manager: bool,
                // contact_name: String,
                // contact_affiliation: String,
                // contact_email: String,
                // contact_phone: String,
                // contact_address: String,
                // additional_contact_name: String,
                // additional_contact_affiliation: String,
                // additional_contact_email: String,
                // additional_contact_phone: String,
                // additional_contact_address: String,
                // technical_contact_email: String,
                // notification_email: String,

                let mut contacts = Vec::with_capacity(5);

                todo!(
                    "Fill the contacts Vec with the manager, contact, additional contact, technical contact, and notification contact"
                );

                proj.contacts = contacts.into_boxed_slice();
                proj.url = todo!();
                proj.video_url = todo!();
                proj.blog_url = todo!();
                proj.twitter_name = todo!();
                proj.facebook_page = todo!();
                proj.instagram_page = todo!();
                proj.topics = todo!();
                proj.fields_of_science = todo!();
                proj.practices = todo!();
                proj.practices_how = todo!();
                proj.search_terms = todo!();
                proj.difficulty = todo!();
                proj.diy = todo!();
                proj.audience = todo!();
                proj.signup_url = todo!();
                proj.prefix_how_to_logged_in = todo!();
                proj.prefix_how_to_logged_out = todo!();
                proj.how_to_join = todo!();
                proj.howto_video = todo!();
                proj.outcomes_label = todo!();
                proj.outcomes_link = todo!();
                proj.time_required = todo!();
                proj.training_materials = todo!();
                proj.classroom_materials = todo!();
                proj.consent_statement = todo!();
                proj.consent_statement_date = todo!();
                proj.participation_tools = todo!();
                proj.participation_prompt = todo!();
                proj.participation_confirmation_message = todo!();
                proj.participation_notification_email = todo!();
                proj.participation_usage = todo!();
                proj.part_of = todo!();
                proj.associated_projects = todo!();
                proj.total_expense = todo!();
                proj.materials = todo!();
                proj.gear = todo!();
                proj.mobile_app_url_android = todo!();
                proj.mobile_app_url_ios = todo!();
                proj.mobile_app_url = todo!();
                proj.location_type = todo!();
                proj.entirely_online = todo!();
                proj.on_demand = todo!();
                proj.in_person = todo!();
                proj.indoors = todo!();
                proj.outdoors = todo!();
                proj.regions = todo!();
                proj.regions_description = todo!();
                proj.point = todo!();
                proj.address = todo!();
                proj.city = todo!();
                proj.state = todo!();
                proj.zip = todo!();
                proj.country = todo!();
                proj.other = todo!();
                proj.location_name = todo!();
                proj.location_landuse = todo!();
                proj.location_description = todo!();
                proj.geographic_scope = todo!();
                proj.united_nations_region = todo!();
                proj.begin = todo!();
                proj.end = todo!();
                proj.project_timing = todo!();
                proj.participation_schedule = todo!();
                proj.time_per_participation = todo!();
                proj.year_started = todo!();
                proj.year_completed = todo!();
                proj.month_completed = todo!();
                proj.participation_tasks = todo!();
                proj.activities = todo!();
                proj.special_skills = todo!();
                proj.data_results = todo!();
                proj.data_aggregation = todo!();
                proj.data_consent = todo!();
                proj.data_visualization = todo!();
                proj.data_access = todo!();
                proj.data_quality = todo!();
                proj.metadata_publication_url = todo!();
                proj.data_publication_url = todo!();
                proj.data_license = todo!();
                proj.intended_outcomes = todo!();
                proj.outcome_other = todo!();
                proj.university_affiliation = todo!();
                proj.sponsors = todo!();
                proj.stakeholders = todo!();
                proj.circuit_id = todo!();
                proj.sustainable_development_goals = todo!();

                proj.save().await?;

                if updated > bookmark {
                    bookmark = updated.clone();
                }
            }

            // misc::set("project-sync-bookmark", bookmark)
            //     .await
            //     .expect("Saving sync bookmark failed");

            if got_projects == 500 {
                sleep(Duration::from_secs(60)).await;
            } else {
                sleep(Duration::from_secs(60 * 60)).await;
            }
        }
    }
}
