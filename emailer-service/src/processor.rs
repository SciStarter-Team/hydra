use anyhow::Error;
use aws_sdk_sesv2::Client as SesClient;
use common::{
    message::user::{UserEmailInfo, UserEmailInfoRequest},
    service::request,
    shared::db,
    timestring::TimeString,
};
use std::time::Duration;
use ulid::Ulid;

use crate::{
    datasets::DATASET_REGISTRY,
    format::{add_footer_to_plain_text, html_to_markdown, wrap_html_layout},
    ratelimit::{mark_sent, wait_until_can_send},
    ses::{from_email, send_email},
    template::merge_template,
};

/// Why a send was skipped. Each variant corresponds to a per-reason counter
/// column on the `sendings` table; `messages_skipped` tracks the grand total.
#[derive(Debug, Clone, Copy)]
enum SkipReason {
    Unsubscribed,
    Bounced,
    Complained,
    Unverified,
    OptedOut,
    Invalid,
    Failed,
}

/// Reject addresses whose domain has no TLD component (e.g. `user@clever`,
/// `user@broward`). Flanker-style parsers accept single-label hostnames as
/// technically valid, but they can never be delivered.
fn is_valid_email_format(email: &str) -> bool {
    match email.split_once('@') {
        Some((local, domain)) => !local.is_empty() && domain.contains('.'),
        None => false,
    }
}

/// Background task to process the email queue
pub async fn run_queue_processor(ses_client: SesClient) {
    loop {
        match process_next_email(&ses_client).await {
            Ok(processed) => {
                if !processed {
                    // No emails in queue, wait before checking again
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
            Err(e) => {
                tracing::error!("Error processing email: {:?}", e);
                tokio::time::sleep(Duration::from_secs(10)).await;
            }
        }
    }
}

/// Background task to process scheduled sends
pub async fn run_scheduled_processor() {
    loop {
        match process_scheduled_sends().await {
            Ok(processed) => {
                if !processed {
                    // No scheduled sends due, wait before checking again
                    tokio::time::sleep(Duration::from_secs(30)).await;
                } else {
                    // Process next one immediately
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
            Err(e) => {
                tracing::error!("Error processing scheduled sends: {:?}", e);
                tokio::time::sleep(Duration::from_secs(60)).await;
            }
        }
    }
}

/// Process the next email in the queue
/// Returns Ok(true) if an email was processed, Ok(false) if queue was empty
async fn process_next_email(ses_client: &SesClient) -> Result<bool, Error> {
    // Fetch next pending email from queue
    let queue_item = sqlx::query!(
        r#"SELECT queue_id, sending_id, target_type, target_value, data_sets, template_html, subject, additional_data, retry_count
           FROM send_queue
           WHERE status = 'pending'
           LIMIT 1"#
    )
    .fetch_optional(db())
    .await?;

    let Some(item) = queue_item else {
        return Ok(false); // Queue is empty
    };

    let queue_id = item.queue_id.as_deref().unwrap_or("unknown");
    let retry_count = item.retry_count;

    tracing::info!(
        queue_id = %queue_id,
        sending_id = %item.sending_id,
        target_type = %item.target_type,
        target_value = %item.target_value,
        retry_count = retry_count,
        "Processing email from queue"
    );

    // Mark as processing
    sqlx::query!(
        r#"UPDATE send_queue SET status = 'processing' WHERE queue_id = ?"#,
        queue_id
    )
    .execute(db())
    .await?;

    // Process the email, handling errors with retry logic
    let result = async {
        // Resolve email address. For user_id targets, enforce the user's
        // verified and accept_emails flags before doing anything else.
        let (email_address, user_id_opt) = match item.target_type.as_str() {
            "email" => {
                tracing::debug!(email = %item.target_value, "Target is email address");
                (item.target_value.clone(), None)
            }
            "user_id" => {
                let user_id = item.target_value.parse::<Ulid>()?;
                tracing::debug!(user_id = %user_id, "Fetching user email info from user-service");
                tracing::debug!("Calling NATS endpoint: user.info.email_info");
                let user_info = request::<UserEmailInfo>(
                    "user.info.email_info",
                    UserEmailInfoRequest { user_id },
                )
                .await
                .map_err(|e| {
                    tracing::error!(
                        user_id = %user_id,
                        endpoint = "user.info.email_info",
                        error = %e,
                        "Failed to call user-service endpoint"
                    );
                    e
                })?;

                let queue_id = item
                    .queue_id
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("queue_id is NULL"))?;

                if !user_info.verified {
                    tracing::info!(
                        user_id = %user_id,
                        email = %user_info.primary_email,
                        sending_id = %item.sending_id,
                        "User is not verified, skipping"
                    );
                    record_skip(
                        queue_id,
                        &item.sending_id,
                        &user_info.primary_email,
                        "unverified",
                    )
                    .await?;
                    delete_from_queue(queue_id).await?;
                    update_sending_skipped(&item.sending_id, SkipReason::Unverified).await?;
                    return Ok(true);
                }

                if !user_info.accept_emails {
                    tracing::info!(
                        user_id = %user_id,
                        email = %user_info.primary_email,
                        sending_id = %item.sending_id,
                        "User has opted out of emails, skipping"
                    );
                    record_skip(
                        queue_id,
                        &item.sending_id,
                        &user_info.primary_email,
                        "opted_out",
                    )
                    .await?;
                    delete_from_queue(queue_id).await?;
                    update_sending_skipped(&item.sending_id, SkipReason::OptedOut).await?;
                    return Ok(true);
                }

                tracing::debug!(
                    user_id = %user_id,
                    email = %user_info.primary_email,
                    "Resolved user to email address"
                );
                (user_info.primary_email, Some(user_id))
            }
            _ => anyhow::bail!("Invalid target_type: {}", item.target_type),
        };

        // Reject syntactically-valid-but-undeliverable addresses (no TLD, etc.)
        // before bothering SES with them.
        if !is_valid_email_format(&email_address) {
            tracing::warn!(
                email = %email_address,
                sending_id = %item.sending_id,
                "Email address has no deliverable domain, skipping"
            );
            let queue_id = item
                .queue_id
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("queue_id is NULL"))?;
            record_skip(queue_id, &item.sending_id, &email_address, "invalid").await?;
            delete_from_queue(queue_id).await?;
            update_sending_skipped(&item.sending_id, SkipReason::Invalid).await?;
            return Ok(true);
        }

        // Check do_not_send table and break out the skip by suppression reason.
        let blocked = sqlx::query!(
            r#"SELECT reason FROM do_not_send WHERE email_address = ?"#,
            email_address
        )
        .fetch_optional(db())
        .await?;

        if let Some(row) = blocked {
            let skip_reason = match row.reason.as_str() {
                "unsubscribe" => SkipReason::Unsubscribed,
                "bounce" => SkipReason::Bounced,
                "complaint" => SkipReason::Complained,
                _ => SkipReason::Unsubscribed,
            };
            tracing::warn!(
                email = %email_address,
                sending_id = %item.sending_id,
                reason = %row.reason,
                "Email address is on do-not-send list, skipping"
            );
            let queue_id = item
                .queue_id
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("queue_id is NULL"))?;
            record_skip(queue_id, &item.sending_id, &email_address, &row.reason).await?;
            delete_from_queue(queue_id).await?;
            update_sending_skipped(&item.sending_id, skip_reason).await?;
            return Ok(true);
        }

        // Load data sets and collect merge fields
        let data_set_names: Vec<String> = serde_json::from_str(&item.data_sets)?;
        let mut all_field_names = Vec::new();
        let mut all_field_values = Vec::new();

        tracing::debug!(
            datasets = ?data_set_names,
            has_user_id = user_id_opt.is_some(),
            "Loading datasets for merge fields"
        );

        if let Some(uid) = user_id_opt {
            for dataset_name in &data_set_names {
                if let Some(dataset) = DATASET_REGISTRY.get(dataset_name) {
                    let (field_names, field_values) = dataset.get_values(uid).await?;
                    tracing::debug!(
                        dataset = %dataset_name,
                        field_count = field_names.len(),
                        "Loaded dataset values"
                    );
                    all_field_names.extend(field_names);
                    all_field_values.extend(field_values);
                } else {
                    tracing::warn!("Unknown dataset: {}", dataset_name);
                }
            }
        }

        // Add additional custom merge fields
        let additional_data: Vec<(String, String)> = serde_json::from_str(&item.additional_data)?;
        let (add_names, add_values): (Vec<String>, Vec<String>) =
            additional_data.into_iter().unzip();
        all_field_names.extend(add_names);
        all_field_values.extend(add_values);

        // Add unsubscribe URL field
        // Use user_id if available, otherwise use email address for non-user recipients
        let unsubscribe_param = user_id_opt
            .map(|id| id.to_string())
            .unwrap_or_else(|| email_address.clone());
        let unsubscribe_path = format!(
            "/messaging/emailer/unsubscribe?a={}&b={}",
            item.sending_id, unsubscribe_param
        );
        let open_path = format!(
            "/messaging/emailer/open?a={}&b={}",
            item.sending_id, unsubscribe_param
        );
        all_field_names.push("UNSUBSCRIBE_URL".to_string());
        all_field_values.push(format!("https://{}{}", env!("DOMAIN"), unsubscribe_path));

        // Merge template
        let merged_html = merge_template(&item.template_html, &all_field_names, &all_field_values)?;

        // Merge subject
        let merged_subject = merge_template(&item.subject, &all_field_names, &all_field_values)?;

        // Wrap in layout
        let final_html = wrap_html_layout(&merged_html, &unsubscribe_path, &open_path);

        // Convert to plain text
        let plain_text = html_to_markdown(&merged_html);
        let final_text = add_footer_to_plain_text(&plain_text);

        // Wait for rate limit and quota
        wait_until_can_send().await;

        tracing::info!(
            email = %email_address,
            subject = %merged_subject,
            sending_id = %item.sending_id,
            "Sending email via SES"
        );

        // Send via SES
        let sending_ulid = item.sending_id.parse::<Ulid>()?;
        let ses_message_id = match send_email(
            ses_client,
            from_email(),
            &email_address,
            &merged_subject,
            &final_html,
            &final_text,
            sending_ulid,
            user_id_opt,
        )
        .await
        {
            Ok(msg_id) => {
                tracing::info!(
                    email = %email_address,
                    ses_message_id = %msg_id,
                    sending_id = %item.sending_id,
                    "SES accepted email"
                );
                msg_id
            }
            Err(e) => {
                tracing::error!(
                    email = %email_address,
                    sending_id = %item.sending_id,
                    error = %e,
                    "SES rejected email"
                );
                return Err(e);
            }
        };

        // Record success
        let queue_id = item
            .queue_id
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("queue_id is NULL"))?;
        record_sent(queue_id, &item.sending_id, &email_address, &ses_message_id).await?;

        // Delete from queue
        delete_from_queue(queue_id).await?;

        // Update sending metrics
        update_sending_sent(&item.sending_id).await?;

        // Mark as sent for quota tracking
        mark_sent().await;

        tracing::info!(
            queue_id = %queue_id,
            email = %email_address,
            "Email sent successfully"
        );

        Ok(true)
    }
    .await;

    // Handle result with retry logic
    match result {
        Ok(success) => Ok(success),
        Err(e) => {
            tracing::error!(
                queue_id = %queue_id,
                error = %e,
                retry_count = retry_count,
                "Failed to process email"
            );

            const MAX_RETRIES: i64 = 3;
            let new_retry_count = retry_count + 1;

            if new_retry_count >= MAX_RETRIES {
                // Too many failures, mark as failed and remove from queue
                tracing::error!(
                    queue_id = %queue_id,
                    sending_id = %item.sending_id,
                    "Max retries reached, marking as failed"
                );

                record_skip(
                    queue_id,
                    &item.sending_id,
                    &item.target_value,
                    &format!("max_retries: {}", e),
                )
                .await?;
                delete_from_queue(queue_id).await?;
                update_sending_skipped(&item.sending_id, SkipReason::Failed).await?;
            } else {
                // Reset to pending with incremented retry count
                if let Err(reset_err) = sqlx::query!(
                    r#"UPDATE send_queue SET status = 'pending', retry_count = ? WHERE queue_id = ?"#,
                    new_retry_count,
                    queue_id
                )
                .execute(db())
                .await
                {
                    tracing::error!(
                        queue_id = %queue_id,
                        error = %reset_err,
                        "Failed to reset queue item back to pending"
                    );
                }
            }
            Err(e)
        }
    }
}

async fn record_sent(
    queue_id: &str,
    sending_id: &str,
    email_address: &str,
    ses_message_id: &str,
) -> Result<(), Error> {
    let history_id = Ulid::new().to_string();
    let sent_at = TimeString::now();

    sqlx::query!(
        r#"INSERT INTO send_history
           (history_id, queue_id, sending_id, email_address, sent_at, status, ses_message_id)
           VALUES (?, ?, ?, ?, ?, 'sent', ?)"#,
        history_id,
        queue_id,
        sending_id,
        email_address,
        sent_at,
        ses_message_id
    )
    .execute(db())
    .await?;

    Ok(())
}

async fn record_skip(
    queue_id: &str,
    sending_id: &str,
    email_address: &str,
    reason: &str,
) -> Result<(), Error> {
    let history_id = Ulid::new().to_string();

    sqlx::query!(
        r#"INSERT INTO send_history
           (history_id, queue_id, sending_id, email_address, sent_at, status, skip_reason)
           VALUES (?, ?, ?, ?, NULL, 'skipped', ?)"#,
        history_id,
        queue_id,
        sending_id,
        email_address,
        reason
    )
    .execute(db())
    .await?;

    Ok(())
}

async fn delete_from_queue(queue_id: &str) -> Result<(), Error> {
    sqlx::query!(r#"DELETE FROM send_queue WHERE queue_id = ?"#, queue_id)
        .execute(db())
        .await?;
    Ok(())
}

async fn update_sending_sent(sending_id: &str) -> Result<(), Error> {
    sqlx::query!(
        r#"UPDATE sendings SET messages_sent = messages_sent + 1 WHERE sending_id = ?"#,
        sending_id
    )
    .execute(db())
    .await?;
    Ok(())
}

async fn update_sending_skipped(sending_id: &str, reason: SkipReason) -> Result<(), Error> {
    match reason {
        SkipReason::Unsubscribed => {
            sqlx::query!(
                r#"UPDATE sendings
                   SET messages_skipped = messages_skipped + 1,
                       skipped_unsubscribed = skipped_unsubscribed + 1
                   WHERE sending_id = ?"#,
                sending_id
            )
            .execute(db())
            .await?;
        }
        SkipReason::Bounced => {
            sqlx::query!(
                r#"UPDATE sendings
                   SET messages_skipped = messages_skipped + 1,
                       skipped_bounced = skipped_bounced + 1
                   WHERE sending_id = ?"#,
                sending_id
            )
            .execute(db())
            .await?;
        }
        SkipReason::Complained => {
            sqlx::query!(
                r#"UPDATE sendings
                   SET messages_skipped = messages_skipped + 1,
                       skipped_complained = skipped_complained + 1
                   WHERE sending_id = ?"#,
                sending_id
            )
            .execute(db())
            .await?;
        }
        SkipReason::Unverified => {
            sqlx::query!(
                r#"UPDATE sendings
                   SET messages_skipped = messages_skipped + 1,
                       skipped_unverified = skipped_unverified + 1
                   WHERE sending_id = ?"#,
                sending_id
            )
            .execute(db())
            .await?;
        }
        SkipReason::OptedOut => {
            sqlx::query!(
                r#"UPDATE sendings
                   SET messages_skipped = messages_skipped + 1,
                       skipped_opted_out = skipped_opted_out + 1
                   WHERE sending_id = ?"#,
                sending_id
            )
            .execute(db())
            .await?;
        }
        SkipReason::Invalid => {
            sqlx::query!(
                r#"UPDATE sendings
                   SET messages_skipped = messages_skipped + 1,
                       skipped_invalid = skipped_invalid + 1
                   WHERE sending_id = ?"#,
                sending_id
            )
            .execute(db())
            .await?;
        }
        SkipReason::Failed => {
            sqlx::query!(
                r#"UPDATE sendings
                   SET messages_skipped = messages_skipped + 1,
                       send_failed = send_failed + 1
                   WHERE sending_id = ?"#,
                sending_id
            )
            .execute(db())
            .await?;
        }
    }
    Ok(())
}

/// Process scheduled sends that are due
/// Returns Ok(true) if a send was processed, Ok(false) if no sends are due
async fn process_scheduled_sends() -> Result<bool, Error> {
    let now = TimeString::now();

    // Fetch next scheduled send that is due
    let scheduled = sqlx::query!(
        r#"SELECT scheduled_id, user_id, hook, template_html, subject, data_sets, additional_data
           FROM scheduled_sends
           WHERE scheduled_time <= ?
           ORDER BY scheduled_time ASC
           LIMIT 1"#,
        now
    )
    .fetch_optional(db())
    .await?;

    let Some(item) = scheduled else {
        return Ok(false); // No scheduled sends are due
    };

    // Create a new sending record
    let sending_id = Ulid::new();
    let sending_id_str = sending_id.to_string();
    let created_at = TimeString::now();

    sqlx::query!(
        r#"INSERT INTO sendings (sending_id, created_at, subject, total_queued)
           VALUES (?, ?, ?, 1)"#,
        sending_id_str,
        created_at,
        item.subject
    )
    .execute(db())
    .await?;

    // Queue the email
    let queue_id = Ulid::new();
    let queue_id_str = queue_id.to_string();
    let queued_at = TimeString::now();

    sqlx::query!(
        r#"INSERT INTO send_queue
           (queue_id, sending_id, target_type, target_value, data_sets, template_html, subject, additional_data, queued_at, status)
           VALUES (?, ?, 'user_id', ?, ?, ?, ?, ?, ?, 'pending')"#,
        queue_id_str,
        sending_id_str,
        item.user_id,
        item.data_sets,
        item.template_html,
        item.subject,
        item.additional_data,
        queued_at
    )
    .execute(db())
    .await?;

    // Delete the scheduled send record
    sqlx::query!(
        r#"DELETE FROM scheduled_sends WHERE scheduled_id = ?"#,
        item.scheduled_id
    )
    .execute(db())
    .await?;

    tracing::info!(
        scheduled_id = %item.scheduled_id,
        user_id = %item.user_id,
        hook = %item.hook,
        sending_id = %sending_id,
        "Scheduled send queued for delivery"
    );

    Ok(true)
}
