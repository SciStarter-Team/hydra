use anyhow::Error;
use async_nats::service::Request;
use common::{
    message::dashboard::{EntitySummary, EntitySummaryKind},
    shared::db,
    transaction::Immediate,
    ulid::AsUlid,
};

/// NATS `frontend.entity.summary_by_id` endpoint. Resolves a single
/// project/event ULID (sent as its string form) to its `EntitySummary`,
/// or `None` when the ULID isn't a known project/event. This lets other
/// services (notably forum-service) classify a message recipient/participant
/// ULID and render its name and link. Any frontend instance can answer,
/// since the entity tables are replicated to all of them.
pub async fn summary_by_id(req: &Request) -> Result<Option<EntitySummary>, Error> {
    let id: String = flexbuffers::from_slice(&req.message.payload)?;
    Ok(lookup_batch(&[id]).await?.into_iter().next())
}

/// Look up entity summaries for the given target ids (project_id or
/// event_id strings). Performs a local read against the frontend's
/// LoadStorePeered tables. Unknown ids are silently dropped. The
/// returned vec preserves the order of `ids`.
pub async fn lookup_batch(ids: &[String]) -> Result<Vec<EntitySummary>, Error> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }

    let mut tx = Immediate::begin_with_retry(db(), None, None).await?;

    let mut summaries: Vec<EntitySummary> = Vec::new();
    for id in ids {
        if let Some(summary) = lookup_one(&mut tx, id).await? {
            summaries.push(summary);
        }
    }

    tx.commit().await?;

    Ok(summaries)
}

/// Look up entity summaries by slug (the form stored on
/// `entity_list_entries.entity_slug`). Unknown slugs are silently
/// dropped; order is preserved.
pub async fn lookup_batch_by_slug(slugs: &[String]) -> Result<Vec<EntitySummary>, Error> {
    if slugs.is_empty() {
        return Ok(Vec::new());
    }

    let mut tx = Immediate::begin_with_retry(db(), None, None).await?;

    let mut summaries: Vec<EntitySummary> = Vec::new();
    for slug in slugs {
        if let Some(summary) = lookup_one_by_slug(&mut tx, slug).await? {
            summaries.push(summary);
        }
    }

    tx.commit().await?;

    Ok(summaries)
}

async fn lookup_one_by_slug(
    tx: &mut Immediate,
    slug: &str,
) -> Result<Option<EntitySummary>, Error> {
    let entity_row = sqlx::query!(
        r#"SELECT "target", "kind", "created"
        FROM "entities"
        WHERE "slug" = ?
        LIMIT 1"#,
        slug
    )
    .fetch_optional(&mut *tx)
    .await?;

    let entity_row = match entity_row {
        Some(row) => row,
        None => return Ok(None),
    };

    fetch_summary(
        tx,
        &entity_row.target,
        slug,
        &entity_row.kind,
        entity_row.created,
    )
    .await
}

async fn lookup_one(tx: &mut Immediate, target: &str) -> Result<Option<EntitySummary>, Error> {
    let entity_row = sqlx::query!(
        r#"SELECT "slug", "kind", "created"
        FROM "entities"
        WHERE "target" = ?
        LIMIT 1"#,
        target
    )
    .fetch_optional(&mut *tx)
    .await?;

    let entity_row = match entity_row {
        Some(row) => row,
        None => return Ok(None),
    };

    fetch_summary(
        tx,
        target,
        &entity_row.slug,
        &entity_row.kind,
        entity_row.created,
    )
    .await
}

async fn fetch_summary(
    tx: &mut Immediate,
    target: &str,
    slug: &str,
    kind: &str,
    created: String,
) -> Result<Option<EntitySummary>, Error> {
    match kind {
        "Project" => {
            let row = sqlx::query!(
                r#"SELECT "name", "image", "status"
                FROM "projects"
                WHERE "project_id" = ?"#,
                target
            )
            .fetch_optional(&mut *tx)
            .await?;

            Ok(row.map(|row| EntitySummary {
                id: target.as_ulid().expect("target is a ulid"),
                kind: EntitySummaryKind::Project,
                name: row.name,
                slug: slug.to_string(),
                image: if row.image.is_empty() {
                    None
                } else {
                    Some(row.image)
                },
                status: row.status,
                created,
                begin: None,
                end: None,
            }))
        }
        "Event" => {
            let row = sqlx::query!(
                r#"SELECT "name", "image", "status",
                    "begin" as "begin_time?", "end" as "end_time?"
                FROM "events"
                WHERE "event_id" = ?"#,
                target
            )
            .fetch_optional(&mut *tx)
            .await?;

            Ok(row.map(|row| EntitySummary {
                id: target.as_ulid().expect("target is a ulid"),
                kind: EntitySummaryKind::Event,
                name: row.name,
                slug: slug.to_string(),
                image: if row.image.is_empty() {
                    None
                } else {
                    Some(row.image)
                },
                status: row.status,
                created,
                begin: row.begin_time.filter(|b| !b.is_empty()),
                end: row.end_time.filter(|e| !e.is_empty()),
            }))
        }
        _ => Ok(None),
    }
}

/// Build the public-facing link for an entity summary. Projects and
/// events live under `/projects/<slug>` and `/events/<slug>`; the
/// top-level `/<slug>` path still resolves them via a permanent
/// redirect, but linking to the canonical path avoids the extra hop.
pub fn entity_link(summary: &EntitySummary) -> String {
    let prefix = match summary.kind {
        EntitySummaryKind::Project => "/projects/",
        EntitySummaryKind::Event => "/events/",
    };
    format!("{}{}", prefix, summary.slug)
}
