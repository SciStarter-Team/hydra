use async_nats::service::ServiceExt;
use common::{
    handler,
    shared::{db, nats},
};
use futures::StreamExt;
use tokio::task::JoinSet;

pub mod sessions;

#[tokio::main]
async fn main() {
    common::shared::init("user").await;

    sqlx::migrate!("./migrations")
        .run(db())
        .await
        .expect("Unable to run migrations");

    let nats = nats();

    let service = nats
        .service_builder()
        .start("user", env!("CARGO_PKG_VERSION"))
        .await
        .expect("Unable to start user service");

    let session = service.group("session");

    let mut toplevel = JoinSet::new();

    toplevel.spawn(handler!(session, "store", sessions::store));
    toplevel.spawn(handler!(session, "retrieve", sessions::retrieve));
    toplevel.spawn(handler!(session, "delete", sessions::delete));

    toplevel.join_all().await;
}
