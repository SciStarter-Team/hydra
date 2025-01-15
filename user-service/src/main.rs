use std::{collections::BTreeMap, time::Duration};

use async_nats::service::ServiceExt;
use common::{
    handler, misc,
    shared::{db, nats},
};
use futures::StreamExt;
use jiff::Timestamp;
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
async fn sessions_synchronize() {
    #[derive(serde::Deserialize)]
    struct SessionDataImport {
        session_key: String,
        session_data: BTreeMap<String, serde_json::Value>,
        expire_date: String,
    }

    loop {
        let last = misc::get("session-sync-bookmark")
            .await
            .expect("Retrieving sync bookmark failed")
            .unwrap_or_else(|| String::from("2000-01-01 00:00:00+00"));

        let client = reqwest::Client::new();

        let sessions: Vec<SessionDataImport> = client
            .get("https://scistarter.org/staff/sync/sessions")
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

        let db = db();
        let mut bookmark = last.clone();

        let got_sessions = sessions.len();

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

        if got_sessions == 500 {
            sleep(Duration::from_secs(60)).await;
        } else {
            sleep(Duration::from_secs(60 * 60)).await;
        }
    }
}

// dev feature, remove when this codebase becomes the primary version
async fn users_synchronize() {
    #[derive(serde::Deserialize)]
    struct UserDataImport {
        id: i32,
        legacy_id: Option<i32>,
        ulid: String,
        updated: String,
        username: String,
        password: String,
        is_superuser: bool,
        is_staff: bool,
        is_test: bool,
        is_active: bool,
        first_name: String,
        last_name: String,
        date_joined: String,
        date_active: String,
        last_login: Option<String>,
        public: bool,
        accept_emails: bool,
        accept_messages: bool,
        track_contributions: bool,
        handle: String,
        image: Option<String>,
        birthday: Option<String>,
        gender: Option<String>,
        bio: String,
        facebook_url: String,
        twitter_profile: String,
        inat_username: String,
        inat_last_update: Option<String>,
        zipcode: String,
        country: String,
        mailing_address: String,
        point: Option<String>,
        scientist: bool,
        educator: bool,
        origin: String,
        referer: String,
        utm_source: String,
        utm_medium: String,
        utm_campaign: String,
        utm_term: String,
        utm_content: String,
        misc: String,
        restricted_navigation: bool,
        received_certificate: bool,
        protected: bool,
        extra_skills: String,
        extra_tools: String,
        regions: Option<String>,
        api_key: Option<String>,
        site_dressing: i64,
        email: String,
        tags: String,
        additional_email: String,
        needs_additional_email: bool,
        country_code: String,
        needs_inat_username: bool,
        opaque_id: Option<String>,
        legacy_userdata_id: Option<i32>,
        zooniverse_username: String,
        degree: Option<String>,
        ethnicity: Option<String>,
        income: Option<String>,
        ai_opt_out: bool,
        umbrella_project_id: Option<String>,
        automatic_affiliate: bool,
        primary_email: i64,
        circuit_id: Option<String>,
        home_page: String,
        newsletter: bool,
        constant_contact_id: Option<String>,
        ai_opt_out_at: Option<String>,
        date_unsubscribed: Option<String>,
        ambassador: Option<String>,
        profile_id: i32,
        profile_legacy_id: Option<i32>,
        profile_alias_id: Option<i32>,
        profile_identifier_username: Option<String>,
        profile_identifier_username_normalized: Option<String>,
        profile_identifier_email: Option<String>,
        profile_identifier_additional: Option<String>,
        profile_anonymized: Option<String>,
        profile_extra: String,
    }

    loop {
        let last = misc::get("user-sync-bookmark")
            .await
            .expect("Retrieving sync bookmark failed")
            .unwrap_or_else(|| String::from("2000-01-01 00:00:00+00"));

        let client = reqwest::Client::new();

        let users: Vec<UserDataImport> = client
            .get("https://scistarter.org/staff/sync/users")
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

        let db = db();
        let mut bookmark = last.clone();

        let got_users = users.len();

        for user in users {
            if user.updated > bookmark {
                bookmark = user.updated.clone();
            }

            sqlx::query!(
                r#"insert into "sessions" ("session_id", "expires", "data")
                   values (?, ?, ?)
                   on conflict ("session_id") do
                   update set "expires" = excluded."expires", "data" = excluded."data""#,
                user.session_key,
                user.expire_date,
                data,
            )
            .execute(db)
            .await
            .expect("Saving session failed");

            println!("Saved user {}", user.username,);
        }

        misc::set("user-sync-bookmark", bookmark)
            .await
            .expect("Saving sync bookmark failed");

        if got_users == 500 {
            sleep(Duration::from_secs(60)).await;
        } else {
            sleep(Duration::from_secs(60 * 60)).await;
        }
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

    let sessions_sync_task = spawn(sessions_synchronize());

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

    sessions_sync_task.abort();
}
