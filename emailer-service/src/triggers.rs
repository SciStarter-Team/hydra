use anyhow::{Error, Result};
use async_nats::service::Request;
use common::{message::email::*, shared::db, timestring::TimeString};
use sqlx::QueryBuilder;
use ulid::Ulid;

use crate::scheduled::schedule_send;
use crate::sends::send_email;

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
    recipients: Option<String>,
    active: Option<bool>,
    created_at: Option<String>,
    updated_at: Option<String>,
}

// ============================================================================
// Helper Functions
// ============================================================================

fn convert_trigger_row(
    row: TriggerRow,
    cancellation_trigger_names: Vec<String>,
) -> Result<TriggerInfo> {
    let trigger_id = row
        .trigger_id
        .ok_or_else(|| anyhow::anyhow!("trigger_id is NULL"))?
        .parse::<Ulid>()?;
    let stored_email_id = row
        .stored_email_id
        .ok_or_else(|| anyhow::anyhow!("stored_email_id is NULL"))?
        .parse::<Ulid>()?;
    let action_type_str = row
        .action_type
        .ok_or_else(|| anyhow::anyhow!("action_type is NULL"))?;
    let action_type = TriggerActionType::from_str(&action_type_str)
        .ok_or_else(|| anyhow::anyhow!("Invalid action_type: {}", action_type_str))?;
    let created_at = TimeString::try_from(
        row.created_at
            .ok_or_else(|| anyhow::anyhow!("created_at is NULL"))?,
    )?;
    let updated_at = TimeString::try_from(
        row.updated_at
            .ok_or_else(|| anyhow::anyhow!("updated_at is NULL"))?,
    )?;

    Ok(TriggerInfo {
        trigger_id,
        trigger_name: row
            .trigger_name
            .ok_or_else(|| anyhow::anyhow!("trigger_name is NULL"))?,
        stored_email_id,
        action_type,
        delay_seconds: row.delay_seconds,
        cancellation_trigger_names,
        recipients: row.recipients,
        active: row
            .active
            .ok_or_else(|| anyhow::anyhow!("active is NULL"))?,
        created_at,
        updated_at,
    })
}

/// The hook a binding's delayed sends are held under. Keyed by binding rather
/// than by event, so that cancelling one binding's send leaves another
/// binding on the same event alone.
fn binding_hook(trigger_id: &str) -> String {
    format!("trigger:{trigger_id}")
}

/// The hook delayed sends were held under before they were keyed by binding.
/// A send scheduled then that the migration could not place still waits under
/// it, so cancellation still has to reach it there.
fn legacy_event_hook(trigger_name: &str) -> String {
    format!("trigger:{trigger_name}")
}

/// The events that cancel one binding.
async fn cancellations_of(trigger_id: &str) -> Result<Vec<String>> {
    Ok(sqlx::query_scalar!(
        r#"SELECT cancellation_trigger_name FROM email_trigger_cancellations
           WHERE trigger_id = ?
           ORDER BY cancellation_trigger_name"#,
        trigger_id
    )
    .fetch_all(db())
    .await?)
}

/// Every binding's cancelling events, by binding, for listing them all at once.
async fn all_cancellations() -> Result<std::collections::HashMap<String, Vec<String>>> {
    let rows = sqlx::query!(
        r#"SELECT trigger_id, cancellation_trigger_name FROM email_trigger_cancellations
           ORDER BY cancellation_trigger_name"#
    )
    .fetch_all(db())
    .await?;

    let mut by_trigger = std::collections::HashMap::<String, Vec<String>>::new();

    for row in rows {
        by_trigger
            .entry(row.trigger_id)
            .or_default()
            .push(row.cancellation_trigger_name);
    }

    Ok(by_trigger)
}

/// Replace a binding's cancelling events, as part of `tx`.
async fn set_cancellations(
    tx: &mut sqlx::SqliteConnection,
    trigger_id: &str,
    names: &[String],
) -> Result<()> {
    sqlx::query!(
        r#"DELETE FROM email_trigger_cancellations WHERE trigger_id = ?"#,
        trigger_id
    )
    .execute(&mut *tx)
    .await?;

    for name in names
        .iter()
        .map(|name| name.trim())
        .filter(|name| !name.is_empty())
    {
        sqlx::query!(
            r#"INSERT OR IGNORE INTO email_trigger_cancellations (trigger_id, cancellation_trigger_name)
               VALUES (?, ?)"#,
            trigger_id,
            name
        )
        .execute(&mut *tx)
        .await?;
    }

    Ok(())
}

/// Note that `user_id` has been through `trigger_name`, keeping the earliest
/// time it is said to have happened.
async fn record_event_seen(
    pool: &sqlx::SqlitePool,
    user_id: &str,
    trigger_name: &str,
    occurred_at: &TimeString,
) -> Result<()> {
    let existing = sqlx::query_scalar!(
        r#"SELECT occurred_at FROM trigger_events_seen WHERE user_id = ? AND trigger_name = ?"#,
        user_id,
        trigger_name
    )
    .fetch_optional(pool)
    .await?;

    // Compared as instants: as text, a time with a fractional second sorts
    // before the same second without one.
    let earlier = match existing {
        None => true,
        Some(existing) => match (
            jiff::Timestamp::try_from(occurred_at.clone()),
            existing.parse::<jiff::Timestamp>(),
        ) {
            (Ok(new), Ok(old)) => new < old,
            _ => false,
        },
    };

    if earlier {
        sqlx::query!(
            r#"INSERT INTO trigger_events_seen (user_id, trigger_name, occurred_at)
               VALUES (?, ?, ?)
               ON CONFLICT (user_id, trigger_name) DO UPDATE SET occurred_at = excluded.occurred_at"#,
            user_id,
            trigger_name,
            occurred_at
        )
        .execute(pool)
        .await?;
    }

    Ok(())
}

/// Whether `user_id` has already been through one of the events that cancel
/// the binding `trigger_id`.
async fn already_cancelled(
    pool: &sqlx::SqlitePool,
    user_id: &str,
    trigger_id: &str,
) -> Result<bool> {
    Ok(sqlx::query_scalar!(
        r#"SELECT EXISTS (
               SELECT 1
                 FROM trigger_events_seen s
                 JOIN email_trigger_cancellations c
                   ON c.cancellation_trigger_name = s.trigger_name
                WHERE s.user_id = ? AND c.trigger_id = ?
           ) AS "cancelled!: bool""#,
        user_id,
        trigger_id
    )
    .fetch_one(pool)
    .await?)
}

/// What one binding does about one event.
#[derive(Debug, PartialEq, Eq)]
enum BindingAction {
    SendNow,
    Schedule(jiff::Timestamp),
    /// Nothing, for the reason given.
    Skip(&'static str),
}

/// Decide what a binding does about an event. `cancelled` is whether the
/// user has already been through one of the binding's cancelling events.
///
/// An event happening now (no `occurred_at`) sends at once, or schedules its
/// delay from now. A replayed event sends nothing at once, since whatever that
/// was owed was sent when it happened, and counts its delay from when it
/// happened -- so a send that would already be due has missed its moment.
fn plan_binding(
    action_type: TriggerActionType,
    occurred_at: Option<&TimeString>,
    delay_seconds: i64,
    cancelled: bool,
    now: jiff::Timestamp,
) -> Result<BindingAction> {
    match action_type {
        TriggerActionType::Immediate if occurred_at.is_some() => Ok(BindingAction::Skip(
            "a replayed event sends nothing immediately",
        )),
        TriggerActionType::Immediate => Ok(BindingAction::SendNow),
        TriggerActionType::Delayed if cancelled => Ok(BindingAction::Skip(
            "the user has already been through a cancelling event",
        )),
        TriggerActionType::Delayed => {
            let delay = jiff::SignedDuration::from_secs(delay_seconds);

            let Some(occurred_at) = occurred_at else {
                return Ok(BindingAction::Schedule(now.checked_add(delay)?));
            };

            let due = jiff::Timestamp::try_from(occurred_at.clone())?.checked_add(delay)?;

            Ok(if due > now {
                BindingAction::Schedule(due)
            } else {
                BindingAction::Skip("the replayed event's send would already be due")
            })
        }
    }
}

/// How the triggering user is reached: at the address the event named, when
/// it named one, and otherwise as the user -- which is subject to their
/// account being verified and accepting mail.
fn triggering_user(user_id: Ulid, address: Option<&str>) -> EmailRecipient {
    match address {
        Some(address) => EmailRecipient::Email(address.to_string()),
        None => EmailRecipient::UserId(user_id),
    }
}

/// The recipients JSON a delayed send should be scheduled with: as
/// configured, except that where the event named the address to reach the
/// triggering user at, that address stands in for them. The address has to be
/// written down now, because when the send comes due there is no event left
/// to ask.
fn pin_triggering_user(recipients_json: &Option<String>, address: Option<&str>) -> Option<String> {
    let Some(address) = address else {
        return recipients_json.clone();
    };

    let at_address = serde_json::json!({ "email": address });

    let configured = recipients_json
        .as_deref()
        .and_then(|json| serde_json::from_str::<Vec<serde_json::Value>>(json).ok())
        .filter(|entries| !entries.is_empty());

    // Unconfigured, unreadable and empty all mean the triggering user, as
    // they do in `resolve_trigger_recipients`.
    let pinned = match configured {
        None => vec![at_address],
        Some(entries) => entries
            .into_iter()
            .map(|entry| match entry {
                serde_json::Value::String(s) if s == "triggering_user" => at_address.clone(),
                other => other,
            })
            .collect(),
    };

    Some(serde_json::Value::Array(pinned).to_string())
}

/// Resolve the recipients JSON into a list of EmailRecipient values.
/// When NULL or empty, defaults to sending to the triggering user.
/// JSON format: ["triggering_user", {"address_set": "slug"}, {"user_set": "slug"}, {"email": "addr"}]
pub(crate) fn resolve_trigger_recipients(
    recipients_json: &Option<String>,
    user_id: Ulid,
    address: Option<&str>,
) -> Vec<EmailRecipient> {
    let Some(json) = recipients_json else {
        return vec![triggering_user(user_id, address)];
    };

    let Ok(entries) = serde_json::from_str::<Vec<serde_json::Value>>(json) else {
        tracing::warn!(recipients = %json, "Invalid recipients JSON, falling back to triggering user");
        return vec![triggering_user(user_id, address)];
    };

    if entries.is_empty() {
        return vec![triggering_user(user_id, address)];
    }

    entries
        .into_iter()
        .filter_map(|entry| match entry {
            serde_json::Value::String(s) if s == "triggering_user" => {
                Some(triggering_user(user_id, address))
            }
            serde_json::Value::Object(ref map) => {
                if let Some(slug) = map.get("address_set").and_then(|v| v.as_str()) {
                    Some(EmailRecipient::AddressSet {
                        slug: slug.to_string(),
                    })
                } else if let Some(slug) = map.get("user_set").and_then(|v| v.as_str()) {
                    Some(EmailRecipient::UserSet {
                        slug: slug.to_string(),
                    })
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

    let mut tx = db().begin().await?;

    sqlx::query!(
        r#"INSERT INTO email_triggers
           (trigger_id, trigger_name, stored_email_id, action_type, delay_seconds, recipients, active, created_at, updated_at)
           VALUES (?, ?, ?, ?, ?, ?, 1, ?, ?)"#,
        trigger_id_str,
        request.trigger_name,
        stored_email_id_str,
        action_type_str,
        request.delay_seconds,
        request.recipients,
        created_at,
        updated_at
    )
    .execute(&mut *tx)
    .await?;

    set_cancellations(
        &mut tx,
        &trigger_id_str,
        &request.cancellation_trigger_names,
    )
    .await?;

    tx.commit().await?;

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
    query_builder.push_bind(trigger_id_str.clone());

    let mut tx = db().begin().await?;

    query_builder.build().execute(&mut *tx).await?;

    if let Some(names) = &request.cancellation_trigger_names {
        set_cancellations(&mut tx, &trigger_id_str, names).await?;
    }

    tx.commit().await?;

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
            r#"SELECT trigger_id, trigger_name, stored_email_id, action_type, delay_seconds, recipients, active, created_at, updated_at
               FROM email_triggers
               WHERE active = 1
               ORDER BY created_at DESC"#
        )
        .fetch_all(db())
        .await?
    } else {
        sqlx::query_as!(
            TriggerRow,
            r#"SELECT trigger_id, trigger_name, stored_email_id, action_type, delay_seconds, recipients, active, created_at, updated_at
               FROM email_triggers
               ORDER BY created_at DESC"#
        )
        .fetch_all(db())
        .await?
    };

    let mut cancellations = all_cancellations().await?;

    let triggers = rows
        .into_iter()
        .map(|row| {
            let names = row
                .trigger_id
                .as_ref()
                .and_then(|id| cancellations.remove(id))
                .unwrap_or_default();
            convert_trigger_row(row, names)
        })
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
        r#"SELECT trigger_id, trigger_name, stored_email_id, action_type, delay_seconds, recipients, active, created_at, updated_at
           FROM email_triggers
           WHERE trigger_id = ?"#,
        trigger_id_str
    )
    .fetch_optional(db())
    .await?;

    let Some(row) = row else {
        anyhow::bail!("Trigger not found: {}", request.trigger_id);
    };

    let names = cancellations_of(&trigger_id_str).await?;
    let trigger = convert_trigger_row(row, names)?;

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
        occurred_at = ?request.occurred_at,
        "Processing trigger event"
    );

    let user_id_str = request.user_id.to_string();
    let now = jiff::Timestamp::now();

    record_event_seen(
        db(),
        &user_id_str,
        &request.trigger_name,
        &request
            .occurred_at
            .clone()
            .unwrap_or_else(|| TimeString::from(now)),
    )
    .await?;

    let mut action_taken = TriggerActionTaken::NoTrigger;

    // First, cancel the pending sends of every binding this event cancels.
    let cancelled_bindings = sqlx::query!(
        r#"SELECT t.trigger_id AS "trigger_id!", t.trigger_name
             FROM email_triggers t
             JOIN email_trigger_cancellations c ON c.trigger_id = t.trigger_id
            WHERE c.cancellation_trigger_name = ? AND t.active = 1"#,
        request.trigger_name
    )
    .fetch_all(db())
    .await?;

    for binding in cancelled_bindings {
        let hook = binding_hook(&binding.trigger_id);
        let legacy_hook = legacy_event_hook(&binding.trigger_name);

        let result = sqlx::query!(
            r#"DELETE FROM scheduled_sends WHERE user_id = ? AND hook IN (?, ?)"#,
            user_id_str,
            hook,
            legacy_hook
        )
        .execute(db())
        .await?;

        let cancelled_count = result.rows_affected();
        if cancelled_count > 0 {
            tracing::info!(
                trigger_name = %request.trigger_name,
                cancelled_trigger = %binding.trigger_name,
                trigger_id = %binding.trigger_id,
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
        r#"SELECT trigger_id, trigger_name, stored_email_id, action_type, delay_seconds, recipients, active, created_at, updated_at
           FROM email_triggers
           WHERE trigger_name = ? AND active = 1"#,
        request.trigger_name
    )
    .fetch_all(db())
    .await?;

    for row in trigger_rows {
        let trigger_id_str = row.trigger_id.clone().unwrap_or_default();
        let trigger = convert_trigger_row(row, cancellations_of(&trigger_id_str).await?)?;

        // Settle whether this binding acts at all before rendering anything.
        let cancelled = trigger.action_type == TriggerActionType::Delayed
            && already_cancelled(db(), &user_id_str, &trigger_id_str).await?;

        let due = match plan_binding(
            trigger.action_type,
            request.occurred_at.as_ref(),
            trigger.delay_seconds.unwrap_or(0),
            cancelled,
            now,
        )? {
            BindingAction::SendNow => None,
            BindingAction::Schedule(due) => Some(due),
            BindingAction::Skip(reason) => {
                tracing::info!(
                    trigger_name = %request.trigger_name,
                    trigger_id = %trigger_id_str,
                    user_id = %request.user_id,
                    reason,
                    "Trigger binding does nothing for this event"
                );
                continue;
            }
        };

        // Get the stored email content
        let stored_email_id_str = trigger.stored_email_id.to_string();
        let stored_email_row = sqlx::query!(
            r#"SELECT subject, body_delta, datasets, reply_to_noreply, no_unsubscribe FROM stored_emails WHERE stored_email_id = ?"#,
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

        let datasets: Vec<String> =
            serde_json::from_str(&stored_email.datasets).unwrap_or_default();

        // Convert body_delta to HTML
        let template_html = common::quill::delta_to_html(&stored_email.body_delta)?;

        let reply_to = common::message::email::noreply_reply_to(stored_email.reply_to_noreply);

        // Whether the email offers to unsubscribe is the stored email's own
        // setting, so that it is sent the same way by this trigger as by a
        // send from the admin.
        let no_unsubscribe = stored_email.no_unsubscribe;

        // Build additional data with user_id
        let mut additional_data = request.additional_data.clone().unwrap_or_default();
        additional_data.push(("user_id".to_string(), request.user_id.to_string()));

        match due {
            None => {
                let recipients = resolve_trigger_recipients(
                    &trigger.recipients,
                    request.user_id,
                    request.address.as_deref(),
                );

                let _sending_id = send_email(
                    template_html,
                    stored_email.subject,
                    recipients,
                    datasets,
                    Some(additional_data),
                    reply_to,
                    no_unsubscribe,
                )
                .await?;

                tracing::info!(
                    trigger_name = %request.trigger_name,
                    user_id = %request.user_id,
                    "Sent immediate trigger email"
                );

                action_taken = TriggerActionTaken::SentImmediately;
            }
            Some(due) => {
                let scheduled = schedule_send(
                    request.user_id,
                    binding_hook(&trigger_id_str),
                    TimeString::from(due),
                    template_html,
                    stored_email.subject,
                    datasets,
                    Some(additional_data),
                    pin_triggering_user(&trigger.recipients, request.address.as_deref()),
                    reply_to,
                    no_unsubscribe,
                )
                .await?;

                if scheduled.is_some() {
                    tracing::info!(
                        trigger_name = %request.trigger_name,
                        trigger_id = %trigger_id_str,
                        user_id = %request.user_id,
                        due = %due,
                        "Scheduled delayed trigger email"
                    );

                    action_taken = TriggerActionTaken::Scheduled;
                }
            }
        }
    }

    Ok(TriggerEmailResponse {
        success: true,
        action_taken,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const USER: &str = "01JGEDS64C0ECBWR472V7788BF";
    const ADDRESS: &str = "ada@example.invalid";

    fn user() -> Ulid {
        USER.parse().unwrap()
    }

    fn targets(recipients: Vec<EmailRecipient>) -> Vec<String> {
        recipients
            .into_iter()
            .map(|recipient| match recipient {
                EmailRecipient::Email(address) => format!("email:{address}"),
                EmailRecipient::UserId(id) => format!("user:{id}"),
                EmailRecipient::UserSet { slug } => format!("user_set:{slug}"),
                EmailRecipient::AddressSet { slug } => format!("address_set:{slug}"),
            })
            .collect()
    }

    #[test]
    fn the_triggering_user_is_reached_as_a_user_by_default() {
        let expected = vec![format!("user:{USER}")];

        assert_eq!(
            targets(resolve_trigger_recipients(&None, user(), None)),
            expected
        );
        assert_eq!(
            targets(resolve_trigger_recipients(
                &Some(r#"["triggering_user"]"#.to_string()),
                user(),
                None
            )),
            expected
        );
    }

    /// A send addressed to a user is skipped while their account is
    /// unverified, which is exactly when the confirmation email is wanted.
    #[test]
    fn an_event_that_names_an_address_reaches_the_triggering_user_there() {
        let expected = vec![format!("email:{ADDRESS}")];

        for recipients in [
            None,
            Some("[]"),
            Some("not json"),
            Some(r#"["triggering_user"]"#),
        ] {
            assert_eq!(
                targets(resolve_trigger_recipients(
                    &recipients.map(str::to_string),
                    user(),
                    Some(ADDRESS)
                )),
                expected,
                "recipients: {recipients:?}"
            );
        }
    }

    #[test]
    fn naming_an_address_leaves_the_other_configured_recipients_alone() {
        let recipients =
            r#"["triggering_user", {"address_set": "staff"}, {"email": "info@example.invalid"}]"#;

        assert_eq!(
            targets(resolve_trigger_recipients(
                &Some(recipients.to_string()),
                user(),
                Some(ADDRESS)
            )),
            vec![
                format!("email:{ADDRESS}"),
                "address_set:staff".to_string(),
                "email:info@example.invalid".to_string(),
            ]
        );
    }

    #[test]
    fn a_delayed_send_keeps_its_recipients_when_no_address_is_named() {
        for recipients in [None, Some(r#"["triggering_user", {"user_set": "staff"}]"#)] {
            let recipients = recipients.map(str::to_string);

            assert_eq!(pin_triggering_user(&recipients, None), recipients);
        }
    }

    /// When the send comes due it is resolved without the event, so what was
    /// scheduled has to resolve to the address on its own.
    #[test]
    fn a_delayed_send_remembers_the_address_the_event_named() {
        for recipients in [
            None,
            Some("[]"),
            Some("not json"),
            Some(r#"["triggering_user"]"#),
        ] {
            let pinned = pin_triggering_user(&recipients.map(str::to_string), Some(ADDRESS));

            assert_eq!(
                targets(resolve_trigger_recipients(&pinned, user(), None)),
                vec![format!("email:{ADDRESS}")],
                "recipients: {recipients:?}"
            );
        }

        let pinned = pin_triggering_user(
            &Some(r#"["triggering_user", {"address_set": "staff"}]"#.to_string()),
            Some(ADDRESS),
        );

        assert_eq!(
            targets(resolve_trigger_recipients(&pinned, user(), None)),
            vec![format!("email:{ADDRESS}"), "address_set:staff".to_string()]
        );
    }

    async fn migrated() -> sqlx::SqlitePool {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("pool");

        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("migrations");

        pool
    }

    const SEED: &str =
        include_str!("../migrations/20261006154457_seed_email_validation_trigger.sql");

    /// The user service fires this trigger by the name the enum gives it, and
    /// the email it is bound to has to carry the confirmation link.
    #[tokio::test]
    async fn the_confirmation_email_is_seeded_and_bound_to_its_trigger() {
        let pool = migrated().await;
        let trigger_name = common::service::TriggerEvent::EmailValidation.as_ref();

        let row = sqlx::query!(
            r#"SELECT t.action_type, t.active, t.recipients, t.created_at, e.subject, e.body_delta, e.no_unsubscribe
               FROM email_triggers t
               JOIN stored_emails e ON e.stored_email_id = t.stored_email_id
               WHERE t.trigger_name = ?"#,
            trigger_name
        )
        .fetch_one(&pool)
        .await
        .expect("seeded trigger");

        assert_eq!(row.action_type, "immediate");
        assert!(row.active);
        assert_eq!(row.recipients, None);
        TimeString::try_from(row.created_at).expect("a timestamp the service can read");

        // It goes out whatever the account's mail settings say, so it is the
        // one email that offers no way to unsubscribe.
        assert!(row.no_unsubscribe);

        let html = common::quill::delta_to_html(&row.body_delta).expect("body");

        // The link is a button, on a line of its own after the paragraphs
        // that say why to follow it.
        let button = html
            .find(r#"<a href="{{VERIFY_URL}}" style=""#)
            .unwrap_or_else(|| panic!("no confirmation button in {html}"));
        let reason = html
            .find("Confirm your email to unlock more of SciStarter")
            .expect("the paragraph before the button");

        assert!(reason < button);
        assert!(html[button..].contains(">CONFIRM MY EMAIL</a></p><p><em>If you didn"));
        assert!(row.subject.contains("confirm your email"));
    }

    #[tokio::test]
    async fn seeding_again_changes_nothing() {
        let pool = migrated().await;

        sqlx::raw_sql(SEED).execute(&pool).await.expect("seed");

        let triggers = sqlx::query_scalar!(
            r#"SELECT count(*) FROM email_triggers WHERE trigger_name = 'email-validation'"#
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let emails = sqlx::query_scalar!(
            r#"SELECT count(*) FROM stored_emails WHERE slug = 'email-validation'"#
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        assert_eq!((triggers, emails), (1, 1));
    }

    /// Where staff bound the trigger to an email of their own before this
    /// seed arrived, theirs is the one that stays.
    #[tokio::test]
    async fn a_trigger_staff_already_made_is_left_alone() {
        let pool = migrated().await;

        sqlx::raw_sql(
            r#"DELETE FROM email_triggers;
               DELETE FROM stored_emails;
               INSERT INTO stored_emails (stored_email_id, slug, subject, body_delta, created_at, updated_at)
               VALUES ('theirs', 'confirm', 'Please confirm', '{"ops":[]}', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z');
               INSERT INTO email_triggers (trigger_id, trigger_name, stored_email_id, action_type, created_at, updated_at)
               VALUES ('theirs', 'email-validation', 'theirs', 'immediate', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z');"#,
        )
        .execute(&pool)
        .await
        .expect("staff's own trigger");

        sqlx::raw_sql(SEED).execute(&pool).await.expect("seed");

        let bound: Vec<String> =
            sqlx::query_scalar!(r#"SELECT stored_email_id FROM email_triggers"#)
                .fetch_all(&pool)
                .await
                .unwrap();
        let emails = sqlx::query_scalar!(r#"SELECT count(*) FROM stored_emails"#)
            .fetch_one(&pool)
            .await
            .unwrap();

        assert_eq!(bound, vec!["theirs".to_string()]);
        assert_eq!(emails, 1);

        // Nor is their email given a setting they did not choose, by the
        // migration that marks the seeded one.
        let theirs = sqlx::query_scalar!(
            r#"SELECT no_unsubscribe FROM stored_emails WHERE stored_email_id = 'theirs'"#
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        assert!(!theirs);
    }

    // ------------------------------------------------------------------
    // What a binding does about an event

    fn at(time: &str) -> jiff::Timestamp {
        time.parse().unwrap()
    }

    fn stamp(time: &str) -> TimeString {
        TimeString::try_from(time.to_string()).unwrap()
    }

    const NOW: &str = "2026-10-09T12:00:00Z";
    const WEEK: i64 = 7 * 86400;

    #[test]
    fn an_event_happening_now_sends_or_schedules_from_now() {
        assert_eq!(
            plan_binding(TriggerActionType::Immediate, None, 0, false, at(NOW)).unwrap(),
            BindingAction::SendNow
        );
        assert_eq!(
            plan_binding(TriggerActionType::Delayed, None, WEEK, false, at(NOW)).unwrap(),
            BindingAction::Schedule(at("2026-10-16T12:00:00Z"))
        );
    }

    /// orrery sent whatever the event owed at the time it happened.
    #[test]
    fn a_replayed_event_sends_nothing_immediately() {
        let occurred = stamp("2026-10-09T11:00:00Z");

        assert!(matches!(
            plan_binding(
                TriggerActionType::Immediate,
                Some(&occurred),
                0,
                false,
                at(NOW)
            )
            .unwrap(),
            BindingAction::Skip(_)
        ));
    }

    #[test]
    fn a_replayed_event_counts_its_delay_from_when_it_happened() {
        let occurred = stamp("2026-10-05T08:30:00Z");

        assert_eq!(
            plan_binding(
                TriggerActionType::Delayed,
                Some(&occurred),
                WEEK,
                false,
                at(NOW)
            )
            .unwrap(),
            BindingAction::Schedule(at("2026-10-12T08:30:00Z"))
        );
    }

    /// Replaying old history must not queue years of stale nudges.
    #[test]
    fn a_replayed_event_whose_send_would_already_be_due_schedules_nothing() {
        let occurred = stamp("2026-09-01T00:00:00Z");

        assert!(matches!(
            plan_binding(
                TriggerActionType::Delayed,
                Some(&occurred),
                WEEK,
                false,
                at(NOW)
            )
            .unwrap(),
            BindingAction::Skip(_)
        ));
    }

    #[test]
    fn a_user_past_a_cancelling_event_is_not_scheduled() {
        assert!(matches!(
            plan_binding(TriggerActionType::Delayed, None, WEEK, true, at(NOW)).unwrap(),
            BindingAction::Skip(_)
        ));
    }

    // ------------------------------------------------------------------
    // The events a user has been through

    async fn bind(pool: &sqlx::SqlitePool, trigger_id: &str, event: &str, cancelled_by: &[&str]) {
        sqlx::query(
            r#"INSERT INTO stored_emails (stored_email_id, slug, subject, body_delta, created_at, updated_at)
               VALUES (?1, ?1, 'Subject', '{"ops":[]}', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')"#,
        )
        .bind(trigger_id)
        .execute(pool)
        .await
        .unwrap();

        sqlx::query(
            r#"INSERT INTO email_triggers (trigger_id, trigger_name, stored_email_id, action_type, delay_seconds, created_at, updated_at)
               VALUES (?1, ?2, ?1, 'delayed', 604800, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')"#,
        )
        .bind(trigger_id)
        .bind(event)
        .execute(pool)
        .await
        .unwrap();

        for name in cancelled_by {
            sqlx::query(
                r#"INSERT INTO email_trigger_cancellations (trigger_id, cancellation_trigger_name) VALUES (?, ?)"#,
            )
            .bind(trigger_id)
            .bind(name)
            .execute(pool)
            .await
            .unwrap();
        }
    }

    /// The sync replays orrery's tables independently, so the step that
    /// makes a nudge moot can arrive before the event that would schedule it.
    #[tokio::test]
    async fn a_cancelling_event_counts_whichever_order_it_arrives_in() {
        let pool = migrated().await;
        bind(
            &pool,
            "nudge",
            "user-welcome",
            &["foundations-tutorial", "badge-foundations"],
        )
        .await;

        assert!(!already_cancelled(&pool, USER, "nudge").await.unwrap());

        record_event_seen(
            &pool,
            USER,
            "badge-foundations",
            &stamp("2026-10-01T00:00:00Z"),
        )
        .await
        .unwrap();

        assert!(already_cancelled(&pool, USER, "nudge").await.unwrap());
        assert!(
            !already_cancelled(&pool, "someone-else", "nudge")
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn an_event_is_noted_once_at_the_earliest_time_it_is_said_to_have_happened() {
        let pool = migrated().await;

        for time in [
            "2026-10-05T00:00:00Z",
            "2026-10-03T00:00:00.5Z",
            "2026-10-07T00:00:00Z",
        ] {
            record_event_seen(&pool, USER, "user-welcome", &stamp(time))
                .await
                .unwrap();
        }

        let noted: Vec<String> =
            sqlx::query_scalar(r#"SELECT occurred_at FROM trigger_events_seen WHERE user_id = ?"#)
                .bind(USER)
                .fetch_all(&pool)
                .await
                .unwrap();

        assert_eq!(noted, vec![stamp("2026-10-03T00:00:00.5Z").to_string()]);
    }

    // ------------------------------------------------------------------
    // The seeded nudges

    const NUDGE_SEED: &str = include_str!("../migrations/20261009012844_seed_nudge_triggers.sql");

    async fn nudge_bindings(pool: &sqlx::SqlitePool) -> Vec<(String, String, i64, String)> {
        sqlx::query_as(
            r#"SELECT t.trigger_name, e.slug, t.delay_seconds,
                      coalesce((SELECT group_concat(c.cancellation_trigger_name, ' ')
                                  FROM (SELECT cancellation_trigger_name FROM email_trigger_cancellations
                                         WHERE trigger_id = t.trigger_id
                                         ORDER BY cancellation_trigger_name) c), '')
                 FROM email_triggers t
                 JOIN stored_emails e ON e.stored_email_id = t.stored_email_id
                WHERE t.action_type = 'delayed'
                ORDER BY e.slug, t.trigger_name"#,
        )
        .fetch_all(pool)
        .await
        .unwrap()
    }

    /// The events the bindings name are the ones the code raises, under the
    /// names the enum gives them.
    #[tokio::test]
    async fn the_nudges_are_seeded_as_delayed_bindings_with_their_cancellations() {
        use common::service::TriggerEvent::*;

        let pool = migrated().await;
        let day = 86400;
        let started = [
            BadgeFoundations,
            FoundationsDebris,
            FoundationsSquirrel,
            FoundationsStall,
            FoundationsTutorial,
        ]
        .map(|event| event.to_string())
        .join(" ");
        let applied = [AmbassadorApplicationSubmitted, AmbassadorPromoted]
            .map(|event| event.to_string())
            .join(" ");

        let row = |event: common::service::TriggerEvent, slug: &str, days: i64, cancels: &str| {
            (
                event.to_string(),
                slug.to_string(),
                days * day,
                cancels.to_string(),
            )
        };

        assert_eq!(
            nudge_bindings(&pool).await,
            vec![
                row(
                    BadgeAmbassador,
                    "ambassador-onboarding-reminder",
                    30,
                    &[
                        AmbassadorDemoted,
                        AmbassadorOrientationRecorded,
                        AmbassadorPromoted
                    ]
                    .map(|event| event.to_string())
                    .join(" ")
                ),
                row(
                    AmbassadorTrainingComplete,
                    "champion-ambassador-followup",
                    30,
                    &applied
                ),
                row(
                    AmbassadorTrainingComplete,
                    "champion-training-complete-reminder-2daysafter",
                    2,
                    &applied
                ),
                row(
                    BadgeFoundations,
                    "champion-training-invitation",
                    7,
                    &AmbassadorTrainingComplete.to_string()
                ),
                row(UserWelcome, "foundations-invitation", 7, &started),
                row(UserWelcomeGirlScout, "foundations-invitation", 7, &started),
                row(
                    UserWelcomeGirlScoutVolunteer,
                    "foundations-invitation",
                    7,
                    &started
                ),
            ]
        );
    }

    #[tokio::test]
    async fn seeding_the_nudges_again_changes_nothing() {
        let pool = migrated().await;
        let before = nudge_bindings(&pool).await;

        sqlx::raw_sql(NUDGE_SEED)
            .execute(&pool)
            .await
            .expect("seed");

        assert_eq!(nudge_bindings(&pool).await, before);
    }

    #[tokio::test]
    async fn the_nudges_greet_the_member_and_end_in_their_button() {
        let pool = migrated().await;

        let bodies: Vec<(String, String, String)> = sqlx::query_as(
            r#"SELECT slug, datasets, body_delta FROM stored_emails
                WHERE slug IN ('foundations-invitation', 'champion-training-invitation',
                               'champion-ambassador-followup', 'ambassador-onboarding-reminder',
                               'champion-training-complete-reminder-2daysafter')"#,
        )
        .fetch_all(&pool)
        .await
        .unwrap();

        assert_eq!(bodies.len(), 5);

        for (slug, datasets, delta) in bodies {
            let html = common::quill::delta_to_html(&delta).expect("body");

            // The greeting's name comes from the user_identity dataset.
            assert_eq!(datasets, r#"["user_identity"]"#, "{slug}");
            assert!(html.contains("{{FRIENDLY_NAME}}"), "{slug}: {html}");

            // Staff's two-day reminder has its link in the text; the rest
            // carry a button.
            if slug != "champion-training-complete-reminder-2daysafter" {
                assert!(html.contains("background-color:#D63511"), "{slug}: {html}");
            }
        }
    }

    // ------------------------------------------------------------------
    // The migrations, run against what they found

    /// Run every migration before `version`, as a database that has not had
    /// it yet stands.
    async fn migrated_before(version: i64) -> sqlx::SqlitePool {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("pool");

        for migration in sqlx::migrate!("./migrations").iter() {
            if migration.version < version {
                sqlx::raw_sql(migration.sql.clone())
                    .execute(&pool)
                    .await
                    .expect("migration");
            }
        }

        pool
    }

    async fn run_migration(pool: &sqlx::SqlitePool, version: i64) {
        let migrator = sqlx::migrate!("./migrations");
        let migration = migrator
            .iter()
            .find(|migration| migration.version == version)
            .expect("migration");

        sqlx::raw_sql(migration.sql.clone())
            .execute(pool)
            .await
            .expect("migration");
    }

    #[tokio::test]
    async fn a_binding_keeps_the_cancelling_event_it_had() {
        let pool = migrated_before(20261009012841).await;

        sqlx::raw_sql(
            r#"INSERT INTO stored_emails (stored_email_id, slug, subject, body_delta, created_at, updated_at)
               VALUES ('e', 'e', 'Subject', '{"ops":[]}', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z');
               INSERT INTO email_triggers (trigger_id, trigger_name, stored_email_id, action_type, delay_seconds, cancellation_trigger_name, created_at, updated_at)
               VALUES ('with', 'user-welcome', 'e', 'delayed', 60, 'foundations-tutorial', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z'),
                      ('without', 'user-welcome', 'e', 'delayed', 60, NULL, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z');"#,
        )
        .execute(&pool)
        .await
        .unwrap();

        run_migration(&pool, 20261009012841).await;

        let kept: Vec<(String, String)> = sqlx::query_as(
            r#"SELECT trigger_id, cancellation_trigger_name FROM email_trigger_cancellations"#,
        )
        .fetch_all(&pool)
        .await
        .unwrap();

        assert_eq!(
            kept,
            vec![("with".to_string(), "foundations-tutorial".to_string())]
        );
    }

    #[tokio::test]
    async fn waiting_sends_are_moved_under_the_binding_that_scheduled_them() {
        let pool = migrated_before(20261009012842).await;

        sqlx::raw_sql(
            r#"INSERT INTO stored_emails (stored_email_id, slug, subject, body_delta, created_at, updated_at)
               VALUES ('a', 'a', 'Subject A', '{"ops":[]}', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z'),
                      ('b', 'b', 'Subject B', '{"ops":[]}', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z');
               INSERT INTO email_triggers (trigger_id, trigger_name, stored_email_id, action_type, delay_seconds, created_at, updated_at)
               VALUES ('ta', 'badge-foundations', 'a', 'delayed', 60, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z'),
                      ('tb', 'badge-foundations', 'b', 'delayed', 60, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z');
               INSERT INTO scheduled_sends (scheduled_id, user_id, hook, scheduled_time, template_html, subject, data_sets, additional_data, created_at)
               VALUES ('01', 'u', 'trigger:badge-foundations', '2026-11-01T00:00:00Z', '', 'Subject A', '[]', '[]', '2026-01-01T00:00:00Z'),
                      ('02', 'u', 'trigger:badge-foundations', '2026-11-01T00:00:00Z', '', 'Subject A', '[]', '[]', '2026-01-01T00:00:00Z'),
                      ('03', 'u', 'trigger:badge-foundations', '2026-11-01T00:00:00Z', '', 'Subject B', '[]', '[]', '2026-01-01T00:00:00Z'),
                      ('04', 'u', 'trigger:badge-foundations', '2026-11-01T00:00:00Z', '', 'Unknown', '[]', '[]', '2026-01-01T00:00:00Z');"#,
        )
        .execute(&pool)
        .await
        .unwrap();

        run_migration(&pool, 20261009012842).await;

        let waiting: Vec<(String, String)> = sqlx::query_as(
            r#"SELECT scheduled_id, hook FROM scheduled_sends ORDER BY scheduled_id"#,
        )
        .fetch_all(&pool)
        .await
        .unwrap();

        // The duplicate goes, keeping the first; the one no binding can claim
        // keeps its event key.
        assert_eq!(
            waiting,
            vec![
                ("01".to_string(), "trigger:ta".to_string()),
                ("03".to_string(), "trigger:tb".to_string()),
                ("04".to_string(), "trigger:badge-foundations".to_string()),
            ]
        );

        // And from here on, one per user and hook.
        let again = sqlx::raw_sql(
            r#"INSERT INTO scheduled_sends (scheduled_id, user_id, hook, scheduled_time, template_html, subject, data_sets, additional_data, created_at)
               VALUES ('05', 'u', 'trigger:ta', '2026-11-01T00:00:00Z', '', 'Subject A', '[]', '[]', '2026-01-01T00:00:00Z')"#,
        )
        .execute(&pool)
        .await;

        assert!(again.is_err());
    }
}
