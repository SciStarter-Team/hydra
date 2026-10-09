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

/// Core logic for scheduling an email send.
///
/// A user has at most one send pending under any one hook, so scheduling
/// again while one waits changes nothing and returns None.
pub async fn schedule_send(
    user_id: Ulid,
    hook: String,
    scheduled_time: TimeString,
    template_html: String,
    subject: String,
    data_sets: Vec<String>,
    additional_data: Option<Vec<(String, String)>>,
    recipients: Option<String>,
    reply_to: Option<Vec<String>>,
    no_unsubscribe: bool,
) -> Result<Option<Ulid>, Error> {
    let scheduled_id = Ulid::new();
    let created_at = TimeString::now();

    let user_id_str = user_id.to_string();
    let scheduled_id_str = scheduled_id.to_string();
    let data_sets_json = serde_json::to_string(&data_sets)?;
    let additional_data_json = serde_json::to_string(&additional_data.unwrap_or_default())?;
    let reply_to_json = reply_to
        .filter(|addresses| !addresses.is_empty())
        .map(|addresses| serde_json::to_string(&addresses))
        .transpose()?;

    let inserted = sqlx::query!(
        r#"INSERT INTO scheduled_sends
           (scheduled_id, user_id, hook, scheduled_time, template_html, subject, data_sets, additional_data, recipients, reply_to, no_unsubscribe, created_at)
           VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
           ON CONFLICT (user_id, hook) DO NOTHING"#,
        scheduled_id_str,
        user_id_str,
        hook,
        scheduled_time,
        template_html,
        subject,
        data_sets_json,
        additional_data_json,
        recipients,
        reply_to_json,
        no_unsubscribe,
        created_at
    )
    .execute(db())
    .await?
    .rows_affected();

    if inserted == 0 {
        tracing::info!(
            user_id = %user_id,
            hook = %hook,
            "A send is already pending for this user and hook"
        );
        return Ok(None);
    }

    tracing::info!(
        scheduled_id = %scheduled_id,
        user_id = %user_id,
        hook = %hook,
        scheduled_time = %scheduled_time,
        "Email scheduled"
    );

    Ok(Some(scheduled_id))
}

/// Handler for scheduling an email send
#[tracing::instrument(skip_all)]
pub async fn handle_schedule_send(req: &Request) -> Result<ScheduleSendResponse, Error> {
    let request: ScheduleSendRequest = flexbuffers::from_slice(&req.message.payload)?;

    let hook = request.hook.clone();

    let scheduled_id = match schedule_send(
        request.user_id,
        request.hook,
        request.scheduled_time,
        request.template_html,
        request.subject,
        request.data_sets,
        request.additional_data,
        request.recipients,
        request.reply_to,
        request.no_unsubscribe,
    )
    .await?
    {
        Some(scheduled_id) => scheduled_id,
        // The send already waiting under this hook is the one that stands.
        None => {
            let user_id_str = request.user_id.to_string();

            sqlx::query_scalar!(
                r#"SELECT scheduled_id FROM scheduled_sends WHERE user_id = ? AND hook = ?"#,
                user_id_str,
                hook
            )
            .fetch_one(db())
            .await?
            .parse()?
        }
    };

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
