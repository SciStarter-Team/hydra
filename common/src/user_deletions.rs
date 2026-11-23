//! Following the accounts user-service deletes, so that every service
//! forgets what it keeps about each one.
//!
//! An account is deleted in user-service alone, which records the deletion
//! (`user.deleted.since` hands them out, in the order they were made). Each
//! service that keeps anything about accounts follows those records with
//! [`follow`], forgetting each account as its own data requires, and keeps
//! its place in its own `misc` table. A service that was down catches up
//! when it comes back, and a frontend instance, with its own copy of the
//! database, follows them for that copy.

use std::{future::Future, time::Duration};

use tokio::time::sleep;
use ulid::Ulid;

use crate::{
    message::user::{UserDeletionsRequest, UserDeletionsResponse},
    misc,
    service::request,
};

/// Where in user-service's deletions a service has forgotten up to.
const APPLIED: &str = "user-deletions-applied";

/// The pause once every deletion so far is applied.
const CAUGHT_UP: Duration = Duration::from_secs(10 * 60);

/// The pause after a failure, before trying again from where it stopped.
const AFTER_FAILURE: Duration = Duration::from_secs(60);

/// Forget each account user-service deletes, with `forget`, forever: each
/// deletion once, in order, and none passed until it is forgotten. `service`
/// names the follower in what it reports.
pub async fn follow<F, Fut>(service: &str, forget: F)
where
    F: Fn(Ulid) -> Fut,
    Fut: Future<Output = Result<(), anyhow::Error>>,
{
    loop {
        match catch_up(&forget).await {
            Ok(0) => sleep(CAUGHT_UP).await,
            Ok(forgotten) => println!("{service} forgot {forgotten} deleted accounts"),
            Err(e) => {
                eprintln!("{service} failed to forget deleted accounts: {e:#}");
                sleep(AFTER_FAILURE).await;
            }
        }
    }
}

/// Forget the next page of deleted accounts, moving the place on past each
/// once it is forgotten; the number forgotten.
async fn catch_up<F, Fut>(forget: &F) -> Result<usize, anyhow::Error>
where
    F: Fn(Ulid) -> Fut,
    Fut: Future<Output = Result<(), anyhow::Error>>,
{
    let after = misc::get::<i64>(APPLIED).await?.unwrap_or(0);
    let page: UserDeletionsResponse =
        request("user.deleted.since", UserDeletionsRequest { after }).await?;

    for deletion in &page.deletions {
        forget(deletion.user_id).await?;
        misc::set(APPLIED, deletion.seq).await?;
    }

    Ok(page.deletions.len())
}
