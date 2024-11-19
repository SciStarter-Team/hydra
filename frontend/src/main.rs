pub mod channel;
pub mod session;
pub mod upload;

use common::shared::db;
use poem::{
    endpoint::{EmbeddedFileEndpoint, EmbeddedFilesEndpoint},
    get, handler,
    listener::TcpListener,
    session::{CookieConfig, ServerSession, Session},
    web::{cookie::SameSite, Html},
    EndpointExt, IntoResponse, Route, Server,
};
use rust_embed::Embed;
use session::UserServiceSessionStorage;
use ulid::Ulid;

#[cfg(debug_assertions)]
pub const SESSION_COOKIE: &'static str = "session";

#[cfg(not(debug_assertions))]
pub const SESSION_COOKIE: &'static str = "__Host-session";

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
fn index(session: &Session) -> impl IntoResponse {
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

    Html(
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
  </body>
</html>
"#,
    )
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    common::shared::init("frontend").await;

    sqlx::migrate!("./migrations").run(db()).await?;

    let session_storage = UserServiceSessionStorage;

    let routes = Route::new()
        .nest("/-static", StaticFiles::new())
        .nest("/-upload", upload::routes())
        .nest("/-channel", channel::routes())
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

    Server::new(TcpListener::bind(env!("LISTEN_ON")))
        .run(routes)
        .await?;

    Ok(())
}
