//! Asking Panoptes whether a project's classifications can be read as each
//! of SciStarter's Zooniverse accounts. Mirrors orrery's
//! `bridge.platforms.zooniverse_access`, which `manage.py
//! check_zooniverse_access` reports.
//!
//! Lacking access isn't an error on Panoptes: an account that isn't one of
//! the project's collaborators just gets empty pages of its classifications,
//! so a project connected to the wrong account stalls without a word. This
//! asks about the account's roles on the project instead.

use std::time::Duration;

use anyhow::Result;
use common::message::log::{ZooniverseAccess, ZooniverseAccessRequest, ZooniverseReader};
use common::message::user::ZooniverseAccount;
use common::panoptes;
use common::service::decode_structured_message;
use serde::Deserialize;
use serde::de::DeserializeOwned;

use super::{application, get_zooniverse_token};

/// How long one request to Panoptes may take. A project's classifications,
/// asked for by an account that can't read them, can take Panoptes over
/// 30s to find nothing, which is why those are only asked for once the
/// roles grant access.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

/// How long the whole check of one reader may take, which is what keeps an
/// answer to the admin page inside the time it waits.
const CHECK_DEADLINE: Duration = Duration::from_secs(40);

/// NATS `log.zooniverse.admin.access`: what Panoptes says about reading the
/// project as each reader, in [`ZooniverseReader::ALL`] order. The readers
/// are asked about at the same time.
pub async fn handle_access(req: &async_nats::service::Request) -> Result<Vec<ZooniverseAccess>> {
    let request: ZooniverseAccessRequest = decode_structured_message(req.message.clone())?;
    let client = client()?;

    Ok(futures::future::join_all(
        ZooniverseReader::ALL.map(|reader| check(&client, reader, request.zooniverse_id)),
    )
    .await)
}

/// A client for asking Panoptes, whose every request is bounded by
/// [`REQUEST_TIMEOUT`].
pub fn client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()?)
}

/// Ask Panoptes whether Panoptes project `project_id` can be read as
/// `reader`. Whatever stopped it being asked is in `trouble`.
pub async fn check(
    client: &reqwest::Client,
    reader: ZooniverseReader,
    project_id: i64,
) -> ZooniverseAccess {
    let mut access = ZooniverseAccess {
        reader,
        account: None,
        roles: Vec::new(),
        readable: false,
        trouble: None,
    };

    if application(reader).is_none() {
        access.trouble = Some("no Panoptes application is configured for it".to_string());
        return access;
    }

    let asked = tokio::time::timeout(CHECK_DEADLINE, async {
        let Ok(token) = get_zooniverse_token(client, reader).await else {
            access.trouble = Some("Panoptes would not issue a token".to_string());
            return Ok(());
        };

        ask(&mut access, client, &token, project_id).await
    })
    .await;

    match asked {
        Ok(Ok(())) => {}
        Ok(Err(e)) if e.is_timeout() => {
            access.trouble = Some("Panoptes didn't answer in time".to_string());
        }
        Ok(Err(e)) => access.trouble = Some(format!("Panoptes couldn't be asked ({e})")),
        Err(_) => access.trouble = Some("Panoptes didn't answer in time".to_string()),
    }

    access
}

/// A GET from Panoptes' API, with what it answered: its JSON body if it
/// answered 200, or the status it answered otherwise.
async fn get<T: DeserializeOwned>(
    client: &reqwest::Client,
    token: &str,
    path: &str,
    query: &[(&str, String)],
) -> reqwest::Result<Result<T, reqwest::StatusCode>> {
    let response = client
        .get(format!("https://panoptes.zooniverse.org/api/{path}"))
        .bearer_auth(token)
        .header("Accept", panoptes::ACCEPT)
        .query(query)
        .send()
        .await?;

    if response.status() != reqwest::StatusCode::OK {
        return Ok(Err(response.status()));
    }

    Ok(Ok(response.json().await?))
}

async fn ask(
    access: &mut ZooniverseAccess,
    client: &reqwest::Client,
    token: &str,
    project_id: i64,
) -> reqwest::Result<()> {
    let me: MeResponse = match get(client, token, "me", &[]).await? {
        Ok(me) => me,
        Err(status) => {
            access.trouble = Some(format!(
                "asking whose application this is returned {status}"
            ));
            return Ok(());
        }
    };

    let Some(account) = me.account() else {
        access.trouble = Some("Panoptes didn't say whose application this is".to_string());
        return Ok(());
    };

    // The account's own classifications: Panoptes refuses a token without
    // the classification scope here exactly as it does for a project's, but
    // finds the answer at once.
    if let Err(status) = get::<serde_json::Value>(
        client,
        token,
        "classifications",
        &[("page_size", "1".to_string())],
    )
    .await?
    {
        access.trouble = Some(format!(
            "reading classifications returned {status}; check that the application has the \
             classification scope"
        ));
        return Ok(());
    }

    let mut roles = Vec::new();

    for page in 1.. {
        let roles_page: RolesPage = match get(
            client,
            token,
            "project_roles",
            &[
                ("project_id", project_id.to_string()),
                ("user_id", account.id.to_string()),
                ("page", page.to_string()),
            ],
        )
        .await?
        {
            Ok(roles_page) => roles_page,
            Err(status) => {
                access.trouble = Some(format!("asking about project roles returned {status}"));
                access.account = Some(account);
                return Ok(());
            }
        };

        roles.extend(roles_page.roles_of(account.id));

        if !roles_page.has_next_page() {
            break;
        }
    }

    roles.sort();
    roles.dedup();
    access.roles = roles;
    access.account = Some(account);

    if !access.granted() {
        return Ok(());
    }

    match get::<ClassificationsPage>(
        client,
        token,
        "classifications/project",
        &[
            ("project_id", project_id.to_string()),
            ("page_size", "1".to_string()),
        ],
    )
    .await?
    {
        Ok(page) => access.readable = !page.classifications.is_empty(),
        Err(status) => {
            access.trouble = Some(format!(
                "reading the project's classifications returned {status}"
            ));
        }
    }

    Ok(())
}

/// Panoptes' `/me`: the account the token acts as.
#[derive(Debug, Deserialize)]
struct MeResponse {
    #[serde(default)]
    users: Vec<PanoptesUser>,
}

#[derive(Debug, Deserialize)]
struct PanoptesUser {
    id: String,
    login: String,
}

impl MeResponse {
    /// The one account `/me` names, or `None` if it names none, several, or
    /// one whose id isn't a number.
    fn account(self) -> Option<ZooniverseAccount> {
        let [user] = <[PanoptesUser; 1]>::try_from(self.users).ok()?;

        Some(ZooniverseAccount {
            id: user.id.parse().ok()?,
            login: user.login,
        })
    }
}

/// One page of Panoptes' `/project_roles`.
#[derive(Debug, Default, Deserialize)]
struct RolesPage {
    #[serde(default)]
    project_roles: Vec<ProjectRole>,

    #[serde(default)]
    meta: RolesMeta,
}

#[derive(Debug, Deserialize)]
struct ProjectRole {
    #[serde(default)]
    roles: Vec<String>,

    #[serde(default)]
    links: RoleLinks,
}

#[derive(Debug, Default, Deserialize)]
struct RoleLinks {
    owner: Option<RoleOwner>,
}

/// Who holds a role: a user, or a user group, which can hold roles on a
/// project too and doesn't make its members collaborators for reading.
#[derive(Debug, Deserialize)]
struct RoleOwner {
    id: serde_json::Value,

    #[serde(rename = "type", default = "users")]
    kind: String,
}

fn users() -> String {
    "users".to_string()
}

#[derive(Debug, Default, Deserialize)]
struct RolesMeta {
    #[serde(default)]
    project_roles: RolesPaging,
}

#[derive(Debug, Default, Deserialize)]
struct RolesPaging {
    next_page: Option<serde_json::Value>,
}

impl RolesPage {
    /// The roles on this page held by the user `account_id` itself.
    fn roles_of(&self, account_id: i64) -> impl Iterator<Item = String> + '_ {
        let account_id = account_id.to_string();

        self.project_roles
            .iter()
            .filter(move |role| {
                role.links
                    .owner
                    .as_ref()
                    .is_some_and(|owner| owner.kind == "users" && id_text(&owner.id) == account_id)
            })
            .flat_map(|role| role.roles.iter().cloned())
    }

    fn has_next_page(&self) -> bool {
        self.meta
            .project_roles
            .next_page
            .as_ref()
            .is_some_and(|next| !next.is_null())
    }
}

/// A Panoptes id as text, which Panoptes sends as a string but which is a
/// number all the same.
fn id_text(id: &serde_json::Value) -> String {
    match id {
        serde_json::Value::String(id) => id.clone(),
        other => other.to_string(),
    }
}

/// One page of Panoptes' `/classifications/project`, of which only whether
/// it has any matters here.
#[derive(Debug, Deserialize)]
struct ClassificationsPage {
    #[serde(default)]
    classifications: Vec<serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use assert2::{check, let_assert};

    use super::*;

    fn roles_page(json: &str) -> RolesPage {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn only_the_accounts_own_roles_count() {
        let page = roles_page(
            r#"{
              "project_roles": [
                {"roles": ["collaborator"], "links": {"owner": {"id": "3128908", "type": "users"}}},
                {"roles": ["owner"], "links": {"owner": {"id": "42", "type": "users"}}},
                {"roles": ["owner"], "links": {"owner": {"id": "3128908", "type": "user_groups"}}},
                {"roles": ["tester"], "links": {"owner": {"id": 3128908}}}
              ],
              "meta": {"project_roles": {"next_page": null}}
            }"#,
        );

        let roles: Vec<String> = page.roles_of(3128908).collect();

        check!(roles == ["collaborator", "tester"]);
        check!(!page.has_next_page());
    }

    #[test]
    fn a_page_naming_a_next_page_is_followed() {
        check!(roles_page(r#"{"meta": {"project_roles": {"next_page": 2}}}"#).has_next_page());
        check!(!roles_page(r#"{"project_roles": []}"#).has_next_page());
    }

    #[test]
    fn me_names_exactly_one_account() {
        let me: MeResponse =
            serde_json::from_str(r#"{"users": [{"id": "3128908", "login": "scistarter"}]}"#)
                .unwrap();
        let_assert!(Some(account) = me.account());
        check!(account.id == 3128908);
        check!(account.login == "scistarter");

        let none: MeResponse = serde_json::from_str(r#"{"users": []}"#).unwrap();
        check!(none.account().is_none());
    }

    #[test]
    fn only_owners_and_collaborators_are_granted() {
        let access = |roles: &[&str], trouble: Option<&str>| ZooniverseAccess {
            reader: ZooniverseReader::Scistarter,
            account: None,
            roles: roles.iter().map(|role| role.to_string()).collect(),
            readable: false,
            trouble: trouble.map(str::to_string),
        };

        check!(access(&["collaborator"], None).connectable());
        check!(access(&["owner", "tester"], None).connectable());
        check!(!access(&["tester", "translator"], None).granted());
        check!(!access(&[], None).connectable());

        // Roles that grant it, but a question Panoptes didn't answer, leave
        // it unconfirmed.
        let unanswered = access(&["collaborator"], Some("Panoptes didn't answer in time"));
        check!(unanswered.granted());
        check!(!unanswered.connectable());
    }
}
