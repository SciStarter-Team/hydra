use std::collections::{HashMap, HashSet};
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use common::message::user::{InatListUsersRequest, InatProjectMap, InatUpdateLastSync, InatUser};
use common::{contribution_time, linked_platform};
use jiff::Timestamp;
use serde::Deserialize;
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;

use log_service::database::{Database, tables::ParticipationRow};
use log_service::ledger::{self, Feed};

use super::{Link, Platform};

const INATURALIST_ORIGIN: i16 = 5;

/// The origin orrery files its own iNaturalist imports under, "Import", as it
/// always has.
const ORRERY_ORIGINS: &[i16] = &[5];
const INAT_USER_PAGE_SIZE: i64 = 500;

#[derive(Debug, Deserialize)]
struct InatObservation {
    id: i64,
    created_at: String,
    project_ids: Option<Vec<i64>>,
}

#[derive(Debug, Deserialize)]
struct InatApiResponse {
    results: Vec<InatObservation>,
}

/// Which project an observation is credited to, fetched from the frontend at
/// the start of each sweep.
struct ProjectMapping {
    by_inat_project: HashMap<i64, String>,
    fallback: String,
}

impl ProjectMapping {
    async fn fetch() -> anyhow::Result<Self> {
        let map: InatProjectMap =
            common::service::request("frontend.inaturalist.projects", ()).await?;

        Ok(ProjectMapping {
            by_inat_project: map
                .projects
                .into_iter()
                .map(|project| (project.inat_project_id, project.project_id.to_string()))
                .collect(),
            fallback: map.fallback.to_string(),
        })
    }

    /// Every project an observation can be credited to.
    fn projects(&self) -> HashSet<String> {
        self.by_inat_project
            .values()
            .cloned()
            .chain([self.fallback.clone()])
            .collect()
    }

    /// The projects one observation is credited to: each mapped project it
    /// was recorded in, or the iNaturalist project when there are none.
    fn credited(&self, inat_project_ids: Option<&[i64]>) -> Vec<String> {
        let mapped: Vec<String> = inat_project_ids
            .unwrap_or_default()
            .iter()
            .filter_map(|inat_project_id| self.by_inat_project.get(inat_project_id).cloned())
            .collect();

        if mapped.is_empty() {
            vec![self.fallback.clone()]
        } else {
            mapped
        }
    }
}

/// What this bridge credits; see [`BridgeCoverage`](super::BridgeCoverage).
pub async fn coverage() -> anyhow::Result<Platform> {
    let mapping = ProjectMapping::fetch().await?;
    let mut links = HashMap::new();
    let mut offset: i64 = 0;

    loop {
        let users: Vec<InatUser> = common::service::request(
            "user.inat.list_users",
            InatListUsersRequest {
                offset,
                limit: INAT_USER_PAGE_SIZE,
            },
        )
        .await?;

        let page_len = users.len() as i64;

        for user in users {
            // Each account is walked from its own floor, whenever the
            // bridge comes to know of it, so nothing it connected for is
            // ever passed over.
            links.insert(
                user.user_id.to_string(),
                Link {
                    linked_at: user.inat_linked_at,
                    matched_since: None,
                },
            );
        }

        if page_len < INAT_USER_PAGE_SIZE {
            break;
        }

        offset += INAT_USER_PAGE_SIZE;
    }

    Ok(Platform {
        orrery_origins: ORRERY_ORIGINS,
        projects: mapping
            .projects()
            .into_iter()
            .map(|project| (project, None))
            .collect(),
        links,
    })
}

async fn process_user(
    user: &InatUser,
    database: &Database,
    project_mapping: &ProjectMapping,
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
    let sender = format!("iNaturalist user {}", user.inat_username);

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

        let received = Timestamp::now();

        for obs in &data.results {
            // The recorded time, not the reported one, also moves the
            // watermark: one observation stamped in the future would
            // otherwise put every later one below it.
            let (observed, when) =
                match contribution_time::recorded_platform_time(&obs.created_at, received, &sender)
                {
                    Ok(recorded) => recorded,
                    Err(e) => {
                        eprintln!("{e} for {sender}");
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

            // The watermark is inclusive -- it is the newest observation the
            // last sweep credited, so this sweep meets that one again -- and
            // the ledger is what keeps it from being credited twice. Leaving
            // the user without moving the watermark is always safe: the next
            // sweep walks the same ground, and the ledger skips what is done.
            match ledger::claim(Feed::InatObservations, obs.id).await {
                Ok(true) => {}
                Ok(false) => continue,
                Err(e) => {
                    eprintln!("Failed to claim iNat observation {}: {}", obs.id, e);
                    return;
                }
            }

            for entity_id in project_mapping.credited(obs.project_ids.as_deref()) {
                let row = ParticipationRow {
                    origin: INATURALIST_ORIGIN,
                    when: when.clone(),
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
                    ledger::release(Feed::InatObservations, obs.id).await;
                    return;
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

        // Without the mapping there is nowhere true to credit an observation,
        // so the sweep waits for it rather than crediting somewhere false.
        let project_mapping = match ProjectMapping::fetch().await {
            Ok(mapping) => mapping,
            Err(e) => {
                eprintln!("Error fetching iNat project mapping: {}", e);

                tokio::select! {
                    _ = sleep(Duration::from_secs(5 * 60)) => {}
                    _ = cancellation_token.cancelled() => { return; }
                }

                continue;
            }
        };

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
                    &project_mapping,
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

#[cfg(test)]
mod tests {
    use assert2::check;

    use super::*;

    fn mapping() -> ProjectMapping {
        ProjectMapping {
            by_inat_project: HashMap::from([
                (11, "01JKXMFYFZ0000000000PROJ11".to_string()),
                (12, "01JKXMFYFZ0000000000PROJ12".to_string()),
            ]),
            fallback: "01JKXMFYFZ00000000000INAT0".to_string(),
        }
    }

    #[test]
    fn an_observation_is_credited_to_each_mapped_project_it_is_in() {
        check!(
            mapping().credited(Some(&[11, 99, 12]))
                == vec![
                    "01JKXMFYFZ0000000000PROJ11".to_string(),
                    "01JKXMFYFZ0000000000PROJ12".to_string()
                ]
        );
    }

    #[test]
    fn an_observation_in_no_mapped_project_is_credited_to_inaturalist() {
        for project_ids in [None, Some(&[][..]), Some(&[99][..])] {
            check!(
                mapping().credited(project_ids) == vec!["01JKXMFYFZ00000000000INAT0".to_string()]
            );
        }
    }

    #[test]
    fn every_project_an_observation_can_land_on_is_covered() {
        check!(mapping().projects().len() == 3);
        check!(mapping().projects().contains("01JKXMFYFZ00000000000INAT0"));
    }
}
