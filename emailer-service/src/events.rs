use anyhow::Error;
use async_nats::service::Request;
use common::{
    message::email::{BounceEvent, ComplaintEvent, OpenEvent, UnsubscribeEvent},
    shared::db,
    timestring::TimeString,
};

/// Handle bounce event
/// Adds email to do_not_send and updates sending metrics
#[tracing::instrument(skip_all)]
pub async fn handle_bounce(req: &Request) -> Result<(), Error> {
    let event: BounceEvent = flexbuffers::from_slice(&req.message.payload)?;
    let suppressed_at = TimeString::now();

    // Add to do_not_send list
    sqlx::query!(
        r#"INSERT OR IGNORE INTO do_not_send (email_address, reason, suppressed_at)
           VALUES (?, 'bounce', ?)"#,
        event.email_address,
        suppressed_at
    )
    .execute(db())
    .await?;

    // Update sending metrics if sending_id is provided
    if let Some(sending_id) = event.sending_id {
        let sending_id_str = sending_id.to_string();
        sqlx::query!(
            r#"UPDATE sendings SET messages_bounced = messages_bounced + 1 WHERE sending_id = ?"#,
            sending_id_str
        )
        .execute(db())
        .await?;
    }

    tracing::info!(
        email = %event.email_address,
        bounce_type = %event.bounce_type,
        "Bounce processed"
    );

    // Log the bounce event
    if let Some(sending_id) = event.sending_id {
        let _ = common::service::log_email_event(
            None,
            "bounce",
            sending_id,
            &event.email_address,
            event.user_id,
            Some(event.bounce_type),
        )
        .await;
    }

    Ok(())
}

/// Handle complaint event
/// Adds email to do_not_send and updates sending metrics
#[tracing::instrument(skip_all)]
pub async fn handle_complaint(req: &Request) -> Result<(), Error> {
    let event: ComplaintEvent = flexbuffers::from_slice(&req.message.payload)?;
    let suppressed_at = TimeString::now();

    // Add to do_not_send list
    sqlx::query!(
        r#"INSERT OR IGNORE INTO do_not_send (email_address, reason, suppressed_at)
           VALUES (?, 'complaint', ?)"#,
        event.email_address,
        suppressed_at
    )
    .execute(db())
    .await?;

    // Update sending metrics if sending_id is provided
    if let Some(sending_id) = event.sending_id {
        let sending_id_str = sending_id.to_string();
        sqlx::query!(
            r#"UPDATE sendings SET complaints = complaints + 1 WHERE sending_id = ?"#,
            sending_id_str
        )
        .execute(db())
        .await?;
    }

    tracing::warn!(
        email = %event.email_address,
        complaint_type = ?event.complaint_type,
        "Complaint processed"
    );

    // Log the complaint event
    if let Some(sending_id) = event.sending_id {
        let _ = common::service::log_email_event(
            None,
            "complaint",
            sending_id,
            &event.email_address,
            event.user_id,
            event.complaint_type,
        )
        .await;
    }

    Ok(())
}

/// Handle unsubscribe event
/// Adds email to do_not_send and updates sending metrics
#[tracing::instrument(skip_all)]
pub async fn handle_unsubscribe(req: &Request) -> Result<(), Error> {
    let event: UnsubscribeEvent = flexbuffers::from_slice(&req.message.payload)?;
    let suppressed_at = TimeString::now();

    // Add to do_not_send list
    sqlx::query!(
        r#"INSERT OR IGNORE INTO do_not_send (email_address, reason, suppressed_at)
           VALUES (?, 'unsubscribe', ?)"#,
        event.email_address,
        suppressed_at
    )
    .execute(db())
    .await?;

    // Update sending metrics if sending_id is provided
    if let Some(sending_id) = event.sending_id {
        let sending_id_str = sending_id.to_string();
        sqlx::query!(
            r#"UPDATE sendings SET unsubscribes = unsubscribes + 1 WHERE sending_id = ?"#,
            sending_id_str
        )
        .execute(db())
        .await?;
    }

    tracing::info!(
        email = %event.email_address,
        user_id = ?event.user_id,
        "Unsubscribe processed"
    );

    // Log the unsubscribe event
    if let Some(sending_id) = event.sending_id {
        let _ = common::service::log_email_event(
            None,
            "unsubscribe",
            sending_id,
            &event.email_address,
            event.user_id,
            None::<String>,
        )
        .await;
    }

    Ok(())
}

/// Handle open event (tracking pixel)
/// Updates sending metrics
#[tracing::instrument(skip_all)]
pub async fn handle_open(req: &Request) -> Result<(), Error> {
    let event: OpenEvent = flexbuffers::from_slice(&req.message.payload)?;
    let sending_id_str = event.sending_id.to_string();

    // Update sending metrics
    sqlx::query!(
        r#"UPDATE sendings SET messages_opened = messages_opened + 1 WHERE sending_id = ?"#,
        sending_id_str
    )
    .execute(db())
    .await?;

    tracing::debug!(
        sending_id = %event.sending_id,
        user_id = ?event.user_id,
        email = %event.email_address,
        "Open tracked"
    );

    // Log the open event
    let _ = common::service::log_email_event(
        None,
        "open",
        event.sending_id,
        &event.email_address,
        event.user_id,
        None::<String>,
    )
    .await;

    Ok(())
}
