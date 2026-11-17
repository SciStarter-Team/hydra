use anyhow::{Error, Result, bail};
use async_nats::service::Request;
use common::{message::email::*, primary::CopyKeeper, shared::db, timestring::TimeString};
use sqlx::SqlitePool;
use ulid::Ulid;

// ============================================================================
// Database Row Structs
// ============================================================================

#[derive(Debug)]
struct OneoffEmailRow {
    oneoff_email_id: Option<String>,
    subject: Option<String>,
    body_delta: Option<String>,
    datasets: Option<String>,
    reply_to_noreply: Option<bool>,
    created_at: Option<String>,
    archived: Option<i64>,
    legacy_slug: Option<String>,
    orrery_id: Option<i64>,
    no_unsubscribe: Option<bool>,
}

impl TryFrom<OneoffEmailRow> for OneoffEmail {
    type Error = Error;

    fn try_from(row: OneoffEmailRow) -> Result<OneoffEmail, Error> {
        let datasets_json = row
            .datasets
            .ok_or_else(|| anyhow::anyhow!("datasets is NULL"))?;

        Ok(OneoffEmail {
            oneoff_email_id: row
                .oneoff_email_id
                .ok_or_else(|| anyhow::anyhow!("oneoff_email_id is NULL"))?
                .parse::<Ulid>()?,
            subject: row
                .subject
                .ok_or_else(|| anyhow::anyhow!("subject is NULL"))?,
            body_delta: row
                .body_delta
                .ok_or_else(|| anyhow::anyhow!("body_delta is NULL"))?,
            datasets: serde_json::from_str(&datasets_json).unwrap_or_else(|_| vec![]),
            reply_to_noreply: row.reply_to_noreply.unwrap_or(false),
            created_at: TimeString::try_from(
                row.created_at
                    .ok_or_else(|| anyhow::anyhow!("created_at is NULL"))?,
            )?,
            archived: row
                .archived
                .ok_or_else(|| anyhow::anyhow!("archived is NULL"))?
                != 0,
            legacy_slug: row.legacy_slug,
            orrery_id: row.orrery_id,
            no_unsubscribe: row.no_unsubscribe.unwrap_or(false),
        })
    }
}

// ============================================================================
// List Oneoff Emails
// ============================================================================

pub async fn handle_list_oneoff_emails(req: &Request) -> Result<ListOneoffEmailsResponse, Error> {
    let request: ListOneoffEmailsRequest = flexbuffers::from_slice(&req.message.payload)?;

    let limit = request.per_page.min(100) as i64;
    let offset = ((request.page - 1) * request.per_page) as i64;

    let (rows, total) = if request.include_archived {
        let rows = sqlx::query_as!(
            OneoffEmailRow,
            r#"
            SELECT oneoff_email_id, subject, body_delta, datasets, reply_to_noreply, created_at, archived, legacy_slug,
               orrery_id, no_unsubscribe
            FROM oneoff_emails
            ORDER BY created_at DESC
            LIMIT ? OFFSET ?
            "#,
            limit,
            offset
        )
        .fetch_all(db())
        .await?;

        let total_row = sqlx::query!(r#"SELECT COUNT(*) as "count: i64" FROM oneoff_emails"#)
            .fetch_one(db())
            .await?;

        (rows, total_row.count as u32)
    } else {
        let rows = sqlx::query_as!(
            OneoffEmailRow,
            r#"
            SELECT oneoff_email_id, subject, body_delta, datasets, reply_to_noreply, created_at, archived, legacy_slug,
               orrery_id, no_unsubscribe
            FROM oneoff_emails
            WHERE archived = 0
            ORDER BY created_at DESC
            LIMIT ? OFFSET ?
            "#,
            limit,
            offset
        )
        .fetch_all(db())
        .await?;

        let total_row = sqlx::query!(
            r#"SELECT COUNT(*) as "count: i64" FROM oneoff_emails WHERE archived = 0"#
        )
        .fetch_one(db())
        .await?;

        (rows, total_row.count as u32)
    };

    let emails = rows
        .into_iter()
        .map(OneoffEmail::try_from)
        .collect::<Result<Vec<_>, Error>>()?;

    Ok(ListOneoffEmailsResponse { emails, total })
}

// ============================================================================
// Get Oneoff Email
// ============================================================================

pub async fn handle_get_oneoff_email(req: &Request) -> Result<GetOneoffEmailResponse, Error> {
    let request: GetOneoffEmailRequest = flexbuffers::from_slice(&req.message.payload)?;
    let id = request.oneoff_email_id.to_string();

    let row = sqlx::query_as!(
        OneoffEmailRow,
        r#"
        SELECT oneoff_email_id, subject, body_delta, datasets, reply_to_noreply, created_at, archived, legacy_slug,
               orrery_id, no_unsubscribe
        FROM oneoff_emails
        WHERE oneoff_email_id = ?
        "#,
        id
    )
    .fetch_one(db())
    .await?;

    Ok(GetOneoffEmailResponse {
        email: OneoffEmail::try_from(row)?,
    })
}

// ============================================================================
// Create Oneoff Email
// ============================================================================

pub async fn handle_create_oneoff_email(req: &Request) -> Result<CreateOneoffEmailResponse, Error> {
    let request: CreateOneoffEmailRequest = flexbuffers::from_slice(&req.message.payload)?;

    let oneoff_email_id = Ulid::new();
    let now = TimeString::now();
    let id_str = oneoff_email_id.to_string();
    let now_str = now.to_string();
    let datasets_json = serde_json::to_string(&request.datasets)?;

    sqlx::query!(
        r#"
        INSERT INTO oneoff_emails (oneoff_email_id, subject, body_delta, datasets, reply_to_noreply, created_at, archived)
        VALUES (?, ?, ?, ?, ?, ?, 0)
        "#,
        id_str,
        request.subject,
        request.body_delta,
        datasets_json,
        request.reply_to_noreply,
        now_str
    )
    .execute(db())
    .await?;

    Ok(CreateOneoffEmailResponse { oneoff_email_id })
}

// ============================================================================
// Update Oneoff Email
// ============================================================================

pub async fn handle_update_oneoff_email(req: &Request) -> Result<UpdateOneoffEmailResponse, Error> {
    let request: UpdateOneoffEmailRequest = flexbuffers::from_slice(&req.message.payload)?;

    update(db(), request, CopyKeeper::NOW).await?;

    Ok(UpdateOneoffEmailResponse { success: true })
}

/// Apply `request`, unless it changes what orrery keeps of a copy `keeper`
/// says orrery keeps: its subject, body or reply-to.
async fn update(
    pool: &SqlitePool,
    request: UpdateOneoffEmailRequest,
    keeper: CopyKeeper,
) -> Result<(), Error> {
    let id = request.oneoff_email_id.to_string();

    let changes_copy = request.subject.is_some()
        || request.body_delta.is_some()
        || request.reply_to_noreply.is_some();
    if changes_copy && keeper.orrery_owns(orrery_id_of(pool, &id).await?) {
        bail!("this one-off is a copy of an orrery message template; edit it in orrery");
    }

    // Build dynamic UPDATE query based on what fields are provided
    if let Some(subject) = request.subject {
        sqlx::query!(
            "UPDATE oneoff_emails SET subject = ? WHERE oneoff_email_id = ?",
            subject,
            id
        )
        .execute(pool)
        .await?;
    }

    if let Some(body_delta) = request.body_delta {
        sqlx::query!(
            "UPDATE oneoff_emails SET body_delta = ? WHERE oneoff_email_id = ?",
            body_delta,
            id
        )
        .execute(pool)
        .await?;
    }

    if let Some(datasets) = request.datasets {
        let datasets_json = serde_json::to_string(&datasets)?;
        sqlx::query!(
            "UPDATE oneoff_emails SET datasets = ? WHERE oneoff_email_id = ?",
            datasets_json,
            id
        )
        .execute(pool)
        .await?;
    }

    if let Some(reply_to_noreply) = request.reply_to_noreply {
        sqlx::query!(
            "UPDATE oneoff_emails SET reply_to_noreply = ? WHERE oneoff_email_id = ?",
            reply_to_noreply,
            id
        )
        .execute(pool)
        .await?;
    }

    Ok(())
}

// ============================================================================
// Delete Oneoff Email
// ============================================================================

pub async fn handle_delete_oneoff_email(req: &Request) -> Result<DeleteOneoffEmailResponse, Error> {
    let request: DeleteOneoffEmailRequest = flexbuffers::from_slice(&req.message.payload)?;

    delete(db(), request.oneoff_email_id, CopyKeeper::NOW).await?;

    Ok(DeleteOneoffEmailResponse { success: true })
}

/// Delete the one-off `oneoff_email_id`, unless it is a copy `keeper` says
/// orrery keeps: the sync would not bring it back until orrery next changed
/// the template.
async fn delete(pool: &SqlitePool, oneoff_email_id: Ulid, keeper: CopyKeeper) -> Result<(), Error> {
    let id = oneoff_email_id.to_string();

    if keeper.orrery_owns(orrery_id_of(pool, &id).await?) {
        bail!("this one-off is a copy of an orrery message template; delete it in orrery");
    }

    sqlx::query!("DELETE FROM oneoff_emails WHERE oneoff_email_id = ?", id)
        .execute(pool)
        .await?;

    Ok(())
}

/// Orrery's id for the template the one-off `id` is a copy of, if it is one.
async fn orrery_id_of(pool: &SqlitePool, id: &str) -> Result<Option<i64>, Error> {
    Ok(sqlx::query_scalar!(
        r#"SELECT orrery_id FROM oneoff_emails WHERE oneoff_email_id = ?"#,
        id
    )
    .fetch_optional(pool)
    .await?
    .flatten())
}

// ============================================================================
// Archive Oneoff Email
// ============================================================================

pub async fn handle_archive_oneoff_email(
    req: &Request,
) -> Result<ArchiveOneoffEmailResponse, Error> {
    let request: ArchiveOneoffEmailRequest = flexbuffers::from_slice(&req.message.payload)?;
    let id = request.oneoff_email_id.to_string();
    let archived = if request.archived { 1 } else { 0 };

    sqlx::query!(
        "UPDATE oneoff_emails SET archived = ? WHERE oneoff_email_id = ?",
        archived,
        id
    )
    .execute(db())
    .await?;

    Ok(ArchiveOneoffEmailResponse { success: true })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::migrated;
    use assert2::check;

    /// A one-off made here, and a copy of orrery's template 7.
    async fn emails(pool: &SqlitePool) -> (Ulid, Ulid) {
        let own = Ulid::new();
        let copy = Ulid::new();

        for (id, orrery_id) in [(own, None), (copy, Some(7))] {
            sqlx::query(
                r#"INSERT INTO oneoff_emails
                       (oneoff_email_id, subject, body_delta, datasets, created_at, archived,
                        orrery_id)
                   VALUES (?, 'Hi', '{"ops":[]}', '[]', '2026-11-17T00:00:00Z', 0, ?)"#,
            )
            .bind(id.to_string())
            .bind(orrery_id)
            .execute(pool)
            .await
            .unwrap();
        }

        (own, copy)
    }

    fn edit(oneoff_email_id: Ulid) -> UpdateOneoffEmailRequest {
        UpdateOneoffEmailRequest {
            oneoff_email_id,
            subject: Some("Changed".to_string()),
            body_delta: None,
            datasets: None,
            reply_to_noreply: None,
        }
    }

    #[tokio::test]
    async fn a_copy_orrery_keeps_takes_only_its_data_sets_here() {
        let pool = migrated().await;
        let (own, copy) = emails(&pool).await;

        check!(update(&pool, edit(own), CopyKeeper::Orrery).await.is_ok());
        check!(update(&pool, edit(copy), CopyKeeper::Orrery).await.is_err());
        check!(update(&pool, edit(copy), CopyKeeper::Hydra).await.is_ok());

        let datasets = UpdateOneoffEmailRequest {
            subject: None,
            datasets: Some(vec!["user".to_string()]),
            ..edit(copy)
        };
        check!(update(&pool, datasets, CopyKeeper::Orrery).await.is_ok());
    }

    #[tokio::test]
    async fn a_copy_orrery_keeps_is_not_deleted_here() {
        let pool = migrated().await;
        let (own, copy) = emails(&pool).await;

        check!(delete(&pool, copy, CopyKeeper::Orrery).await.is_err());
        check!(delete(&pool, own, CopyKeeper::Orrery).await.is_ok());
        check!(delete(&pool, copy, CopyKeeper::Hydra).await.is_ok());
    }
}
