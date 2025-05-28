pub mod channel;
pub mod contact;
pub mod entity;
pub mod middleware;
pub mod project;
pub mod session;
pub mod upload;

use std::sync::LazyLock;

use anyhow::Error;
use common::{message::location::LocationInfo, shared::db};
use middleware::authorization::{NotAuthorized, SessionSubject};
use poem::{
    EndpointExt, IntoResponse, Response, Route, Server,
    endpoint::{EmbeddedFileEndpoint, EmbeddedFilesEndpoint},
    get, handler,
    http::HeaderMap,
    listener::TcpListener,
    session::{CookieConfig, ServerSession, Session},
    web::{Html, RealIp, Redirect, cookie::SameSite},
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

#[handler]
async fn index(
    _session: &Session,
    headers: &HeaderMap,
    subject: SessionSubject,
    RealIp(client_ip): RealIp,
) -> poem::Result<Html<String>> {
    tokio::spawn(async move {
        for counter in 0..32 {
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
            println!("send");
            channel::send(
                subject.as_ref(),
                "moocow",
                channel::Event::new("test", format!("<p>iteration {}</p>", counter)),
            )
            .await
            .unwrap();
        }

        channel::send(
            subject.as_ref(),
            "moocow",
            channel::Event::new("shutdown", ""),
        )
        .await
        .unwrap();
    });

    let location = if let Some(addr) = client_ip {
        common::service::request("location.ip", addr.to_string())
            .await
            .unwrap_or_default()
    } else {
        LocationInfo::default()
    };

    Ok(Html(format!(
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
    )))
}

async fn not_found(_err: poem::Error) -> Response {
    Response::builder()
        .content_type("text/html")
        .body("Not found") // Render 'not found' error page template
}

async fn not_authorized(err: poem::Error) -> Response {
    if let Ok(err) = err.downcast::<NotAuthorized>() {
        // In the login handler, remember to reject all next
        // values that don't start with '/'
        Redirect::see_other(format!("/login?unauthorized=true&next={}", err.target()))
            .into_response()
    } else {
        Redirect::see_other("/login&unauthorized=true").into_response()
    }
}

async fn forbidden(_err: poem::Error) -> Response {
    Response::builder()
        .content_type("text/html")
        .body("Not allowed") // Render 'forbidden' error page template
}

async fn internal_server_error(err: poem::Error) -> Response {
    eprintln!("{}", err);

    Response::builder()
        .status(poem::http::StatusCode::INTERNAL_SERVER_ERROR)
        .body("Internal Server Error") // Render error page template
}

async fn error_dispatch(err: poem::Error) -> Response {
    match err.status() {
        poem::http::StatusCode::NOT_FOUND => not_found(err).await,
        poem::http::StatusCode::UNAUTHORIZED => not_authorized(err).await,
        poem::http::StatusCode::FORBIDDEN => forbidden(err).await,
        _ => internal_server_error(err).await,
    }
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
        .with(middleware::analytics::AnalyticsMiddleware)
        .with(middleware::authorization::AuthMiddleware)
        .with(ServerSession::new(
            CookieConfig::default()
                .name(SESSION_COOKIE)
                .http_only(true)
                .secure(true)
                .same_site(SameSite::Lax)
                .path("/")
                .max_age(std::time::Duration::from_secs(60 * 60 * 24 * 365)),
            session_storage,
        ))
        .catch_all_error(error_dispatch);

    let _updates = tokio::spawn(project::receive_project_updates());

    let _synchronize = tokio::spawn(sync::synchronize());

    println!("Listening on {}", env!("LISTEN_ON"));

    Server::new(TcpListener::bind(env!("LISTEN_ON")))
        .run(routes)
        .await?;

    Ok(())
}

mod sync {
    use std::time::Duration;

    use common::misc;
    use jiff::Timestamp;
    use tokio::time::sleep;
    use ulid::Ulid;

    use crate::{
        contact::{Contact, ContactRole},
        project::{
            ProjectParticipationDegree, ProjectPromotionTime, ProjectStatus, all::ProjectAll,
        },
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
                proj.page_editor_ids = data
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

                let mut contacts = Vec::with_capacity(5);

                if !data.contact_email.is_empty() {
                    contacts.push(Contact {
                        contact_id: Ulid::new(),
                        role: ContactRole::ContactPerson,
                        name: data.contact_name.clone().empty_as_none(),
                        affiliation: data.contact_affiliation.clone().empty_as_none(),
                        email: data.contact_email.clone(),
                        phone: data.contact_phone.clone().empty_as_none(),
                        address: data.contact_address.clone().empty_as_none(),
                    });
                }

                if data.contact_is_manager {
                    contacts.push(Contact {
                        contact_id: Ulid::new(),
                        role: ContactRole::Manager,
                        name: data.contact_name.clone().empty_as_none(),
                        affiliation: data.contact_affiliation.clone().empty_as_none(),
                        email: data.contact_email.clone(),
                        phone: data.contact_phone.clone().empty_as_none(),
                        address: data.contact_address.clone().empty_as_none(),
                    });
                } else if !data.manager_email.is_empty() {
                    contacts.push(Contact {
                        contact_id: Ulid::new(),
                        role: ContactRole::Manager,
                        name: data.manager_name.empty_as_none(),
                        affiliation: data.manager_affiliation.empty_as_none(),
                        email: data.manager_email,
                        phone: data.manager_phone.empty_as_none(),
                        address: data.manager_address.empty_as_none(),
                    });
                }

                if !data.additional_contact_email.is_empty() {
                    contacts.push(Contact {
                        contact_id: Ulid::new(),
                        role: ContactRole::ContactPerson,
                        name: data.additional_contact_name.clone().empty_as_none(),
                        affiliation: data.additional_contact_affiliation.clone().empty_as_none(),
                        email: data.additional_contact_email.clone(),
                        phone: data.additional_contact_phone.clone().empty_as_none(),
                        address: data.additional_contact_address.clone().empty_as_none(),
                    });
                }

                if !data.technical_contact_email.is_empty() {
                    contacts.push(Contact {
                        contact_id: Ulid::new(),
                        role: ContactRole::Technical,
                        name: None,
                        affiliation: None,
                        email: data.technical_contact_email.clone(),
                        phone: None,
                        address: None,
                    });
                }

                if !data.notification_email.is_empty() {
                    contacts.push(Contact {
                        contact_id: Ulid::new(),
                        role: ContactRole::Notification,
                        name: None,
                        affiliation: None,
                        email: data.notification_email.clone(),
                        phone: None,
                        address: None,
                    });
                }

                proj.contacts = contacts.into_boxed_slice();
                proj.url = data.url;
                proj.video_url = data.video_url;
                proj.blog_url = data.blog_url;
                proj.twitter_name = data.twitter_name;
                proj.facebook_page = data.facebook_page;
                proj.instagram_page = data.instagram_page;
                proj.topics = data
                    .topics
                    .split("||")
                    .filter_map(|s| s.trim_matches('|').to_string().empty_as_none())
                    .collect::<Vec<_>>()
                    .into_boxed_slice();
                proj.fields_of_science = data
                    .fields_of_science
                    .split("||")
                    .filter_map(|s| s.trim_matches('|').to_string().empty_as_none())
                    .collect::<Vec<_>>()
                    .into_boxed_slice();
                proj.practices = data
                    .practices
                    .split("||")
                    .filter_map(|s| s.trim_matches('|').to_string().empty_as_none())
                    .collect::<Vec<_>>()
                    .into_boxed_slice();
                proj.practices_how = data.practices_how;
                proj.search_terms = data.search_terms;
                proj.difficulty = data.difficulty;
                proj.diy = data.diy;
                proj.audience = data
                    .audience
                    .split("||")
                    .filter_map(|s| s.trim_matches('|').to_string().empty_as_none())
                    .collect::<Vec<_>>()
                    .into_boxed_slice();
                proj.signup_url = data.signup_url;
                proj.prefix_how_to_logged_in = data.prefix_how_to_logged_in;
                proj.prefix_how_to_logged_out = data.prefix_how_to_logged_out;
                proj.how_to_join = data.how_to_join;
                proj.howto_video = data.howto_video;
                proj.outcomes_label = data.outcomes_label;
                proj.outcomes_link = data.outcomes_link;
                proj.time_required = data.time_required;
                proj.training_materials = data.training_materials;
                proj.classroom_materials = data.classroom_materials;
                proj.consent_statement = data.consent_statement;
                proj.consent_statement_date = data.consent_statement_date.map(|ts| ts.into());
                proj.participation_tools = data.participation_tools;
                proj.participation_prompt = data.participation_prompt;
                proj.participation_confirmation_message = data.participation_confirmation_message;
                proj.participation_notification_email = data.participation_notification_email;
                proj.participation_usage = data.participation_usage;
                proj.part_of = data.part_of.parse().ok();
                proj.associated_projects = data
                    .associated_projects
                    .split(',')
                    .filter_map(|s| s.parse().ok())
                    .collect::<Vec<_>>()
                    .into_boxed_slice();
                proj.total_expense = data.total_expense;
                proj.materials = data.materials;
                proj.gear = data.gear;
                proj.mobile_app_url_android = data.mobile_app_url_android;
                proj.mobile_app_url_ios = data.mobile_app_url_ios;
                proj.mobile_app_url = data.mobile_app_url;
                proj.location_type = data.location_type;
                proj.entirely_online = data.entirely_online;
                proj.on_demand = data.on_demand;
                proj.in_person = data.in_person;
                proj.indoors = data.indoors;
                proj.outdoors = data.outdoors;
                proj.regions = data.regions.empty_as_none();
                proj.regions_description = data.regions_description;
                proj.point = data.point.empty_as_none();
                proj.address = data.address;
                proj.city = data.city;
                proj.state = data.state;
                proj.zip = data.zip;
                proj.country = data.country;
                proj.other = data.other;
                proj.location_name = data.location_name;
                proj.location_landuse = data.location_landuse;
                proj.location_description = data.location_description;
                proj.geographic_scope = data
                    .geographic_scope
                    .split("||")
                    .filter_map(|s| s.trim_matches('|').to_string().empty_as_none())
                    .collect::<Vec<_>>()
                    .into_boxed_slice();
                proj.united_nations_region = data.united_nations_region;
                proj.begin = data.begin.map(|ts| ts.into());
                proj.end = data.end.map(|ts| ts.into());
                proj.project_timing = data.project_timing;
                proj.participation_schedule = data.participation_schedule;
                proj.time_per_participation = data.time_per_participation;
                proj.year_started = data.year_started;
                proj.year_completed = data.year_completed;
                proj.month_completed = data.month_completed;
                proj.participation_tasks = data
                    .participation_tasks
                    .split("||")
                    .filter_map(|s| s.trim_matches('|').to_string().empty_as_none())
                    .collect::<Vec<_>>()
                    .into_boxed_slice();
                proj.activities = data
                    .activities
                    .split("||")
                    .filter_map(|s| s.trim_matches('|').to_string().empty_as_none())
                    .collect::<Vec<_>>()
                    .into_boxed_slice();
                proj.special_skills = data.special_skills;
                proj.data_results = data.data_results;
                proj.data_aggregation = data
                    .data_aggregation
                    .split("||")
                    .filter_map(|s| s.trim_matches('|').to_string().empty_as_none())
                    .collect::<Vec<_>>()
                    .into_boxed_slice();
                proj.data_consent = data
                    .data_consent
                    .split("||")
                    .filter_map(|s| s.trim_matches('|').to_string().empty_as_none())
                    .collect::<Vec<_>>()
                    .into_boxed_slice();
                proj.data_visualization = data.data_visualization;
                proj.data_access = data
                    .data_access
                    .split("||")
                    .filter_map(|s| s.trim_matches('|').to_string().empty_as_none())
                    .collect::<Vec<_>>()
                    .into_boxed_slice();
                proj.data_quality = data.data_quality;
                proj.metadata_publication_url = data.metadata_publication_url;
                proj.data_publication_url = data.data_publication_url;
                proj.data_license = data.data_license;
                proj.intended_outcomes = data
                    .intended_outcomes
                    .split("||")
                    .filter_map(|s| s.trim_matches('|').to_string().empty_as_none())
                    .collect::<Vec<_>>()
                    .into_boxed_slice();
                proj.outcome_other = data.outcome_other;
                proj.university_affiliation = data.university_affiliation;
                proj.sponsors = data
                    .sponsors
                    .split("||")
                    .filter_map(|s| s.trim_matches('|').to_string().empty_as_none())
                    .collect::<Vec<_>>()
                    .into_boxed_slice();
                proj.stakeholders = data
                    .stakeholders
                    .split("||")
                    .filter_map(|s| s.trim_matches('|').to_string().empty_as_none())
                    .collect::<Vec<_>>()
                    .into_boxed_slice();
                proj.circuit_id = data.circuit_id;
                proj.sustainable_development_goals = data.sustainable_development_goals;

                proj.save().await?;

                if updated > bookmark {
                    bookmark = updated.clone();
                }
            }

            todo!("Enable project sync bookmarking");
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
