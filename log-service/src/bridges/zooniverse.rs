use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use serde::Deserialize;
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;

use log_service::database::{
    Database, get_sync_last_id, set_sync_last_id, tables::ParticipationRow,
};

const ZOONIVERSE_ORIGIN: i16 = 10;
const PAGE_SIZE: u32 = 200;
const REQUEST_DELAY: Duration = Duration::from_millis(500);

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

/// Synchronize Zooniverse project mappings from orrery.
///
/// Project rows are append-only on the orrery side, so we track an `id`
/// watermark and pull forward. The local copy of `last_classification_id`
/// is owned by hydra after initial setup, so we only insert it for new
/// projects and never overwrite an existing value.
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
            .unwrap_or(0);

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
            // INSERT OR IGNORE so we never roll the local last_classification_id
            // backwards if hydra has already advanced it past orrery's value.
            sqlx::query!(
                "INSERT OR IGNORE INTO zooniverse_projects (zooniverse_id, entity_id, last_classification_id) VALUES (?, ?, ?)",
                project.zooniverse_id,
                project.entity_ulid,
                project.last_classification_id,
            )
            .execute(common::shared::db())
            .await
            .ok();
        }

        if max_id > last_id {
            set_sync_last_id(&database, "zooniverse-project-sync-last-id", max_id)
                .await
                .ok();
        }

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

/// Synchronize Zooniverse user mappings from orrery (full replace).
///
/// One Zooniverse account can be claimed by multiple SciStarter users; the
/// orrery endpoint emits an array of matching user ULIDs per Zooniverse
/// account. We store one row per (zooniverse_id, user_id) pair so the
/// classification importer can fan participation out to all of them.
///
/// The download streams into `zooniverse_users_staging` and only replaces
/// the live table once the full download succeeds. A failure mid-download
/// leaves the previous live mapping intact.
pub async fn zooniverse_sync_users(
    _database: Arc<Database>,
    cancellation_token: CancellationToken,
) {
    #[derive(Debug, Deserialize)]
    struct ZooniverseUserData {
        id: i32,
        zooniverse_id: i64,
        zooniverse_username: String,
        user_ulids: Vec<String>,
    }

    loop {
        if cancellation_token.is_cancelled() {
            return;
        }

        let client = reqwest::Client::new();
        let sync_token = env!("SYNC_TOKEN");

        if let Err(e) = sqlx::query!("DELETE FROM zooniverse_users_staging")
            .execute(common::shared::db())
            .await
        {
            eprintln!("Error clearing zooniverse_users_staging: {}", e);
            sleep(Duration::from_secs(60)).await;
            continue;
        }

        let mut last_id: i32 = 0;
        let mut download_ok = true;

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
                            download_ok = false;
                            break;
                        }
                    },
                    Err(e) => {
                        eprintln!("HTTP error syncing zooniverse users: {}", e);
                        download_ok = false;
                        break;
                    }
                },
                Err(e) => {
                    eprintln!("Request error syncing zooniverse users: {}", e);
                    download_ok = false;
                    break;
                }
            };

            if users.is_empty() {
                break;
            }

            let batch_done = users.len() < 500;

            let staged = async {
                let mut tx = common::shared::db().begin().await?;
                for user in &users {
                    if user.user_ulids.is_empty() {
                        // No SciStarter user has claimed this Zooniverse
                        // account yet; record the username mapping with a
                        // null user_id so we can still resolve the username
                        // when one is added later.
                        sqlx::query!(
                            "INSERT OR IGNORE INTO zooniverse_users_staging \
                                (zooniverse_id, zooniverse_username, user_id) \
                             VALUES (?, ?, NULL)",
                            user.zooniverse_id,
                            user.zooniverse_username,
                        )
                        .execute(&mut *tx)
                        .await?;
                    } else {
                        for user_ulid in &user.user_ulids {
                            sqlx::query!(
                                "INSERT OR IGNORE INTO zooniverse_users_staging \
                                    (zooniverse_id, zooniverse_username, user_id) \
                                 VALUES (?, ?, ?)",
                                user.zooniverse_id,
                                user.zooniverse_username,
                                user_ulid,
                            )
                            .execute(&mut *tx)
                            .await?;
                        }
                    }
                }
                tx.commit().await?;
                Ok::<_, sqlx::Error>(())
            }
            .await;

            if let Err(e) = staged {
                eprintln!("Error staging zooniverse_users batch: {}", e);
                download_ok = false;
                break;
            }

            for user in &users {
                if user.id > last_id {
                    last_id = user.id;
                }
            }

            if batch_done {
                break;
            }
        }

        if download_ok {
            let swap = async {
                let mut tx = common::shared::db().begin().await?;
                sqlx::query!("DELETE FROM zooniverse_users")
                    .execute(&mut *tx)
                    .await?;
                sqlx::query!(
                    "INSERT INTO zooniverse_users (zooniverse_id, zooniverse_username, user_id) \
                     SELECT zooniverse_id, zooniverse_username, user_id FROM zooniverse_users_staging"
                )
                .execute(&mut *tx)
                .await?;
                tx.commit().await?;
                Ok::<_, sqlx::Error>(())
            }
            .await;

            if let Err(e) = swap {
                eprintln!("Error swapping zooniverse_users from staging: {}", e);
            }
        }

        // Full-replace once a day.
        tokio::select! {
            _ = sleep(Duration::from_secs(24 * 60 * 60)) => {}
            _ = cancellation_token.cancelled() => { return; }
        }
    }
}

/// Poll Zooniverse for new classifications and write participation rows.
///
/// Pagination is `last_id`-only: each page advances the watermark, which we
/// persist before fetching the next page. A crash caps re-imports at the
/// in-flight page rather than the entire project history. Each classification
/// is fanned out to every SciStarter user that claims the Zooniverse account.
pub async fn zooniverse_synchronize(
    database: Arc<Database>,
    cancellation_token: CancellationToken,
) {
    loop {
        if cancellation_token.is_cancelled() {
            return;
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

        for project in projects {
            if cancellation_token.is_cancelled() {
                return;
            }

            sync_project(&database, &client, &token, project, &cancellation_token).await;
        }

        tokio::select! {
            _ = sleep(Duration::from_secs(15 * 60)) => {}
            _ = cancellation_token.cancelled() => { return; }
        }
    }
}

struct ProjectRow {
    zooniverse_id: i64,
    entity_id: String,
    last_classification_id: i64,
}

async fn sync_project(
    database: &Database,
    client: &reqwest::Client,
    token: &str,
    project: ProjectRow,
    cancellation_token: &CancellationToken,
) {
    let ProjectRow {
        zooniverse_id,
        entity_id,
        mut last_classification_id,
    } = project;

    loop {
        if cancellation_token.is_cancelled() {
            return;
        }

        sleep(REQUEST_DELAY).await;

        let resp = match client
            .get("https://panoptes.zooniverse.org/api/classifications/project")
            .bearer_auth(token)
            .header("Accept", "application/vnd.api+json; version=1")
            .query(&[
                ("project_id", &zooniverse_id.to_string()),
                ("page_size", &PAGE_SIZE.to_string()),
                ("last_id", &last_classification_id.to_string()),
            ])
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                eprintln!("Zooniverse API error for project {}: {}", zooniverse_id, e);
                return;
            }
        };

        if resp.status() == 429 {
            eprintln!("Zooniverse rate limit hit, backing off");
            sleep(Duration::from_secs(60)).await;
            continue;
        }

        let classifications: ClassificationsResponse = match resp.error_for_status() {
            Ok(r) => match r.json().await {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Zooniverse JSON parse error: {}", e);
                    return;
                }
            },
            Err(e) => {
                eprintln!("Zooniverse HTTP error: {}", e);
                return;
            }
        };

        if classifications.classifications.is_empty() {
            return;
        }

        let mut page_max_id = last_classification_id;

        for classification in &classifications.classifications {
            let class_id = match classification.id.parse::<i64>() {
                Ok(id) => id,
                Err(_) => {
                    eprintln!(
                        "Skipping classification with unparseable id: {:?}",
                        classification.id
                    );
                    continue;
                }
            };

            if class_id > page_max_id {
                page_max_id = class_id;
            }

            let zoo_user_id = match classification.links.user.as_ref() {
                Some(s) => match s.parse::<i64>() {
                    Ok(id) => id,
                    Err(_) => {
                        eprintln!("Skipping classification {} with unparseable user id", class_id);
                        continue;
                    }
                },
                None => {
                    // Anonymous classification — no SciStarter user to credit.
                    continue;
                }
            };

            let user_ids: Vec<Option<String>> = match sqlx::query_scalar!(
                "SELECT user_id FROM zooniverse_users WHERE zooniverse_id = ?",
                zoo_user_id,
            )
            .fetch_all(common::shared::db())
            .await
            {
                Ok(rows) => rows,
                Err(e) => {
                    eprintln!("Error looking up zooniverse user {}: {}", zoo_user_id, e);
                    continue;
                }
            };

            // Filter to mappings that resolve to a SciStarter user. If the
            // Zooniverse user has no claimed SciStarter account, skip — a
            // classification with no user to credit is not useful here.
            let scistarter_users: Vec<String> = user_ids.into_iter().flatten().collect();
            if scistarter_users.is_empty() {
                continue;
            }

            for user_id in scistarter_users {
                let row = ParticipationRow {
                    origin: ZOONIVERSE_ORIGIN,
                    when: classification.created_at.clone(),
                    duration: 1.0,
                    where_point: None,
                    magnitude: 1,
                    authenticated: true,
                    extra: String::new(),
                    fidelity: 0.5,
                    entity_id: entity_id.clone(),
                    user_id: Some(user_id),
                    task_name: None,
                };

                if let Err(e) = database.participation.insert(row).await {
                    eprintln!("Error inserting zooniverse participation: {}", e);
                }
            }
        }

        // Persist progress after every page so a crash limits re-import to
        // the in-flight page rather than the whole project history.
        if page_max_id > last_classification_id {
            if let Err(e) = sqlx::query!(
                "UPDATE zooniverse_projects SET last_classification_id = ? \
                 WHERE zooniverse_id = ? AND last_classification_id < ?",
                page_max_id,
                zooniverse_id,
                page_max_id,
            )
            .execute(common::shared::db())
            .await
            {
                eprintln!(
                    "Error updating last_classification_id for {}: {}",
                    zooniverse_id, e
                );
            }
            last_classification_id = page_max_id;
        } else {
            // The API returned classifications but none of them had a
            // parseable id. Bail out rather than spin forever on the
            // same page.
            eprintln!(
                "No id progress on project {}; aborting this run",
                zooniverse_id
            );
            return;
        }

        if classifications.classifications.len() < PAGE_SIZE as usize {
            return;
        }
    }
}

