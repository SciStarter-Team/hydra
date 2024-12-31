use std::{collections::BTreeMap, time::Duration};

use async_nats::service::ServiceExt;
use common::{
    handler, misc,
    shared::{db, nats},
};
use futures::StreamExt;
use tokio::{spawn, task::JoinSet, time::sleep};

pub mod sessions;

async fn cleanup() {
    loop {
        if let Err(err) =
            sqlx::query!(r#"delete from "sessions" where datetime("expires") < datetime()"#,)
                .execute(db())
                .await
        {
            eprintln!("Error while performing session cleanup: {:?}", err);
        }

        sleep(Duration::from_secs(60 * 60 * 13)).await;
    }
}

// dev feature, remove when this codebase becomes the primary version
async fn synchronize() {
    #[derive(serde::Deserialize)]
    struct SessionData {
        session_key: String,
        session_data: BTreeMap<String, serde_json::Value>,
        expire_date: String,
    }

    loop {
        let last = misc::get("session-sync-bookmark")
            .await
            .expect("Retrieving sync bookmark failed")
            .unwrap_or_else(|| String::from("2000-01-01 00:00:00"));

        let client = reqwest::Client::new();

        let sessions: Vec<SessionData> = client
            .get("https://scistarter.org/staff/sync/sessions")
            .bearer_auth(env!("SYNC_TOKEN"))
            .query(&[("since", &last)])
            .send()
            .await
            .expect("Retrieving session data failed")
            .json()
            .await
            .expect("JSON decoding of session data failed");

        let db = db();
        let mut bookmark = last.clone();

        for session in sessions {
            let data =
                flexbuffers::to_vec(session.session_data).expect("Encoding session data failed");

            if session.expire_date > bookmark {
                bookmark = session.expire_date.clone();
            }

            sqlx::query!(
                r#"insert into "sessions" ("session_id", "expires", "data")
                   values (?, ?, ?)
                   on conflict ("session_id") do
                   update set "expires" = excluded."expires", "data" = excluded."data""#,
                session.session_key,
                session.expire_date,
                data,
            )
            .execute(db)
            .await
            .expect("Saving session failed");

            println!(
                "Saved session {} expiring {}",
                session.session_key, session.expire_date
            );
        }

        misc::set("session-sync-bookmark", bookmark)
            .await
            .expect("Saving sync bookmark failed");

        sleep(Duration::from_secs(60 * 60)).await;
    }
}

#[tokio::main]
async fn main() {
    common::shared::init("user").await;

    sqlx::migrate!("./migrations")
        .run(db())
        .await
        .expect("Unable to run migrations");

    let cleanup_task = spawn(cleanup());

    let sync_task = spawn(synchronize());

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

    cleanup_task.abort();

    sync_task.abort();
}
