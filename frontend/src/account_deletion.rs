//! Forgetting a deleted account in this instance's copy of the database, as
//! orrery forgets one (`common::user_deletions` says when). Every instance
//! follows the deletions for its own copy, so nothing here is passed on to
//! peers.
//!
//! - What hangs off the account goes: its concierge conversations, its page
//!   editing rights, its OAuth grants and tokens, the entity lists it owns
//!   (with their entries), its standing on others' lists, and when it last
//!   looked at the import review.
//! - A list it was delegate of stays, with no delegate, and an OAuth client
//!   it owned stays, with no owner: orrery's `SET_NULL`.
//! - What records staff work keeps its id: news items it wrote, projects it
//!   hid from an exchange, files it uploaded.
//! - Pages it created keep it as their creator. Orrery refuses to delete an
//!   account that owns an entity, so no account orrery deletes created one,
//!   and the admin refuses likewise ([`created_pages`]).

use anyhow::Error;
use common::{shared::db, transaction::Immediate};
use ulid::Ulid;

/// Forget the deleted account `user_id` in this instance's copy.
pub async fn forget(user_id: Ulid) -> Result<(), Error> {
    let user = user_id.to_string();
    let mut tx = Immediate::begin_with_retry(db(), None, None).await?;

    for statement in [
        "DELETE FROM concierge_conversations WHERE user_id = ?",
        "DELETE FROM entity_lists WHERE owner_id = ?",
        "UPDATE entity_lists SET delegate_id = NULL WHERE delegate_id = ?",
        "DELETE FROM entity_list_identities WHERE user_id = ?",
        "DELETE FROM entity_list_viewers WHERE user_id = ?",
        "DELETE FROM event_page_editors WHERE user_id = ?",
        "DELETE FROM project_page_editors WHERE user_id = ?",
        "DELETE FROM import_review_visits WHERE user_id = ?",
        "DELETE FROM oauth2_authorization_codes WHERE user_id = ?",
        "DELETE FROM oauth2_access_tokens WHERE user_id = ?",
        "DELETE FROM oauth2_refresh_tokens WHERE user_id = ?",
        "UPDATE oauth2_clients SET owner_user_id = NULL WHERE owner_user_id = ?",
    ] {
        sqlx::query(statement).bind(&user).execute(&mut tx).await?;
    }

    tx.commit().await?;
    Ok(())
}

/// How many project and event pages the account `user_id` created. An
/// account that created any is not deleted until they are given to someone
/// else, as orrery protects an entity's owner.
pub async fn created_pages(user_id: Ulid) -> Result<i64, Error> {
    let user = user_id.to_string();
    Ok(sqlx::query_scalar!(
        r#"SELECT (SELECT COUNT(*) FROM projects WHERE page_creator_id = ?1)
                + (SELECT COUNT(*) FROM events WHERE page_creator_id = ?1) AS "pages!: i64""#,
        user
    )
    .fetch_one(db())
    .await?)
}
