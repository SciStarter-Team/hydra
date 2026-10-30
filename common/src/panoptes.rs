//! Asking Panoptes, Zooniverse's API, who a login belongs to.
//!
//! A classification names its classifier by Panoptes id and nothing else,
//! so an account here is credited once the id behind the login it names is
//! known. That id is the one thing about anybody on Zooniverse we keep, and
//! only for logins somebody here has named. Mirrors orrery's
//! `bridge.platforms.find_zooniverse_account`.

use anyhow::Result;
use serde::Deserialize;

use crate::linked_platform::normalize_zooniverse_login;
use crate::message::user::ZooniverseAccount;

pub const ACCEPT: &str = "application/vnd.api+json; version=1";

#[derive(Deserialize)]
struct UsersResponse {
    #[serde(default)]
    users: Vec<User>,
}

#[derive(Deserialize)]
struct User {
    id: String,
    login: String,
}

/// The Panoptes account whose login is `login`, or `None`.
///
/// Panoptes logins are unique ignoring case, and the lookup ignores case
/// too, so this finds the account however its capitalization was typed and
/// answers with the login as its owner spells it. A failure to get an
/// answer is an error, so that a caller who can retry knows to. The answer
/// is only accepted if it is the account that was asked about.
pub async fn find_account(
    client: &reqwest::Client,
    login: &str,
) -> Result<Option<ZooniverseAccount>> {
    let login = normalize_zooniverse_login(login);

    if login.is_empty() {
        return Ok(None);
    }

    let response: UsersResponse = client
        .get("https://panoptes.zooniverse.org/api/users")
        .header("Accept", ACCEPT)
        .query(&[("login", login)])
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    for user in response.users {
        if user.login.to_lowercase() == login.to_lowercase() {
            return Ok(Some(ZooniverseAccount {
                id: user.id.parse()?,
                login: user.login,
            }));
        }
    }

    Ok(None)
}
