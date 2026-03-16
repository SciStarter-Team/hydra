use anyhow::{Error, Result};
use async_nats::service::Request;
use common::{
    message::email::*,
    shared::db,
    timestring::TimeString,
};
use jiff::Zoned;
use sqlx::QueryBuilder;
use ulid::Ulid;

use crate::sends::send_email;
use crate::scheduled::schedule_send;

// ============================================================================
// Database Row Structs
// ============================================================================

#[derive(Debug)]
struct TriggerRow {
    trigger_id: Option<String>,
    trigger_name: Option<String>,
    stored_email_id: Option<String>,
    action_type: Option<String>,
    delay_seconds: Option<i64>,
    cancellation_trigger_name: Option<String>,
    recipients: Option<String>,
    active: Option<bool>,
    created_at: Option<String>,
    updated_at: Option<String>,
}

// ============================================================================
// Helper Functions
// ============================================================================

fn convert_trigger_row(row: TriggerRow) -> Result<TriggerInfo> {
    let trigger_id = row.trigger_id
        .ok_or_else(|| anyhow::anyhow!("trigger_id is NULL"))?
        .parse::<Ulid>()?;
    let stored_email_id = row.stored_email_id
        .ok_or_else(|| anyhow::anyhow!("stored_email_id is NULL"))?
        .parse::<Ulid>()?;
    let action_type_str = row.action_type
        .ok_or_else(|| anyhow::anyhow!("action_type is NULL"))?;
    let action_type = TriggerActionType::from_str(&action_type_str)
        .ok_or_else(|| anyhow::anyhow!("Invalid action_type: {}", action_type_str))?;
    let created_at = TimeString::try_from(
        row.created_at.ok_or_else(|| anyhow::anyhow!("created_at is NULL"))?
    )?;
    let updated_at = TimeString::try_from(
        row.updated_at.ok_or_else(|| anyhow::anyhow!("updated_at is NULL"))?
    )?;

    Ok(TriggerInfo {
        trigger_id,
        trigger_name: row.trigger_name.ok_or_else(|| anyhow::anyhow!("trigger_name is NULL"))?,
        stored_email_id,
        action_type,
        delay_seconds: row.delay_seconds,
        cancellation_trigger_name: row.cancellation_trigger_name,
        recipients: row.recipients,
        active: row.active.ok_or_else(|| anyhow::anyhow!("active is NULL"))?,
        created_at,
        updated_at,
    })
}

/// Resolve the recipients JSON into a list of EmailRecipient values.
/// When NULL or empty, defaults to sending to the triggering user.
/// JSON format: ["triggering_user", {"address_set": "slug"}, {"user_set": "slug"}, {"email": "addr"}]
fn resolve_trigger_recipients(recipients_json: &Option<String>, user_id: Ulid) -> Vec<EmailRecipient> {
    let Some(json) = recipients_json else {
        return vec![EmailRecipient::UserId(user_id)];
    };

    let Ok(entries) = serde_json::from_str::<Vec<serde_json::Value>>(json) else {
        tracing::warn!(recipients = %json, "Invalid recipients JSON, falling back to triggering user");
        return vec![EmailRecipient::UserId(user_id)];
    };

    if entries.is_empty() {
        return vec![EmailRecipient::UserId(user_id)];
    }

    entries
        .into_iter()
        .filter_map(|entry| match entry {
            serde_json::Value::String(s) if s == "triggering_user" => {
                Some(EmailRecipient::UserId(user_id))
            }
            serde_json::Value::Object(ref map) => {
                if let Some(slug) = map.get("address_set").and_then(|v| v.as_str()) {
                    Some(EmailRecipient::AddressSet { slug: slug.to_string() })
                } else if let Some(slug) = map.get("user_set").and_then(|v| v.as_str()) {
                    Some(EmailRecipient::UserSet { slug: slug.to_string() })
                } else if let Some(email) = map.get("email").and_then(|v| v.as_str()) {
                    Some(EmailRecipient::Email(email.to_string()))
                } else {
                    tracing::warn!(?entry, "Unknown recipient entry");
                    None
                }
            }
            _ => {
                tracing::warn!(?entry, "Unknown recipient entry");
                None
            }
        })
        .collect()
}

// ============================================================================
// CRUD Handlers
// ============================================================================

/// Create a new trigger
#[tracing::instrument(skip_all)]
pub async fn handle_create_trigger(req: &Request) -> Result<CreateTriggerResponse, Error> {
    let request: CreateTriggerRequest = flexbuffers::from_slice(&req.message.payload)?;

    let trigger_id = Ulid::new();
    let trigger_id_str = trigger_id.to_string();
    let stored_email_id_str = request.stored_email_id.to_string();
    let action_type_str = request.action_type.as_str();
    let created_at = TimeString::now();
    let updated_at = created_at.clone();

    sqlx::query!(
        r#"INSERT INTO email_triggers
           (trigger_id, trigger_name, stored_email_id, action_type, delay_seconds, cancellation_trigger_name, recipients, active, created_at, updated_at)
           VALUES (?, ?, ?, ?, ?, ?, ?, 1, ?, ?)"#,
        trigger_id_str,
        request.trigger_name,
        stored_email_id_str,
        action_type_str,
        request.delay_seconds,
        request.cancellation_trigger_name,
        request.recipients,
        created_at,
        updated_at
    )
    .execute(db())
    .await?;

    tracing::info!(
        trigger_id = %trigger_id,
        trigger_name = %request.trigger_name,
        "Trigger created"
    );

    Ok(CreateTriggerResponse { trigger_id })
}

/// Update an existing trigger
#[tracing::instrument(skip_all)]
pub async fn handle_update_trigger(req: &Request) -> Result<UpdateTriggerResponse, Error> {
    let request: UpdateTriggerRequest = flexbuffers::from_slice(&req.message.payload)?;

    let trigger_id_str = request.trigger_id.to_string();
    let updated_at = TimeString::now();

    // Build dynamic UPDATE query
    let mut query_builder = QueryBuilder::new("UPDATE email_triggers SET ");
    let mut separated = query_builder.separated(", ");

    if let Some(stored_email_id) = request.stored_email_id {
        separated.push("stored_email_id = ");
        separated.push_bind_unseparated(stored_email_id.to_string());
    }

    if let Some(action_type) = request.action_type {
        separated.push("action_type = ");
        separated.push_bind_unseparated(action_type.as_str());
    }

    if let Some(delay_seconds) = request.delay_seconds {
        separated.push("delay_seconds = ");
        separated.push_bind_unseparated(delay_seconds);
    }

    if let Some(cancellation_trigger_name) = request.cancellation_trigger_name {
        separated.push("cancellation_trigger_name = ");
        separated.push_bind_unseparated(cancellation_trigger_name);
    }

    if let Some(recipients) = request.recipients {
        separated.push("recipients = ");
        separated.push_bind_unseparated(recipients);
    }

    if let Some(active) = request.active {
        separated.push("active = ");
        separated.push_bind_unseparated(active);
    }

    separated.push("updated_at = ");
    separated.push_bind_unseparated(updated_at.to_string());

    query_builder.push(" WHERE trigger_id = ");
    query_builder.push_bind(trigger_id_str);

    let query = query_builder.build();
    query.execute(db()).await?;

    tracing::info!(
        trigger_id = %request.trigger_id,
        "Trigger updated"
    );

    Ok(UpdateTriggerResponse { success: true })
}

/// Delete a trigger
#[tracing::instrument(skip_all)]
pub async fn handle_delete_trigger(req: &Request) -> Result<DeleteTriggerResponse, Error> {
    let request: DeleteTriggerRequest = flexbuffers::from_slice(&req.message.payload)?;

    let trigger_id_str = request.trigger_id.to_string();

    sqlx::query!(
        r#"DELETE FROM email_triggers WHERE trigger_id = ?"#,
        trigger_id_str
    )
    .execute(db())
    .await?;

    tracing::info!(
        trigger_id = %request.trigger_id,
        "Trigger deleted"
    );

    Ok(DeleteTriggerResponse { success: true })
}

/// List triggers
#[tracing::instrument(skip_all)]
pub async fn handle_list_triggers(req: &Request) -> Result<ListTriggersResponse, Error> {
    let request: ListTriggersRequest = flexbuffers::from_slice(&req.message.payload)?;

    let rows: Vec<TriggerRow> = if request.active_only {
        sqlx::query_as!(
            TriggerRow,
            r#"SELECT trigger_id, trigger_name, stored_email_id, action_type, delay_seconds, cancellation_trigger_name, recipients, active, created_at, updated_at
               FROM email_triggers
               WHERE active = 1
               ORDER BY created_at DESC"#
        )
        .fetch_all(db())
        .await?
    } else {
        sqlx::query_as!(
            TriggerRow,
            r#"SELECT trigger_id, trigger_name, stored_email_id, action_type, delay_seconds, cancellation_trigger_name, recipients, active, created_at, updated_at
               FROM email_triggers
               ORDER BY created_at DESC"#
        )
        .fetch_all(db())
        .await?
    };

    let triggers = rows
        .into_iter()
        .map(convert_trigger_row)
        .collect::<Result<Vec<_>>>()?;

    Ok(ListTriggersResponse { triggers })
}

/// Get a specific trigger
#[tracing::instrument(skip_all)]
pub async fn handle_get_trigger(req: &Request) -> Result<GetTriggerResponse, Error> {
    let request: GetTriggerRequest = flexbuffers::from_slice(&req.message.payload)?;

    let trigger_id_str = request.trigger_id.to_string();

    let row = sqlx::query_as!(
        TriggerRow,
        r#"SELECT trigger_id, trigger_name, stored_email_id, action_type, delay_seconds, cancellation_trigger_name, recipients, active, created_at, updated_at
           FROM email_triggers
           WHERE trigger_id = ?"#,
        trigger_id_str
    )
    .fetch_optional(db())
    .await?;

    let Some(row) = row else {
        anyhow::bail!("Trigger not found: {}", request.trigger_id);
    };

    let trigger = convert_trigger_row(row)?;

    Ok(GetTriggerResponse { trigger })
}

// ============================================================================
// Trigger Event Handler
// ============================================================================

/// Handle a trigger event - potentially send or schedule an email
#[tracing::instrument(skip_all)]
pub async fn handle_trigger_event(req: &Request) -> Result<TriggerEmailResponse, Error> {
    let request: TriggerEmailRequest = flexbuffers::from_slice(&req.message.payload)?;

    tracing::info!(
        trigger_name = %request.trigger_name,
        user_id = %request.user_id,
        "Processing trigger event"
    );

    let mut action_taken = TriggerActionTaken::NoTrigger;

    // First, check if this trigger name cancels any pending sends
    let cancellation_rows = sqlx::query!(
        r#"SELECT trigger_name FROM email_triggers
           WHERE cancellation_trigger_name = ? AND active = 1"#,
        request.trigger_name
    )
    .fetch_all(db())
    .await?;

    for row in cancellation_rows {
        let trigger_name = row.trigger_name;
        let hook = format!("trigger:{}", trigger_name);
        let user_id_str = request.user_id.to_string();

        let result = sqlx::query!(
            r#"DELETE FROM scheduled_sends WHERE user_id = ? AND hook = ?"#,
            user_id_str,
            hook
        )
        .execute(db())
        .await?;

        let cancelled_count = result.rows_affected();
        if cancelled_count > 0 {
            tracing::info!(
                trigger_name = %request.trigger_name,
                cancelled_trigger = %trigger_name,
                user_id = %request.user_id,
                cancelled_count = %cancelled_count,
                "Cancelled pending scheduled sends"
            );
            action_taken = TriggerActionTaken::CancelledPending;
        }
    }

    // Now, look up active triggers for this event
    let trigger_rows = sqlx::query_as!(
        TriggerRow,
        r#"SELECT trigger_id, trigger_name, stored_email_id, action_type, delay_seconds, cancellation_trigger_name, recipients, active, created_at, updated_at
           FROM email_triggers
           WHERE trigger_name = ? AND active = 1"#,
        request.trigger_name
    )
    .fetch_all(db())
    .await?;

    for row in trigger_rows {
        let trigger = convert_trigger_row(row)?;

        // Get the stored email content
        let stored_email_id_str = trigger.stored_email_id.to_string();
        let stored_email_row = sqlx::query!(
            r#"SELECT subject, body_delta, datasets FROM stored_emails WHERE stored_email_id = ?"#,
            stored_email_id_str
        )
        .fetch_optional(db())
        .await?;

        let Some(stored_email) = stored_email_row else {
            tracing::error!(
                trigger_id = %trigger.trigger_id,
                stored_email_id = %trigger.stored_email_id,
                "Stored email not found for trigger"
            );
            continue;
        };

        let datasets: Vec<String> = serde_json::from_str(&stored_email.datasets).unwrap_or_default();

        // Convert body_delta to HTML
        let template_html = common::quill::delta_to_html(&stored_email.body_delta)?;

        // Build additional data with user_id
        let mut additional_data = request.additional_data.clone().unwrap_or_default();
        additional_data.push(("user_id".to_string(), request.user_id.to_string()));

        let recipients = resolve_trigger_recipients(&trigger.recipients, request.user_id);

        match trigger.action_type {
            TriggerActionType::Immediate => {
                // Send immediately
                let _sending_id = send_email(
                    template_html,
                    stored_email.subject,
                    recipients,
                    datasets,
                    Some(additional_data),
                )
                .await?;

                tracing::info!(
                    trigger_name = %request.trigger_name,
                    user_id = %request.user_id,
                    "Sent immediate trigger email"
                );

                action_taken = TriggerActionTaken::SentImmediately;
            }
            TriggerActionType::Delayed => {
                // Schedule for later
                let delay_seconds = trigger.delay_seconds.unwrap_or(0);
                let scheduled_zoned = Zoned::now()
                    .checked_add(jiff::Span::new().seconds(delay_seconds))?;
                let scheduled_time = TimeString::from(scheduled_zoned.timestamp());

                let hook = format!("trigger:{}", trigger.trigger_name);

                let _scheduled_id = schedule_send(
                    request.user_id,
                    hook,
                    scheduled_time,
                    template_html,
                    stored_email.subject,
                    datasets,
                    Some(additional_data),
                    trigger.recipients.clone(),
                )
                .await?;

                tracing::info!(
                    trigger_name = %request.trigger_name,
                    user_id = %request.user_id,
                    delay_seconds = %delay_seconds,
                    "Scheduled delayed trigger email"
                );

                action_taken = TriggerActionTaken::Scheduled;
            }
        }
    }

    Ok(TriggerEmailResponse {
        success: true,
        action_taken,
    })
}
