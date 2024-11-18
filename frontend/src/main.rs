pub mod session;

use common::shared::db;
use poem::{
    endpoint::{EmbeddedFileEndpoint, EmbeddedFilesEndpoint},
    get, handler,
    listener::TcpListener,
    session::{CookieConfig, ServerSession, Session},
    web::{cookie::SameSite, Path},
    EndpointExt, Route, Server,
};
use rust_embed::Embed;
use session::UserServiceSessionStorage;

#[derive(Embed)]
#[folder = "dist/"]
struct Static;

type StaticFiles = EmbeddedFilesEndpoint<Static>;
type StaticFile = EmbeddedFileEndpoint<Static>;

#[handler]
fn hello(session: &Session, Path(name): Path<String>) -> String {
    let prev = session.get("moo");
    session.set("moo", &name);
    format!(
        "hello: {}, {}",
        prev.unwrap_or_else(|| String::from("default")),
        name
    )
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    common::shared::init("frontend").await;

    sqlx::migrate!("./migrations").run(db()).await?;

    let session_storage = UserServiceSessionStorage;

    let routes = Route::new()
        .at("/hello/:name", get(hello))
        .nest("/static", StaticFiles::new())
        .with(ServerSession::new(
            CookieConfig::default()
                .name("session")
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
