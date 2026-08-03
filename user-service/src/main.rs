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
pub mod ambassador;
pub mod auth;
pub mod bookmarks;
pub mod certificates;
pub mod communities;
pub mod constant_contact;
pub mod geocode;
pub mod girlscouts;
pub mod health;
pub mod inat;
pub mod legacy_id;
pub mod link_tokens;
pub mod permissions;
pub mod places;
pub mod profile;
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

        if let Err(err) = sqlx::query!(
            r#"delete from "pending_signups" where datetime("created") < datetime('now', '-7 days')"#
        )
        .execute(db())
        .await
        {
            eprintln!("Error while sweeping expired pending signups: {:?}", err);
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
        state: String,
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
        signup_tags: String,
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
        constant_contact_digest: String,
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
                     "regions",
                     "state",
                     -- Carried from orrery rather than recomputed, so that
                     -- taking over the sweep does not look like 107,000
                     -- changed accounts and spend three weeks of the daily
                     -- request allowance re-sending what is already there.
                     "constant_contact_digest",
                     -- Which of this account's tags arrived from a signup
                     -- context. Carried rather than re-derived because it
                     -- cannot be: nothing about a tag says where it came
                     -- from, and the mirror on this side needs the same
                     -- answer orrery reached or the two compute different
                     -- digests for the same account.
                     "signup_tags",
                     -- Named explicitly because this is an INSERT OR
                     -- REPLACE: a column left out of the list is reset to
                     -- its default on every pass of the sync, which would
                     -- silently empty the sweep's work list.
                     "constant_contact_dirty"
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
                     GeomFromEWKT(?70),
                     ?71,
                     ?72,
                     ?73,
                     -- Orrery is doing the pushing for as long as it is the
                     -- primary and this sync is running, and the digest it
                     -- sends says the record is current.
                     0
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
                user.state,
                user.constant_contact_digest,
                user.signup_tags,
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

            // Check for any badges this user has and create missing workflow records
            let user_badges = sqlx::query!(
                r#"SELECT badgeclass, "when" FROM user_badges WHERE user_id = ?"#,
                user.ulid
            )
            .fetch_all(db)
            .await
            .unwrap_or_default();

            for badge in user_badges {
                match badge.badgeclass.as_str() {
                    "6Jg_IwmfQTaJ2kUNePsO5Q" => {
                        // Foundations of Citizen Science
                        let exists = sqlx::query_scalar::<_, i64>(
                            "SELECT COUNT(*) FROM foundations_progress WHERE user_id = ?",
                        )
                        .bind(&user.ulid)
                        .fetch_one(db)
                        .await
                        .unwrap_or(0)
                            > 0;

                        if !exists {
                            if let Err(e) = sqlx::query!(
                                "INSERT INTO foundations_progress (user_id, hour1_completed_at, squirrel_completed_at, stall_completed_at, debris_completed_at, badge_granted_at) VALUES (?, ?, ?, ?, ?, ?)",
                                user.ulid, badge.when, badge.when, badge.when, badge.when, badge.when
                            )
                            .execute(db)
                            .await {
                                eprintln!("Failed to create foundations workflow record during user sync for {}: {}", user.ulid, e);
                            } else {
                                println!("Created missing foundations workflow record for user {}", user.ulid);
                            }
                        }
                    }
                    "CWZrsNMPSZSHqKJoz8pLMQ" => {
                        // Libraries as Community Hubs
                        let exists = sqlx::query_scalar::<_, i64>(
                            "SELECT COUNT(*) FROM libraries_hubs_progress WHERE user_id = ?",
                        )
                        .bind(&user.ulid)
                        .fetch_one(db)
                        .await
                        .unwrap_or(0)
                            > 0;

                        if !exists {
                            if let Err(e) = sqlx::query!(
                                "INSERT INTO libraries_hubs_progress (user_id, hour2_completed_at, supplemental_signup_at, badge_granted_at) VALUES (?, ?, ?, ?)",
                                user.ulid, badge.when, badge.when, badge.when
                            )
                            .execute(db)
                            .await {
                                eprintln!("Failed to create libraries_hubs workflow record during user sync for {}: {}", user.ulid, e);
                            } else {
                                println!("Created missing libraries_hubs workflow record for user {}", user.ulid);
                            }
                        }
                    }
                    "e_t41dudSc6dXymoEa8iNw" => {
                        // Data Literacy
                        let exists = sqlx::query_scalar::<_, i64>(
                            "SELECT COUNT(*) FROM data_literacy_progress WHERE user_id = ?",
                        )
                        .bind(&user.ulid)
                        .fetch_one(db)
                        .await
                        .unwrap_or(0)
                            > 0;

                        if !exists {
                            if let Err(e) = sqlx::query!(
                                "INSERT INTO data_literacy_progress (user_id, tutorial_completed_at, badge_granted_at) VALUES (?, ?, ?)",
                                user.ulid, badge.when, badge.when
                            )
                            .execute(db)
                            .await {
                                eprintln!("Failed to create data_literacy workflow record during user sync for {}: {}", user.ulid, e);
                            } else {
                                println!("Created missing data_literacy workflow record for user {}", user.ulid);
                            }
                        }
                    }
                    "OP_wrFm7QauZgaXrZY-DhA" => {
                        // Higher Education
                        let exists = sqlx::query_scalar::<_, i64>(
                            "SELECT COUNT(*) FROM higher_ed_progress WHERE user_id = ?",
                        )
                        .bind(&user.ulid)
                        .fetch_one(db)
                        .await
                        .unwrap_or(0)
                            > 0;

                        if !exists {
                            if let Err(e) = sqlx::query!(
                                "INSERT INTO higher_ed_progress (user_id, tutorial_completed_at, badge_granted_at) VALUES (?, ?, ?)",
                                user.ulid, badge.when, badge.when
                            )
                            .execute(db)
                            .await {
                                eprintln!("Failed to create higher_ed workflow record during user sync for {}: {}", user.ulid, e);
                            } else {
                                println!("Created missing higher_ed workflow record for user {}", user.ulid);
                            }
                        }
                    }
                    "1OeTfgY1Q-exWqz---kPAQ" => {
                        // Data Ethics
                        let exists = sqlx::query_scalar::<_, i64>(
                            "SELECT COUNT(*) FROM data_ethics_progress WHERE user_id = ?",
                        )
                        .bind(&user.ulid)
                        .fetch_one(db)
                        .await
                        .unwrap_or(0)
                            > 0;

                        if !exists {
                            if let Err(e) = sqlx::query!(
                                "INSERT INTO data_ethics_progress (user_id, tutorial_completed_at, badge_granted_at) VALUES (?, ?, ?)",
                                user.ulid, badge.when, badge.when
                            )
                            .execute(db)
                            .await {
                                eprintln!("Failed to create data_ethics workflow record during user sync for {}: {}", user.ulid, e);
                            } else {
                                println!("Created missing data_ethics workflow record for user {}", user.ulid);
                            }
                        }
                    }
                    "a8cFJfeBTRec05glGM3_9w" => {
                        // IDEAL
                        let exists = sqlx::query_scalar::<_, i64>(
                            "SELECT COUNT(*) FROM ideal_progress WHERE user_id = ?",
                        )
                        .bind(&user.ulid)
                        .fetch_one(db)
                        .await
                        .unwrap_or(0)
                            > 0;

                        if !exists {
                            if let Err(e) = sqlx::query!(
                                "INSERT INTO ideal_progress (user_id, tutorial_completed_at, badge_granted_at) VALUES (?, ?, ?)",
                                user.ulid, badge.when, badge.when
                            )
                            .execute(db)
                            .await {
                                eprintln!("Failed to create ideal workflow record during user sync for {}: {}", user.ulid, e);
                            } else {
                                println!("Created missing ideal workflow record for user {}", user.ulid);
                            }
                        }
                    }
                    "9csPkdhzQLSZuk-OWREaKg" => {
                        // Ambassador
                        let exists = sqlx::query_scalar::<_, i64>(
                            "SELECT COUNT(*) FROM ambassador_progress WHERE user_id = ?",
                        )
                        .bind(&user.ulid)
                        .fetch_one(db)
                        .await
                        .unwrap_or(0)
                            > 0;

                        if !exists {
                            if let Err(e) = sqlx::query!(
                                "INSERT INTO ambassador_progress (user_id, tutorial_completed_at, badge_granted_at) VALUES (?, ?, ?)",
                                user.ulid, badge.when, badge.when
                            )
                            .execute(db)
                            .await {
                                eprintln!("Failed to create ambassador workflow record during user sync for {}: {}", user.ulid, e);
                            } else {
                                println!("Created missing ambassador workflow record for user {}", user.ulid);
                            }
                        }
                    }
                    "QLLs6Nm9TfqIj3_722yFQg" => {
                        // One Million Acts 2024
                        let exists = sqlx::query_scalar::<_, i64>(
                            "SELECT COUNT(*) FROM one_million_2024_progress WHERE user_id = ?",
                        )
                        .bind(&user.ulid)
                        .fetch_one(db)
                        .await
                        .unwrap_or(0)
                            > 0;

                        if !exists {
                            if let Err(e) = sqlx::query!(
                                "INSERT INTO one_million_2024_progress (user_id, completed_at, badge_granted_at) VALUES (?, ?, ?)",
                                user.ulid, badge.when, badge.when
                            )
                            .execute(db)
                            .await {
                                eprintln!("Failed to create one_million_2024 workflow record during user sync for {}: {}", user.ulid, e);
                            } else {
                                println!("Created missing one_million_2024 workflow record for user {}", user.ulid);
                            }
                        }
                    }
                    "U431wPFaTaKwUQO0WhQJRg" => {
                        // One Million Acts 2025
                        let exists = sqlx::query_scalar::<_, i64>(
                            "SELECT COUNT(*) FROM one_million_2025_progress WHERE user_id = ?",
                        )
                        .bind(&user.ulid)
                        .fetch_one(db)
                        .await
                        .unwrap_or(0)
                            > 0;

                        if !exists {
                            if let Err(e) = sqlx::query!(
                                "INSERT INTO one_million_2025_progress (user_id, completed_at, badge_granted_at) VALUES (?, ?, ?)",
                                user.ulid, badge.when, badge.when
                            )
                            .execute(db)
                            .await {
                                eprintln!("Failed to create one_million_2025 workflow record during user sync for {}: {}", user.ulid, e);
                            } else {
                                println!("Created missing one_million_2025 workflow record for user {}", user.ulid);
                            }
                        }
                    }
                    "01KPAY9A4VN929P02MECQ1W90N" => {
                        // Citizen Science for K-12 Educators
                        let exists = sqlx::query_scalar::<_, i64>(
                            "SELECT COUNT(*) FROM k12_progress WHERE user_id = ?",
                        )
                        .bind(&user.ulid)
                        .fetch_one(db)
                        .await
                        .unwrap_or(0)
                            > 0;

                        if !exists {
                            if let Err(e) = sqlx::query!(
                                "INSERT INTO k12_progress (user_id, tutorial_completed_at, badge_granted_at) VALUES (?, ?, ?)",
                                user.ulid, badge.when, badge.when
                            )
                            .execute(db)
                            .await {
                                eprintln!("Failed to create k12 workflow record during user sync for {}: {}", user.ulid, e);
                            } else {
                                println!("Created missing k12 workflow record for user {}", user.ulid);
                            }
                        }
                    }
                    _ => {
                        // Other badges don't have workflow tracking
                    }
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

            // Insert the badge into the database
            let result = sqlx::query!(
                r#"insert into "user_badges" ("user_id", "badgeclass", "when")
                   values (?, ?, ?)
                   on conflict ("user_id", "badgeclass") do nothing"#,
                badge.user_ulid,
                badge.badgeclass,
                badge_when,
            )
            .execute(db)
            .await
            .expect("Saving user badge failed");

            // Log the badge earning activity
            log_activity(
                Some(badge_when.clone()),
                "badge_earned",
                badge.user_ulid.as_ulid().ok(),
                None::<Ulid>,
                Some(badge.badgeclass.clone()),
            )
            .await
            .expect("Error logging badge activity");

            // If badge was actually inserted (not a duplicate), create workflow record
            // Only create workflow record if user exists in users table
            if result.rows_affected() > 0 {
                // Check if user exists in users table first
                let user_exists =
                    sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM users WHERE user_id = ?")
                        .bind(&badge.user_ulid)
                        .fetch_one(db)
                        .await
                        .unwrap_or(0)
                        > 0;

                if user_exists {
                    let now_str = badge_when.to_string();
                    match badge.badgeclass.as_str() {
                        // Foundations of Citizen Science
                        "6Jg_IwmfQTaJ2kUNePsO5Q" => {
                            if let Err(e) = sqlx::query!(
                                "INSERT OR IGNORE INTO foundations_progress (user_id, hour1_completed_at, squirrel_completed_at, stall_completed_at, debris_completed_at, badge_granted_at) VALUES (?, ?, ?, ?, ?, ?)",
                                badge.user_ulid, now_str, now_str, now_str, now_str, now_str
                            )
                            .execute(db)
                            .await {
                                eprintln!("Failed to create foundations workflow record for user {}: {}", badge.user_ulid, e);
                            }
                        }
                        // Libraries as Community Hubs
                        "CWZrsNMPSZSHqKJoz8pLMQ" => {
                            if let Err(e) = sqlx::query!(
                                "INSERT OR IGNORE INTO libraries_hubs_progress (user_id, hour2_completed_at, supplemental_signup_at, badge_granted_at) VALUES (?, ?, ?, ?)",
                                badge.user_ulid, now_str, now_str, now_str
                            )
                            .execute(db)
                            .await {
                                eprintln!("Failed to create libraries_hubs workflow record for user {}: {}", badge.user_ulid, e);
                            }
                        }
                        // Data Literacy
                        "e_t41dudSc6dXymoEa8iNw" => {
                            if let Err(e) = sqlx::query!(
                                "INSERT OR IGNORE INTO data_literacy_progress (user_id, tutorial_completed_at, badge_granted_at) VALUES (?, ?, ?)",
                                badge.user_ulid, now_str, now_str
                            )
                            .execute(db)
                            .await {
                                eprintln!("Failed to create data_literacy workflow record for user {}: {}", badge.user_ulid, e);
                            }
                        }
                        // Higher Education
                        "OP_wrFm7QauZgaXrZY-DhA" => {
                            if let Err(e) = sqlx::query!(
                                "INSERT OR IGNORE INTO higher_ed_progress (user_id, tutorial_completed_at, badge_granted_at) VALUES (?, ?, ?)",
                                badge.user_ulid, now_str, now_str
                            )
                            .execute(db)
                            .await {
                                eprintln!("Failed to create higher_ed workflow record for user {}: {}", badge.user_ulid, e);
                            }
                        }
                        // Data Ethics
                        "1OeTfgY1Q-exWqz---kPAQ" => {
                            if let Err(e) = sqlx::query!(
                                "INSERT OR IGNORE INTO data_ethics_progress (user_id, tutorial_completed_at, badge_granted_at) VALUES (?, ?, ?)",
                                badge.user_ulid, now_str, now_str
                            )
                            .execute(db)
                            .await {
                                eprintln!("Failed to create data_ethics workflow record for user {}: {}", badge.user_ulid, e);
                            }
                        }
                        // IDEAL
                        "a8cFJfeBTRec05glGM3_9w" => {
                            if let Err(e) = sqlx::query!(
                                "INSERT OR IGNORE INTO ideal_progress (user_id, tutorial_completed_at, badge_granted_at) VALUES (?, ?, ?)",
                                badge.user_ulid, now_str, now_str
                            )
                            .execute(db)
                            .await {
                                eprintln!("Failed to create ideal workflow record for user {}: {}", badge.user_ulid, e);
                            }
                        }
                        // Ambassador
                        "9csPkdhzQLSZuk-OWREaKg" => {
                            if let Err(e) = sqlx::query!(
                                "INSERT OR IGNORE INTO ambassador_progress (user_id, tutorial_completed_at, badge_granted_at) VALUES (?, ?, ?)",
                                badge.user_ulid, now_str, now_str
                            )
                            .execute(db)
                            .await {
                                eprintln!("Failed to create ambassador workflow record for user {}: {}", badge.user_ulid, e);
                            }
                        }
                        // One Million Acts 2024
                        "QLLs6Nm9TfqIj3_722yFQg" => {
                            if let Err(e) = sqlx::query!(
                                "INSERT OR IGNORE INTO one_million_2024_progress (user_id, completed_at, badge_granted_at) VALUES (?, ?, ?)",
                                badge.user_ulid, now_str, now_str
                            )
                            .execute(db)
                            .await {
                                eprintln!("Failed to create one_million_2024 workflow record for user {}: {}", badge.user_ulid, e);
                            }
                        }
                        // One Million Acts 2025
                        "U431wPFaTaKwUQO0WhQJRg" => {
                            if let Err(e) = sqlx::query!(
                                "INSERT OR IGNORE INTO one_million_2025_progress (user_id, completed_at, badge_granted_at) VALUES (?, ?, ?)",
                                badge.user_ulid, now_str, now_str
                            )
                            .execute(db)
                            .await {
                                eprintln!("Failed to create one_million_2025 workflow record for user {}: {}", badge.user_ulid, e);
                            }
                        }
                        // Citizen Science for K-12 Educators
                        "01KPAY9A4VN929P02MECQ1W90N" => {
                            if let Err(e) = sqlx::query!(
                                "INSERT OR IGNORE INTO k12_progress (user_id, tutorial_completed_at, badge_granted_at) VALUES (?, ?, ?)",
                                badge.user_ulid, now_str, now_str
                            )
                            .execute(db)
                            .await {
                                eprintln!("Failed to create k12 workflow record for user {}: {}", badge.user_ulid, e);
                            }
                        }
                        _ => {
                            // Other badges don't have workflow tracking
                        }
                    }
                } else {
                    println!(
                        "Badge {} for user {} - user not found in users table, skipping workflow record creation",
                        badge.badgeclass, badge.user_ulid
                    );
                }
            }

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

/// Mirror orrery's join records into the `project_joins` operational
/// store. Only kind 1 rows ("clicked to join") are joins; kind 0 rows
/// are explicit page visits and are skipped. Inactive joins are
/// removed. Starting bookmarks at the epoch backfills the full orrery
/// history on first run.
async fn joins_synchronize() {
    #[derive(serde::Deserialize)]
    struct JoinDataImport {
        id: i64,
        user_ulid: String,
        entity_ulid: String,
        active: bool,
        activity: String,
        kind: i64,
    }

    loop {
        let last: String = misc::get("join-sync-bookmark")
            .await
            .expect("Retrieving sync bookmark failed")
            .unwrap_or_else(|| String::from("1900-01-01T00:00:00Z"));
        let last_id: i64 = misc::get("join-sync-last-id")
            .await
            .expect("Retrieving sync last_id failed")
            .unwrap_or(0);

        println!("Syncing joins since {} with last_id {}", last, last_id);

        let client = reqwest::Client::new();

        let joins: Vec<JoinDataImport> = match client
            .get("https://scistarter.org/staff/sync/joins")
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
                            eprintln!("Failed to read joins response body: {}", e);
                            sleep(Duration::from_secs(60)).await;
                            continue;
                        }
                    };

                    match serde_json::from_str(&response_text) {
                        Ok(joins) => joins,
                        Err(e) => {
                            eprintln!("Joins JSON parsing error: {}", e);
                            eprintln!("Response body: {}", response_text);
                            sleep(Duration::from_secs(60)).await;
                            continue;
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Joins HTTP error: {}", e);
                    sleep(Duration::from_secs(60)).await;
                    continue;
                }
            },
            Err(e) => {
                eprintln!("Joins request error: {}", e);
                sleep(Duration::from_secs(60)).await;
                continue;
            }
        };

        let db = db();
        let mut bookmark = last.clone();
        let mut bookmark_id = last_id;
        let got_joins = joins.len();
        let mut stopped_early = false;

        for join in joins {
            let result = if join.kind != 1 {
                // Kind 0 rows are page visits, not joins; skip but
                // advance past them.
                Ok(())
            } else if join.active {
                sqlx::query!(
                    r#"INSERT OR IGNORE INTO "project_joins" ("user_id", "project_id")
                       VALUES (?, ?)"#,
                    join.user_ulid,
                    join.entity_ulid,
                )
                .execute(db)
                .await
                .map(|_| ())
            } else {
                sqlx::query!(
                    r#"DELETE FROM "project_joins" WHERE "user_id" = ? AND "project_id" = ?"#,
                    join.user_ulid,
                    join.entity_ulid,
                )
                .execute(db)
                .await
                .map(|_| ())
            };

            if let Err(e) = result {
                // Most likely the user row hasn't been synced yet.
                // Stop without advancing past this row; the retry after
                // the user sync catches up will pick it up again.
                eprintln!(
                    "Failed to save join {} for user {}: {}",
                    join.id, join.user_ulid, e
                );
                stopped_early = true;
                break;
            }

            if join.activity > bookmark || (join.activity == bookmark && join.id > bookmark_id) {
                bookmark = join.activity;
                bookmark_id = join.id;
            }
        }

        misc::set("join-sync-bookmark", bookmark)
            .await
            .expect("Saving sync bookmark failed");
        misc::set("join-sync-last-id", bookmark_id)
            .await
            .expect("Saving sync last_id failed");

        if stopped_early {
            sleep(Duration::from_secs(60)).await;
        } else if got_joins == 500 {
            // Full page: more history is waiting, keep going quickly so
            // the initial backfill completes in reasonable time.
            sleep(Duration::from_secs(1)).await;
        } else {
            sleep(Duration::from_secs(60 * 60)).await;
        }
    }
}

/// Mirror orrery's bookmarks into the `bookmarks` operational store,
/// carrying the active flag so un-bookmarking in orrery is reflected
/// here. Starting bookmarks at the epoch backfills the full orrery
/// history on first run.
async fn bookmarks_synchronize() {
    #[derive(serde::Deserialize)]
    struct BookmarkDataImport {
        id: i64,
        when: String,
        user_ulid: String,
        entity_ulid: String,
        active: bool,
        activity: String,
        contacted: Option<String>,
    }

    loop {
        let last: String = misc::get("bookmark-sync-bookmark")
            .await
            .expect("Retrieving sync bookmark failed")
            .unwrap_or_else(|| String::from("1900-01-01T00:00:00Z"));
        let last_id: i64 = misc::get("bookmark-sync-last-id")
            .await
            .expect("Retrieving sync last_id failed")
            .unwrap_or(0);

        println!("Syncing bookmarks since {} with last_id {}", last, last_id);

        let client = reqwest::Client::new();

        let bookmarks: Vec<BookmarkDataImport> = match client
            .get("https://scistarter.org/staff/sync/bookmarks")
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
                            eprintln!("Failed to read bookmarks response body: {}", e);
                            sleep(Duration::from_secs(60)).await;
                            continue;
                        }
                    };

                    match serde_json::from_str(&response_text) {
                        Ok(bookmarks) => bookmarks,
                        Err(e) => {
                            eprintln!("Bookmarks JSON parsing error: {}", e);
                            eprintln!("Response body: {}", response_text);
                            sleep(Duration::from_secs(60)).await;
                            continue;
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Bookmarks HTTP error: {}", e);
                    sleep(Duration::from_secs(60)).await;
                    continue;
                }
            },
            Err(e) => {
                eprintln!("Bookmarks request error: {}", e);
                sleep(Duration::from_secs(60)).await;
                continue;
            }
        };

        let db = db();
        let mut bookmark = last.clone();
        let mut bookmark_id = last_id;
        let got_bookmarks = bookmarks.len();
        let mut stopped_early = false;

        for bm in bookmarks {
            let result = sqlx::query!(
                r#"INSERT INTO "bookmarks"
                       ("user_id", "project_id", "created", "activity", "contacted", "active")
                   VALUES (?, ?, ?, ?, ?, ?)
                   ON CONFLICT ("user_id", "project_id") DO UPDATE SET
                       "created" = excluded."created",
                       "activity" = excluded."activity",
                       "contacted" = excluded."contacted",
                       "active" = excluded."active""#,
                bm.user_ulid,
                bm.entity_ulid,
                bm.when,
                bm.activity,
                bm.contacted,
                bm.active,
            )
            .execute(db)
            .await;

            if let Err(e) = result {
                // Most likely the user row hasn't been synced yet.
                // Stop without advancing past this row; the retry after
                // the user sync catches up will pick it up again.
                eprintln!(
                    "Failed to save bookmark {} for user {}: {}",
                    bm.id, bm.user_ulid, e
                );
                stopped_early = true;
                break;
            }

            if bm.activity > bookmark || (bm.activity == bookmark && bm.id > bookmark_id) {
                bookmark = bm.activity;
                bookmark_id = bm.id;
            }
        }

        misc::set("bookmark-sync-bookmark", bookmark)
            .await
            .expect("Saving sync bookmark failed");
        misc::set("bookmark-sync-last-id", bookmark_id)
            .await
            .expect("Saving sync last_id failed");

        if stopped_early {
            sleep(Duration::from_secs(60)).await;
        } else if got_bookmarks == 500 {
            // Full page: more history is waiting, keep going quickly so
            // the initial backfill completes in reasonable time.
            sleep(Duration::from_secs(1)).await;
        } else {
            sleep(Duration::from_secs(60 * 60)).await;
        }
    }
}

async fn certificates_synchronize() {
    #[derive(serde::Deserialize)]
    struct CertificateDataImport {
        id: i32,
        user_ulid: String,
        project: String,
        mode: String,
        when: String,
    }

    loop {
        let last = misc::get("certificate-sync-bookmark")
            .await
            .expect("Retrieving sync bookmark failed")
            .unwrap_or_else(|| String::from("1900-01-01T00:00:00Z"));
        let last_id = misc::get("certificate-sync-last-id")
            .await
            .expect("Retrieving sync last_id failed")
            .unwrap_or(0);

        let client = reqwest::Client::new();

        let certs: Vec<CertificateDataImport> = match client
            .get("https://scistarter.org/staff/sync/certificates")
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
                        Ok(certs) => certs,
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

        let mut bookmark = last.clone();
        let mut bookmark_id = last_id;

        let got_certs = certs.len();

        for cert in certs {
            if cert.when > bookmark || (cert.when == bookmark && cert.id > bookmark_id) {
                bookmark = cert.when.clone();
                bookmark_id = cert.id;
            }

            let certificate_id = Ulid::new().to_string();

            // INSERT OR IGNORE deduplicates on (user_id, project, mode)
            if let Err(e) = sqlx::query!(
                r#"INSERT OR IGNORE INTO certificates (certificate_id, created_at, user_id, project, mode)
                   VALUES (?, ?, ?, ?, ?)"#,
                certificate_id,
                cert.when,
                cert.user_ulid,
                cert.project,
                cert.mode,
            )
            .execute(db())
            .await
            {
                eprintln!(
                    "Failed to save certificate for user {}: {}",
                    cert.user_ulid, e
                );
            }
        }

        misc::set("certificate-sync-bookmark", bookmark)
            .await
            .expect("Saving sync bookmark failed");
        misc::set("certificate-sync-last-id", bookmark_id)
            .await
            .expect("Saving sync last_id failed");

        if got_certs == 500 {
            sleep(Duration::from_secs(60)).await;
        } else {
            sleep(Duration::from_secs(60 * 60)).await;
        }
    }
}

async fn workflow_states_synchronize() {
    #[derive(serde::Deserialize)]
    struct WorkflowStateImport {
        id: i32,
        ulid: String,
        workflow: String,
        state: String,
        state_changed: String,
    }

    loop {
        let last = misc::get("workflow-state-sync-bookmark")
            .await
            .expect("Retrieving sync bookmark failed")
            .unwrap_or_else(|| String::from("1900-01-01T00:00:00Z"));
        let last_id = misc::get("workflow-state-sync-last-id")
            .await
            .expect("Retrieving sync last_id failed")
            .unwrap_or(0);

        let client = reqwest::Client::new();

        let states: Vec<WorkflowStateImport> = match client
            .get("https://scistarter.org/staff/sync/workflow_states")
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
                            eprintln!("Failed to read workflow_states response body: {}", e);
                            sleep(Duration::from_secs(60)).await;
                            continue;
                        }
                    };

                    match serde_json::from_str(&response_text) {
                        Ok(states) => states,
                        Err(e) => {
                            eprintln!("JSON parsing error for workflow_states: {}", e);
                            eprintln!("Response body: {}", response_text);
                            sleep(Duration::from_secs(60)).await;
                            continue;
                        }
                    }
                }
                Err(e) => {
                    eprintln!("HTTP error for workflow_states: {}", e);
                    sleep(Duration::from_secs(60)).await;
                    continue;
                }
            },
            Err(e) => {
                eprintln!("Request error for workflow_states: {}", e);
                sleep(Duration::from_secs(60)).await;
                continue;
            }
        };

        let db = db();
        let mut bookmark = last.clone();
        let mut bookmark_id = last_id;

        let got_states = states.len();

        for ws in states {
            if ws.state_changed > bookmark || (ws.state_changed == bookmark && ws.id > bookmark_id)
            {
                bookmark = ws.state_changed.clone();
                bookmark_id = ws.id;
            }

            // Use state_changed as the upper bound for completed step timestamps
            let ts = &ws.state_changed;

            let result = match ws.workflow.as_str() {
                "foundations" => apply_foundations_state(db, &ws.ulid, &ws.state, ts).await,
                "libraries_hubs" => apply_libraries_hubs_state(db, &ws.ulid, &ws.state, ts).await,
                "ambassador" => apply_ambassador_state(db, &ws.ulid, &ws.state, ts).await,
                "data_literacy" => {
                    apply_simple_tutorial_state(
                        db,
                        "data_literacy_progress",
                        &ws.ulid,
                        &ws.state,
                        ts,
                    )
                    .await
                }
                "higher_ed" => apply_higher_ed_state(db, &ws.ulid, &ws.state, ts).await,
                "data_ethics" => {
                    apply_simple_tutorial_state(db, "data_ethics_progress", &ws.ulid, &ws.state, ts)
                        .await
                }
                "ideal" => {
                    apply_simple_tutorial_state(db, "ideal_progress", &ws.ulid, &ws.state, ts).await
                }
                "k12" => apply_k12_state(db, &ws.ulid, &ws.state, ts).await,
                other => {
                    eprintln!("Unknown workflow type: {}", other);
                    Ok(())
                }
            };

            if let Err(e) = result {
                eprintln!(
                    "Failed to apply workflow state {} {} for user {}: {}",
                    ws.workflow, ws.state, ws.ulid, e
                );
            }
        }

        misc::set("workflow-state-sync-bookmark", bookmark)
            .await
            .expect("Saving sync bookmark failed");
        misc::set("workflow-state-sync-last-id", bookmark_id)
            .await
            .expect("Saving sync last_id failed");

        if got_states == 500 {
            sleep(Duration::from_secs(60)).await;
        } else {
            sleep(Duration::from_secs(60 * 60)).await;
        }
    }
}

/// Foundations states: initial → hour1 → squirrel/stall/debris → bookmark2
/// Each later state implies all earlier steps were completed.
async fn apply_foundations_state(
    db: &sqlx::SqlitePool,
    user_id: &str,
    state: &str,
    ts: &str,
) -> Result<(), anyhow::Error> {
    // Ensure row exists
    sqlx::query!(
        "INSERT OR IGNORE INTO foundations_progress (user_id) VALUES (?)",
        user_id
    )
    .execute(db)
    .await?;

    match state {
        "hour1" => {
            sqlx::query!(
                "UPDATE foundations_progress SET hour1_completed_at = COALESCE(hour1_completed_at, ?) WHERE user_id = ?",
                ts, user_id
            ).execute(db).await?;
        }
        "squirrel" => {
            sqlx::query!(
                "UPDATE foundations_progress SET hour1_completed_at = COALESCE(hour1_completed_at, ?), squirrel_completed_at = COALESCE(squirrel_completed_at, ?) WHERE user_id = ?",
                ts, ts, user_id
            ).execute(db).await?;
        }
        "stall" => {
            sqlx::query!(
                "UPDATE foundations_progress SET hour1_completed_at = COALESCE(hour1_completed_at, ?), stall_completed_at = COALESCE(stall_completed_at, ?) WHERE user_id = ?",
                ts, ts, user_id
            ).execute(db).await?;
        }
        "debris" => {
            sqlx::query!(
                "UPDATE foundations_progress SET hour1_completed_at = COALESCE(hour1_completed_at, ?), debris_completed_at = COALESCE(debris_completed_at, ?) WHERE user_id = ?",
                ts, ts, user_id
            ).execute(db).await?;
        }
        "bookmark2" => {
            sqlx::query!(
                r#"UPDATE foundations_progress SET
                    hour1_completed_at = COALESCE(hour1_completed_at, ?),
                    squirrel_completed_at = COALESCE(squirrel_completed_at, ?),
                    stall_completed_at = COALESCE(stall_completed_at, ?),
                    debris_completed_at = COALESCE(debris_completed_at, ?),
                    badge_granted_at = COALESCE(badge_granted_at, ?)
                   WHERE user_id = ?"#,
                ts,
                ts,
                ts,
                ts,
                ts,
                user_id
            )
            .execute(db)
            .await?;
        }
        // Legacy intermediate states
        "participation" | "bookmark1" => {
            sqlx::query!(
                "UPDATE foundations_progress SET hour1_completed_at = COALESCE(hour1_completed_at, ?) WHERE user_id = ?",
                ts, user_id
            ).execute(db).await?;
        }
        _ => {}
    }

    Ok(())
}

/// Libraries Hubs states: initial → hour2 → supplemental
async fn apply_libraries_hubs_state(
    db: &sqlx::SqlitePool,
    user_id: &str,
    state: &str,
    ts: &str,
) -> Result<(), anyhow::Error> {
    sqlx::query!(
        "INSERT OR IGNORE INTO libraries_hubs_progress (user_id) VALUES (?)",
        user_id
    )
    .execute(db)
    .await?;

    match state {
        "hour2" => {
            sqlx::query!(
                "UPDATE libraries_hubs_progress SET hour2_completed_at = COALESCE(hour2_completed_at, ?) WHERE user_id = ?",
                ts, user_id
            ).execute(db).await?;
        }
        "supplemental" => {
            sqlx::query!(
                r#"UPDATE libraries_hubs_progress SET
                    hour2_completed_at = COALESCE(hour2_completed_at, ?),
                    supplemental_signup_at = COALESCE(supplemental_signup_at, ?),
                    badge_granted_at = COALESCE(badge_granted_at, ?)
                   WHERE user_id = ?"#,
                ts,
                ts,
                ts,
                user_id
            )
            .execute(db)
            .await?;
        }
        _ => {}
    }

    Ok(())
}

/// Ambassador states: initial → tutorial → application → verified → passed
async fn apply_ambassador_state(
    db: &sqlx::SqlitePool,
    user_id: &str,
    state: &str,
    ts: &str,
) -> Result<(), anyhow::Error> {
    sqlx::query!(
        "INSERT OR IGNORE INTO ambassador_progress (user_id) VALUES (?)",
        user_id
    )
    .execute(db)
    .await?;

    // tutorial_completed_at = the Champion tutorial (grants the Champion
    // certificate in orrery). The Certified Ambassador badge is granted at the
    // "verified" state (orrery's verified_enter); "passed" follows verified, so
    // it also implies the badge. "application"/"international"/"rejected"/"failed"
    // are tutorial-complete (Champion) but not badge-earning.
    match state {
        "tutorial" | "application" | "international" | "rejected" | "failed" => {
            sqlx::query!(
                "UPDATE ambassador_progress SET tutorial_completed_at = COALESCE(tutorial_completed_at, ?) WHERE user_id = ?",
                ts, user_id
            ).execute(db).await?;
        }
        "verified" | "passed" => {
            sqlx::query!(
                r#"UPDATE ambassador_progress SET
                    tutorial_completed_at = COALESCE(tutorial_completed_at, ?),
                    badge_granted_at = COALESCE(badge_granted_at, ?)
                   WHERE user_id = ?"#,
                ts,
                ts,
                user_id
            )
            .execute(db)
            .await?;
        }
        _ => {}
    }

    // Keep the native state-machine column in step with orrery so a later native
    // staff review sees the correct prior state and does not re-fire an entry
    // email for a transition orrery already handled. The orrery state strings map
    // one-to-one onto the native states. This is a sync, so it never emails.
    if matches!(
        state,
        "tutorial"
            | "application"
            | "verified"
            | "passed"
            | "rejected"
            | "international"
            | "failed"
    ) {
        sqlx::query!(
            "UPDATE ambassador_progress SET state = ? WHERE user_id = ?",
            state,
            user_id
        )
        .execute(db)
        .await?;
    }

    Ok(())
}

/// Higher Ed states: initial → tutorial → list → explore
async fn apply_higher_ed_state(
    db: &sqlx::SqlitePool,
    user_id: &str,
    state: &str,
    ts: &str,
) -> Result<(), anyhow::Error> {
    sqlx::query!(
        "INSERT OR IGNORE INTO higher_ed_progress (user_id) VALUES (?)",
        user_id
    )
    .execute(db)
    .await?;

    // orrery's HigherEdState is sequential (tutorial → list → explore), and each
    // state's levels are cumulative, so a later state implies the earlier ones.
    // Reaching explore grants the badge.
    match state {
        "tutorial" => {
            sqlx::query!(
                "UPDATE higher_ed_progress SET tutorial_completed_at = COALESCE(tutorial_completed_at, ?) WHERE user_id = ?",
                ts, user_id
            ).execute(db).await?;
        }
        "list" => {
            sqlx::query!(
                r#"UPDATE higher_ed_progress SET
                    tutorial_completed_at = COALESCE(tutorial_completed_at, ?),
                    list_completed_at = COALESCE(list_completed_at, ?)
                   WHERE user_id = ?"#,
                ts,
                ts,
                user_id
            )
            .execute(db)
            .await?;
        }
        "explore" => {
            sqlx::query!(
                r#"UPDATE higher_ed_progress SET
                    tutorial_completed_at = COALESCE(tutorial_completed_at, ?),
                    list_completed_at = COALESCE(list_completed_at, ?),
                    explore_completed_at = COALESCE(explore_completed_at, ?),
                    badge_granted_at = COALESCE(badge_granted_at, ?)
                   WHERE user_id = ?"#,
                ts,
                ts,
                ts,
                ts,
                user_id
            )
            .execute(db)
            .await?;
        }
        _ => {}
    }

    Ok(())
}

/// K-12 Educators states: initial → tutorial
/// Reaching tutorial grants the badge.
async fn apply_k12_state(
    db: &sqlx::SqlitePool,
    user_id: &str,
    state: &str,
    ts: &str,
) -> Result<(), anyhow::Error> {
    sqlx::query!(
        "INSERT OR IGNORE INTO k12_progress (user_id) VALUES (?)",
        user_id
    )
    .execute(db)
    .await?;

    match state {
        "tutorial" => {
            sqlx::query!(
                r#"UPDATE k12_progress SET
                    tutorial_completed_at = COALESCE(tutorial_completed_at, ?),
                    badge_granted_at = COALESCE(badge_granted_at, ?)
                   WHERE user_id = ?"#,
                ts,
                ts,
                user_id
            )
            .execute(db)
            .await?;
        }
        _ => {}
    }

    Ok(())
}

/// Simple tutorial workflows (data_literacy, data_ethics, ideal):
/// initial → tutorial (terminal, grants badge)
async fn apply_simple_tutorial_state(
    db: &sqlx::SqlitePool,
    table: &str,
    user_id: &str,
    state: &str,
    ts: &str,
) -> Result<(), anyhow::Error> {
    // `table` cannot be a bind parameter (it is a SQL identifier), so it is
    // interpolated directly. SAFETY: every caller passes a hardcoded table name,
    // never untrusted input, so this is not an injection vector.
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "INSERT OR IGNORE INTO {} (user_id) VALUES (?)",
        table
    )))
    .bind(user_id)
    .execute(db)
    .await?;

    if state == "tutorial" {
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "UPDATE {} SET tutorial_completed_at = COALESCE(tutorial_completed_at, ?), badge_granted_at = COALESCE(badge_granted_at, ?) WHERE user_id = ?",
            table
        )))
        .bind(ts)
        .bind(ts)
        .bind(user_id)
        .execute(db)
        .await?;
    }

    Ok(())
}

async fn claim_codes_synchronize() {
    if cfg!(debug_assertions) {
        return;
    }

    #[derive(serde::Deserialize)]
    struct ClaimCodeDataImport {
        id: i32,
        code: String,
        category: String,
        claimed: bool,
        claimed_by: Option<String>,
        claimed_at: Option<String>,
    }

    loop {
        let last_id: i32 = misc::get("claim-code-sync-last-id")
            .await
            .expect("Retrieving sync last_id failed")
            .unwrap_or(0);
        let claimed_since: String = misc::get("claim-code-sync-claimed-since")
            .await
            .expect("Retrieving sync claimed_since failed")
            .unwrap_or_else(|| String::from("1900-01-01T00:00:00Z"));

        println!(
            "Syncing claim codes with last_id {} and claimed_since {}",
            last_id, claimed_since
        );

        let client = reqwest::Client::new();

        let claim_codes: Vec<ClaimCodeDataImport> = match client
            .get("https://scistarter.org/staff/sync/claim_codes")
            .bearer_auth(env!("SYNC_TOKEN"))
            .query(&[
                ("last_id", &last_id.to_string()),
                ("claimed_since", &claimed_since),
            ])
            .send()
            .await
        {
            Ok(response) => match response.error_for_status() {
                Ok(resp) => {
                    let response_text = match resp.text().await {
                        Ok(text) => text,
                        Err(e) => {
                            eprintln!("Failed to read claim_codes response body: {}", e);
                            sleep(Duration::from_secs(60)).await;
                            continue;
                        }
                    };

                    match serde_json::from_str(&response_text) {
                        Ok(codes) => codes,
                        Err(e) => {
                            eprintln!("JSON parsing error for claim_codes: {}", e);
                            eprintln!("Response body: {}", response_text);
                            sleep(Duration::from_secs(60)).await;
                            continue;
                        }
                    }
                }
                Err(e) => {
                    eprintln!("HTTP error for claim_codes: {}", e);
                    sleep(Duration::from_secs(60)).await;
                    continue;
                }
            },
            Err(e) => {
                eprintln!("Request error for claim_codes: {}", e);
                sleep(Duration::from_secs(60)).await;
                continue;
            }
        };

        let db = db();
        let mut new_last_id = last_id;
        let mut new_claimed_since = claimed_since.clone();

        let got_codes = claim_codes.len();

        for code in claim_codes {
            if code.id > new_last_id {
                new_last_id = code.id;
            }

            if let Some(ref ca) = code.claimed_at {
                if *ca > new_claimed_since {
                    new_claimed_since = ca.clone();
                }
            }

            let claimed = code.claimed as i32;

            if let Err(e) = sqlx::query!(
                r#"INSERT OR REPLACE INTO claim_codes
                   (id, code, category, claimed, claimed_by, claimed_at)
                   VALUES (?, ?, ?, ?, ?, ?)"#,
                code.id,
                code.code,
                code.category,
                claimed,
                code.claimed_by,
                code.claimed_at,
            )
            .execute(db)
            .await
            {
                eprintln!("Failed to insert claim code {}: {}", code.id, e);
                continue;
            }

            println!(
                "Synced claim code {} (category: {})",
                code.id, code.category
            );
        }

        misc::set("claim-code-sync-last-id", new_last_id)
            .await
            .expect("Saving sync last_id failed");
        misc::set("claim-code-sync-claimed-since", new_claimed_since)
            .await
            .expect("Saving sync claimed_since failed");

        if got_codes == 0 {
            println!("No new claim codes to sync");
        }

        if got_codes == 500 {
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

    // One-off Constant Contact operations, so that a single account can be
    // inspected or pushed by hand without standing the whole service up. Gated
    // on the exact first argument, so nothing else the service is started with
    // is affected.
    let mut arguments = std::env::args().skip(1);

    if arguments.next().as_deref() == Some("constant-contact") {
        let what = arguments.next();
        let argument = arguments.next();

        constant_contact::command(what.as_deref(), argument.as_deref()).await;

        return;
    }

    let cleanup_task = spawn(cleanup());

    let analyze_task = spawn(analyze_tables());

    let sessions_sync_task = spawn(sessions_synchronize());

    let users_sync_task = spawn(users_synchronize());

    let userbadges_sync_task = spawn(userbadges_synchronize());

    let _joins_sync_task = spawn(joins_synchronize());

    let _bookmarks_sync_task = spawn(bookmarks_synchronize());

    let _certificates_sync_task = spawn(certificates_synchronize());

    let _workflow_states_sync_task = spawn(workflow_states_synchronize());

    let _claim_codes_sync_task = spawn(claim_codes_synchronize());

    let _constant_contact_task = spawn(constant_contact::sweep_periodically());

    let _constant_contact_reconcile_task = spawn(constant_contact::reconcile_periodically());

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
    toplevel.spawn(handler!(search, "by_email", users::user_by_email));
    toplevel.spawn(handler!(search, "recipients", recipient_search::search));

    let session = service.group("user.session");
    toplevel.spawn(recv_handler!(session, "store", sessions::store));
    toplevel.spawn(handler!(session, "retrieve", sessions::retrieve));
    toplevel.spawn(recv_handler!(session, "delete", sessions::delete));

    let auth = service.group("user.auth");
    toplevel.spawn(handler!(auth, "signup", auth::signup));
    toplevel.spawn(handler!(auth, "defer_signup", auth::defer_signup));
    toplevel.spawn(handler!(auth, "complete_signup", auth::complete_signup));
    toplevel.spawn(handler!(auth, "login", auth::login));
    toplevel.spawn(handler!(auth, "snm_login", auth::snm_login));

    let tags = service.group("user.tags");
    toplevel.spawn(handler!(tags, "get", users::get_user_tags));
    toplevel.spawn(recv_handler!(tags, "set", users::set_user_tags));
    toplevel.spawn(recv_handler!(tags, "add", users::add_user_tag));
    toplevel.spawn(recv_handler!(tags, "add_signup", users::add_signup_tag));
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
    toplevel.spawn(handler!(admin, "user_list", users::user_details_list));
    toplevel.spawn(handler!(admin, "user_search", users::user_details_search));
    toplevel.spawn(handler!(admin, "user_export", users::user_details_export));
    toplevel.spawn(handler!(admin, "user_get", users::user_details_get));
    toplevel.spawn(handler!(
        admin,
        "user_batch_get",
        users::user_details_batch_get
    ));
    toplevel.spawn(handler!(admin, "user_create", users::user_details_create));
    toplevel.spawn(handler!(admin, "user_update", users::user_details_update));
    toplevel.spawn(handler!(admin, "user_delete", users::user_details_delete));
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

    // Community endpoints
    let communities = service.group("user.communities");
    toplevel.spawn(handler!(communities, "list", communities::list));
    toplevel.spawn(handler!(communities, "get", communities::get));
    toplevel.spawn(handler!(communities, "create", communities::create));
    toplevel.spawn(handler!(communities, "update", communities::update));
    toplevel.spawn(handler!(communities, "delete", communities::delete));
    toplevel.spawn(handler!(communities, "join", communities::join));
    toplevel.spawn(handler!(communities, "leave", communities::leave));
    toplevel.spawn(handler!(
        communities,
        "check",
        communities::check_membership
    ));
    toplevel.spawn(handler!(communities, "members", communities::members));
    toplevel.spawn(handler!(
        communities,
        "user_list",
        communities::user_communities
    ));

    // Bookmark endpoints
    let bookmarks = service.group("user.bookmarks");
    toplevel.spawn(handler!(bookmarks, "create", bookmarks::create_bookmark));
    toplevel.spawn(handler!(bookmarks, "check", bookmarks::check_bookmark));
    toplevel.spawn(handler!(bookmarks, "list_by_user", bookmarks::list_by_user));
    toplevel.spawn(handler!(
        bookmarks,
        "count_by_projects",
        bookmarks::count_by_projects
    ));

    // Project join endpoints
    let joins = service.group("user.joins");
    toplevel.spawn(handler!(joins, "create", project_joins::create_join));
    toplevel.spawn(handler!(joins, "check", project_joins::check_join));
    toplevel.spawn(handler!(
        joins,
        "list_by_user",
        project_joins::list_user_joins
    ));
    toplevel.spawn(handler!(
        joins,
        "list_by_user_id",
        project_joins::list_by_user_id
    ));
    toplevel.spawn(handler!(
        joins,
        "list_by_project_id",
        project_joins::list_by_project_id
    ));
    toplevel.spawn(handler!(
        joins,
        "count_by_projects",
        project_joins::count_by_projects
    ));

    let permissions = service.group("user.permissions");
    toplevel.spawn(handler!(
        permissions,
        "check",
        permissions::check_permission
    ));
    toplevel.spawn(handler!(
        permissions,
        "grant",
        permissions::grant_permission
    ));
    toplevel.spawn(handler!(
        permissions,
        "revoke",
        permissions::revoke_permission
    ));
    toplevel.spawn(handler!(permissions, "list", permissions::list_permissions));
    toplevel.spawn(handler!(
        permissions,
        "list_by_resource",
        permissions::list_permissions_by_resource
    ));

    // User matching for API participation recording
    let user_match = service.group("user");
    toplevel.spawn(handler!(user_match, "match", user_matching::match_user));

    // API key management
    let api_key = service.group("user.api_key");
    toplevel.spawn(handler!(api_key, "get", user_matching::get_api_key));
    toplevel.spawn(handler!(
        api_key,
        "validate",
        user_matching::validate_api_key
    ));

    // Opaque ID retrieval
    let opaque_id = service.group("user.opaque_id");
    toplevel.spawn(handler!(opaque_id, "get", user_matching::get_opaque_id));

    // Profile ID lookup and API user creation (v1 API compat)
    let profile = service.group("user.profile");
    toplevel.spawn(handler!(
        profile,
        "lookup",
        user_matching::lookup_profile_id
    ));
    toplevel.spawn(handler!(
        profile,
        "api_create",
        user_matching::api_create_user
    ));
    toplevel.spawn(handler!(
        profile,
        "ai_opted_out",
        user_matching::ai_opted_out
    ));
    toplevel.spawn(handler!(
        profile,
        "ai_opt_out_status",
        user_matching::ai_opt_out_status
    ));
    toplevel.spawn(handler!(profile, "get", profile::get));
    toplevel.spawn(handler!(profile, "save", profile::save));
    toplevel.spawn(handler!(profile, "public_get", profile::public_get));

    // Girl Scouts Take Action write-ups
    let girlscouts_group = service.group("user.girlscouts");
    toplevel.spawn(handler!(
        girlscouts_group,
        "take_action_submit",
        girlscouts::take_action_submit
    ));
    toplevel.spawn(handler!(
        girlscouts_group,
        "take_action_list",
        girlscouts::take_action_list
    ));
    toplevel.spawn(handler!(
        girlscouts_group,
        "take_action_delete",
        girlscouts::take_action_delete
    ));

    // Ambassador application
    let ambassador_group = service.group("user.ambassador");
    toplevel.spawn(handler!(ambassador_group, "get", ambassador::get));
    toplevel.spawn(handler!(ambassador_group, "update", ambassador::update));
    toplevel.spawn(handler!(
        ambassador_group,
        "save_draft",
        ambassador::save_draft
    ));
    toplevel.spawn(handler!(ambassador_group, "submit", ambassador::submit));
    toplevel.spawn(handler!(ambassador_group, "list", ambassador::list));
    toplevel.spawn(handler!(ambassador_group, "review", ambassador::review));
    toplevel.spawn(handler!(
        ambassador_group,
        "directory",
        ambassador::directory
    ));
    toplevel.spawn(handler!(ambassador_group, "profile", ambassador::profile));
    toplevel.spawn(handler!(
        ambassador_group,
        "message_send",
        ambassador::message_send
    ));
    toplevel.spawn(handler!(
        ambassador_group,
        "message_list",
        ambassador::message_list
    ));
    toplevel.spawn(handler!(
        ambassador_group,
        "message_review",
        ambassador::message_review
    ));
    toplevel.spawn(handler!(
        ambassador_group,
        "passed_locations",
        ambassador::passed_locations
    ));

    // Legacy user-id lookup (User-level legacy ids only — distinct from
    // user.profile.lookup which handles Profile-level legacy ids).
    let legacy_id_group = service.group("user.legacy_id");
    toplevel.spawn(handler!(legacy_id_group, "lookup", legacy_id::lookup));

    // User-saved point-of-interest places
    let places_group = service.group("user.places");
    toplevel.spawn(handler!(places_group, "list_by_user", places::list_by_user));
    toplevel.spawn(handler!(places_group, "add", places::add));
    toplevel.spawn(handler!(places_group, "delete", places::delete));

    // User-issued API link tokens
    let link_tokens_group = service.group("user.link_tokens");
    toplevel.spawn(handler!(
        link_tokens_group,
        "list_by_user",
        link_tokens::list_by_user
    ));
    toplevel.spawn(handler!(link_tokens_group, "add", link_tokens::add));
    toplevel.spawn(handler!(link_tokens_group, "delete", link_tokens::delete));

    // Workflow progress tracking
    let workflow_group = service.group("user.workflow");
    toplevel.spawn(handler!(
        workflow_group,
        "progress",
        workflow::record_progress
    ));
    toplevel.spawn(handler!(workflow_group, "status", workflow::get_status));
    toplevel.spawn(handler!(
        workflow_group,
        "tutorial_count",
        workflow::tutorial_count
    ));
    toplevel.spawn(handler!(workflow_group, "funnel", workflow::funnel));
    toplevel.spawn(handler!(
        workflow_group,
        "training_states",
        workflow::training_states
    ));

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
    toplevel.spawn(handler!(
        address_sets,
        "add_member",
        address_sets::add_member
    ));
    toplevel.spawn(handler!(
        address_sets,
        "remove_member",
        address_sets::remove_member
    ));

    // User sets endpoints
    let user_sets = service.group("user.user_sets");
    toplevel.spawn(handler!(user_sets, "list", user_sets::list));
    toplevel.spawn(handler!(user_sets, "get", user_sets::get));
    toplevel.spawn(handler!(user_sets, "create", user_sets::create));
    toplevel.spawn(handler!(user_sets, "update", user_sets::update));
    toplevel.spawn(handler!(user_sets, "delete", user_sets::delete));
    toplevel.spawn(handler!(user_sets, "get_detail", user_sets::get_detail));
    toplevel.spawn(handler!(
        user_sets,
        "add_criterion",
        user_sets::add_criterion
    ));
    toplevel.spawn(handler!(
        user_sets,
        "remove_criterion",
        user_sets::remove_criterion
    ));
    toplevel.spawn(handler!(
        user_sets,
        "release_results",
        user_sets::release_results
    ));

    let geocode = service.group("user.geocode");
    toplevel.spawn(handler!(geocode, "search", geocode::geocode));
    toplevel.spawn(handler!(geocode, "resolve", geocode::resolve));
    toplevel.spawn(handler!(geocode, "forward", geocode::forward));
    toplevel.spawn(handler!(geocode, "reverse", geocode::reverse));

    // Certificate endpoints
    let certs = service.group("user.certificates");
    toplevel.spawn(handler!(certs, "create", certificates::create));
    toplevel.spawn(handler!(certs, "check", certificates::check));
    toplevel.spawn(handler!(certs, "list", certificates::list));
    toplevel.spawn(handler!(certs, "get", certificates::get));

    // iNaturalist endpoints
    let inat = service.group("user.inat");
    toplevel.spawn(handler!(inat, "list_users", inat::list_users));
    toplevel.spawn(handler!(inat, "username_set", inat::username_set));
    toplevel.spawn(recv_handler!(
        inat,
        "update_last_sync",
        inat::update_last_sync
    ));

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
