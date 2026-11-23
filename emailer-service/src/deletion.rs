//! Forgetting a deleted account (`common::user_deletions` says when): the
//! sends waiting for it, and the events it has been through. What has been
//! sent to its addresses, and the addresses that may not be written to, stay,
//! since they are about addresses, which may come back on another account.

use anyhow::Error;
use common::{shared::db, transaction::Immediate};
use ulid::Ulid;

/// Forget the deleted account `user_id`.
pub async fn forget(user_id: Ulid) -> Result<(), Error> {
    let user = user_id.to_string();
    let mut tx = Immediate::begin_with_retry(db(), None, None).await?;
    sqlx::query!("DELETE FROM scheduled_sends WHERE user_id = ?", user)
        .execute(&mut tx)
        .await?;
    sqlx::query!("DELETE FROM trigger_events_seen WHERE user_id = ?", user)
        .execute(&mut tx)
        .await?;
    tx.commit().await?;
    Ok(())
}
