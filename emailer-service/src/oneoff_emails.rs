use anyhow::{Error, Result};
use async_nats::service::Request;
use common::{message::email::*, shared::db, timestring::TimeString};
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
            SELECT oneoff_email_id, subject, body_delta, datasets, reply_to_noreply, created_at, archived, legacy_slug
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
            SELECT oneoff_email_id, subject, body_delta, datasets, reply_to_noreply, created_at, archived, legacy_slug
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
        .map(|row| {
            let oneoff_email_id = row
                .oneoff_email_id
                .ok_or_else(|| anyhow::anyhow!("oneoff_email_id is NULL"))?
                .parse::<Ulid>()?;
            let created_at = TimeString::try_from(
                row.created_at
                    .ok_or_else(|| anyhow::anyhow!("created_at is NULL"))?,
            )?;
            let datasets_json = row
                .datasets
                .ok_or_else(|| anyhow::anyhow!("datasets is NULL"))?;
            let datasets: Vec<String> =
                serde_json::from_str(&datasets_json).unwrap_or_else(|_| vec![]);
            Ok(OneoffEmail {
                oneoff_email_id,
                subject: row
                    .subject
                    .ok_or_else(|| anyhow::anyhow!("subject is NULL"))?,
                body_delta: row
                    .body_delta
                    .ok_or_else(|| anyhow::anyhow!("body_delta is NULL"))?,
                datasets,
                reply_to_noreply: row.reply_to_noreply.unwrap_or(false),
                created_at,
                archived: row
                    .archived
                    .ok_or_else(|| anyhow::anyhow!("archived is NULL"))?
                    != 0,
                legacy_slug: row.legacy_slug,
            })
        })
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
        SELECT oneoff_email_id, subject, body_delta, datasets, reply_to_noreply, created_at, archived, legacy_slug
        FROM oneoff_emails
        WHERE oneoff_email_id = ?
        "#,
        id
    )
    .fetch_one(db())
    .await?;

    let oneoff_email_id = row
        .oneoff_email_id
        .ok_or_else(|| anyhow::anyhow!("oneoff_email_id is NULL"))?
        .parse::<Ulid>()?;
    let created_at = TimeString::try_from(
        row.created_at
            .ok_or_else(|| anyhow::anyhow!("created_at is NULL"))?,
    )?;
    let datasets_json = row
        .datasets
        .ok_or_else(|| anyhow::anyhow!("datasets is NULL"))?;
    let datasets: Vec<String> = serde_json::from_str(&datasets_json).unwrap_or_else(|_| vec![]);

    Ok(GetOneoffEmailResponse {
        email: OneoffEmail {
            oneoff_email_id,
            subject: row
                .subject
                .ok_or_else(|| anyhow::anyhow!("subject is NULL"))?,
            body_delta: row
                .body_delta
                .ok_or_else(|| anyhow::anyhow!("body_delta is NULL"))?,
            datasets,
            reply_to_noreply: row.reply_to_noreply.unwrap_or(false),
            created_at,
            archived: row
                .archived
                .ok_or_else(|| anyhow::anyhow!("archived is NULL"))?
                != 0,
            legacy_slug: row.legacy_slug,
        },
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

    let id = request.oneoff_email_id.to_string();

    // Build dynamic UPDATE query based on what fields are provided
    if let Some(subject) = request.subject {
        sqlx::query!(
            "UPDATE oneoff_emails SET subject = ? WHERE oneoff_email_id = ?",
            subject,
            id
        )
        .execute(db())
        .await?;
    }

    if let Some(body_delta) = request.body_delta {
        sqlx::query!(
            "UPDATE oneoff_emails SET body_delta = ? WHERE oneoff_email_id = ?",
            body_delta,
            id
        )
        .execute(db())
        .await?;
    }

    if let Some(datasets) = request.datasets {
        let datasets_json = serde_json::to_string(&datasets)?;
        sqlx::query!(
            "UPDATE oneoff_emails SET datasets = ? WHERE oneoff_email_id = ?",
            datasets_json,
            id
        )
        .execute(db())
        .await?;
    }

    if let Some(reply_to_noreply) = request.reply_to_noreply {
        sqlx::query!(
            "UPDATE oneoff_emails SET reply_to_noreply = ? WHERE oneoff_email_id = ?",
            reply_to_noreply,
            id
        )
        .execute(db())
        .await?;
    }

    Ok(UpdateOneoffEmailResponse { success: true })
}

// ============================================================================
// Delete Oneoff Email
// ============================================================================

pub async fn handle_delete_oneoff_email(req: &Request) -> Result<DeleteOneoffEmailResponse, Error> {
    let request: DeleteOneoffEmailRequest = flexbuffers::from_slice(&req.message.payload)?;
    let id = request.oneoff_email_id.to_string();

    sqlx::query!("DELETE FROM oneoff_emails WHERE oneoff_email_id = ?", id)
        .execute(db())
        .await?;

    Ok(DeleteOneoffEmailResponse { success: true })
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
