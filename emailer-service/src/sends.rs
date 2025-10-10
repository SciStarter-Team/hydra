use anyhow::Error;
use async_nats::service::Request;
use common::{
    message::email::{EmailRecipient, SendEmailRequest, SendEmailResponse},
    shared::db,
    timestring::TimeString,
};
use ulid::Ulid;

/// Handle a request to send emails
/// Creates a sending record and queues individual sends
#[tracing::instrument(skip(req))]
pub async fn handle_send_request(req: &Request) -> Result<SendEmailResponse, Error> {
    let request: SendEmailRequest = flexbuffers::from_slice(&req.message.payload)?;
    let sending_id = Ulid::new();
    let sending_id_str = sending_id.to_string();
    let created_at = TimeString::now();
    let total_queued = request.recipients.len() as u32;

    // Create sending record
    sqlx::query!(
        r#"INSERT INTO sendings (sending_id, created_at, subject, total_queued)
           VALUES (?, ?, ?, ?)"#,
        sending_id_str,
        created_at,
        request.subject,
        total_queued
    )
    .execute(db())
    .await?;

    // Queue individual sends
    let data_sets_json = serde_json::to_string(&request.data_sets)?;
    let additional_data_json = serde_json::to_string(&request.additional_data.unwrap_or_default())?;
    let queued_at = TimeString::now();

    for recipient in request.recipients {
        let queue_id = Ulid::new();
        let queue_id_str = queue_id.to_string();
        let (target_type, target_value) = match recipient {
            EmailRecipient::Email(email) => ("email", email),
            EmailRecipient::UserId(user_id) => ("user_id", user_id.to_string()),
        };

        sqlx::query!(
            r#"INSERT INTO send_queue
               (queue_id, sending_id, target_type, target_value, data_sets, template_html, subject, additional_data, queued_at, status)
               VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 'pending')"#,
            queue_id_str,
            sending_id_str,
            target_type,
            target_value,
            data_sets_json,
            request.template_html,
            request.subject,
            additional_data_json,
            queued_at,
        )
        .execute(db())
        .await?;
    }

    tracing::info!(
        sending_id = %sending_id,
        total_queued = total_queued,
        subject = %request.subject,
        "Email sending queued"
    );

    Ok(SendEmailResponse {
        sending_id,
        total_queued,
    })
}
