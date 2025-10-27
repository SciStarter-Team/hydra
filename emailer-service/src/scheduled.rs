use anyhow::Error;
use async_nats::service::Request;
use common::{
    message::email::{
        CancelScheduledSendRequest, CancelScheduledSendResponse, ScheduleSendRequest,
        ScheduleSendResponse,
    },
    shared::db,
    timestring::TimeString,
};
use ulid::Ulid;

/// Core logic for scheduling an email send
pub async fn schedule_send(
    user_id: Ulid,
    hook: String,
    scheduled_time: TimeString,
    template_html: String,
    subject: String,
    data_sets: Vec<String>,
    additional_data: Option<Vec<(String, String)>>,
) -> Result<Ulid, Error> {
    let scheduled_id = Ulid::new();
    let created_at = TimeString::now();

    let user_id_str = user_id.to_string();
    let scheduled_id_str = scheduled_id.to_string();
    let data_sets_json = serde_json::to_string(&data_sets)?;
    let additional_data_json = serde_json::to_string(&additional_data.unwrap_or_default())?;

    sqlx::query!(
        r#"INSERT INTO scheduled_sends
           (scheduled_id, user_id, hook, scheduled_time, template_html, subject, data_sets, additional_data, created_at)
           VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
        scheduled_id_str,
        user_id_str,
        hook,
        scheduled_time,
        template_html,
        subject,
        data_sets_json,
        additional_data_json,
        created_at
    )
    .execute(db())
    .await?;

    tracing::info!(
        scheduled_id = %scheduled_id,
        user_id = %user_id,
        hook = %hook,
        scheduled_time = %scheduled_time,
        "Email scheduled"
    );

    Ok(scheduled_id)
}

/// Handler for scheduling an email send
#[tracing::instrument(skip_all)]
pub async fn handle_schedule_send(req: &Request) -> Result<ScheduleSendResponse, Error> {
    let request: ScheduleSendRequest = flexbuffers::from_slice(&req.message.payload)?;

    let scheduled_id = schedule_send(
        request.user_id,
        request.hook,
        request.scheduled_time,
        request.template_html,
        request.subject,
        request.data_sets,
        request.additional_data,
    )
    .await?;

    Ok(ScheduleSendResponse { scheduled_id })
}

/// Handler for cancelling scheduled emails
#[tracing::instrument(skip_all)]
pub async fn handle_cancel_scheduled_send(
    req: &Request,
) -> Result<CancelScheduledSendResponse, Error> {
    let request: CancelScheduledSendRequest = flexbuffers::from_slice(&req.message.payload)?;

    let user_id_str = request.user_id.to_string();

    let result = sqlx::query!(
        r#"DELETE FROM scheduled_sends WHERE user_id = ? AND hook = ?"#,
        user_id_str,
        request.hook
    )
    .execute(db())
    .await?;

    let cancelled_count = result.rows_affected() as u32;

    tracing::info!(
        user_id = %request.user_id,
        hook = %request.hook,
        cancelled_count = %cancelled_count,
        "Scheduled emails cancelled"
    );

    Ok(CancelScheduledSendResponse { cancelled_count })
}
