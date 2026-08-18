use anyhow::Error;
use async_nats::service::Request;
use common::{
    message::user::{ZooniverseLink, ZooniverseListLinksRequest},
    shared::db,
    ulid::AsUlid,
};

/// Every account with a Zooniverse username, and when it connected.
///
/// The classification importer works from a project's whole history rather
/// than from a per-account watermark, so it cannot ask about one account at a
/// time: it takes the whole list once per sweep and floors each credit
/// against the matching entry. A null connection time means the account was
/// connected before August 2026, which `common::linked_platform` reads as the
/// cutover.
pub async fn list_links(req: &Request) -> Result<Vec<ZooniverseLink>, Error> {
    let input: ZooniverseListLinksRequest = flexbuffers::from_slice(&req.message.payload)?;

    struct Row {
        user_id: String,
        zooniverse_linked_at: Option<String>,
    }

    let rows = sqlx::query_as!(
        Row,
        r#"SELECT user_id, zooniverse_linked_at
           FROM users
           WHERE zooniverse_username != '' AND zooniverse_username IS NOT NULL
           ORDER BY user_id
           LIMIT ? OFFSET ?"#,
        input.limit,
        input.offset,
    )
    .fetch_all(db())
    .await?;

    let links = rows
        .into_iter()
        .filter_map(|row| {
            Some(ZooniverseLink {
                user_id: row.user_id.as_ulid().ok()?,
                zooniverse_linked_at: row.zooniverse_linked_at,
            })
        })
        .collect();

    Ok(links)
}
