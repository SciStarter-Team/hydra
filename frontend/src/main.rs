pub mod acts_connected;
pub mod add;
pub mod admin;
pub mod ambassador;
pub mod api;
pub mod articulate;
pub mod auth;
pub mod badge;
pub mod builder;
pub mod certificates;
pub mod channel;
pub mod communities;
pub mod component;
pub mod concierge;
pub mod contact;
pub mod csm;
pub mod dashboard;
pub mod dev;
pub mod emailer;
pub mod embed;
pub mod entity;
pub mod entity_list;
pub mod event;
pub mod evolveme;
pub mod exchange;
pub mod external_link;
pub mod form;
pub mod geocode;
pub mod girlscouts;
pub mod gourl;
pub mod health;
pub mod home;
pub mod i18n;
pub mod invitation;
pub mod lazy_cache;
pub mod library_signup;
pub mod local_time;
pub mod lru_cache;
pub mod markdown;
pub mod mcp;
pub mod mcp_token;
pub mod middleware;
pub mod misc;
pub mod news;
pub mod number;
pub mod oauth2;
pub mod page_manager;
pub mod pagebuilder;
pub mod pagecontent;
pub mod pages;
pub mod participant_file;
pub mod peering;
pub mod profile;
pub mod profile_card;
pub mod project;
pub mod region;
pub mod schedule;
pub mod search;
pub mod session;
pub mod ssrf;
pub mod staff;
pub mod staff_pick;
pub mod thumbnail;
pub mod training;
pub mod ui;
pub mod upload;
pub mod webinar;
pub mod wiki;
pub mod wordpress;
pub mod zipcode;

use anyhow::Error;
use async_nats::service::ServiceExt;
use common::shared::db;
use component::Component;
use entity::Entity;
use event::{all::EventAll, field::EventField};
use futures::StreamExt;
use middleware::authorization::Authenticated;
use pages::index::index_page_pseudo_handler;
use peering::LoadStorePeered;
use poem::{
    Endpoint, EndpointExt, IntoResponse, Route, Server,
    endpoint::{EmbeddedFileEndpoint, EmbeddedFilesEndpoint},
    get, handler,
    listener::TcpListener,
    middleware::{ForceHttps, NormalizePath, SetHeader, TrailingSlash},
    post, put,
    web::{Form, Path, Redirect},
};
use project::{all::ProjectAll, field::ProjectField};
use rust_embed::Embed;
use tracing::Level;

use middleware::session_cookie::server_session;

// Serving the files from static instead of dist doesn't work well
// when many of those files are inputs for compilers and are not ready
// for direct use
// #[cfg(debug_assertions)]
// #[derive(Embed)]
// #[folder = "static/"]
// struct Static;
// #[cfg(not(debug_assertions))]
#[derive(Embed)]
#[folder = "dist/"]
struct Static;

type StaticFiles = EmbeddedFilesEndpoint<Static>;
type StaticFile = EmbeddedFileEndpoint<Static>;

// This will be the implementation after https://github.com/poem-web/poem/issues/1098 is fixed
// #[handler]
// #[tracing::instrument]
// async fn entity_page(Path(slug): Path<String>) -> poem::Result<poem::Response> {
//     Entity::response_for_slug(dbg!(slug)).await
// }

// Temporary implementation until https://github.com/poem-web/poem/issues/1098 is fixed
#[handler]
#[tracing::instrument]
async fn entity_page(
    poem::web::RealIp(client_ip): poem::web::RealIp,
    Authenticated(user_info): Authenticated,
    session: &poem::session::Session,
    req: &poem::Request,
    Path(slug): Path<String>,
) -> poem::Result<poem::Response> {
    if &slug == "" {
        Ok(index_page_pseudo_handler(client_ip, user_info).await?)
    } else {
        Entity::response_for_slug(
            slug,
            "/",
            user_info,
            session,
            req.uri().query().unwrap_or(""),
        )
        .await
    }
}

/// Serve a project at its canonical `/projects/<slug>` path. Entities
/// of other kinds (or projects reached under the wrong prefix) are
/// permanently redirected to their canonical path by
/// `response_for_slug`.
#[handler]
#[tracing::instrument]
async fn project_page(
    Authenticated(user_info): Authenticated,
    session: &poem::session::Session,
    req: &poem::Request,
    Path(slug): Path<String>,
) -> poem::Result<poem::Response> {
    Entity::response_for_slug(
        slug,
        "/projects/",
        user_info,
        session,
        req.uri().query().unwrap_or(""),
    )
    .await
}

/// Serve an event at its canonical `/events/<slug>` path.
#[handler]
#[tracing::instrument]
async fn event_page(
    Authenticated(user_info): Authenticated,
    session: &poem::session::Session,
    req: &poem::Request,
    Path(slug): Path<String>,
) -> poem::Result<poem::Response> {
    Entity::response_for_slug(
        slug,
        "/events/",
        user_info,
        session,
        req.uri().query().unwrap_or(""),
    )
    .await
}

#[derive(Debug, serde::Deserialize)]
struct EntityPasswordForm {
    password: String,
}

#[handler]
#[tracing::instrument]
async fn entity_password_verify(
    Authenticated(user_info): Authenticated,
    session: &poem::session::Session,
    Path(slug): Path<String>,
    Form(form): Form<EntityPasswordForm>,
) -> poem::Result<poem::Response> {
    // Load the entity
    let entity = match Entity::load(slug.clone()).await {
        Ok(e) => e,
        Err(_) => return Err(poem::Error::from_status(poem::http::StatusCode::NOT_FOUND)),
    };

    // Check if the password matches, using a constant-time comparison so the
    // response time does not leak how much of the password was correct.
    if let Some(correct_password) = entity.password() {
        use subtle::ConstantTimeEq;
        let matches: bool = form
            .password
            .as_bytes()
            .ct_eq(correct_password.as_bytes())
            .into();
        if matches {
            // Password correct - grant access in session and redirect to entity
            entity::grant_password_authorization(session, &slug);
            return Ok(Redirect::see_other(entity.canonical_path()).into_response());
        }
    }

    // Password incorrect - show error
    entity.password_prompt_response(user_info, true).await
}

#[derive(sailfish::TemplateOnce)]
#[template(path = "dev/demo_forum.stpl.html")]
struct DemoForum {
    head: <component::Head as Component>::Prepared,
    layout: <component::StandardLayout<DemoForumContent> as Component>::Prepared,
}

#[derive(sailfish::TemplateOnce)]
#[template(path = "dev/demo_forum_content.stpl.html")]
struct DemoForumContent {
    forum: <component::Forum as Component>::Prepared,
}

#[handler]
#[tracing::instrument]
async fn demo_forum(
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let layout = component::StandardLayout::new(
        "/demo-forum",
        DemoForumContent {
            forum: component::Forum::new("general")
                .prepare(user_info.clone())
                .await?,
        },
    )
    .prepare(user_info.clone())
    .await?;

    let prepared = DemoForum {
        head: component::Head::default().prepare(None).await?,
        layout,
    };
    prepared.into_response(None).await
}

fn user_facing() -> impl Endpoint {
    Route::new()
        .at("/", get(pages::index::index_page))
        .at("/index.html", get(pages::index::index_page))
        .at(
            "/signup",
            post(auth::signup_action)
                .with(auth::credential_rate_limit())
                .with(middleware::session_cookie::EnablePartitionedCookies),
        )
        .at(
            "/login",
            get(auth::login_signup_page).post(
                auth::login_action
                    .with(auth::credential_rate_limit())
                    .with(middleware::session_cookie::EnablePartitionedCookies),
            ),
        )
        .at(
            "/logout",
            get(auth::logout_action).with(middleware::session_cookie::EnablePartitionedCookies),
        )
        .at("/check-email", get(auth::check_email_page))
        .at(
            "/complete-signup",
            get(auth::complete_signup_action)
                .with(middleware::session_cookie::EnablePartitionedCookies),
        )
        .at(
            "/invitations/:invitation_id",
            get(invitation::accept_invitation),
        )
        .at("/add", get(add::add_chooser_page))
        .at(
            "/add-event",
            get(add::add_event_page).post(add::add_event_submit),
        )
        .at("/add-event/submitted", get(add::add_event_submitted_page))
        .at(
            "/add-event/parent-search",
            get(add::add_event_parent_search),
        )
        .at(
            "/add-event/parent-select",
            get(add::add_event_parent_select),
        )
        .at("/add-event/parent-clear", get(add::add_event_parent_clear))
        .at("/add/tag-search", get(add::add_tag_search))
        .at("/add/upload-image", post(add::add_upload_image))
        .at("/add/region-lookup", get(add::add_region_lookup))
        .at("/add/place-search", get(add::add_place_search))
        .at("/add/zip-lookup", get(add::add_zip_lookup))
        .nest("/staff", staff::routes())
        .at(
            "/add-project",
            get(add::add_project_page).post(add::add_project_submit),
        )
        .at(
            "/add-project/submitted",
            get(add::add_project_submitted_page),
        )
        .at("/demo-forum", get(demo_forum))
        .at("/go/*friendly_url", get(gourl::handle_go_url))
        .at("/-link/:link_id", get(external_link::handle_external_link))
        .at("/-clickthrough", post(external_link::record_clickthrough))
        .nest("/badges", badge::routes())
        .nest("/evolveme", evolveme::routes())
        .nest("/exchange", exchange::routes())
        .nest("/-search", search::routes())
        .nest("/-concierge", concierge::routes())
        .at("/finder", get(search::handle_finder))
        .at("/data", get(pages::data::data_page))
        .at("/-news/:id/content", get(news::news_content))
        .nest("/list", entity_list::page::routes())
        .nest("/communities", communities::routes())
        .nest("/dashboard/communities", communities::dashboard_routes())
        .nest("/dashboard", dashboard::routes())
        .at("/profile", get(profile::legacy_profile_redirect))
        .at("/profile/:user_id", get(profile::public_profile_page))
        .nest("/citizensciencemonth", csm::routes())
        .at("/-csm/acts-to-date", get(csm::acts_to_date_api))
        .at("/-csm/picks", get(csm::csm_picks))
        .at("/-csm/events", get(csm::csm_events))
        .at("/-csm/wall-of-fame", get(csm::csm_wall_of_fame))
        .at(
            "/-csm/autocomplete/projects",
            get(csm::csm_autocomplete_projects),
        )
        .at(
            "/-csm/autocomplete/countries",
            get(csm::csm_autocomplete_countries),
        )
        .at("/-zipcode/:code", get(zipcode::zipcode_lookup))
        .at(
            "/-entity-password/*slug",
            post(entity_password_verify).with(auth::credential_rate_limit()),
        )
        .at(
            "/-detail/:id/like",
            post(component::detail_page::toggle_like),
        )
        .at(
            "/-detail/:id/save",
            post(component::detail_page::save_project),
        )
        .at(
            "/-detail/:id/share",
            get(component::detail_page::share_modal),
        )
        .at(
            "/-detail/:id/message",
            get(component::detail_page::message_modal),
        )
        .at(
            "/-detail/:id/share-click/:platform",
            post(component::detail_page::record_share),
        )
        .at("/-detail/:id/map", get(component::detail_page::map_modal))
        .at(
            "/-detail/:id/review",
            post(component::detail_page::submit_review),
        )
        .at(
            "/-detail/:id/lists",
            get(component::detail_page::lists_modal),
        )
        .at(
            "/-detail/:id/add-to-list",
            post(component::detail_page::add_to_list),
        )
        .at(
            "/-detail/:id/join",
            post(component::detail_page::record_join),
        )
        .at(
            "/-detail/:id/edit-log",
            get(component::detail_page::edit_log),
        )
        .at(
            "/-detail/:id/field/:field_type",
            get(component::detail_page_edit::get_edit_form)
                .put(component::detail_page_edit::save_field),
        )
        .at(
            "/-detail/:id/field/:field_type/display",
            get(component::detail_page_edit::get_display),
        )
        .at(
            "/-detail/:id/instances",
            get(component::detail_page_edit::get_instances_display)
                .post(component::detail_page_edit::add_instance),
        )
        .at(
            "/-detail/:id/instances/:index",
            put(component::detail_page_edit::update_instance)
                .delete(component::detail_page_edit::delete_instance),
        )
        .at(
            "/-detail/:id/contacts",
            get(component::detail_page_edit::get_contacts_display)
                .post(component::detail_page_edit::add_contact),
        )
        .at(
            "/-detail/:id/contacts/:contact_id",
            put(component::detail_page_edit::update_contact)
                .delete(component::detail_page_edit::delete_contact),
        )
        .at(
            "/-detail/:id/contacts/:contact_id/edit",
            get(component::detail_page_edit::get_contact_edit_form),
        )
        .at(
            "/-detail/:id/managers",
            get(component::detail_page_edit::get_managers_display)
                .post(component::detail_page_edit::add_manager),
        )
        .at(
            "/-detail/:id/managers/:user_id",
            put(component::detail_page_edit::update_manager)
                .delete(component::detail_page_edit::delete_manager),
        )
        .at(
            "/-detail/:id/managers/:user_id/edit",
            get(component::detail_page_edit::get_manager_edit_form),
        )
        .at(
            "/-detail/:id/participant-files",
            get(component::detail_page_edit::get_participant_files_display)
                .post(component::detail_page_edit::add_participant_file),
        )
        .at(
            "/-detail/:id/participant-files/:index",
            put(component::detail_page_edit::update_participant_file)
                .delete(component::detail_page_edit::delete_participant_file),
        )
        .at("/training/foundations", get(training::foundations::page))
        .at(
            "/training/foundations/-progress",
            get(training::foundations::progress),
        )
        .at("/training/data-ethics", get(training::dataethics::page))
        .at(
            "/training/data-ethics/-progress",
            get(training::dataethics::progress),
        )
        .at(
            "/training/data-ethics-es",
            get(training::dataethics::page_es),
        )
        .at(
            "/training/data-ethics-es/-progress",
            get(training::dataethics::progress_es),
        )
        .at(
            "/training/data-ethics-pt",
            get(training::dataethics::page_pt),
        )
        .at(
            "/training/data-ethics-pt/-progress",
            get(training::dataethics::progress_pt),
        )
        .at("/training/data-literacy", get(training::dataliteracy::page))
        .at(
            "/training/data-literacy/-progress",
            get(training::dataliteracy::progress),
        )
        .at("/training/higher-ed", get(training::highered::page))
        .at(
            "/training/higher-ed/-progress",
            get(training::highered::progress),
        )
        .at(
            "/training/higher-ed/explore",
            get(training::highered::explore),
        )
        .at("/training/libraries-hubs", get(training::libraries::page))
        .at(
            "/training/libraries-hubs/-progress",
            get(training::libraries::progress),
        )
        .at("/training/k12", get(training::k12::page))
        .at("/training/k12/-progress", get(training::k12::progress))
        .at(
            "/l/library-network-signup",
            get(library_signup::index).post(library_signup::submit),
        )
        .at(
            "/l/acts-of-science-connected-apply",
            post(acts_connected::submit),
        )
        .at("/training/champion", get(training::champion::page))
        .at(
            "/training/champion/-progress",
            get(training::champion::progress),
        )
        // Paths and their history: see the `ambassador` module docs.
        .at(
            "/ambassador-apply",
            get(ambassador::apply::index).post(ambassador::apply::submit),
        )
        .at("/ambassador-apply/draft", post(ambassador::apply::draft))
        .at(ambassador::DIRECTORY_PATH, get(ambassador::directory::directory))
        .at(
            format!("{}/:id", ambassador::DIRECTORY_PATH),
            get(ambassador::directory::profile_page),
        )
        .at(
            format!("{}/:id/contact", ambassador::DIRECTORY_PATH),
            post(ambassador::directory::contact),
        )
        .at(
            "/ambassador/:id",
            get(ambassador::directory::legacy_profile_redirect),
        )
        .at(
            "/girlscouts-action",
            get(girlscouts::index).post(girlscouts::submit),
        )
        .at("/projects/*slug", get(project_page))
        .at("/events/*slug", get(event_page))
        .at("/*slug", get(entity_page))
        .with(middleware::analytics::AnalyticsMiddleware)
}

fn dynamic() -> impl Endpoint {
    // IMPORTANT: When adding project/event creation, editing, or page editor management endpoints,
    // remember to call the appropriate page_manager sync functions:
    // - page_manager::after_project_added() when projects are created
    // - page_manager::after_event_added() when events are created
    // - page_manager::after_project_editor_added/removed() when editors are modified
    // - page_manager::after_event_editor_added/removed() when editors are modified
    // This ensures user page manager status stays synchronized with the user service.

    Route::new()
        // Orrery's path for this, which the two ported resource pages fetch
        // verbatim from their own copy of the gate script. It reads the
        // logged-in user through `Authenticated`, so it belongs in this tree:
        // that extractor panics where AuthMiddleware is not installed.
        .at("/-api/me/program-status", get(api::me::program_status))
        .nest("/-dev", dev::routes())
        .nest(
            "/-admin",
            admin::routes().with(middleware::error_pages::ErrorPages),
        )
        .nest("/-channel", channel::routes())
        .nest("/-ui", ui::routes())
        .at(
            "/-certificate/:certificate_id",
            get(certificates::download_certificate),
        )
        .nest("/-upload", upload::authenticated_routes())
        .nest("/-forum", component::forum::forum_routes())
        .nest("/-webauthn", auth::webauthn_routes())
        .nest("/-embed", embed::routes())
        .nest_no_strip("/", user_facing())
        .with(middleware::error_pages::ErrorPages)
        // Wraps the whole dynamic tree rather than just the user-facing
        // branch, so that `?lang=` means the same thing everywhere it can be
        // written. Applied inside the session middleware, because a language
        // chosen once has to be remembered for the requests that follow.
        .with(middleware::language::LanguageMiddleware)
        .with(middleware::authorization::AuthMiddleware)
        .with(server_session())
        .with(middleware::session_cookie::SessionCookieMiddleware)
        .with(SetHeader::new().overriding("Cache-Control", "no-store"))
        .with(NormalizePath::new(TrailingSlash::Trim))
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    // might want a 'vDuration, intervalne option for setting the it up with pretty and max level TRACE
    if cfg!(debug_assertions) {
        let subscriber = tracing_subscriber::FmtSubscriber::builder()
            .pretty()
            .with_max_level(Level::INFO)
            .finish();

        tracing::subscriber::set_global_default(subscriber)
            .expect("setting default tracing subscriber failed");
    } else {
        let subscriber = tracing_subscriber::FmtSubscriber::builder()
            .with_max_level(Level::WARN)
            .finish();

        tracing::subscriber::set_global_default(subscriber)
            .expect("setting default tracing subscriber failed");
    }

    common::shared::init("frontend").await;

    sqlx::migrate!("./migrations").run(db()).await?;

    let routes = Route::new()
        .at("/favicon.ico", StaticFile::new("favicon.ico"))
        .at(
            "/apple-touch-icon.png",
            StaticFile::new("apple-touch-icon.png"),
        )
        .nest("/-static", StaticFiles::new())
        .nest(upload::storage::UNTRACKED_PATH, upload::anonymous_routes())
        .nest("/messaging/emailer", emailer::routes())
        .nest("/api", api::routes().with(api::middleware::api_cors()))
        .at("/-mcp", post(mcp::mcp_handler))
        .at("/-content-mcp", post(mcp::content::content_mcp_handler))
        .nest(
            "/oauth2",
            // The authorize endpoint reads the logged-in user via the session and
            // the `Authenticated` extractor, so the OAuth2 routes need the same
            // session + auth middleware as the main route tree. Without this the
            // `Authenticated` extractor panics.
            oauth2::oauth2_routes()
                .with(middleware::authorization::AuthMiddleware)
                .with(server_session()),
        )
        .nest("/.well-known", oauth2::well_known_routes())
        .nest_no_strip("/", dynamic())
        .with_if(
            cfg!(not(debug_assertions)),
            SetHeader::new().overriding(
                "Strict-Transport-Security",
                "max-age=31536000; includeSubDomains",
            ),
        )
        .with_if(
            cfg!(not(debug_assertions)),
            ForceHttps::new().filter(|req| {
                // Only redirect if the original request was HTTP
                // Check X-Forwarded-Proto header set by reverse proxy
                match req.headers().get("X-Forwarded-Proto") {
                    Some(proto) => proto != "https",
                    None => req.uri().scheme_str() != Some("https"),
                }
            }),
        );

    let _project_all_updates = tokio::spawn(ProjectAll::peering_task());
    let _project_field_updates = tokio::spawn(ProjectField::peering_task());
    let _event_all_updates = tokio::spawn(EventAll::peering_task());
    let _event_field_updates = tokio::spawn(EventField::peering_task());
    let _exchange_updates = tokio::spawn(exchange::Exchange::peering_task());
    let _oauth2_client_updates = tokio::spawn(oauth2::types::Client::peering_task());
    let _oauth2_authz_code_updates = tokio::spawn(oauth2::types::AuthorizationCode::peering_task());
    let _oauth2_access_token_updates = tokio::spawn(oauth2::types::AccessToken::peering_task());
    let _oauth2_refresh_token_updates = tokio::spawn(oauth2::types::RefreshToken::peering_task());
    let _wiki_page_updates = tokio::spawn(wiki::WikiPage::peering_task());
    let _news_item_updates = tokio::spawn(news::NewsItem::peering_task());
    let _staff_pick_updates = tokio::spawn(staff_pick::StaffPick::peering_task());
    let _translation_updates = tokio::spawn(i18n::Phrase::peering_task());
    let _webinar_updates = tokio::spawn(webinar::Webinar::peering_task());
    concierge::spawn_background_tasks();
    let _pagebuilder_component_updates =
        tokio::spawn(pagebuilder::PageBuilderComponent::peering_task());
    let _pagebuilder_page_updates = tokio::spawn(pagebuilder::PageBuilderPage::peering_task());
    let _project_reference_updates =
        tokio::spawn(project::reference::ProjectReference::peering_task());
    let _project_investigator_updates =
        tokio::spawn(project::investigator::ProjectInvestigator::peering_task());
    let _entity_list_updates = tokio::spawn(entity_list::EntityList::peering_task());
    let _entity_list_entry_updates = tokio::spawn(entity_list::EntityListEntry::peering_task());
    let _entity_list_viewer_updates = tokio::spawn(entity_list::EntityListViewer::peering_task());
    let _go_url_updates = tokio::spawn(gourl::GoUrl::peering_task());
    let _schedule_updates = tokio::spawn(schedule::Schedule::peering_task());
    let _entity_updates = tokio::spawn(entity::Entity::peering_task());
    let _redirect_updates = tokio::spawn(misc::Redirect::peering_task());
    let _html_updates = tokio::spawn(misc::Html::peering_task());
    let _home_settings_updates = tokio::spawn(home::HomeSettings::peering_task());
    let _pagecontent_updates = tokio::spawn(pagecontent::PageContent::peering_task());
    let _mcp_token_updates = tokio::spawn(mcp_token::McpToken::peering_task());
    let _foundations_content_updates =
        tokio::spawn(training::foundations::FoundationsContent::peering_task());
    let _dataethics_content_updates =
        tokio::spawn(training::dataethics::DataEthicsContent::peering_task());
    let _dataliteracy_content_updates =
        tokio::spawn(training::dataliteracy::DataLiteracyContent::peering_task());
    let _highered_content_updates =
        tokio::spawn(training::highered::HigherEdContent::peering_task());
    let _libraries_content_updates =
        tokio::spawn(training::libraries::LibrariesContent::peering_task());
    let _champion_content_updates =
        tokio::spawn(training::champion::ChampionContent::peering_task());
    let _archive_entry_updates = tokio::spawn(admin::archive::ArchiveEntry::peering_task());
    let _file_metadata_updates = tokio::spawn(upload::storage::FileMetadata::peering_task());

    // Loads the translation catalog into memory and starts the writer that
    // records newly displayed phrases. Awaited rather than spawned so the
    // first page served is already translated.
    if let Err(err) = i18n::start().await {
        tracing::error!("unable to start translation: {err}");
    }

    let _synchronize_projects = tokio::spawn(sync::synchronize_projects());
    let _synchronize_events = tokio::spawn(sync::synchronize_events());
    let _synchronize_references = tokio::spawn(sync::synchronize_references());
    let _synchronize_investigators = tokio::spawn(sync::synchronize_investigators());
    let _synchronize_entity_lists = tokio::spawn(sync::synchronize_entity_lists());
    let _synchronize_go_urls = tokio::spawn(sync::synchronize_go_urls());
    let _synchronize_regions = tokio::spawn(sync::synchronize_regions());
    let _update_expired_statuses = tokio::spawn(sync::update_expired_statuses());
    let _periodic_maintenance = tokio::spawn(periodic_maintenance());

    // Start NATS service
    let nats = common::shared::nats();
    let service = nats
        .service_builder()
        .start("frontend", env!("CARGO_PKG_VERSION"))
        .await
        .expect("Unable to start frontend NATS service");

    let health_group = service.group("frontend.health");
    tokio::spawn(async move {
        use common::handler;
        let mut tasks = tokio::task::JoinSet::new();
        tasks.spawn(handler!(health_group, "check", health::health_check));
        tasks.join_all().await;
    });

    // Entity resolution for other services (e.g. forum-service classifying a
    // message recipient/participant ULID). The Service API queue-groups this
    // across all frontend instances, so a request is answered by exactly one.
    let entity_group = service.group("frontend.entity");
    tokio::spawn(async move {
        use common::handler;
        let mut tasks = tokio::task::JoinSet::new();
        tasks.spawn(handler!(
            entity_group,
            "summary_by_id",
            dashboard::entity_summary::summary_by_id
        ));
        tasks.spawn(handler!(
            entity_group,
            "summary_by_slug",
            dashboard::entity_summary::summary_by_slug
        ));
        tasks.join_all().await;
    });

    println!("Listening on {}", env!("LISTEN_ON"));
    println!(
        "Frontend NATS service started: frontend.health.check, frontend.entity.summary_by_id, frontend.entity.summary_by_slug"
    );
    // The id peer messages are tagged with, and the one the staff query
    // tool reports as having answered a query against this instance's
    // copy of the frontend database.
    println!("Frontend instance id: {}", *peering::SERVER_ID_STR);

    // Set up graceful shutdown
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let shutdown_tx = std::sync::Arc::new(std::sync::Mutex::new(Some(shutdown_tx)));

    // Handle SIGINT and SIGTERM for graceful shutdown
    use tokio::signal::unix::{SignalKind, signal};

    let shutdown_tx_sigint = shutdown_tx.clone();
    let mut sigint = signal(SignalKind::interrupt())?;
    tokio::spawn(async move {
        sigint.recv().await;
        tracing::info!("Received SIGINT, initiating graceful shutdown...");
        if let Some(tx) = shutdown_tx_sigint.lock().unwrap().take() {
            let _ = tx.send(());
        }
    });

    let shutdown_tx_sigterm = shutdown_tx.clone();
    let mut sigterm = signal(SignalKind::terminate())?;
    tokio::spawn(async move {
        sigterm.recv().await;
        tracing::info!("Received SIGTERM, initiating graceful shutdown...");
        if let Some(tx) = shutdown_tx_sigterm.lock().unwrap().take() {
            let _ = tx.send(());
        }
    });

    // Run server with graceful shutdown
    let server = Server::new(TcpListener::bind(env!("LISTEN_ON")));
    let server_task = tokio::spawn(server.run(routes));

    tokio::select! {
        result = server_task => {
            match result {
                Ok(Ok(())) => tracing::info!("Server completed successfully"),
                Ok(Err(e)) => tracing::error!("Server error: {}", e),
                Err(e) => tracing::error!("Server task panicked: {}", e),
            }
        }
        _ = &mut shutdown_rx => {
            tracing::info!("Shutdown signal received, stopping server...");
        }
    }

    tracing::info!("Frontend service shutting down...");
    Ok(())
}

/// Background task for periodic maintenance operations
async fn periodic_maintenance() {
    use tokio::time::{Duration, interval};

    let mut interval = interval(Duration::from_secs(300)); // Run every 5 minutes

    loop {
        interval.tick().await;

        // Clean expired entries from WordPress cache
        if let Ok(mut cache) = wordpress::cache().lock() {
            cache.clean();
            tracing::debug!("WordPress cache cleaned");
        }

        // Add other periodic maintenance tasks here as needed
    }
}

mod sync {
    use std::time::Duration;

    use common::{insensitivestring::InsensitiveString, misc};
    use jiff::Timestamp;
    use tokio::time::sleep;
    use ulid::Ulid;

    use crate::{
        contact::{Contact, ContactRole},
        entity::Entity,
        event::{EventInstance, all::EventAll},
        page_manager,
        peering::LoadStorePeered,
        project::{
            ProjectParticipationDegree, ProjectPromotionTime, ProjectStatus, all::ProjectAll,
        },
    };

    trait EmptyAsNone: Sized {
        fn empty_as_none(self) -> Option<Self>;
    }

    impl EmptyAsNone for String {
        fn empty_as_none(self) -> Option<Self> {
            if self.is_empty() { None } else { Some(self) }
        }
    }

    impl EmptyAsNone for InsensitiveString {
        fn empty_as_none(self) -> Option<Self> {
            if self.is_empty() { None } else { Some(self) }
        }
    }

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
        last_editor: Option<String>,
        updated_staff: Option<Timestamp>,
        last_editor_staff: Option<String>,
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
        training_materials: Option<String>,
        classroom_materials: Option<String>,
        consent_statement: Option<String>,
        consent_statement_date: Option<Timestamp>,
        participation_tools: Option<bool>, // deprecated, maybe don't implement
        participation_prompt: Option<String>, // deprecated, maybe don't implement
        participation_confirmation_message: Option<String>, // deprecated, maybe don't implement
        participation_notification_email: Option<String>, // deprecated, maybe don't implement
        participation_usage: Option<String>, // deprecated, maybe don't implement
        // section Relations
        part_of: Option<String>,
        associated_projects: Option<String>, // multiple, joined by ,
        associated_tools: Option<String>,    // multiple, joined by , but also deprecated
        // section Participants Need
        total_expense: Option<f32>,
        materials: Option<String>,
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

    #[tracing::instrument]
    pub async fn synchronize_projects() -> Result<(), anyhow::Error> {
        loop {
            let last_update = misc::get("project-sync-bookmark-update")
                .await
                .expect("Retrieving sync bookmark failed")
                .unwrap_or_else(|| String::from("2000-01-01T00:00:00.0Z"));

            let last_id = misc::get("project-sync-bookmark-id")
                .await
                .expect("Retrieving sync bookmark failed.")
                .unwrap_or(0);

            let client = reqwest::Client::new();

            tracing::info!("Retrieving projects since {} {}...", &last_update, last_id);

            let projects: Vec<ProjectData> = match client
                .get("https://scistarter.org/staff/sync/projects")
                .bearer_auth(env!("SYNC_TOKEN"))
                .query(&[("since", &last_update), ("last_id", &last_id.to_string())])
                .send()
                .await
            {
                Ok(response) => match response.error_for_status() {
                    Ok(resp) => {
                        let response_text = match resp.text().await {
                            Ok(text) => text,
                            Err(e) => {
                                eprintln!("Failed to read response body: {}", e);
                                sleep(Duration::from_secs(60)).await;
                                continue;
                            }
                        };

                        match serde_json::from_str(&response_text) {
                            Ok(projects) => projects,
                            Err(e) => {
                                eprintln!("JSON parsing error: {}", e);
                                eprintln!("Response body: {}", response_text);
                                sleep(Duration::from_secs(60)).await;
                                continue;
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("HTTP error: {}", e);
                        sleep(Duration::from_secs(60)).await;
                        continue;
                    }
                },
                Err(e) => {
                    eprintln!("Request error: {}", e);
                    sleep(Duration::from_secs(60)).await;
                    continue;
                }
            };

            let mut bookmark = (last_update.clone(), last_id);

            let got_projects = projects.len();
            tracing::info!("Synchronization received {} projects", got_projects);

            for data in projects {
                let updated = data.updated.to_string(); //.strftime("%Y-%m-%d %H:%M:%S+00").to_string();
                let legacy_id = data.legacy_v2_project_id;

                let mut proj = ProjectAll::default();

                proj.project_id = match data.id.parse() {
                    Ok(ulid) => ulid,
                    Err(err) => {
                        tracing::error!("Error parsing ULID {:?}: {}", data.id, err);
                        continue;
                    }
                };

                proj.legacy_id = data.legacy_v1_project_id.map(|x| x as i64);
                proj.legacy_v1_project_id = data.legacy_v1_project_id.map(|x| x as i64);
                proj.legacy_v2_project_id = Some(data.legacy_v2_project_id as i64);
                proj.foreign_id = data.foreign_id.empty_as_none();
                proj.duplicate = data.duplicate;
                proj.authoritative = data.authoritative;
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
                        tracing::error!(
                            "Unknown project status `{}` in {}, setting to Hidden",
                            s,
                            &data.name,
                        );
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
                        tracing::error!(
                            "Unknown project protion time `{}` in {}, setting to PromoteAtAnyTime ",
                            s,
                            &data.name,
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
                    .and_then(|s| s.empty_as_none())
                    .and_then(|s| s.parse().ok());
                proj.project_last_edited = Some(data.updated.into());
                proj.scistarter_last_editor = data
                    .last_editor_staff
                    .and_then(|s| s.empty_as_none())
                    .and_then(|s| s.parse().ok());
                proj.scistarter_last_edited = data.updated_staff.map(|ts| ts.into());
                proj.draft = data.draft;
                proj.name = data.name;
                proj.tags = data
                    .tags
                    .split("||")
                    .filter_map(|s| {
                        InsensitiveString::from(s.trim_matches('|').trim()).empty_as_none()
                    })
                    .collect();
                proj.participation_degree = match data.participation_degree.as_str() {
                    "Collegial" => ProjectParticipationDegree::Collegial,
                    "Co-Created" => ProjectParticipationDegree::CoCreated,
                    "Collaborative" => ProjectParticipationDegree::Collaborative,
                    "Contributory" => ProjectParticipationDegree::Contributory,
                    "Contractual" => ProjectParticipationDegree::Contractual,
                    s => {
                        tracing::error!(
                            "Unknown participation degree `{}` in {}, defaulting to Collegial",
                            s,
                            &proj.name
                        );
                        ProjectParticipationDegree::Collegial
                    }
                };
                proj.participant_instructions = data.participant_instructions;
                proj.participant_instructions_email = data.participant_instructions_email;
                proj.participant_files = {
                    let raw = data.participant_files.trim();
                    if raw.starts_with('[') {
                        // Current format: JSON array of {name, url} objects.
                        serde_json::from_str::<Vec<crate::participant_file::ParticipantFile>>(raw)
                            .unwrap_or_default()
                    } else {
                        // Legacy/resilience: a "||"-joined list of bare URLs.
                        raw.split("||")
                            .filter_map(|s| s.trim_matches('|').trim().to_string().empty_as_none())
                            .map(|url| crate::participant_file::ParticipantFile {
                                name: String::new(),
                                url,
                            })
                            .collect::<Vec<_>>()
                    }
                    .into_boxed_slice()
                };
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
                proj.training_materials = data.training_materials.unwrap_or_default();
                proj.classroom_materials = data.classroom_materials.unwrap_or_default();
                proj.consent_statement = data.consent_statement.unwrap_or_default();
                proj.consent_statement_date = data.consent_statement_date.map(|ts| ts.into());
                proj.participation_tools = data.participation_tools.unwrap_or_default();
                proj.participation_prompt = data.participation_prompt.unwrap_or_default();
                proj.participation_confirmation_message =
                    data.participation_confirmation_message.unwrap_or_default();
                proj.participation_notification_email =
                    data.participation_notification_email.unwrap_or_default();
                proj.participation_usage = data.participation_usage.unwrap_or_default();
                proj.part_of = data.part_of.and_then(|s| s.parse().ok());
                proj.associated_projects = data
                    .associated_projects
                    .map(|s| {
                        s.split(',')
                            .filter_map(|s| s.parse().ok())
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default()
                    .into_boxed_slice();
                proj.total_expense = data.total_expense;
                proj.materials = data.materials.unwrap_or_default();
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

                // Mark synced projects as reviewed (they come from the main database)
                proj.reviewed = true;

                // Use Legacy source so higher-priority sources aren't overwritten
                proj.update_source = Some(crate::project::field_source::FieldSource::Legacy);

                if let Err(err) = proj.store().await {
                    tracing::error!("Error saving {}: {}", &proj.name, err);
                    continue;
                }

                // Update page manager status for users associated with this project
                let creator_id = if proj.page_creator_id != Ulid::nil() {
                    Some(proj.page_creator_id)
                } else {
                    None
                };
                let editor_ids: Vec<Ulid> = proj.page_editor_ids.iter().copied().collect();
                if let Err(err) = page_manager::after_project_added(creator_id, &editor_ids).await {
                    tracing::error!(
                        "Error updating page manager status for project {}: {}",
                        &proj.name,
                        err
                    );
                    // Continue processing - don't fail the sync for this
                }

                let entity = Entity::new_with_publish(
                    data.slug,
                    crate::entity::EntityKind::Project,
                    crate::entity::EntityTarget::Ulid(proj.project_id),
                    data.publish,
                );

                if let Err(err) = entity.store().await {
                    tracing::error!(
                        "Error creating entity for {} ({}): {}",
                        &proj.name,
                        &entity.slug(),
                        err
                    );
                    continue;
                }

                let new_bookmark = (updated, legacy_id);
                if new_bookmark > bookmark {
                    bookmark = new_bookmark;
                }
            }

            misc::set("project-sync-bookmark-update", bookmark.0)
                .await
                .expect("Saving sync bookmark failed");

            misc::set("project-sync-bookmark-id", bookmark.1)
                .await
                .expect("Saving sync bookmark failed");

            if got_projects == 50 {
                sleep(Duration::from_secs(60)).await;
            } else {
                sleep(Duration::from_secs(60 * 60)).await;
            }
        }
    }

    #[derive(Default, Debug, serde::Deserialize)]
    #[serde(default)]
    struct EventData {
        legacy_id: i32,
        event_id: String, // Ulid,
        slug: String,
        created: Timestamp,
        publish: bool,
        published_at: Option<Timestamp>,
        flagged: bool,
        admin_notes: String,
        welcomed: bool,
        page_creator_id: String,           // Ulid,        // NB was "owner"
        page_editor_ids: String,           // Box<[Ulid]>, // NB was "managers"
        event_last_editor: Option<String>, // Option<Ulid>,
        event_last_edited: Option<Timestamp>, // Option<TimeString>,
        scistarter_last_editor: Option<String>, // Option<Ulid>,
        scistarter_last_edited: Option<Timestamp>, // Option<TimeString>,
        name: String,
        tags: String, // HashSet<InsensitiveString>,
        meta_description: String,
        description: String,
        // contacts: Box<[Contact]>,
        manager_name: String,
        manager_affiliation: String,
        manager_email: String,
        manager_phone: String,
        manager_address: String,
        contact_name: String,
        contact_affiliation: String,
        contact_email: String,
        contact_phone: String,
        contact_address: String,
        presenter: String,
        url: String,
        image: String,
        image_credit: String,
        how_to_join: String,
        part_of: Option<String>, // Option<Ulid>,
        location_type: String,
        entirely_online: bool,
        location_name: String,
        location_description: String,
        address: String,
        city: String,
        zip: String,
        state: String,
        country: String,
        regions: Option<String>,
        point: Option<String>,
        event_type: String,                  // EventType,
        ticketing_begins: Option<Timestamp>, // TimeString,
        outcomes_label: String,
        outcomes_link: String,
        circuit_id: String,
        // instances: Box<[EventInstance]>,
        begin0: Option<Timestamp>,
        end0: Option<Timestamp>,
        begin1: Option<Timestamp>,
        end1: Option<Timestamp>,
        begin2: Option<Timestamp>,
        end2: Option<Timestamp>,
        begin3: Option<Timestamp>,
        end3: Option<Timestamp>,
        begin4: Option<Timestamp>,
        end4: Option<Timestamp>,
        begin5: Option<Timestamp>,
        end5: Option<Timestamp>,
        begin6: Option<Timestamp>,
        end6: Option<Timestamp>,
        begin7: Option<Timestamp>,
        end7: Option<Timestamp>,
        begin8: Option<Timestamp>,
        end8: Option<Timestamp>,
        begin9: Option<Timestamp>,
        end9: Option<Timestamp>,
        begin10: Option<Timestamp>,
        end10: Option<Timestamp>,
        begin11: Option<Timestamp>,
        end11: Option<Timestamp>,
        begin12: Option<Timestamp>,
        end12: Option<Timestamp>,
        begin13: Option<Timestamp>,
        end13: Option<Timestamp>,
        begin14: Option<Timestamp>,
        end14: Option<Timestamp>,
        begin15: Option<Timestamp>,
        end15: Option<Timestamp>,
        begin16: Option<Timestamp>,
        end16: Option<Timestamp>,
        begin17: Option<Timestamp>,
        end17: Option<Timestamp>,
        begin18: Option<Timestamp>,
        end18: Option<Timestamp>,
        begin19: Option<Timestamp>,
        end19: Option<Timestamp>,
    }

    #[tracing::instrument]
    pub async fn synchronize_events() -> Result<(), anyhow::Error> {
        loop {
            let last_update = misc::get("event-sync-bookmark-update")
                .await
                .expect("Retrieving sync bookmark failed")
                .unwrap_or_else(|| String::from("2000-01-01T00:00:00.0Z"));

            let last_id = misc::get("event-sync-bookmark-id")
                .await
                .expect("Retrieving sync bookmark failed.")
                .unwrap_or(0);

            let client = reqwest::Client::new();

            tracing::info!("Retrieving events since {} {}...", &last_update, last_id);

            let events: Vec<EventData> = match client
                .get("https://scistarter.org/staff/sync/events")
                .bearer_auth(env!("SYNC_TOKEN"))
                .query(&[("since", &last_update), ("last_id", &last_id.to_string())])
                .send()
                .await
            {
                Ok(response) => match response.error_for_status() {
                    Ok(resp) => {
                        let response_text = match resp.text().await {
                            Ok(text) => text,
                            Err(e) => {
                                eprintln!("Failed to read response body: {}", e);
                                sleep(Duration::from_secs(60)).await;
                                continue;
                            }
                        };

                        match serde_json::from_str(&response_text) {
                            Ok(events) => events,
                            Err(e) => {
                                eprintln!("JSON parsing error: {}", e);
                                eprintln!("Response body: {}", response_text);
                                sleep(Duration::from_secs(60)).await;
                                continue;
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("HTTP error: {}", e);
                        sleep(Duration::from_secs(60)).await;
                        continue;
                    }
                },
                Err(e) => {
                    eprintln!("Request error: {}", e);
                    sleep(Duration::from_secs(60)).await;
                    continue;
                }
            };

            let mut bookmark = (last_update.clone(), last_id);

            let got_events = events.len();
            tracing::info!("Synchronization received {} events", got_events);

            for data in events {
                let updated = data
                    .event_last_edited
                    .unwrap_or_else(|| Timestamp::now())
                    .to_string();

                let legacy_id = data.legacy_id;

                let mut evt = EventAll::default();

                evt.event_id = match data.event_id.parse() {
                    Ok(ulid) => ulid,
                    Err(err) => {
                        tracing::error!("Error parsing ULID {:?}: {}", data.event_id, err);
                        continue;
                    }
                };

                evt.created = data.created.into();
                evt.flagged = data.flagged;
                evt.admin_notes = data.admin_notes;
                evt.welcomed = data.welcomed;

                evt.page_creator_id = match data.page_creator_id.parse() {
                    Ok(ulid) => ulid,
                    Err(err) => {
                        tracing::error!("Error parsing ULID {:?}: {}", data.event_id, err);
                        continue;
                    }
                };

                evt.page_editor_ids = data
                    .page_editor_ids
                    .split(",")
                    .flat_map(|s| s.trim().parse().ok())
                    .collect::<Vec<_>>()
                    .into_boxed_slice();

                evt.event_last_editor = data
                    .event_last_editor
                    .and_then(|s| s.empty_as_none())
                    .and_then(|s| s.parse().ok());

                evt.event_last_edited = data.event_last_edited.map(|ts| ts.into());

                evt.scistarter_last_editor = data
                    .scistarter_last_editor
                    .and_then(|s| s.empty_as_none())
                    .and_then(|s| s.parse().ok());

                evt.scistarter_last_edited = data.scistarter_last_edited.map(|ts| ts.into());

                evt.name = data.name;

                evt.tags = data
                    .tags
                    .trim_matches('|')
                    .split("||")
                    .map(|s| s.into())
                    .collect();

                evt.description = data.description;

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

                if !data.manager_email.is_empty() {
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

                evt.contacts = contacts.into_boxed_slice();

                evt.presenter = data.presenter;
                evt.url = data.url;
                evt.image = data.image;
                evt.image_credit = data.image_credit;
                evt.how_to_join = data.how_to_join;
                evt.part_of = data.part_of.and_then(|s| s.parse().ok());
                evt.location_type = data.location_type;
                evt.entirely_online = data.entirely_online;
                evt.location_name = data.location_name;
                evt.location_description = data.location_description;
                evt.address = data.address;
                evt.city = data.city;
                evt.zip = data.zip;
                evt.state = data.state;
                evt.country = data.country;
                evt.regions = data.regions;
                evt.point = data.point;
                evt.event_type = data.event_type.parse().unwrap_or_default();
                evt.ticketing_begins = data.ticketing_begins.map(|ts| ts.into());
                evt.outcomes_label = data.outcomes_label.empty_as_none();
                evt.outcomes_link = data.outcomes_link.empty_as_none();
                evt.circuit_id = data.circuit_id.empty_as_none();

                let mut instances = Vec::with_capacity(20);

                if let (Some(begin), Some(end)) = (data.begin0, data.end0) {
                    instances.push(EventInstance {
                        begin: begin.into(),
                        end: end.into(),
                    });
                }

                evt.instances = instances.into_boxed_slice();

                if let Err(err) = evt.store().await {
                    tracing::error!("Error saving {}: {}", &evt.name, err);
                    continue;
                }

                // Update page manager status for users associated with this event
                let creator_id = if evt.page_creator_id != Ulid::nil() {
                    Some(evt.page_creator_id)
                } else {
                    None
                };
                let editor_ids: Vec<Ulid> = evt.page_editor_ids.iter().copied().collect();
                if let Err(err) = page_manager::after_event_added(creator_id, &editor_ids).await {
                    tracing::error!(
                        "Error updating page manager status for event {}: {}",
                        &evt.name,
                        err
                    );
                    // Continue processing - don't fail the sync for this
                }

                let entity = Entity::new_with_publish(
                    data.slug,
                    crate::entity::EntityKind::Event,
                    crate::entity::EntityTarget::Ulid(evt.event_id),
                    data.publish,
                );

                if let Err(err) = entity.store().await {
                    tracing::error!(
                        "Error creating entity for {} ({}): {}",
                        &evt.name,
                        &entity.slug(),
                        err
                    );
                    continue;
                }

                let new_bookmark = (updated, legacy_id);
                if new_bookmark > bookmark {
                    bookmark = new_bookmark;
                }
            }

            misc::set("event-sync-bookmark-update", bookmark.0)
                .await
                .expect("Saving sync bookmark failed");

            misc::set("event-sync-bookmark-id", bookmark.1)
                .await
                .expect("Saving sync bookmark failed");

            if got_events == 50 {
                sleep(Duration::from_secs(60)).await;
            } else {
                sleep(Duration::from_secs(60 * 60)).await;
            }
        }
    }

    #[derive(Debug, serde::Deserialize)]
    struct ReferenceData {
        id: i32,
        entity_ulid: String,
        label: String,
        reference_type: String,
        url: String,
        citation: String,
        detailed_citation: String,
        updated: String,
    }

    #[tracing::instrument]
    pub async fn synchronize_references() -> Result<(), anyhow::Error> {
        use crate::project::reference::{ProjectReference, ReferenceType};

        loop {
            let last_update = misc::get("reference-sync-bookmark-update")
                .await
                .expect("Retrieving sync bookmark failed")
                .unwrap_or_else(|| String::from("2000-01-01T00:00:00.0Z"));

            let last_id = misc::get("reference-sync-bookmark-id")
                .await
                .expect("Retrieving sync bookmark failed.")
                .unwrap_or(0);

            let client = reqwest::Client::new();

            tracing::info!(
                "Retrieving references since {} {}...",
                &last_update,
                last_id
            );

            let references: Vec<ReferenceData> = match client
                .get("https://scistarter.org/staff/sync/references")
                .bearer_auth(env!("SYNC_TOKEN"))
                .query(&[("since", &last_update), ("last_id", &last_id.to_string())])
                .send()
                .await
            {
                Ok(response) => match response.error_for_status() {
                    Ok(resp) => {
                        let response_text = match resp.text().await {
                            Ok(text) => text,
                            Err(e) => {
                                eprintln!("Failed to read response body: {}", e);
                                sleep(Duration::from_secs(60)).await;
                                continue;
                            }
                        };

                        match serde_json::from_str(&response_text) {
                            Ok(refs) => refs,
                            Err(e) => {
                                eprintln!("JSON parsing error: {}", e);
                                eprintln!("Response body: {}", response_text);
                                sleep(Duration::from_secs(60)).await;
                                continue;
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("HTTP error: {}", e);
                        sleep(Duration::from_secs(60)).await;
                        continue;
                    }
                },
                Err(e) => {
                    eprintln!("Request error: {}", e);
                    sleep(Duration::from_secs(60)).await;
                    continue;
                }
            };

            let mut bookmark = (last_update.clone(), last_id);

            let got_references = references.len();
            tracing::info!("Synchronization received {} references", got_references);

            for data in references {
                let entity_id: Ulid = match data.entity_ulid.parse() {
                    Ok(ulid) => ulid,
                    Err(err) => {
                        tracing::error!(
                            "Error parsing entity ULID {:?}: {}",
                            data.entity_ulid,
                            err
                        );
                        continue;
                    }
                };

                let reference_type: ReferenceType = data.reference_type.parse().unwrap_or_default();

                let reference = ProjectReference {
                    reference_id: Ulid::new(),
                    entity_id,
                    label: data.label,
                    reference_type,
                    url: data.url,
                    citation: data.citation,
                    detailed_citation: data.detailed_citation,
                    sort_order: 0,
                };

                if let Err(err) = reference.store().await {
                    tracing::error!("Error saving reference: {}", err);
                    continue;
                }

                let new_bookmark = (data.updated.clone(), data.id);
                if new_bookmark > bookmark {
                    bookmark = new_bookmark;
                }
            }

            misc::set("reference-sync-bookmark-update", bookmark.0)
                .await
                .expect("Saving sync bookmark failed");

            misc::set("reference-sync-bookmark-id", bookmark.1)
                .await
                .expect("Saving sync bookmark failed");

            if got_references == 500 {
                sleep(Duration::from_secs(60)).await;
            } else {
                sleep(Duration::from_secs(60 * 60)).await;
            }
        }
    }

    #[derive(Debug, serde::Deserialize)]
    struct InvestigatorData {
        id: i32,
        entity_ulid: String,
        name: String,
        affiliation: String,
        updated: String,
    }

    #[tracing::instrument]
    pub async fn synchronize_investigators() -> Result<(), anyhow::Error> {
        use crate::project::investigator::ProjectInvestigator;

        loop {
            let last_update = misc::get("investigator-sync-bookmark-update")
                .await
                .expect("Retrieving sync bookmark failed")
                .unwrap_or_else(|| String::from("2000-01-01T00:00:00.0Z"));

            let last_id = misc::get("investigator-sync-bookmark-id")
                .await
                .expect("Retrieving sync bookmark failed.")
                .unwrap_or(0);

            let client = reqwest::Client::new();

            tracing::info!(
                "Retrieving investigators since {} {}...",
                &last_update,
                last_id
            );

            let investigators: Vec<InvestigatorData> = match client
                .get("https://scistarter.org/staff/sync/investigators")
                .bearer_auth(env!("SYNC_TOKEN"))
                .query(&[("since", &last_update), ("last_id", &last_id.to_string())])
                .send()
                .await
            {
                Ok(response) => match response.error_for_status() {
                    Ok(resp) => {
                        let response_text = match resp.text().await {
                            Ok(text) => text,
                            Err(e) => {
                                eprintln!("Failed to read response body: {}", e);
                                sleep(Duration::from_secs(60)).await;
                                continue;
                            }
                        };

                        match serde_json::from_str(&response_text) {
                            Ok(invs) => invs,
                            Err(e) => {
                                eprintln!("JSON parsing error: {}", e);
                                eprintln!("Response body: {}", response_text);
                                sleep(Duration::from_secs(60)).await;
                                continue;
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("HTTP error: {}", e);
                        sleep(Duration::from_secs(60)).await;
                        continue;
                    }
                },
                Err(e) => {
                    eprintln!("Request error: {}", e);
                    sleep(Duration::from_secs(60)).await;
                    continue;
                }
            };

            let mut bookmark = (last_update.clone(), last_id);

            let got_investigators = investigators.len();
            tracing::info!(
                "Synchronization received {} investigators",
                got_investigators
            );

            for data in investigators {
                let entity_id: Ulid = match data.entity_ulid.parse() {
                    Ok(ulid) => ulid,
                    Err(err) => {
                        tracing::error!(
                            "Error parsing entity ULID {:?}: {}",
                            data.entity_ulid,
                            err
                        );
                        continue;
                    }
                };

                let investigator = ProjectInvestigator {
                    investigator_id: Ulid::new(),
                    entity_id,
                    name: data.name,
                    affiliation: data.affiliation,
                    sort_order: 0,
                };

                if let Err(err) = investigator.store().await {
                    tracing::error!("Error saving investigator: {}", err);
                    continue;
                }

                let new_bookmark = (data.updated.clone(), data.id);
                if new_bookmark > bookmark {
                    bookmark = new_bookmark;
                }
            }

            misc::set("investigator-sync-bookmark-update", bookmark.0)
                .await
                .expect("Saving sync bookmark failed");

            misc::set("investigator-sync-bookmark-id", bookmark.1)
                .await
                .expect("Saving sync bookmark failed");

            if got_investigators == 500 {
                sleep(Duration::from_secs(60)).await;
            } else {
                sleep(Duration::from_secs(60 * 60)).await;
            }
        }
    }

    #[derive(Debug, serde::Deserialize)]
    struct EntityListEntryData {
        id: i32,
        #[allow(dead_code)]
        list_orrery_id: i32,
        priority: i32,
        when: String,
        entity_slug: String,
        note: String,
        link: String,
    }

    #[derive(Debug, serde::Deserialize)]
    struct EntityListData {
        id: i32,
        created: Option<String>,
        private: bool,
        owner_ulid: String,
        delegate_ulid: Option<String>,
        name: String,
        note: String,
        sidebar: String,
        slug: Option<String>,
        hide_identity_form: bool,
        image: Option<String>,
        entries: Vec<EntityListEntryData>,
    }

    #[tracing::instrument]
    pub async fn synchronize_entity_lists() -> Result<(), anyhow::Error> {
        use crate::entity_list::{EntityList, EntityListEntry};
        use common::shared::db;

        loop {
            let last_update = misc::get("entity-list-sync-bookmark-update")
                .await
                .expect("Retrieving sync bookmark failed")
                .unwrap_or_else(|| String::from("2000-01-01T00:00:00.0Z"));

            let last_id = misc::get("entity-list-sync-bookmark-id")
                .await
                .expect("Retrieving sync bookmark failed.")
                .unwrap_or(0);

            let client = reqwest::Client::new();

            tracing::info!(
                "Retrieving entity lists since {} {}...",
                &last_update,
                last_id
            );

            let lists: Vec<EntityListData> = match client
                .get("https://scistarter.org/staff/sync/entity_lists_with_entries")
                .bearer_auth(env!("SYNC_TOKEN"))
                .query(&[("since", &last_update), ("last_id", &last_id.to_string())])
                .send()
                .await
            {
                Ok(response) => match response.error_for_status() {
                    Ok(resp) => {
                        let response_text = match resp.text().await {
                            Ok(text) => text,
                            Err(e) => {
                                eprintln!("Failed to read response body: {}", e);
                                sleep(Duration::from_secs(60)).await;
                                continue;
                            }
                        };

                        match serde_json::from_str(&response_text) {
                            Ok(lists) => lists,
                            Err(e) => {
                                eprintln!("JSON parsing error: {}", e);
                                eprintln!("Response body: {}", response_text);
                                sleep(Duration::from_secs(60)).await;
                                continue;
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("HTTP error: {}", e);
                        sleep(Duration::from_secs(60)).await;
                        continue;
                    }
                },
                Err(e) => {
                    eprintln!("Request error: {}", e);
                    sleep(Duration::from_secs(60)).await;
                    continue;
                }
            };

            let mut bookmark = (last_update.clone(), last_id);

            let got_lists = lists.len();
            tracing::info!("Synchronization received {} entity lists", got_lists);

            for data in lists {
                let owner_id: Ulid = match data.owner_ulid.parse() {
                    Ok(ulid) => ulid,
                    Err(err) => {
                        tracing::error!("Error parsing owner ULID {:?}: {}", data.owner_ulid, err);
                        continue;
                    }
                };

                let delegate_id: Option<Ulid> =
                    data.delegate_ulid.as_deref().and_then(|s| s.parse().ok());

                // Check if we already have a mapping for this orrery ID
                let existing = sqlx::query_scalar!(
                    "SELECT list_id FROM entity_list_id_map WHERE orrery_id = ?",
                    data.id,
                )
                .fetch_optional(db())
                .await
                .ok()
                .flatten();

                let list_id: Ulid = if let Some(existing_id) = existing {
                    existing_id.parse().unwrap_or_else(|_| Ulid::new())
                } else {
                    Ulid::new()
                };

                let entity_list = EntityList {
                    list_id,
                    // An unknown creation date stays empty rather than
                    // becoming the current time. The bookmark below
                    // advances to the highest created_at seen, and
                    // orrery sorts its null dates first, so standing in
                    // "now" for them would push the bookmark past every
                    // real list and stall the sync permanently.
                    created_at: data.created.unwrap_or_default(),
                    private: data.private,
                    owner_id,
                    delegate_id,
                    name: data.name,
                    note: data.note,
                    sidebar: data.sidebar,
                    slug: data.slug,
                    hide_identity_form: data.hide_identity_form,
                    image: data.image.unwrap_or_default(),
                };

                if let Err(err) = entity_list.store().await {
                    tracing::error!("Error saving entity list: {}", err);
                    continue;
                }

                // Store orrery ID → hydra ULID mapping
                let list_id_str = list_id.to_string();
                sqlx::query!(
                    "INSERT OR REPLACE INTO entity_list_id_map (orrery_id, list_id) VALUES (?, ?)",
                    data.id,
                    list_id_str,
                )
                .execute(db())
                .await
                .ok();

                // Orrery sends the full entry set for every list it
                // returns, so anything already here that isn't in this
                // batch has been removed upstream.
                let previous_entries: Vec<String> = sqlx::query_scalar!(
                    "SELECT entry_id FROM entity_list_entries WHERE list_id = ?",
                    list_id_str,
                )
                .fetch_all(db())
                .await
                .unwrap_or_default();

                let mut current_entries = std::collections::HashSet::new();

                for entry_data in data.entries {
                    // Reusing the id orrery's entry mapped to last time
                    // is what keeps a re-sync from inserting a second
                    // copy of every entry.
                    let mapped = sqlx::query_scalar!(
                        "SELECT entry_id FROM entity_list_entry_id_map WHERE orrery_id = ?",
                        entry_data.id,
                    )
                    .fetch_optional(db())
                    .await
                    .ok()
                    .flatten();

                    let entry_id: Ulid = mapped
                        .and_then(|id| id.parse().ok())
                        .unwrap_or_else(Ulid::new);

                    let entry = EntityListEntry {
                        entry_id,
                        list_id,
                        priority: entry_data.priority,
                        added_at: entry_data.when,
                        entity_slug: entry_data.entity_slug,
                        note: entry_data.note,
                        link: entry_data.link,
                        // orrery's entity lists have no per-entry draft state,
                        // so everything it sends is on the page.
                        published: true,
                    };

                    if let Err(err) = entry.store().await {
                        tracing::error!("Error saving entity list entry: {}", err);
                        continue;
                    }

                    let entry_id_str = entry_id.to_string();
                    sqlx::query!(
                        "INSERT OR REPLACE INTO entity_list_entry_id_map (orrery_id, entry_id) VALUES (?, ?)",
                        entry_data.id,
                        entry_id_str,
                    )
                    .execute(db())
                    .await
                    .ok();

                    current_entries.insert(entry_id_str);
                }

                for stale in previous_entries
                    .into_iter()
                    .filter(|entry_id| !current_entries.contains(entry_id))
                {
                    match stale.parse::<Ulid>() {
                        Ok(entry_id) => {
                            if let Err(err) = EntityListEntry::delete(entry_id).await {
                                tracing::error!(
                                    "Error removing withdrawn entity list entry {}: {}",
                                    stale,
                                    err
                                );
                            }
                        }
                        Err(err) => {
                            tracing::error!("Unparsable entry id {} in list: {}", stale, err)
                        }
                    }
                }

                let new_bookmark = (entity_list.created_at.clone(), data.id);
                if new_bookmark > bookmark {
                    bookmark = new_bookmark;
                }
            }

            misc::set("entity-list-sync-bookmark-update", bookmark.0)
                .await
                .expect("Saving sync bookmark failed");

            misc::set("entity-list-sync-bookmark-id", bookmark.1)
                .await
                .expect("Saving sync bookmark failed");

            if got_lists == 500 {
                sleep(Duration::from_secs(60)).await;
            } else {
                sleep(Duration::from_secs(60 * 60)).await;
            }
        }
    }

    #[derive(Debug, serde::Deserialize)]
    struct GolinkRecordData {
        id: i32,
        when: String,
        slug: String,
        target: String,
        file: String,
        statemachine: String,
        transition: String,
        session_set_key: String,
        session_set_value: String,
    }

    #[derive(Debug, serde::Deserialize)]
    struct RegionIndexEntry {
        id: i64,
        name: String,
        digest: String,
    }

    #[derive(Debug, serde::Deserialize)]
    struct RegionShape {
        id: i64,
        name: String,
        digest: String,
        value: String,
    }

    /// Keep the region vocabulary and its polygons in step with orrery.
    ///
    /// Regions change perhaps once a year and run to about ten megabytes of
    /// geometry, so this reads an index of digests and fetches only the
    /// shapes that actually moved. When any of them does, every project's
    /// cached matches are suspect, so the whole cache is rebuilt -- a few
    /// seconds, against a change that rare.
    #[tracing::instrument]
    pub async fn synchronize_regions() -> Result<(), anyhow::Error> {
        loop {
            let client = reqwest::Client::new();

            tracing::info!("Retrieving region index...");

            let index: Vec<RegionIndexEntry> = match client
                .get("https://scistarter.org/staff/sync/regions")
                .bearer_auth(env!("SYNC_TOKEN"))
                .send()
                .await
            {
                Ok(response) => match response.error_for_status() {
                    Ok(resp) => match resp.text().await {
                        Ok(text) => match serde_json::from_str(&text) {
                            Ok(index) => index,
                            Err(e) => {
                                eprintln!("Region index JSON parsing error: {}", e);
                                eprintln!("Response body: {}", text);
                                sleep(Duration::from_secs(60)).await;
                                continue;
                            }
                        },
                        Err(e) => {
                            eprintln!("Failed to read region index body: {}", e);
                            sleep(Duration::from_secs(60)).await;
                            continue;
                        }
                    },
                    Err(e) => {
                        eprintln!("Region index HTTP error: {}", e);
                        sleep(Duration::from_secs(60)).await;
                        continue;
                    }
                },
                Err(e) => {
                    eprintln!("Region index request error: {}", e);
                    sleep(Duration::from_secs(60)).await;
                    continue;
                }
            };

            let mut moved = 0;

            // A change to the matching rule itself moves no polygon, so
            // nothing above would notice it. The rule the cache was last
            // built under is recorded alongside the bookmarks, and a
            // mismatch is as good a reason to rebuild as a moved boundary.
            let built_under: Option<String> = misc::get("region-match-rule").await.ok().flatten();
            let rule = crate::region::MIN_OVERLAP.to_string();

            if built_under.as_deref() != Some(rule.as_str()) {
                tracing::info!(
                    "Region matching rule is now {rule} (cache built under {built_under:?}); \
                     rebuilding project matches"
                );

                match crate::region::rebuild_all().await {
                    Ok(_) => {
                        if let Err(e) = misc::set("region-match-rule", rule).await {
                            eprintln!("Could not record the region matching rule: {}", e);
                        }
                    }
                    Err(e) => eprintln!("Region match rebuild failed: {}", e),
                }
            }

            for entry in index.iter() {
                match crate::region::digest_of(entry.id).await {
                    Ok(Some(held)) if held == entry.digest => continue,
                    Ok(_) => {}
                    Err(e) => {
                        eprintln!("Could not read held digest for region {}: {}", entry.id, e);
                        continue;
                    }
                }

                tracing::info!("Region {} ({}) has moved; fetching", entry.id, entry.name);

                let shape: RegionShape = match client
                    .get(format!(
                        "https://scistarter.org/staff/sync/regions/{}",
                        entry.id
                    ))
                    .bearer_auth(env!("SYNC_TOKEN"))
                    .send()
                    .await
                {
                    Ok(response) => match response.error_for_status() {
                        Ok(resp) => match resp.text().await {
                            Ok(text) => match serde_json::from_str(&text) {
                                Ok(shape) => shape,
                                Err(e) => {
                                    eprintln!("Region {} JSON parsing error: {}", entry.id, e);
                                    continue;
                                }
                            },
                            Err(e) => {
                                eprintln!("Failed to read region {} body: {}", entry.id, e);
                                continue;
                            }
                        },
                        Err(e) => {
                            eprintln!("Region {} HTTP error: {}", entry.id, e);
                            continue;
                        }
                    },
                    Err(e) => {
                        eprintln!("Region {} request error: {}", entry.id, e);
                        continue;
                    }
                };

                match crate::region::store(shape.id, &shape.name, &shape.digest, &shape.value).await
                {
                    Ok(true) => moved += 1,
                    Ok(false) => {}
                    Err(e) => eprintln!("Could not store region {}: {}", shape.id, e),
                }
            }

            let ids: Vec<i64> = index.iter().map(|entry| entry.id).collect();

            match crate::region::retain_only(&ids).await {
                Ok(0) => {}
                Ok(dropped) => {
                    tracing::info!("Dropped {} region(s) orrery no longer has", dropped);
                    moved += dropped;
                }
                Err(e) => eprintln!("Could not drop stale regions: {}", e),
            }

            if moved > 0 {
                tracing::info!("{} region(s) changed; rebuilding project matches", moved);

                if let Err(e) = crate::region::rebuild_all().await {
                    eprintln!("Region match rebuild failed: {}", e);
                }
            }

            sleep(Duration::from_secs(60 * 60)).await;
        }
    }

    #[tracing::instrument]
    pub async fn synchronize_go_urls() -> Result<(), anyhow::Error> {
        use crate::gourl::GoUrl;
        use crate::peering::LoadStorePeered;

        loop {
            let last_update = misc::get("go-url-sync-bookmark-update")
                .await
                .expect("Retrieving sync bookmark failed")
                .unwrap_or_else(|| String::from("2000-01-01T00:00:00.0Z"));

            let last_id: i32 = misc::get("go-url-sync-bookmark-id")
                .await
                .expect("Retrieving sync bookmark failed")
                .unwrap_or(0);

            let client = reqwest::Client::new();

            tracing::info!("Retrieving go URLs since {} {}...", &last_update, last_id);

            let records: Vec<GolinkRecordData> = match client
                .get("https://scistarter.org/staff/sync/golink_records")
                .bearer_auth(env!("SYNC_TOKEN"))
                .query(&[("since", &last_update), ("last_id", &last_id.to_string())])
                .send()
                .await
            {
                Ok(response) => match response.error_for_status() {
                    Ok(resp) => match resp.text().await {
                        Ok(text) => match serde_json::from_str(&text) {
                            Ok(records) => records,
                            Err(e) => {
                                eprintln!("JSON parsing error: {}", e);
                                eprintln!("Response body: {}", text);
                                sleep(Duration::from_secs(60)).await;
                                continue;
                            }
                        },
                        Err(e) => {
                            eprintln!("Failed to read response body: {}", e);
                            sleep(Duration::from_secs(60)).await;
                            continue;
                        }
                    },
                    Err(e) => {
                        eprintln!("HTTP error: {}", e);
                        sleep(Duration::from_secs(60)).await;
                        continue;
                    }
                },
                Err(e) => {
                    eprintln!("Request error: {}", e);
                    sleep(Duration::from_secs(60)).await;
                    continue;
                }
            };

            let got_records = records.len();
            tracing::info!("Synchronization received {} go URLs", got_records);

            let mut bookmark = (last_update.clone(), last_id);

            for data in records {
                // Prefer target; fall back to file if target is empty. Skip
                // entries with no usable destination — orrery has some legacy
                // rows with neither populated.
                let target_url = if !data.target.is_empty() {
                    data.target
                } else if !data.file.is_empty() {
                    data.file
                } else {
                    tracing::warn!("Skipping go URL {} with no target or file", data.slug);
                    continue;
                };

                let go_url = GoUrl {
                    slug: data.slug.clone(),
                    url_type: crate::gourl::GoUrlType::Redirect { target_url },
                    statemachine: data.statemachine,
                    transition: data.transition,
                    session_set_key: data.session_set_key,
                    session_set_value: data.session_set_value,
                };
                if let Err(err) = go_url.store().await {
                    tracing::error!("Error saving go URL {}: {}", data.slug, err);
                    continue;
                }

                let new_bookmark = (data.when, data.id);
                if new_bookmark > bookmark {
                    bookmark = new_bookmark;
                }
            }

            misc::set("go-url-sync-bookmark-update", bookmark.0)
                .await
                .expect("Saving sync bookmark failed");

            misc::set("go-url-sync-bookmark-id", bookmark.1)
                .await
                .expect("Saving sync bookmark failed");

            if got_records == 500 {
                sleep(Duration::from_secs(60)).await;
            } else {
                sleep(Duration::from_secs(60 * 60)).await;
            }
        }
    }

    #[tracing::instrument]
    pub async fn update_expired_statuses() -> Result<(), anyhow::Error> {
        use crate::event::{EventStatus, field::EventField};
        use crate::peering::LoadStorePeered;
        use crate::project::{ProjectStatus, field::ProjectField};
        use common::{
            elect,
            shared::{db, nats},
        };
        use jiff::{Timestamp, tz::TimeZone};
        use std::time::Duration as StdDuration;

        loop {
            let now = Timestamp::now();
            let today = now.to_zoned(TimeZone::UTC).date();
            let tomorrow = today.tomorrow()?;
            let midnight_tomorrow = tomorrow.at(0, 0, 0, 0).to_zoned(TimeZone::UTC)?;

            // Calculate seconds until next midnight UTC
            let sleep_duration =
                (midnight_tomorrow.timestamp() - now).total(jiff::Unit::Second)? as u64;

            // Sleep until next midnight UTC
            sleep(Duration::from_secs(sleep_duration)).await;

            // Get fresh timestamp after sleep
            let execution_time = Timestamp::now();
            let today_str = execution_time.to_zoned(TimeZone::UTC).date().to_string();
            let subject = format!("tasks.expired-status.claim.{}", today_str);
            let instance_id = ulid::Ulid::new().to_string();

            // Use elect function to determine if we should run the task
            let elected = elect::elect_instance(
                "TASK_CLAIMS",
                &subject,
                &instance_id,
                StdDuration::from_secs(25 * 60 * 60), // 25 hours to handle timezone edge cases
            )
            .await?;

            if elected {
                tracing::info!("Elected to run expired status task for {}", today_str);

                // Publish notification that this instance is running the task
                let notification = format!("running:{}:{}", instance_id, today_str);
                if let Err(err) = nats()
                    .publish(
                        "frontend.tasks.expired-status.status",
                        notification.into_bytes().into(),
                    )
                    .await
                {
                    tracing::warn!("Failed to publish task notification: {}", err);
                }

                let now_str = execution_time.to_string();

                // Find and update expired projects using ProjectField and peering
                let expired_projects = sqlx::query!(
                    r#"SELECT project_id FROM projects 
                       WHERE status IN ('Active', 'Periodically Active') 
                       AND end IS NOT NULL 
                       AND end < ?"#,
                    now_str
                )
                .fetch_all(db())
                .await?;

                let mut projects_updated = 0;
                for project in expired_projects {
                    let project_id: ulid::Ulid = project.project_id.parse()?;
                    let status_field = ProjectField {
                        project_id,
                        field: crate::project::field::ProjectFieldValue::Status(
                            ProjectStatus::Expired,
                        ),
                        source: None,
                    };

                    // Use the peering system to update the status
                    if let Err(err) = status_field.store().await {
                        tracing::error!("Failed to update project {} status: {}", project_id, err);
                    } else {
                        projects_updated += 1;
                    }
                }

                // Find and update expired events using EventField and peering
                let expired_events = sqlx::query!(
                    r#"SELECT DISTINCT e.event_id 
                       FROM events e
                       INNER JOIN event_instances ei ON e.event_id = ei.event_id
                       WHERE e.status = 'Active'
                       GROUP BY e.event_id
                       HAVING MAX(ei.end) < ?"#,
                    now_str
                )
                .fetch_all(db())
                .await?;

                let mut events_updated = 0;
                for event in expired_events {
                    let event_id: ulid::Ulid = event
                        .event_id
                        .ok_or_else(|| anyhow::anyhow!("Event has no event_id"))?
                        .parse()?;
                    let status_field = EventField {
                        event_id,
                        field: crate::event::field::EventFieldValue::Status(EventStatus::Expired),
                    };

                    // Use the peering system to update the status
                    if let Err(err) = status_field.store().await {
                        tracing::error!("Failed to update event {} status: {}", event_id, err);
                    } else {
                        events_updated += 1;
                    }
                }

                tracing::info!(
                    "Updated {} projects and {} events to expired status for {}",
                    projects_updated,
                    events_updated,
                    today_str
                );

                // Publish completion notification
                let completion = format!(
                    "completed:{}:{}:projects={}:events={}",
                    instance_id, today_str, projects_updated, events_updated
                );
                if let Err(err) = nats()
                    .publish(
                        "frontend.tasks.expired-status.status",
                        completion.into_bytes().into(),
                    )
                    .await
                {
                    tracing::warn!("Failed to publish completion notification: {}", err);
                }
            } else {
                tracing::debug!("Not elected to run expired status task for {}", today_str);
            }
        }
    }
}
