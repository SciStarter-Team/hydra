use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use common::linked_platform;
use common::message::user::{InatListUsersRequest, InatUpdateLastSync, InatUser};
use jiff::Timestamp;
use serde::Deserialize;
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;

use log_service::database::{Database, tables::ParticipationRow};

const INATURALIST_ORIGIN: i16 = 5;
const INAT_USER_PAGE_SIZE: i64 = 500;

#[derive(Debug, Deserialize)]
struct InatObservation {
    #[allow(dead_code)]
    id: i64,
    created_at: String,
    project_ids: Option<Vec<i64>>,
}

#[derive(Debug, Deserialize)]
struct InatApiResponse {
    results: Vec<InatObservation>,
}

struct ProjectMapping {
    entity_slug: String,
    entity_id: String,
    inat_project_id: i64,
}

async fn process_user(
    user: &InatUser,
    database: &Database,
    project_mappings: &[ProjectMapping],
    client: &reqwest::Client,
    cancellation_token: &CancellationToken,
) {
    // Walk newest-first and stop at whichever is later: how far the last
    // sweep reached, or the moment the account was connected. The connection
    // is a floor because SciStarter stopped backfilling what happened on a
    // platform before it was linked here -- so an account connected today
    // starts today, however far back iNaturalist is willing to go.
    //
    // Compared as instants rather than as strings: iNaturalist reports an
    // offset and the stored connection time a Z, and "+00:00" sorts before
    // "Z" whatever the instants are.
    let floor = linked_platform::import_floor(user.inat_linked_at.as_deref());
    let since = user
        .inat_last_update
        .as_deref()
        .and_then(|w| Timestamp::from_str(w).ok())
        .map_or(floor, |watermark| watermark.max(floor));

    let mut page = 1;
    let mut max_time = since;
    let mut going = true;
    let user_id_str = user.user_id.to_string();

    while going {
        if cancellation_token.is_cancelled() {
            return;
        }

        // Rate limit: 5 seconds between requests
        sleep(Duration::from_secs(5)).await;

        let resp = match client
            .get("https://api.inaturalist.org/v1/observations")
            .query(&[
                ("order", "desc"),
                ("order_by", "created_at"),
                ("user_login", user.inat_username.as_str()),
                ("per_page", "200"),
            ])
            .query(&[("page", &page.to_string())])
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                eprintln!("iNat API error for {}: {}", user.inat_username, e);
                break;
            }
        };

        if resp.status().as_u16() == 429 {
            eprintln!("iNat rate limit hit, backing off");
            sleep(Duration::from_secs(60 * 5)).await;
            continue;
        }

        let data: InatApiResponse = match resp.error_for_status() {
            Ok(r) => match r.json().await {
                Ok(d) => d,
                Err(e) => {
                    eprintln!("iNat JSON parse error for {}: {}", user.inat_username, e);
                    break;
                }
            },
            Err(e) => {
                eprintln!(
                    "iNat HTTP error for {}: {} — skipping user",
                    user.inat_username, e
                );
                return;
            }
        };

        if data.results.is_empty() {
            break;
        }

        for obs in &data.results {
            let observed = match Timestamp::from_str(&obs.created_at) {
                Ok(observed) => observed,
                Err(e) => {
                    eprintln!(
                        "Unparseable iNat observation time {:?} for {}: {}",
                        obs.created_at, user.inat_username, e
                    );
                    continue;
                }
            };

            if observed < since {
                going = false;
                break;
            }

            if observed > max_time {
                max_time = observed;
            }

            // Resolve iNat project IDs to entity IDs
            let mut entity_ids: Vec<String> = Vec::new();

            if let Some(project_ids) = &obs.project_ids {
                for inat_pid in project_ids {
                    if let Some(mapping) = project_mappings
                        .iter()
                        .find(|m| m.inat_project_id == *inat_pid)
                    {
                        entity_ids.push(mapping.entity_id.clone());
                    }
                }
            }

            // Default to "inaturalist" slug if no matching projects
            if entity_ids.is_empty() {
                let fallback = project_mappings
                    .iter()
                    .find(|m| m.entity_slug == "inaturalist")
                    .map(|m| m.entity_id.clone())
                    .unwrap_or_else(|| "inaturalist".to_string());
                entity_ids.push(fallback);
            }

            for entity_id in entity_ids {
                let row = ParticipationRow {
                    origin: INATURALIST_ORIGIN,
                    when: obs.created_at.clone(),
                    duration: 1.0,
                    where_point: None,
                    magnitude: 1,
                    authenticated: true,
                    extra: String::new(),
                    fidelity: 0.5,
                    entity_id,
                    user_id: Some(user_id_str.clone()),
                    task_name: None,
                };

                if let Err(e) = database.participation.insert(row).await {
                    eprintln!("Error inserting iNat participation: {}", e);
                }
            }
        }

        page += 1;
    }

    // Update the user's inat_last_update timestamp
    if max_time > since {
        if let Err(e) = common::service::send(
            "user.inat.update_last_sync",
            InatUpdateLastSync {
                user_id: user.user_id,
                inat_last_update: max_time.to_string(),
            },
        )
        .await
        {
            eprintln!("Error updating iNat last sync for {}: {}", user.user_id, e);
        }
    }
}

/// Poll iNaturalist API for observations and write participation records to WAL
pub async fn inaturalist_synchronize(
    database: Arc<Database>,
    cancellation_token: CancellationToken,
) {
    loop {
        if cancellation_token.is_cancelled() {
            return;
        }

        // Load iNat project mappings
        let project_mappings: Vec<ProjectMapping> = sqlx::query_as!(
            ProjectMapping,
            "SELECT entity_slug, entity_id, inat_project_id FROM inaturalist_projects"
        )
        .fetch_all(common::shared::db())
        .await
        .unwrap_or_default();

        let client = reqwest::Client::new();
        let mut offset: i64 = 0;
        let mut processed_any = false;

        // Page through iNat users, processing each batch before requesting the next
        loop {
            if cancellation_token.is_cancelled() {
                return;
            }

            let users: Vec<InatUser> = match common::service::request(
                "user.inat.list_users",
                InatListUsersRequest {
                    offset,
                    limit: INAT_USER_PAGE_SIZE,
                },
            )
            .await
            {
                Ok(users) => users,
                Err(e) => {
                    eprintln!("Error fetching iNat users (offset {}): {}", offset, e);
                    break;
                }
            };

            let batch_len = users.len() as i64;

            if users.is_empty() && !processed_any {
                // No iNat users at all, sleep longer
                tokio::select! {
                    _ = sleep(Duration::from_secs(60 * 60)) => {}
                    _ = cancellation_token.cancelled() => { return; }
                }
                break;
            }

            for user in &users {
                if cancellation_token.is_cancelled() {
                    return;
                }
                process_user(
                    user,
                    &database,
                    &project_mappings,
                    &client,
                    &cancellation_token,
                )
                .await;
                processed_any = true;
            }

            if batch_len < INAT_USER_PAGE_SIZE {
                break;
            }

            offset += INAT_USER_PAGE_SIZE;
        }

        // Poll every 6 hours
        tokio::select! {
            _ = sleep(Duration::from_secs(6 * 60 * 60)) => {}
            _ = cancellation_token.cancelled() => { return; }
        }
    }
}
