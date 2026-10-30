use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use common::message::user::{
    ZooniverseAccount, ZooniverseIdentityAnswer, ZooniverseIdentityQuestion, ZooniverseLink,
    ZooniverseListLinksRequest,
};
use common::{contribution_time, linked_platform, panoptes};
use jiff::Timestamp;
use serde::Deserialize;
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;
use ulid::Ulid;

use log_service::database::{
    Database, get_sync_last_id, set_sync_last_id, tables::ParticipationRow,
};

use super::{Link, Platform};

/// "Import" -- how the contribution reached us, which for a classification is
/// a scheduled pull against the Panoptes API. Deliberately the same value as
/// `INATURALIST_ORIGIN` in the sibling bridge, because it is the same kind of
/// arrival; the platform is told apart by the entity.
const ZOONIVERSE_ORIGIN: i16 = 5;

/// The origins orrery has filed its own Zooniverse imports under: "Import",
/// and before April 2026, when its importer named no origin, "Unspecified".
/// (Between the two it used 10, and has since renumbered those to 5.)
const ORRERY_ORIGINS: &[i16] = &[5, 0];
const PAGE_SIZE: u32 = 200;
const REQUEST_DELAY: Duration = Duration::from_millis(500);
const LINK_PAGE_SIZE: i64 = 500;

/// Every identified account's Zooniverse connection, as user-service holds
/// it.
///
/// A project's classification history is walked whole, and a project newly
/// joining the affiliate program starts from classification zero -- so
/// without the connection times its entire back catalogue would land on
/// everybody already connected. The importer has no `users` table of its
/// own, so it takes the list from user-service.
struct Links {
    /// Each account's connection, keyed by user id.
    links: HashMap<String, Link>,

    /// The accounts claiming each Panoptes account, which is how a
    /// classification names its classifier. Nothing stops two accounts here
    /// naming the same Zooniverse login, and each of them is credited. An
    /// account whose login hasn't been identified is in no list.
    by_zooniverse_id: HashMap<i64, Vec<String>>,
}

/// From when the bridge can match an account's classifications, if that is
/// later than the account connected.
///
/// The project watermarks only move forward, so a classification read
/// before the bridge knew who made it is passed over for good. orrery
/// credits it all the same, and coverage leaves orrery's copy standing
/// until here.
///
/// The bridge reads the connections again after fetching each page, so it
/// knows an account from the moment user-service does -- which, for an id
/// orrery found, is when the user sync brought it over. Before this release
/// the bridge matched through a mapping orrery sent whole: an account it
/// matched that way has always been matched, and any other only since this
/// release began matching by the id on the account.
struct Matching {
    by_mapping: HashSet<(String, i64)>,
    by_id_since: Timestamp,
}

impl Matching {
    async fn load() -> Result<Matching> {
        let by_mapping = sqlx::query!(
            r#"SELECT user_id AS "user_id!", zooniverse_id AS "zooniverse_id!"
               FROM zooniverse_matched_by_mapping"#
        )
        .fetch_all(common::shared::db())
        .await?
        .into_iter()
        .map(|row| (row.user_id, row.zooniverse_id))
        .collect();

        let by_id_since =
            sqlx::query_scalar!(r#"SELECT since AS "since!" FROM zooniverse_matching_by_id"#)
                .fetch_one(common::shared::db())
                .await?
                .parse()?;

        Ok(Matching {
            by_mapping,
            by_id_since,
        })
    }

    /// When the bridge could first match `link`'s account, or `None` if it
    /// could all along. An arrival time that can't be read means the
    /// account can't be placed, and `Err` leaves it unmatched: orrery's copy
    /// then stands for all of it.
    fn since(&self, link: &ZooniverseLink, zooniverse_id: i64) -> Result<Option<Timestamp>> {
        let received = link
            .zooniverse_id_received_at
            .as_deref()
            .map(str::parse::<Timestamp>)
            .transpose()?;

        let by_id = (!self
            .by_mapping
            .contains(&(link.user_id.to_string(), zooniverse_id)))
        .then_some(self.by_id_since);

        Ok(received.max(by_id))
    }
}

async fn load_links() -> Result<Links> {
    let matching = Matching::load().await?;
    let mut links = Links {
        links: HashMap::new(),
        by_zooniverse_id: HashMap::new(),
    };
    let mut offset: i64 = 0;

    loop {
        let page: Vec<ZooniverseLink> = common::service::request(
            "user.zooniverse.list_links",
            ZooniverseListLinksRequest {
                offset,
                limit: LINK_PAGE_SIZE,
            },
        )
        .await?;

        let page_len = page.len() as i64;

        for link in page {
            let Some(zooniverse_id) = link.zooniverse_id else {
                continue;
            };

            let matched_since = match matching.since(&link, zooniverse_id) {
                Ok(since) => since,
                Err(e) => {
                    eprintln!(
                        "Unreadable Panoptes id arrival {:?} for {}: {e}",
                        link.zooniverse_id_received_at, link.user_id
                    );
                    continue;
                }
            };

            let user_id = link.user_id.to_string();

            links
                .by_zooniverse_id
                .entry(zooniverse_id)
                .or_default()
                .push(user_id.clone());

            links.links.insert(
                user_id,
                Link {
                    linked_at: link.zooniverse_linked_at,
                    matched_since,
                },
            );
        }

        if page_len < LINK_PAGE_SIZE {
            return Ok(links);
        }

        offset += LINK_PAGE_SIZE;
    }
}

/// Find the Panoptes account behind each Zooniverse login that hasn't been
/// identified and is due to be asked about.
///
/// Run between fetching each page and matching it, because the project
/// watermarks only move forward: a classification by somebody connected but
/// not yet identified would be walked past for good. Linking identifies an
/// account at once unless Panoptes was unreachable, so this usually finds
/// nobody to ask about. Each login is asked about once however many
/// accounts name it. A failure leaves the accounts not yet reached as they
/// were.
async fn identify_accounts(client: &reqwest::Client) -> Result<()> {
    let questions: Vec<ZooniverseIdentityQuestion> =
        common::service::request("user.zooniverse.to_identify", ()).await?;

    let mut answers: HashMap<String, Option<ZooniverseAccount>> = HashMap::new();

    for question in questions {
        let key = linked_platform::normalize_zooniverse_login(&question.zooniverse_username)
            .to_lowercase();

        let account = match answers.get(&key) {
            Some(account) => account.clone(),
            None => {
                sleep(REQUEST_DELAY).await;
                let account = panoptes::find_account(client, &key).await?;
                answers.insert(key, account.clone());
                account
            }
        };

        let _: () = common::service::request(
            "user.zooniverse.record_identity",
            ZooniverseIdentityAnswer {
                user_id: question.user_id,
                looked_up: question.zooniverse_username,
                account,
            },
        )
        .await?;
    }

    Ok(())
}

/// NATS `log.zooniverse.projects`: every project this bridge credits.
///
/// Which projects belong to the Zooniverse affiliate program is known only
/// here, where the sweep keeps them. The frontend asks so that a project page
/// can invite its visitors to connect their Zooniverse accounts, and so that
/// the finder can list the projects that would credit them.
pub async fn handle_projects(_req: &async_nats::service::Request) -> Result<Vec<Ulid>> {
    let entity_ids = sqlx::query_scalar!(
        r#"SELECT DISTINCT entity_id AS "entity_id!" FROM zooniverse_projects ORDER BY entity_id"#
    )
    .fetch_all(common::shared::db())
    .await?;

    Ok(entity_ids
        .into_iter()
        .filter_map(|entity_id| {
            entity_id
                .parse::<Ulid>()
                .inspect_err(|e| {
                    eprintln!("Unreadable Zooniverse project entity {entity_id:?}: {e}")
                })
                .ok()
        })
        .collect())
}

/// What this bridge credits; see [`BridgeCoverage`](super::BridgeCoverage).
///
/// A classification is credited to every account that claims the Zooniverse
/// account it was made by, so the accounts are those whose login has been
/// identified, each with its connection as the sweep reads it.
pub async fn coverage() -> Result<Platform> {
    let projects = sqlx::query!(
        r#"SELECT entity_id AS "entity_id!", MIN(credited_since) AS credited_since
           FROM zooniverse_projects GROUP BY entity_id"#
    )
    .fetch_all(common::shared::db())
    .await?
    .into_iter()
    .map(|project| {
        let credited_since = project.credited_since.and_then(|since| {
            since
                .parse::<jiff::Timestamp>()
                .inspect_err(|e| {
                    eprintln!(
                        "Unreadable credited_since {since:?} for Zooniverse project {}: {e}",
                        project.entity_id
                    )
                })
                .ok()
        });

        (project.entity_id, credited_since)
    })
    .collect();

    let links = load_links().await?.links;

    Ok(Platform {
        orrery_origins: ORRERY_ORIGINS,
        projects,
        links,
    })
}

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
            // A project new here starts from classification zero, not from
            // wherever orrery's importer has reached: this importer is the
            // one of record for the classifications it credits, and what
            // orrery credited before this project arrived would otherwise be
            // credited by neither. The connection floor keeps the walk from
            // crediting anybody's back catalogue. INSERT OR IGNORE so a
            // project already here keeps the watermark it has advanced.
            sqlx::query!(
                "INSERT OR IGNORE INTO zooniverse_projects (zooniverse_id, entity_id, last_classification_id) VALUES (?, ?, 0)",
                project.zooniverse_id,
                project.entity_ulid,
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

/// Poll Zooniverse for new classifications and write participation rows.
///
/// Pagination is `last_id`-only: each page advances the watermark, which we
/// persist before fetching the next page. A crash caps re-imports at the
/// in-flight page rather than the entire project history. Each classification
/// is fanned out to every SciStarter user that claims the Zooniverse account.
///
/// While orrery is primary it identifies the Panoptes accounts and the user
/// sync carries them here; once hydra is, the sweep identifies any due
/// accounts itself before matching each page.
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
            .header("Accept", panoptes::ACCEPT)
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

        if common::primary::PRIMARY {
            if let Err(e) = identify_accounts(client).await {
                eprintln!(
                    "Zooniverse account lookup failed, so project {zooniverse_id} sits this run out: {e}"
                );
                return;
            }
        }

        // Read after the page rather than once per sweep, so that everybody
        // connected before this page's classifications were fetched -- and
        // so before any of them was made -- is matched. Without the
        // connection times there is no way to tell which classifications
        // predate a connection, so the project sits this run out rather
        // than credit the lot.
        let links = match load_links().await {
            Ok(links) => links,
            Err(e) => {
                eprintln!("Error loading Zooniverse connections: {}", e);
                return;
            }
        };

        let mut page_max_id = last_classification_id;
        let received = Timestamp::now();
        let sender = format!("Zooniverse project {zooniverse_id}");

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
                        eprintln!(
                            "Skipping classification {} with unparseable user id",
                            class_id
                        );
                        continue;
                    }
                },
                None => {
                    // Anonymous classification — no SciStarter user to credit.
                    continue;
                }
            };

            // A classifier no SciStarter account claims has nobody to credit.
            let Some(scistarter_users) = links.by_zooniverse_id.get(&zoo_user_id) else {
                continue;
            };

            // Unreadable is refused here as well as by the connection floor
            // below, which cannot credit a time it cannot place.
            let when = match contribution_time::recorded_platform_time(
                &classification.created_at,
                received,
                &sender,
            ) {
                Ok((_, when)) => when,
                Err(e) => {
                    eprintln!("{e} for {sender}, classification {class_id}");
                    continue;
                }
            };

            for user_id in scistarter_users {
                // A classification made before the account was connected here
                // is not backfilled. The project watermark does not settle
                // this on its own: it is per project, and a project joining
                // the affiliate program starts at zero.
                if !links
                    .links
                    .get(user_id)
                    .is_some_and(|link| link.credits(&classification.created_at))
                {
                    continue;
                }

                let row = ParticipationRow {
                    origin: ZOONIVERSE_ORIGIN,
                    when: when.clone(),
                    duration: 1.0,
                    where_point: None,
                    magnitude: 1,
                    authenticated: true,
                    extra: String::new(),
                    fidelity: 0.5,
                    entity_id: entity_id.clone(),
                    user_id: Some(user_id.clone()),
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

#[cfg(test)]
mod tests {
    use assert2::check;

    use super::*;

    const RELEASE: &str = "2026-10-30T12:00:00Z";

    fn matching(mapped: &[(Ulid, i64)]) -> Matching {
        Matching {
            by_mapping: mapped
                .iter()
                .map(|(user_id, zooniverse_id)| (user_id.to_string(), *zooniverse_id))
                .collect(),
            by_id_since: RELEASE.parse().unwrap(),
        }
    }

    fn link(user_id: Ulid, received_at: Option<&str>) -> ZooniverseLink {
        ZooniverseLink {
            user_id,
            zooniverse_linked_at: Some("2026-09-01T00:00:00Z".to_string()),
            zooniverse_id: Some(7),
            zooniverse_id_received_at: received_at.map(str::to_string),
        }
    }

    fn at(when: &str) -> Option<Timestamp> {
        Some(when.parse().unwrap())
    }

    #[test]
    fn an_account_the_old_mapping_matched_always_has_been() {
        let user = Ulid::new();

        check!(matching(&[(user, 7)]).since(&link(user, None), 7).unwrap() == None);
    }

    #[test]
    fn any_other_account_known_before_the_release_is_matched_from_it() {
        let user = Ulid::new();

        check!(matching(&[]).since(&link(user, None), 7).unwrap() == at(RELEASE));

        // The mapping vouches for the account it named, not another.
        check!(matching(&[(user, 8)]).since(&link(user, None), 7).unwrap() == at(RELEASE));
    }

    #[test]
    fn an_id_arriving_after_the_release_is_matched_from_its_arrival() {
        let user = Ulid::new();
        let arrived = "2026-11-02T08:00:00Z";

        check!(matching(&[]).since(&link(user, Some(arrived)), 7).unwrap() == at(arrived));
        check!(
            matching(&[(user, 7)])
                .since(&link(user, Some(arrived)), 7)
                .unwrap()
                == at(arrived)
        );
    }

    #[test]
    fn an_unreadable_arrival_leaves_the_account_unmatched() {
        let user = Ulid::new();

        check!(
            matching(&[])
                .since(&link(user, Some("whenever")), 7)
                .is_err()
        );
    }
}
