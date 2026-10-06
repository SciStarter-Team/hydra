use anyhow::{Error, Result};
use async_nats::service::Request;
use common::{message::email::*, shared::db, timestring::TimeString};
use jiff::Zoned;
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
        cancellation_trigger_name: row.cancellation_trigger_name,
        recipients: row.recipients,
        active: row
            .active
            .ok_or_else(|| anyhow::anyhow!("active is NULL"))?,
        created_at,
        updated_at,
    })
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

        let recipients = resolve_trigger_recipients(
            &trigger.recipients,
            request.user_id,
            request.address.as_deref(),
        );

        match trigger.action_type {
            TriggerActionType::Immediate => {
                // Send immediately
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
            TriggerActionType::Delayed => {
                // Schedule for later
                let delay_seconds = trigger.delay_seconds.unwrap_or(0);
                let scheduled_zoned =
                    Zoned::now().checked_add(jiff::Span::new().seconds(delay_seconds))?;
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
                    pin_triggering_user(&trigger.recipients, request.address.as_deref()),
                    reply_to,
                    no_unsubscribe,
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

        let triggers = sqlx::query_scalar!(r#"SELECT count(*) FROM email_triggers"#)
            .fetch_one(&pool)
            .await
            .unwrap();
        let emails = sqlx::query_scalar!(r#"SELECT count(*) FROM stored_emails"#)
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
}
