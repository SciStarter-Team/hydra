use anyhow::Error;
use async_nats::service::Request;
use common::{
    message::email::{
        CheckSendabilityRequest, CheckSendabilityResponse, GetSendingRequest, GetSendingResponse,
        ListSendingsRequest, ListSendingsResponse, QueueStatusResponse,
        SendingMetrics,
    },
    shared::db,
    timestring::TimeString,
};
use ulid::Ulid;

/// Get queue status with pending/processing counts
#[tracing::instrument(skip_all)]
pub async fn handle_queue_status(_req: &Request) -> Result<QueueStatusResponse, Error> {
    // Count pending emails
    let pending_result = sqlx::query!(
        r#"SELECT COUNT(*) as count FROM send_queue WHERE status = 'pending'"#
    )
    .fetch_one(db())
    .await?;
    let pending_count = pending_result.count as u32;

    // Count processing emails
    let processing_result = sqlx::query!(
        r#"SELECT COUNT(*) as count FROM send_queue WHERE status = 'processing'"#
    )
    .fetch_one(db())
    .await?;
    let processing_count = processing_result.count as u32;

    // Calculate estimated completion time based on rate limit
    let rate_per_second: u32 = option_env!("EMAIL_SEND_RATE_PER_SECOND")
        .and_then(|s| s.parse().ok())
        .unwrap_or(14);

    let total_remaining = pending_count + processing_count;
    let estimated_completion_seconds = if rate_per_second > 0 {
        (total_remaining + rate_per_second - 1) / rate_per_second // Ceiling division
    } else {
        0
    };

    Ok(QueueStatusResponse {
        pending_count,
        processing_count,
        estimated_completion_seconds,
    })
}

/// List sendings with pagination
#[tracing::instrument(skip_all)]
pub async fn handle_list_sendings(req: &Request) -> Result<ListSendingsResponse, Error> {
    let request: ListSendingsRequest = flexbuffers::from_slice(&req.message.payload)?;

    // Calculate offset
    let offset = (request.page.saturating_sub(1)) * request.per_page;

    // Get total count
    let count_result = sqlx::query!(r#"SELECT COUNT(*) as count FROM sendings"#)
        .fetch_one(db())
        .await?;
    let total = count_result.count as u32;

    // Fetch sendings
    let rows = sqlx::query!(
        r#"SELECT sending_id, created_at, subject, total_queued,
                  messages_sent, messages_skipped, messages_opened,
                  messages_bounced, unsubscribes, complaints
           FROM sendings
           ORDER BY created_at DESC
           LIMIT ? OFFSET ?"#,
        request.per_page,
        offset
    )
    .fetch_all(db())
    .await?;

    let mut sendings = Vec::new();
    for row in rows {
        let sending_id_str = row.sending_id.ok_or_else(|| anyhow::anyhow!("sending_id is NULL"))?;
        let sending_id = sending_id_str.parse::<Ulid>()?;
        let created_at = TimeString::try_from(row.created_at)?;

        sendings.push(SendingMetrics {
            sending_id,
            created_at,
            subject: row.subject,
            total_queued: row.total_queued as u32,
            messages_sent: row.messages_sent as u32,
            messages_skipped: row.messages_skipped as u32,
            messages_opened: row.messages_opened as u32,
            messages_bounced: row.messages_bounced as u32,
            unsubscribes: row.unsubscribes as u32,
            complaints: row.complaints as u32,
        });
    }

    Ok(ListSendingsResponse { sendings, total })
}

/// Get metrics for a specific sending
#[tracing::instrument(skip_all)]
pub async fn handle_get_sending(req: &Request) -> Result<GetSendingResponse, Error> {
    let request: GetSendingRequest = flexbuffers::from_slice(&req.message.payload)?;
    let sending_id_str = request.sending_id.to_string();

    let row = sqlx::query!(
        r#"SELECT sending_id, created_at, subject, total_queued,
                  messages_sent, messages_skipped, messages_opened,
                  messages_bounced, unsubscribes, complaints
           FROM sendings
           WHERE sending_id = ?"#,
        sending_id_str
    )
    .fetch_optional(db())
    .await?;

    let Some(row) = row else {
        anyhow::bail!("Sending not found: {}", request.sending_id);
    };

    let sending_id_str = row.sending_id.ok_or_else(|| anyhow::anyhow!("sending_id is NULL"))?;
    let sending_id = sending_id_str.parse::<Ulid>()?;
    let created_at = TimeString::try_from(row.created_at)?;

    let metrics = SendingMetrics {
        sending_id,
        created_at,
        subject: row.subject,
        total_queued: row.total_queued as u32,
        messages_sent: row.messages_sent as u32,
        messages_skipped: row.messages_skipped as u32,
        messages_opened: row.messages_opened as u32,
        messages_bounced: row.messages_bounced as u32,
        unsubscribes: row.unsubscribes as u32,
        complaints: row.complaints as u32,
    };

    Ok(GetSendingResponse { metrics })
}

/// Check if an email address can receive emails
#[tracing::instrument(skip_all)]
pub async fn handle_check_sendability(req: &Request) -> Result<CheckSendabilityResponse, Error> {
    let request: CheckSendabilityRequest = flexbuffers::from_slice(&req.message.payload)?;

    // Check do_not_send table
    let row = sqlx::query!(
        r#"SELECT email_address, reason, suppressed_at FROM do_not_send WHERE email_address = ?"#,
        request.email_address
    )
    .fetch_optional(db())
    .await?;

    if let Some(row) = row {
        let suppressed_at = TimeString::try_from(row.suppressed_at)?;
        Ok(CheckSendabilityResponse {
            can_send: false,
            reason: Some(row.reason),
            suppressed_at: Some(suppressed_at),
        })
    } else {
        Ok(CheckSendabilityResponse {
            can_send: true,
            reason: None,
            suppressed_at: None,
        })
    }
}
