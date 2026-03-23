use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use serde::Deserialize;
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;

use log_service::database::{
    Database, get_sync_last_id, set_sync_last_id, tables::ParticipationRow,
};

const ZOONIVERSE_ORIGIN: i16 = 10; // Distinct origin for Zooniverse-sourced participation

#[derive(Debug, Deserialize)]
struct OAuthTokenResponse {
    access_token: String,
}

#[derive(Debug, Deserialize)]
struct ClassificationsResponse {
    classifications: Vec<Classification>,
}

#[derive(Debug, Deserialize)]
struct Classification {
    id: String,
    created_at: String,
    links: ClassificationLinks,
}

#[derive(Debug, Deserialize)]
struct ClassificationLinks {
    #[allow(dead_code)]
    project: String,
    user: Option<String>,
}

async fn get_zooniverse_token(client: &reqwest::Client) -> Result<String> {
    let zoo_id = env!("ZOONIVERSE_ID");
    let zoo_secret = env!("ZOONIVERSE_SECRET");

    let resp: OAuthTokenResponse = client
        .post("https://panoptes.zooniverse.org/oauth/token")
        .form(&[
            ("grant_type", "client_credentials"),
            ("client_id", zoo_id),
            ("client_secret", zoo_secret),
        ])
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    Ok(resp.access_token)
}

/// Synchronize Zooniverse project and user mappings from orrery
pub async fn zooniverse_sync_projects(
    database: Arc<Database>,
    cancellation_token: CancellationToken,
) {
    #[derive(Debug, Deserialize)]
    struct ZooniverseProjectData {
        id: i32,
        zooniverse_id: i64,
        entity_ulid: String,
        last_classification_id: i64,
    }

    loop {
        if cancellation_token.is_cancelled() {
            return;
        }

        let last_id = get_sync_last_id(&database, "zooniverse-project-sync-last-id")
            .await
            .unwrap_or_else(|_| 0);

        let client = reqwest::Client::new();
        let sync_token = env!("SYNC_TOKEN");

        let projects: Vec<ZooniverseProjectData> = match client
            .get("https://scistarter.org/staff/sync/zooniverse_projects")
            .bearer_auth(sync_token)
            .query(&[
                ("since", "1900-01-01T00:00:00Z"),
                ("last_id", &last_id.to_string()),
            ])
            .send()
            .await
        {
            Ok(response) => match response.error_for_status() {
                Ok(resp) => match resp.json().await {
                    Ok(data) => data,
                    Err(e) => {
                        eprintln!("JSON parsing error for zooniverse projects: {}", e);
                        sleep(Duration::from_secs(60)).await;
                        continue;
                    }
                },
                Err(e) => {
                    eprintln!("HTTP error syncing zooniverse projects: {}", e);
                    sleep(Duration::from_secs(60)).await;
                    continue;
                }
            },
            Err(e) => {
                eprintln!("Request error syncing zooniverse projects: {}", e);
                sleep(Duration::from_secs(60)).await;
                continue;
            }
        };

        let mut max_id = last_id;

        for project in &projects {
            if project.id > max_id {
                max_id = project.id;
            }
            let zoo_id = project.zooniverse_id;
            let last_class = project.last_classification_id;
            sqlx::query!(
                "INSERT OR REPLACE INTO zooniverse_projects (zooniverse_id, entity_id, last_classification_id) VALUES (?, ?, ?)",
                zoo_id,
                project.entity_ulid,
                last_class,
            )
            .execute(common::shared::db())
            .await
            .ok();
        }

        set_sync_last_id(&database, "zooniverse-project-sync-last-id", max_id)
            .await
            .ok();

        let sleep_duration = if projects.len() >= 500 {
            Duration::from_secs(60)
        } else {
            Duration::from_secs(60 * 60)
        };

        tokio::select! {
            _ = sleep(sleep_duration) => {}
            _ = cancellation_token.cancelled() => { return; }
        }
    }
}

/// Synchronize Zooniverse user mappings from orrery
pub async fn zooniverse_sync_users(
    _database: Arc<Database>,
    cancellation_token: CancellationToken,
) {
    #[derive(Debug, Deserialize)]
    struct ZooniverseUserData {
        id: i32,
        zooniverse_id: i64,
        zooniverse_username: String,
        user_ulid: Option<String>,
    }

    loop {
        if cancellation_token.is_cancelled() {
            return;
        }

        let client = reqwest::Client::new();
        let sync_token = env!("SYNC_TOKEN");

        // Full-replace: paginate through all records starting from id 0
        let mut last_id: i32 = 0;

        loop {
            if cancellation_token.is_cancelled() {
                return;
            }

            let users: Vec<ZooniverseUserData> = match client
                .get("https://scistarter.org/staff/sync/zooniverse_users")
                .bearer_auth(sync_token)
                .query(&[
                    ("since", "1900-01-01T00:00:00Z"),
                    ("last_id", &last_id.to_string()),
                ])
                .send()
                .await
            {
                Ok(response) => match response.error_for_status() {
                    Ok(resp) => match resp.json().await {
                        Ok(data) => data,
                        Err(e) => {
                            eprintln!("JSON parsing error for zooniverse users: {}", e);
                            break;
                        }
                    },
                    Err(e) => {
                        eprintln!("HTTP error syncing zooniverse users: {}", e);
                        break;
                    }
                },
                Err(e) => {
                    eprintln!("Request error syncing zooniverse users: {}", e);
                    break;
                }
            };

            if users.is_empty() {
                break;
            }

            for user in &users {
                if user.id > last_id {
                    last_id = user.id;
                }
                let zoo_id = user.zooniverse_id;
                sqlx::query!(
                    "INSERT OR REPLACE INTO zooniverse_users (zooniverse_id, zooniverse_username, user_id) VALUES (?, ?, ?)",
                    zoo_id,
                    user.zooniverse_username,
                    user.user_ulid,
                )
                .execute(common::shared::db())
                .await
                .ok();
            }

            if users.len() < 500 {
                break;
            }
        }

        // Only sync once per day since this is a full-replace
        tokio::select! {
            _ = sleep(Duration::from_secs(24 * 60 * 60)) => {}
            _ = cancellation_token.cancelled() => { return; }
        }
    }
}

/// Poll Zooniverse API for new classifications and write participation records to WAL
pub async fn zooniverse_synchronize(
    database: Arc<Database>,
    cancellation_token: CancellationToken,
) {
    loop {
        if cancellation_token.is_cancelled() {
            return;
        }

        // Load all tracked projects
        struct ProjectRow {
            zooniverse_id: i64,
            entity_id: String,
            last_classification_id: i64,
        }

        let projects = match sqlx::query_as!(
            ProjectRow,
            "SELECT zooniverse_id, entity_id, last_classification_id FROM zooniverse_projects"
        )
        .fetch_all(common::shared::db())
        .await
        {
            Ok(p) => p,
            Err(e) => {
                eprintln!("Error loading zooniverse projects: {}", e);
                sleep(Duration::from_secs(60 * 5)).await;
                continue;
            }
        };

        if projects.is_empty() {
            tokio::select! {
                _ = sleep(Duration::from_secs(60 * 60)) => { continue; }
                _ = cancellation_token.cancelled() => { return; }
            }
        }

        let client = reqwest::Client::new();
        let token = match get_zooniverse_token(&client).await {
            Ok(t) => t,
            Err(e) => {
                eprintln!("Failed to get Zooniverse OAuth token: {}", e);
                sleep(Duration::from_secs(60 * 5)).await;
                continue;
            }
        };

        'projects: for project in projects {
            if cancellation_token.is_cancelled() {
                return;
            }

            let mut max_id = project.last_classification_id;
            let mut page: u32 = 1;

            loop {
                let resp = match client
                    .get("https://panoptes.zooniverse.org/api/classifications/project")
                    .bearer_auth(&token)
                    .header("Accept", "application/vnd.api+json; version=1")
                    .query(&[
                        ("project_id", &project.zooniverse_id.to_string()),
                        ("page_size", &"200".to_string()),
                        ("last_id", &project.last_classification_id.to_string()),
                        ("page", &page.to_string()),
                        ("sort", &"-created_at".to_string()),
                    ])
                    .send()
                    .await
                {
                    Ok(r) => r,
                    Err(e) => {
                        eprintln!(
                            "Zooniverse API error for project {}: {}",
                            project.zooniverse_id, e
                        );
                        continue 'projects;
                    }
                };

                if resp.status() == 429 {
                    eprintln!("Zooniverse rate limit hit, backing off");
                    sleep(Duration::from_secs(60)).await;
                    continue 'projects;
                }

                let classifications: ClassificationsResponse = match resp.error_for_status() {
                    Ok(r) => match r.json().await {
                        Ok(c) => c,
                        Err(e) => {
                            eprintln!("Zooniverse JSON parse error: {}", e);
                            continue 'projects;
                        }
                    },
                    Err(e) => {
                        eprintln!("Zooniverse HTTP error: {}", e);
                        continue 'projects;
                    }
                };

                if classifications.classifications.is_empty() {
                    break;
                }

                let mut found_new = false;

                for classification in &classifications.classifications {
                    let class_id: i64 = classification.id.parse().unwrap_or(0);
                    if class_id > max_id {
                        max_id = class_id;
                    }
                    if class_id > project.last_classification_id {
                        found_new = true;
                    }

                    // Resolve zooniverse user to hydra user_id
                    let user_id = if let Some(zoo_user_id) = &classification.links.user {
                        let zoo_uid: i64 = zoo_user_id.parse().unwrap_or(0);
                        sqlx::query_scalar!(
                            "SELECT user_id FROM zooniverse_users WHERE zooniverse_id = ?",
                            zoo_uid,
                        )
                        .fetch_optional(common::shared::db())
                        .await
                        .ok()
                        .flatten()
                        .flatten()
                    } else {
                        None
                    };

                    let row = ParticipationRow {
                        origin: ZOONIVERSE_ORIGIN,
                        when: classification.created_at.clone(),
                        duration: 1.0,
                        where_point: None,
                        magnitude: 1,
                        authenticated: true,
                        extra: String::new(),
                        fidelity: 0.5,
                        entity_id: project.entity_id.clone(),
                        user_id,
                        task_name: None,
                    };

                    if let Err(e) = database.participation.insert(row).await {
                        eprintln!("Error inserting zooniverse participation: {}", e);
                    }
                }

                if !found_new || classifications.classifications.len() < 200 {
                    break;
                }

                page += 1;
            }

            // Update last_classification_id
            if max_id > project.last_classification_id {
                sqlx::query!(
                    "UPDATE zooniverse_projects SET last_classification_id = ? WHERE zooniverse_id = ?",
                    max_id,
                    project.zooniverse_id,
                )
                .execute(common::shared::db())
                .await
                .ok();
            }
        }

        // Poll every 15 minutes
        tokio::select! {
            _ = sleep(Duration::from_secs(15 * 60)) => {}
            _ = cancellation_token.cancelled() => { return; }
        }
    }
}
