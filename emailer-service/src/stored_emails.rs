use anyhow::{Error, Result};
use async_nats::service::Request;
use common::{message::email::*, shared::db, timestring::TimeString};
use ulid::Ulid;

// ============================================================================
// Database Row Structs
// ============================================================================

#[derive(Debug)]
struct StoredEmailRow {
    stored_email_id: Option<String>,
    slug: Option<String>,
    subject: Option<String>,
    body_delta: Option<String>,
    datasets: Option<String>,
    reply_to_noreply: Option<bool>,
    created_at: Option<String>,
    updated_at: Option<String>,
}

// ============================================================================
// List Stored Emails
// ============================================================================

pub async fn handle_list_stored_emails(req: &Request) -> Result<ListStoredEmailsResponse, Error> {
    let request: ListStoredEmailsRequest = flexbuffers::from_slice(&req.message.payload)?;

    let limit = request.per_page.min(100) as i64;
    let offset = ((request.page - 1) * request.per_page) as i64;

    let (rows, total) = if let Some(search) = request.search {
        let search_pattern = format!("%{}%", search);

        let rows = sqlx::query_as!(
            StoredEmailRow,
            r#"
            SELECT stored_email_id, slug, subject, body_delta, datasets, reply_to_noreply, created_at, updated_at
            FROM stored_emails
            WHERE slug LIKE ? OR subject LIKE ?
            ORDER BY created_at DESC
            LIMIT ? OFFSET ?
            "#,
            search_pattern,
            search_pattern,
            limit,
            offset
        )
        .fetch_all(db())
        .await?;

        let total_row = sqlx::query!(
            r#"SELECT COUNT(*) as "count: i64" FROM stored_emails WHERE slug LIKE ? OR subject LIKE ?"#,
            search_pattern,
            search_pattern
        )
        .fetch_one(db())
        .await?;

        (rows, total_row.count as u32)
    } else {
        let rows = sqlx::query_as!(
            StoredEmailRow,
            r#"
            SELECT stored_email_id, slug, subject, body_delta, datasets, reply_to_noreply, created_at, updated_at
            FROM stored_emails
            ORDER BY created_at DESC
            LIMIT ? OFFSET ?
            "#,
            limit,
            offset
        )
        .fetch_all(db())
        .await?;

        let total_row = sqlx::query!(r#"SELECT COUNT(*) as "count: i64" FROM stored_emails"#)
            .fetch_one(db())
            .await?;

        (rows, total_row.count as u32)
    };

    let emails = rows
        .into_iter()
        .map(|row| {
            let stored_email_id = row
                .stored_email_id
                .ok_or_else(|| anyhow::anyhow!("stored_email_id is NULL"))?
                .parse::<Ulid>()?;
            let created_at = TimeString::try_from(
                row.created_at
                    .ok_or_else(|| anyhow::anyhow!("created_at is NULL"))?,
            )?;
            let updated_at = TimeString::try_from(
                row.updated_at
                    .ok_or_else(|| anyhow::anyhow!("updated_at is NULL"))?,
            )?;
            let datasets_json = row
                .datasets
                .ok_or_else(|| anyhow::anyhow!("datasets is NULL"))?;
            let datasets: Vec<String> =
                serde_json::from_str(&datasets_json).unwrap_or_else(|_| vec![]);
            Ok(StoredEmail {
                stored_email_id,
                slug: row.slug.ok_or_else(|| anyhow::anyhow!("slug is NULL"))?,
                subject: row
                    .subject
                    .ok_or_else(|| anyhow::anyhow!("subject is NULL"))?,
                body_delta: row
                    .body_delta
                    .ok_or_else(|| anyhow::anyhow!("body_delta is NULL"))?,
                datasets,
                reply_to_noreply: row.reply_to_noreply.unwrap_or(false),
                created_at,
                updated_at,
            })
        })
        .collect::<Result<Vec<_>, Error>>()?;

    Ok(ListStoredEmailsResponse { emails, total })
}

// ============================================================================
// Get Stored Email
// ============================================================================

pub async fn handle_get_stored_email(req: &Request) -> Result<GetStoredEmailResponse, Error> {
    let request: GetStoredEmailRequest = flexbuffers::from_slice(&req.message.payload)?;
    let id = request.stored_email_id.to_string();

    let row = sqlx::query_as!(
        StoredEmailRow,
        r#"
        SELECT stored_email_id, slug, subject, body_delta, datasets, reply_to_noreply, created_at, updated_at
        FROM stored_emails
        WHERE stored_email_id = ?
        "#,
        id
    )
    .fetch_one(db())
    .await?;

    let stored_email_id = row
        .stored_email_id
        .ok_or_else(|| anyhow::anyhow!("stored_email_id is NULL"))?
        .parse::<Ulid>()?;
    let created_at = TimeString::try_from(
        row.created_at
            .ok_or_else(|| anyhow::anyhow!("created_at is NULL"))?,
    )?;
    let updated_at = TimeString::try_from(
        row.updated_at
            .ok_or_else(|| anyhow::anyhow!("updated_at is NULL"))?,
    )?;
    let datasets_json = row
        .datasets
        .ok_or_else(|| anyhow::anyhow!("datasets is NULL"))?;
    let datasets: Vec<String> = serde_json::from_str(&datasets_json).unwrap_or_else(|_| vec![]);

    Ok(GetStoredEmailResponse {
        email: StoredEmail {
            stored_email_id,
            slug: row.slug.ok_or_else(|| anyhow::anyhow!("slug is NULL"))?,
            subject: row
                .subject
                .ok_or_else(|| anyhow::anyhow!("subject is NULL"))?,
            body_delta: row
                .body_delta
                .ok_or_else(|| anyhow::anyhow!("body_delta is NULL"))?,
            datasets,
            reply_to_noreply: row.reply_to_noreply.unwrap_or(false),
            created_at,
            updated_at,
        },
    })
}

// ============================================================================
// Create Stored Email
// ============================================================================

pub async fn handle_create_stored_email(req: &Request) -> Result<CreateStoredEmailResponse, Error> {
    let request: CreateStoredEmailRequest = flexbuffers::from_slice(&req.message.payload)?;

    let stored_email_id = Ulid::new();
    let now = TimeString::now();
    let id_str = stored_email_id.to_string();
    let now_str = now.to_string();
    let datasets_json = serde_json::to_string(&request.datasets)?;

    sqlx::query!(
        r#"
        INSERT INTO stored_emails (stored_email_id, slug, subject, body_delta, datasets, reply_to_noreply, created_at, updated_at)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?)
        "#,
        id_str,
        request.slug,
        request.subject,
        request.body_delta,
        datasets_json,
        request.reply_to_noreply,
        now_str,
        now_str
    )
    .execute(db())
    .await?;

    Ok(CreateStoredEmailResponse { stored_email_id })
}

// ============================================================================
// Update Stored Email
// ============================================================================

pub async fn handle_update_stored_email(req: &Request) -> Result<UpdateStoredEmailResponse, Error> {
    let request: UpdateStoredEmailRequest = flexbuffers::from_slice(&req.message.payload)?;

    let id = request.stored_email_id.to_string();
    let now = TimeString::now();
    let now_str = now.to_string();

    // Build dynamic UPDATE query based on what fields are provided
    if let Some(slug) = request.slug {
        sqlx::query!(
            "UPDATE stored_emails SET slug = ?, updated_at = ? WHERE stored_email_id = ?",
            slug,
            now_str,
            id
        )
        .execute(db())
        .await?;
    }

    if let Some(subject) = request.subject {
        sqlx::query!(
            "UPDATE stored_emails SET subject = ?, updated_at = ? WHERE stored_email_id = ?",
            subject,
            now_str,
            id
        )
        .execute(db())
        .await?;
    }

    if let Some(body_delta) = request.body_delta {
        sqlx::query!(
            "UPDATE stored_emails SET body_delta = ?, updated_at = ? WHERE stored_email_id = ?",
            body_delta,
            now_str,
            id
        )
        .execute(db())
        .await?;
    }

    if let Some(datasets) = request.datasets {
        let datasets_json = serde_json::to_string(&datasets)?;
        sqlx::query!(
            "UPDATE stored_emails SET datasets = ?, updated_at = ? WHERE stored_email_id = ?",
            datasets_json,
            now_str,
            id
        )
        .execute(db())
        .await?;
    }

    if let Some(reply_to_noreply) = request.reply_to_noreply {
        sqlx::query!(
            "UPDATE stored_emails SET reply_to_noreply = ?, updated_at = ? WHERE stored_email_id = ?",
            reply_to_noreply,
            now_str,
            id
        )
        .execute(db())
        .await?;
    }

    Ok(UpdateStoredEmailResponse { success: true })
}

// ============================================================================
// Delete Stored Email
// ============================================================================

pub async fn handle_delete_stored_email(req: &Request) -> Result<DeleteStoredEmailResponse, Error> {
    let request: DeleteStoredEmailRequest = flexbuffers::from_slice(&req.message.payload)?;
    let id = request.stored_email_id.to_string();

    sqlx::query!("DELETE FROM stored_emails WHERE stored_email_id = ?", id)
        .execute(db())
        .await?;

    Ok(DeleteStoredEmailResponse { success: true })
}
