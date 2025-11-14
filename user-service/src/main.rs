use std::{collections::BTreeMap, time::Duration};

use async_nats::service::ServiceExt;
use common::{
    handler, misc, recv_handler,
    service::log_activity,
    shared::{db, hash_identifier, hash_identifier_capfirst, hash_identifier_lower, nats},
    timestring::TimeString,
    ulid::AsUlid,
};
use futures::StreamExt;
use tokio::{spawn, task::JoinSet, time::sleep};
use ulid::Ulid;

pub mod address_sets;
pub mod auth;
pub mod bookmarks;
pub mod health;
pub mod permissions;
pub mod project_joins;
pub mod recipient_search;
pub mod sessions;
pub mod stats;
pub mod sync;
pub mod user_matching;
pub mod user_sets;
pub mod users;
pub mod webauthn;
pub mod workflow;

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

async fn analyze_tables() {
    loop {
        // Wait 7 days between runs
        sleep(Duration::from_secs(60 * 60 * 24 * 7)).await;

        println!("Running weekly ANALYZE on database tables...");

        // Update statistics for the users table and its indexes
        if let Err(err) = sqlx::query!("ANALYZE users").execute(db()).await {
            eprintln!("Error while running ANALYZE on users table: {:?}", err);
        }

        // Update statistics for the emails table (used in joins)
        if let Err(err) = sqlx::query!("ANALYZE emails").execute(db()).await {
            eprintln!("Error while running ANALYZE on emails table: {:?}", err);
        }

        // Update statistics for the user_tags table
        if let Err(err) = sqlx::query!("ANALYZE user_tags").execute(db()).await {
            eprintln!("Error while running ANALYZE on user_tags table: {:?}", err);
        }

        println!("Weekly ANALYZE completed for all tables");
    }
}

// dev feature, remove when this codebase becomes the primary version
async fn sessions_synchronize() {
    if cfg!(debug_assertions) {
        return;
    }

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

        let sessions: Vec<SessionDataImport> = match client
            .get("https://scistarter.org/staff/sync/sessions")
            .bearer_auth(env!("SYNC_TOKEN"))
            .query(&[("since", &last)])
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
                        Ok(sessions) => sessions,
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
    #[allow(dead_code)]
    struct UserDataImport {
        id: i32,
        legacy_id: Option<i32>,
        ulid: String,
        updated: String,
        username: String,
        password: String,
        verified: bool,
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
        profile_id: Option<i32>,
        profile_legacy_id: Option<i32>,
        profile_alias_id: Option<i32>,
        profile_identifier_username: Option<String>,
        profile_identifier_username_normalized: Option<String>,
        profile_identifier_email: Option<String>,
        profile_identifier_additional: Option<String>,
        profile_anonymized: Option<String>,
        profile_extra: Option<String>,
    }

    loop {
        let last = misc::get("user-sync-bookmark")
            .await
            .expect("Retrieving sync bookmark failed")
            .unwrap_or_else(|| String::from("2000-01-01 00:00:00+00"));

        println!("Syncing users since {}", &last);

        let client = reqwest::Client::new();

        let users: Vec<UserDataImport> = match client
            .get("https://scistarter.org/staff/sync/users")
            .bearer_auth(env!("SYNC_TOKEN"))
            .query(&[("since", &last)])
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
                        Ok(users) => users,
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

        let db = db();
        let mut bookmark = last.clone();

        let got_users = users.len();

        for user in users {
            if user.updated > bookmark {
                bookmark = user.updated.clone();
            }

            let user_primary_email = match user.primary_email {
                1 => user.email.clone(),
                2 => user.additional_email.clone(),
                _ => user.username.clone(),
            };

            let user_is_superuser = match user.is_superuser {
                true => 1,
                false => 0,
            };

            let user_is_staff = match user.is_staff {
                true => 1,
                false => 0,
            };

            let user_is_test = match user.is_test {
                true => 1,
                false => 0,
            };

            let user_is_active = match user.is_active {
                true => 1,
                false => 0,
            };

            let user_is_public = match user.public {
                true => 1,
                false => 0,
            };

            let user_accept_emails = match user.accept_emails {
                true => 1,
                false => 0,
            };

            let user_newsletter = match user.newsletter {
                true => 1,
                false => 0,
            };

            let user_accept_messages = match user.accept_messages {
                true => 1,
                false => 0,
            };

            let user_track_contributions = match user.track_contributions {
                true => 1,
                false => 0,
            };

            let user_image = user
                .image
                .and_then(|s| if s.is_empty() { None } else { Some(s) })
                .map(|s| format!("https://orrery-media.s3.amazonaws.com/{}", s));

            let user_scientist = match user.scientist {
                true => 1,
                false => 0,
            };

            let user_educator = match user.educator {
                true => 1,
                false => 0,
            };

            let user_restricted_navigation = match user.restricted_navigation {
                true => 1,
                false => 0,
            };

            let user_protected = match user.protected {
                true => 1,
                false => 0,
            };

            let user_needs_inat_username = match user.needs_inat_username {
                true => 1,
                false => 0,
            };

            let user_ai_opt_out = match user.ai_opt_out {
                true => 1,
                false => 0,
            };

            let user_automatic_affiliate = match user.automatic_affiliate {
                true => 1,
                false => 0,
            };

            let user_date_joined = match TimeString::try_from(user.date_joined) {
                Ok(ts) => ts,
                Err(err) => {
                    eprintln!("{}", err);
                    continue;
                }
            };

            let user_date_active = TimeString::try_from(user.date_active).ok();

            // I would really like to see keyword parameters in SQLx
            // for DBs that support them.
            sqlx::query!(
                r#"insert or replace into "users" (
                     "user_id",
                     "legacy_v2_id",
                     "legacy_v2_userdata_id",
                     "legacy_v3_id",
                     "legacy_v2_profile_id",
                     "legacy_v3_alias_id",
                     "legacy_v3_profile_id",
                     "date_joined",
                     "updated",
                     "password",
                     "handle",
                     "primary_email",
                     "verified",
                     "is_superuser",
                     "is_staff",
                     "is_page_manager",
                     "is_test",
                     "is_active",
                     "public",
                     "accept_emails",
                     "newsletter",
                     "date_unsubscribed",
                     "accept_messages",
                     "track_contributions",
                     "first_name",
                     "last_name",
                     "image",
                     "birthday",
                     "gender",
                     "bio",
                     "facebook_url",
                     "twitter_profile",
                     "inat_username",
                     "inat_last_update",
                     "zipcode",
                     "country",
                     "mailing_address",
                     "scientist",
                     "educator",
                     "origin",
                     "referer",
                     "utm_source",
                     "utm_medium",
                     "utm_campaign",
                     "utm_term",
                     "utm_content",
                     "misc",
                     "restricted_navigation",
                     "protected",
                     "extra_skills",
                     "extra_tools",
                     "api_key",
                     "site_dressing",
                     "country_code",
                     "needs_inat_username",
                     "opaque_id",
                     "zooniverse_username",
                     "degree",
                     "ethnicity",
                     "income",
                     "ai_opt_out",
                     "ai_opt_out_at",
                     "umbrella_project_id",
                     "automatic_affiliate",
                     "circuit_id",
                     "home_page",
                     "constant_contact_id",
                     "ambassador",
                     "point",
                     "regions"
                   )
                   values (
                     ?1,
                     ?2,
                     ?3,
                     ?4,
                     ?5,
                     ?6,
                     ?7,
                     ?8,
                     ?9,
                     ?10,
                     ?11,
                     ?12,
                     ?13,
                     ?14,
                     ?15,
                     ?16,
                     ?17,
                     ?18,
                     ?19,
                     ?20,
                     ?21,
                     ?22,
                     ?23,
                     ?24,
                     ?25,
                     ?26,
                     ?27,
                     ?28,
                     ?29,
                     ?30,
                     ?31,
                     ?32,
                     ?33,
                     ?34,
                     ?35,
                     ?36,
                     ?37,
                     ?38,
                     ?39,
                     ?40,
                     ?41,
                     ?42,
                     ?43,
                     ?44,
                     ?45,
                     ?46,
                     ?47,
                     ?48,
                     ?49,
                     ?50,
                     ?51,
                     ?52,
                     ?53,
                     ?54,
                     ?55,
                     ?56,
                     ?57,
                     ?58,
                     ?59,
                     ?60,
                     ?61,
                     ?62,
                     ?63,
                     ?64,
                     ?65,
                     ?66,
                     ?67,
                     ?68,
                     GeomFromEWKT(?69),
                     GeomFromEWKT(?70)
                   )"#,
                user.ulid,
                user.legacy_id,
                user.legacy_userdata_id,
                user.id,
                user.profile_legacy_id,
                user.profile_alias_id,
                user.profile_id,
                user_date_joined,
                user.updated,
                user.password,
                user.handle,
                user_primary_email,
                user.verified,
                user_is_superuser,
                user_is_staff,
                0, // is_page_manager defaults to false for sync
                user_is_test,
                user_is_active,
                user_is_public,
                user_accept_emails,
                user_newsletter,
                user.date_unsubscribed,
                user_accept_messages,
                user_track_contributions,
                user.first_name,
                user.last_name,
                user_image,
                user.birthday,
                user.gender,
                user.bio,
                user.facebook_url,
                user.twitter_profile,
                user.inat_username,
                user.inat_last_update,
                user.zipcode,
                user.country,
                user.mailing_address,
                user_scientist,
                user_educator,
                user.origin,
                user.referer,
                user.utm_source,
                user.utm_medium,
                user.utm_campaign,
                user.utm_term,
                user.utm_content,
                user.misc,
                user_restricted_navigation,
                user_protected,
                user.extra_skills,
                user.extra_tools,
                user.api_key,
                user.site_dressing,
                user.country_code,
                user_needs_inat_username,
                user.opaque_id,
                user.zooniverse_username,
                user.degree,
                user.ethnicity,
                user.income,
                user_ai_opt_out,
                user.ai_opt_out_at,
                user.umbrella_project_id,
                user_automatic_affiliate,
                user.circuit_id,
                user.home_page,
                user.constant_contact_id,
                user.ambassador,
                user.point,
                user.regions,
            )
            .execute(db)
            .await
            .expect("Saving user failed");

            if let Some(stamp) = user.last_login {
                log_activity(
                    stamp.try_into().ok(),
                    "login",
                    user.ulid.as_ulid().ok(),
                    None,
                    None,
                )
                .await
                .expect("Error sending log entry");
            }

            let verified = user.verified && user.username == user_primary_email;
            let hashed = hash_identifier(&user.username);
            let hashed_lower = hash_identifier_lower(&user.username);
            let hashed_capfirst = hash_identifier_capfirst(&user.username);
            sqlx::query!(
                r#"
                insert into "emails" ("user_id", "email", "verified", "hash", "hash_lower", "hash_capfirst")
                values (?, ?, ?, ?, ?, ?)
                on conflict ("email") do
                update set "user_id" = excluded."user_id"
                on conflict ("hash") do
                update set "user_id" = excluded."user_id"
                on conflict ("hash_lower") do
                update set "user_id" = excluded."user_id"
                on conflict ("hash_capfirst") do
                update set "user_id" = excluded."user_id"
                "#,
                user.ulid,
                user.username,
                verified,
                hashed,
                hashed_lower,
                hashed_capfirst
            )
            .execute(db)
            .await
            .expect("Saving username email failed");

            if !user.email.is_empty() && user.email != user.username {
                let verified = user.verified && user.email == user_primary_email;
                let hashed = hash_identifier(&user.email);
                let hashed_lower = hash_identifier_lower(&user.email);
                let hashed_capfirst = hash_identifier_capfirst(&user.email);
                sqlx::query!(
                    r#"
                insert into "emails" ("user_id", "email", "verified", "hash", "hash_lower", "hash_capfirst")
                values (?, ?, ?, ?, ?, ?)
                on conflict ("email") do nothing
                on conflict ("hash") do nothing
                on conflict ("hash_lower") do nothing
                on conflict ("hash_capfirst") do nothing
                "#,
                    user.ulid,
                    user.email,
                    verified,
                    hashed,
                    hashed_lower,
                    hashed_capfirst
                )
                .execute(db)
                .await
                .expect("Saving first email failed");
            }

            if !user.additional_email.is_empty()
                && user.additional_email != user.username
                && user.additional_email != user.email
            {
                let verified = user.verified && user.additional_email == user_primary_email;
                let hashed = hash_identifier(&user.additional_email);
                let hashed_lower = hash_identifier_lower(&user.additional_email);
                let hashed_capfirst = hash_identifier_capfirst(&user.additional_email);
                sqlx::query!(
                    r#"
                insert into "emails" ("user_id", "email", "verified", "hash", "hash_lower", "hash_capfirst")
                values (?, ?, ?, ?, ?, ?)
                on conflict ("email") do nothing
                on conflict ("hash") do nothing
                on conflict ("hash_lower") do nothing
                on conflict ("hash_capfirst") do nothing
                "#,
                    user.ulid,
                    user.additional_email,
                    verified,
                    hashed,
                    hashed_lower,
                    hashed_capfirst
                )
                .execute(db)
                .await
                .expect("Saving second email failed");
            }

            let tag_list: Vec<String> = user
                .tags
                .split("||")
                .filter(|tag| !tag.is_empty())
                .map(|tag| tag.to_string())
                .collect();

            for tag in &tag_list {
                sqlx::query!(
                    r#"
                    insert into "user_tags" ("user_id", "tag")
                    values (?, ?)
                    on conflict ("user_id", "tag") do nothing"#,
                    user.ulid,
                    tag
                )
                .execute(db)
                .await
                .expect("Error inserting tag");
            }

            // Notify log service of tag update if there are tags
            if !tag_list.is_empty() {
                if let Err(e) =
                    common::service::send("log.user_tags_update", (user.ulid.clone(), tag_list))
                        .await
                {
                    eprintln!("Failed to notify log service of tag update: {}", e);
                }
            }

            log_activity(None, "imported", user.ulid.as_ulid().ok(), None, None)
                .await
                .ok();

            if let Some(date_active) = user_date_active {
                log_activity(
                    date_active,
                    "legacy-active",
                    user.ulid.as_ulid().ok(),
                    None,
                    None,
                )
                .await
                .ok();
            }

            println!(
                "Saved user {} {} {}",
                &user.username, &user.updated, &user.ulid
            );
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

async fn userbadges_synchronize() {
    #[derive(serde::Deserialize)]
    #[allow(dead_code)]
    struct UserbadgeDataImport {
        id: i32,
        badgeclass: String,
        when: String, // ISO 8601 timestamp string
        image: String,
        user_ulid: String,
        name: String,
    }

    loop {
        let last = misc::get("userbadge-sync-bookmark")
            .await
            .expect("Retrieving sync bookmark failed")
            .unwrap_or_else(|| String::from("1900-01-01T00:00:00Z"));
        let last_id = misc::get("userbadge-sync-last-id")
            .await
            .expect("Retrieving sync last_id failed")
            .unwrap_or_else(|| 0);

        println!("Syncing userbadges since {} with last_id {}", last, last_id);

        let client = reqwest::Client::new();

        let userbadges: Vec<UserbadgeDataImport> = match client
            .get("https://scistarter.org/staff/sync/userbadges")
            .bearer_auth(env!("SYNC_TOKEN"))
            .query(&[("since", &last), ("last_id", &last_id.to_string())])
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
                        Ok(badges) => badges,
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

        let db = db();
        let mut bookmark = last.clone();
        let mut bookmark_id = last_id;

        let got_userbadges = userbadges.len();

        for badge in userbadges {
            println!(
                "Saving userbadge {} for user {}",
                &badge.badgeclass, &badge.user_ulid
            );

            // Update bookmark to the latest timestamp and id
            if badge.when > bookmark || (badge.when == bookmark && badge.id > bookmark_id) {
                bookmark = badge.when.clone();
                bookmark_id = badge.id;
            }

            // Parse the timestamp through TimeString
            let badge_when = match TimeString::try_from(badge.when.clone()) {
                Ok(ts) => ts,
                Err(e) => {
                    eprintln!("Failed to parse timestamp '{}': {}", badge.when, e);
                    continue;
                }
            };

            // Handle empty image as null
            let image_value = if badge.image.is_empty() {
                None
            } else {
                Some(badge.image.clone())
            };

            // Insert the badge into the database
            sqlx::query!(
                r#"insert into "user_badges" ("user_id", "badgeclass", "when", "image")
                   values (?, ?, ?, ?)
                   on conflict ("user_id", "badgeclass") do nothing"#,
                badge.user_ulid,
                badge.badgeclass,
                badge_when,
                image_value,
            )
            .execute(db)
            .await
            .expect("Saving user badge failed");

            // Log the badge earning activity
            log_activity(
                Some(badge_when),
                "badge_earned",
                badge.user_ulid.as_ulid().ok(),
                None::<Ulid>,
                Some(badge.badgeclass.clone()),
            )
            .await
            .expect("Error logging badge activity");

            println!(
                "Saved badge {} for user {}",
                badge.badgeclass, badge.user_ulid
            );
        }

        misc::set("userbadge-sync-bookmark", bookmark)
            .await
            .expect("Saving sync bookmark failed");
        misc::set("userbadge-sync-last-id", bookmark_id)
            .await
            .expect("Saving sync last_id failed");

        if got_userbadges == 500 {
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

    let analyze_task = spawn(analyze_tables());

    let sessions_sync_task = spawn(sessions_synchronize());

    let users_sync_task = spawn(users_synchronize());

    let userbadges_sync_task = spawn(userbadges_synchronize());

    // WebAuthn session cleanup task
    let webauthn_cleanup_task = spawn(async {
        use tokio::time::{Duration, interval};
        let mut interval = interval(Duration::from_secs(3600)); // Every hour
        loop {
            interval.tick().await;
            if let Err(e) = webauthn::cleanup_expired_sessions().await {
                eprintln!("WebAuthn session cleanup error: {}", e);
            }
        }
    });

    let nats = nats();

    let service = nats
        .service_builder()
        .start("user", env!("CARGO_PKG_VERSION"))
        .await
        .expect("Unable to start user service");

    let mut toplevel = JoinSet::new();

    let info = service.group("user.basic_info");
    toplevel.spawn(handler!(info, "by_username", users::basic_info_by_username));
    toplevel.spawn(handler!(info, "by_user_id", users::basic_info_by_user_id));

    let user_info = service.group("user.info");
    toplevel.spawn(handler!(user_info, "email_info", users::user_email_info));
    toplevel.spawn(handler!(
        user_info,
        "display_info_batch",
        users::user_display_info_batch
    ));

    let stats = service.group("user.stats");
    toplevel.spawn(handler!(stats, "total_count", stats::total_count));
    toplevel.spawn(handler!(stats, "verified_count", stats::verified_count));
    toplevel.spawn(handler!(stats, "admin_analytics", stats::admin_analytics));
    toplevel.spawn(handler!(stats, "geographic", stats::geographic_analytics));
    toplevel.spawn(handler!(stats, "activity", stats::activity_analytics));

    let search = service.group("user.search");
    toplevel.spawn(handler!(search, "by_handle", users::search_by_handle));
    toplevel.spawn(handler!(search, "recipients", recipient_search::search));

    let session = service.group("user.session");
    toplevel.spawn(recv_handler!(session, "store", sessions::store));
    toplevel.spawn(handler!(session, "retrieve", sessions::retrieve));
    toplevel.spawn(recv_handler!(session, "delete", sessions::delete));

    let auth = service.group("user.auth");
    toplevel.spawn(handler!(auth, "signup", auth::signup));
    toplevel.spawn(handler!(auth, "login", auth::login));

    let tags = service.group("user.tags");
    toplevel.spawn(handler!(tags, "get", users::get_user_tags));
    toplevel.spawn(recv_handler!(tags, "set", users::set_user_tags));
    toplevel.spawn(recv_handler!(tags, "add", users::add_user_tag));
    toplevel.spawn(recv_handler!(tags, "remove", users::remove_user_tag));

    let emails = service.group("user.emails");
    toplevel.spawn(handler!(emails, "get", users::get_user_emails));
    toplevel.spawn(handler!(emails, "has", users::has_user_email));
    toplevel.spawn(recv_handler!(emails, "set", users::set_user_emails));
    toplevel.spawn(recv_handler!(emails, "add", users::add_user_email));
    toplevel.spawn(recv_handler!(emails, "remove", users::remove_user_email));

    let badges = service.group("user.badges");
    toplevel.spawn(handler!(badges, "all", users::get_user_badges));
    toplevel.spawn(handler!(badges, "get", users::get_user_badge));
    toplevel.spawn(recv_handler!(badges, "grant", users::grant_user_badge));
    toplevel.spawn(recv_handler!(badges, "revoke", users::revoke_user_badge));
    toplevel.spawn(handler!(badges, "has", users::has_user_badge));
    toplevel.spawn(handler!(badges, "holders", users::get_badge_holders));

    let cache = service.group("user.cache");
    toplevel.spawn(handler!(cache, "user_tags", users::user_tags));
    toplevel.spawn(handler!(cache, "user_emails", users::user_emails));

    // Admin endpoints
    let admin = service.group("user.admin");
    toplevel.spawn(handler!(admin, "user_list", users::admin_user_list));
    toplevel.spawn(handler!(admin, "user_search", users::admin_user_search));
    toplevel.spawn(handler!(admin, "user_export", users::admin_user_export));
    toplevel.spawn(handler!(admin, "user_get", users::admin_user_get));
    toplevel.spawn(handler!(
        admin,
        "user_batch_get",
        users::admin_user_batch_get
    ));
    toplevel.spawn(handler!(admin, "user_create", users::admin_user_create));
    toplevel.spawn(handler!(admin, "user_update", users::admin_user_update));
    toplevel.spawn(handler!(admin, "user_delete", users::admin_user_delete));
    toplevel.spawn(handler!(
        admin,
        "password_reset",
        users::admin_password_reset
    ));
    toplevel.spawn(handler!(admin, "email_add", users::admin_email_add));
    toplevel.spawn(handler!(admin, "email_update", users::admin_email_update));
    toplevel.spawn(handler!(admin, "email_delete", users::admin_email_delete));
    toplevel.spawn(handler!(
        admin,
        "email_toggle_verified",
        users::admin_email_toggle_verified
    ));

    // Page manager endpoints
    let page_manager = service.group("user.page_manager");
    toplevel.spawn(handler!(page_manager, "sync", users::page_manager_sync));
    toplevel.spawn(handler!(page_manager, "update", users::page_manager_update));

    // Bookmark endpoints
    let bookmarks = service.group("user.bookmarks");
    toplevel.spawn(handler!(bookmarks, "create", bookmarks::create_bookmark));
    toplevel.spawn(handler!(bookmarks, "check", bookmarks::check_bookmark));

    // Project join endpoints
    let joins = service.group("user.joins");
    toplevel.spawn(handler!(joins, "create", project_joins::create_join));
    toplevel.spawn(handler!(joins, "check", project_joins::check_join));

    let permissions = service.group("user.permissions");
    toplevel.spawn(handler!(permissions, "check", permissions::check_permission));
    toplevel.spawn(handler!(permissions, "grant", permissions::grant_permission));
    toplevel.spawn(handler!(permissions, "revoke", permissions::revoke_permission));
    toplevel.spawn(handler!(permissions, "list", permissions::list_permissions));

    // User matching for API participation recording
    let user_match = service.group("user");
    toplevel.spawn(handler!(user_match, "match", user_matching::match_user));

    // API key management
    let api_key = service.group("user.api_key");
    toplevel.spawn(handler!(api_key, "get", user_matching::get_api_key));
    toplevel.spawn(handler!(api_key, "validate", user_matching::validate_api_key));

    // Opaque ID retrieval
    let opaque_id = service.group("user.opaque_id");
    toplevel.spawn(handler!(opaque_id, "get", user_matching::get_opaque_id));

    // Workflow progress tracking
    let workflow_group = service.group("user.workflow");
    toplevel.spawn(handler!(workflow_group, "progress", workflow::record_progress));
    toplevel.spawn(handler!(workflow_group, "status", workflow::get_status));

    let health = service.group("user.health");
    toplevel.spawn(handler!(health, "check", health::health_check));

    // Sync endpoints
    let sync = service.group("sync.sequence");
    toplevel.spawn(handler!(sync, "next_project_id", sync::next_project_id));

    // WebAuthn endpoints
    let webauthn = service.group("user.webauthn");
    toplevel.spawn(handler!(
        webauthn,
        "registration_start",
        webauthn::registration_start
    ));
    toplevel.spawn(handler!(
        webauthn,
        "registration_complete",
        webauthn::registration_complete
    ));
    toplevel.spawn(handler!(
        webauthn,
        "authentication_start",
        webauthn::authentication_start
    ));
    toplevel.spawn(handler!(
        webauthn,
        "authentication_complete",
        webauthn::authentication_complete
    ));
    toplevel.spawn(handler!(
        webauthn,
        "credentials_list",
        webauthn::credentials_list
    ));
    toplevel.spawn(handler!(
        webauthn,
        "credential_delete",
        webauthn::credential_delete
    ));

    // Address sets endpoints
    let address_sets = service.group("user.address_sets");
    toplevel.spawn(handler!(address_sets, "list", address_sets::list));
    toplevel.spawn(handler!(address_sets, "get", address_sets::get));
    toplevel.spawn(handler!(address_sets, "create", address_sets::create));
    toplevel.spawn(handler!(address_sets, "update", address_sets::update));
    toplevel.spawn(handler!(address_sets, "delete", address_sets::delete));
    toplevel.spawn(handler!(address_sets, "add_member", address_sets::add_member));
    toplevel.spawn(handler!(address_sets, "remove_member", address_sets::remove_member));

    // User sets endpoints
    let user_sets = service.group("user.user_sets");
    toplevel.spawn(handler!(user_sets, "list", user_sets::list));
    toplevel.spawn(handler!(user_sets, "get", user_sets::get));
    toplevel.spawn(handler!(user_sets, "create", user_sets::create));
    toplevel.spawn(handler!(user_sets, "update", user_sets::update));
    toplevel.spawn(handler!(user_sets, "delete", user_sets::delete));
    toplevel.spawn(handler!(user_sets, "get_detail", user_sets::get_detail));
    toplevel.spawn(handler!(user_sets, "add_criterion", user_sets::add_criterion));
    toplevel.spawn(handler!(user_sets, "remove_criterion", user_sets::remove_criterion));
    toplevel.spawn(handler!(user_sets, "release_results", user_sets::release_results));

    // Set up graceful shutdown
    use tokio::signal::unix::{SignalKind, signal};

    let mut sigint = signal(SignalKind::interrupt()).expect("Failed to create SIGINT handler");
    let mut sigterm = signal(SignalKind::terminate()).expect("Failed to create SIGTERM handler");

    tokio::select! {
        _ = toplevel.join_all() => {
            println!("All service handlers completed");
        }
        _ = sigint.recv() => {
            println!("Received SIGINT, shutting down user service gracefully...");
        }
        _ = sigterm.recv() => {
            println!("Received SIGTERM, shutting down user service gracefully...");
        }
    }

    println!("User service shutting down...");
    cleanup_task.abort();
    analyze_task.abort();

    sessions_sync_task.abort();
    users_sync_task.abort();
    userbadges_sync_task.abort();
    webauthn_cleanup_task.abort();

    println!("User service shutdown complete");
}
