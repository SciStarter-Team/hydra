use anyhow::{Error, anyhow};
use async_nats::service::Request;
use common::{
    message::email::{
        AddTargetsToSendingRequest, AddTargetsToSendingResponse, EmailRecipient, SendEmailRequest,
        SendEmailResponse,
    },
    message::user::{
        AddressSetGetRequest, AddressSetGetResponse, UserSetGetRequest, UserSetGetResponse,
        UserSetReleaseResultsRequest,
    },
    service::request,
    shared::db,
    timestring::TimeString,
};
use ulid::Ulid;

/// Process recipients in the background, queueing them in batches
/// Updates the database incrementally as recipients are queued
async fn process_recipients_background(
    sending_id: String,
    recipients: Vec<EmailRecipient>,
    template_html: String,
    subject: String,
    data_sets_json: String,
    additional_data_json: String,
    reply_to_json: Option<String>,
    no_unsubscribe: bool,
) -> Result<(), Error> {
    let queued_at = TimeString::now();
    let reply_to_json = reply_to_json.as_deref();

    for (idx, recipient) in recipients.iter().enumerate() {
        tracing::debug!(
            sending_id = %sending_id,
            recipient_index = idx,
            recipient_type = match recipient {
                EmailRecipient::Email(_) => "Email",
                EmailRecipient::UserId(_) => "UserId",
                EmailRecipient::UserSet { .. } => "UserSet",
                EmailRecipient::AddressSet { .. } => "AddressSet",
            },
            "Processing recipient"
        );

        let result = match recipient {
            EmailRecipient::Email(email) => {
                tracing::info!(
                    sending_id = %sending_id,
                    email = %email,
                    "Queueing email recipient"
                );
                let count = queue_single_recipient(
                    &sending_id,
                    "email",
                    &email,
                    &template_html,
                    &subject,
                    &data_sets_json,
                    &additional_data_json,
                    reply_to_json,
                    &queued_at,
                    QueuePriority::Individual,
                    no_unsubscribe,
                )
                .await?;

                // Update database after queueing this recipient
                increment_total_queued(&sending_id, count).await?;
                Ok(())
            }
            EmailRecipient::UserId(user_id) => {
                tracing::info!(
                    sending_id = %sending_id,
                    user_id = %user_id,
                    "Queueing user ID recipient"
                );
                let count = queue_single_recipient(
                    &sending_id,
                    "user_id",
                    &user_id.to_string(),
                    &template_html,
                    &subject,
                    &data_sets_json,
                    &additional_data_json,
                    reply_to_json,
                    &queued_at,
                    QueuePriority::Individual,
                    no_unsubscribe,
                )
                .await?;

                // Update database after queueing this recipient
                increment_total_queued(&sending_id, count).await?;
                Ok(())
            }
            EmailRecipient::UserSet { slug } => {
                tracing::info!(
                    sending_id = %sending_id,
                    slug = %slug,
                    "Processing user set"
                );
                process_user_set(
                    &sending_id,
                    &slug,
                    &template_html,
                    &subject,
                    &data_sets_json,
                    &additional_data_json,
                    reply_to_json,
                    &queued_at,
                    no_unsubscribe,
                )
                .await
            }
            EmailRecipient::AddressSet { slug } => {
                tracing::info!(
                    sending_id = %sending_id,
                    slug = %slug,
                    "Processing address set"
                );
                process_address_set(
                    &sending_id,
                    &slug,
                    &template_html,
                    &subject,
                    &data_sets_json,
                    &additional_data_json,
                    reply_to_json,
                    &queued_at,
                    no_unsubscribe,
                )
                .await
            }
        };

        if let Err(e) = result {
            tracing::error!(
                sending_id = %sending_id,
                error = %e,
                "Error processing recipient"
            );
        }
    }

    tracing::info!(
        sending_id = %sending_id,
        "Finished queueing recipients"
    );

    Ok(())
}

/// Increment the total_queued counter in the database
async fn increment_total_queued(sending_id: &str, count: u32) -> Result<(), Error> {
    sqlx::query!(
        r#"UPDATE sendings SET total_queued = total_queued + ? WHERE sending_id = ?"#,
        count,
        sending_id
    )
    .execute(db())
    .await?;

    Ok(())
}

/// Where a queued message stands relative to the rest of the queue, which is
/// read highest priority first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QueuePriority {
    /// One of the recipients of a mailing to a set, which can number in the
    /// tens of thousands and take hours to drain.
    Mailing,
    /// A recipient the send named in person: a confirmation link, a welcome,
    /// a notice to staff. Somebody is waiting for it, so it does not wait
    /// behind a mailing.
    Individual,
}

impl QueuePriority {
    /// The value stored in `send_queue.priority`.
    fn rank(self) -> i64 {
        match self {
            QueuePriority::Mailing => 0,
            QueuePriority::Individual => 1,
        }
    }
}

/// Queue a single recipient
async fn queue_single_recipient(
    sending_id: &str,
    target_type: &str,
    target_value: &str,
    template_html: &str,
    subject: &str,
    data_sets_json: &str,
    additional_data_json: &str,
    reply_to_json: Option<&str>,
    queued_at: &TimeString,
    priority: QueuePriority,
    no_unsubscribe: bool,
) -> Result<u32, Error> {
    let queue_id = Ulid::new();
    let queue_id_str = queue_id.to_string();
    let priority = priority.rank();

    sqlx::query!(
        r#"INSERT INTO send_queue
           (queue_id, sending_id, target_type, target_value, data_sets, template_html, subject, additional_data, reply_to, queued_at, priority, no_unsubscribe, status)
           VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'pending')"#,
        queue_id_str,
        sending_id,
        target_type,
        target_value,
        data_sets_json,
        template_html,
        subject,
        additional_data_json,
        reply_to_json,
        queued_at,
        priority,
        no_unsubscribe,
    )
    .execute(db())
    .await?;

    Ok(1)
}

/// Process a user set by paginating through results and queueing each batch
/// Updates the database after each page is queued
async fn process_user_set(
    sending_id: &str,
    slug: &str,
    template_html: &str,
    subject: &str,
    data_sets_json: &str,
    additional_data_json: &str,
    reply_to_json: Option<&str>,
    queued_at: &TimeString,
    no_unsubscribe: bool,
) -> Result<(), Error> {
    tracing::info!(
        sending_id = %sending_id,
        slug = %slug,
        "Starting user set processing"
    );

    let mut page = 0;
    let limit = 1000;
    let mut evaluation_id: Option<String> = None;
    let mut total_queued = 0u32;

    loop {
        tracing::debug!(
            sending_id = %sending_id,
            slug = %slug,
            page = page,
            "Fetching user set page"
        );
        let response: UserSetGetResponse = request(
            "user.user_sets.get",
            UserSetGetRequest {
                slug: slug.to_string(),
                page: Some(page),
                limit: Some(limit),
                evaluation_id: evaluation_id.clone(),
            },
        )
        .await?;

        if evaluation_id.is_none() {
            evaluation_id = Some(response.evaluation_id.clone());
        }

        let batch_size = response.user_ids.len() as u32;

        tracing::info!(
            sending_id = %sending_id,
            slug = %slug,
            page = page,
            batch_size = batch_size,
            "Queueing user set batch"
        );

        // Queue all users in this batch
        for user_id in response.user_ids {
            queue_single_recipient(
                sending_id,
                "user_id",
                &user_id.to_string(),
                template_html,
                subject,
                data_sets_json,
                additional_data_json,
                reply_to_json,
                queued_at,
                QueuePriority::Mailing,
                no_unsubscribe,
            )
            .await?;
        }

        // Update database after queueing this page
        increment_total_queued(sending_id, batch_size).await?;
        total_queued += batch_size;

        if !response.has_more {
            tracing::info!(
                sending_id = %sending_id,
                slug = %slug,
                total_queued = total_queued,
                "Finished processing user set"
            );
            // Clean up result set
            if let Some(eval_id) = evaluation_id {
                let _: common::message::user::UserSetReleaseResultsResponse = request(
                    "user.user_sets.release_results",
                    UserSetReleaseResultsRequest {
                        evaluation_id: eval_id,
                    },
                )
                .await?;
            }
            break;
        }

        page += 1;
    }

    Ok(())
}

/// Process an address set by paginating through addresses and queueing each batch
/// Updates the database after each page is queued
async fn process_address_set(
    sending_id: &str,
    slug: &str,
    template_html: &str,
    subject: &str,
    data_sets_json: &str,
    additional_data_json: &str,
    reply_to_json: Option<&str>,
    queued_at: &TimeString,
    no_unsubscribe: bool,
) -> Result<(), Error> {
    tracing::info!(
        sending_id = %sending_id,
        slug = %slug,
        "Starting address set processing"
    );

    let mut page = 0;
    let limit = 1000;
    let mut total_queued = 0u32;

    loop {
        tracing::debug!(
            sending_id = %sending_id,
            slug = %slug,
            page = page,
            "Fetching address set page"
        );
        let response: AddressSetGetResponse = request(
            "user.address_sets.get",
            AddressSetGetRequest {
                slug: slug.to_string(),
                page: Some(page),
                limit: Some(limit),
            },
        )
        .await?;

        let batch_size = response.addresses.len() as u32;

        tracing::info!(
            sending_id = %sending_id,
            slug = %slug,
            page = page,
            batch_size = batch_size,
            "Queueing address set batch"
        );

        // Queue all addresses in this batch
        for email in response.addresses {
            queue_single_recipient(
                sending_id,
                "email",
                &email,
                template_html,
                subject,
                data_sets_json,
                additional_data_json,
                reply_to_json,
                queued_at,
                QueuePriority::Mailing,
                no_unsubscribe,
            )
            .await?;
        }

        // Update database after queueing this page
        increment_total_queued(sending_id, batch_size).await?;
        total_queued += batch_size;

        if !response.has_more {
            tracing::info!(
                sending_id = %sending_id,
                slug = %slug,
                total_queued = total_queued,
                "Finished processing address set"
            );
            break;
        }

        page += 1;
    }

    Ok(())
}

/// A sending as its record holds it: everything the recipients queued for it
/// are sent, now or when more are added later.
struct Sending {
    sending_id: Ulid,
    template_html: String,
    subject: String,
    data_sets_json: String,
    additional_data_json: String,
    reply_to_json: Option<String>,
    no_unsubscribe: bool,
}

impl Sending {
    /// Create the record of a new sending, with nobody queued for it yet.
    async fn create(
        template_html: String,
        subject: String,
        data_sets: Vec<String>,
        additional_data: Option<Vec<(String, String)>>,
        reply_to: Option<Vec<String>>,
        no_unsubscribe: bool,
    ) -> Result<Self, Error> {
        let sending_id = Ulid::new();
        let sending_id_str = sending_id.to_string();
        let created_at = TimeString::now();

        // Serialize data for storage
        let data_sets_json = serde_json::to_string(&data_sets)?;
        let additional_data_json = serde_json::to_string(&additional_data.unwrap_or_default())?;
        // Reply-To is stored as a JSON array, or NULL when unset.
        let reply_to_json = reply_to
            .filter(|r| !r.is_empty())
            .map(|r| serde_json::to_string(&r))
            .transpose()?;

        // Create sending record with total_queued = 0 initially
        sqlx::query!(
            r#"INSERT INTO sendings (sending_id, created_at, subject, total_queued, template_html, data_sets, additional_data, reply_to, no_unsubscribe)
               VALUES (?, ?, ?, 0, ?, ?, ?, ?, ?)"#,
            sending_id_str,
            created_at,
            subject,
            template_html,
            data_sets_json,
            additional_data_json,
            reply_to_json,
            no_unsubscribe
        )
        .execute(db())
        .await?;

        Ok(Sending {
            sending_id,
            template_html,
            subject,
            data_sets_json,
            additional_data_json,
            reply_to_json,
            no_unsubscribe,
        })
    }

    /// Queue recipients for this sending.
    async fn queue(self, recipients: Vec<EmailRecipient>) -> Result<(), Error> {
        process_recipients_background(
            self.sending_id.to_string(),
            recipients,
            self.template_html,
            self.subject,
            self.data_sets_json,
            self.additional_data_json,
            self.reply_to_json,
            self.no_unsubscribe,
        )
        .await
    }
}

/// Core logic for sending an email
/// Creates a sending record and processes recipients in the background
pub async fn send_email(
    template_html: String,
    subject: String,
    recipients: Vec<EmailRecipient>,
    data_sets: Vec<String>,
    additional_data: Option<Vec<(String, String)>>,
    reply_to: Option<Vec<String>>,
    no_unsubscribe: bool,
) -> Result<Ulid, Error> {
    let sending = Sending::create(
        template_html,
        subject,
        data_sets,
        additional_data,
        reply_to,
        no_unsubscribe,
    )
    .await?;
    let sending_id = sending.sending_id;

    tracing::info!(
        sending_id = %sending_id,
        subject = %sending.subject,
        "Email sending created, processing recipients in background"
    );

    // Spawn background task to process recipients
    tokio::spawn(async move {
        if let Err(e) = sending.queue(recipients).await {
            tracing::error!(
                sending_id = %sending_id,
                error = %e,
                "Error processing recipients in background"
            );
        }
    });

    Ok(sending_id)
}

/// As [`send_email`], but returns only once the recipients are in the queue.
///
/// For a caller that is about to forget the send was ever wanted -- a
/// scheduled send deleting its own record -- and so must not do that while
/// the queueing could still be lost.
pub async fn send_email_queued(
    template_html: String,
    subject: String,
    recipients: Vec<EmailRecipient>,
    data_sets: Vec<String>,
    additional_data: Option<Vec<(String, String)>>,
    reply_to: Option<Vec<String>>,
    no_unsubscribe: bool,
) -> Result<Ulid, Error> {
    let sending = Sending::create(
        template_html,
        subject,
        data_sets,
        additional_data,
        reply_to,
        no_unsubscribe,
    )
    .await?;
    let sending_id = sending.sending_id;

    sending.queue(recipients).await?;

    Ok(sending_id)
}

/// Handle a request to send emails
/// Creates a sending record and returns immediately, processing recipients in the background
#[tracing::instrument(skip(req))]
pub async fn handle_send_request(req: &Request) -> Result<SendEmailResponse, Error> {
    tracing::info!("Received send email request");

    let request: SendEmailRequest = flexbuffers::from_slice(&req.message.payload)?;

    tracing::info!(
        recipient_count = request.recipients.len(),
        subject = %request.subject,
        "Parsed send request"
    );

    let sending_id = send_email(
        request.template_html,
        request.subject,
        request.recipients,
        request.data_sets,
        request.additional_data,
        request.reply_to,
        request.no_unsubscribe,
    )
    .await?;

    Ok(SendEmailResponse { sending_id })
}

/// Handle a request to add additional targets to an existing sending
/// Returns immediately and processes recipients in the background
#[tracing::instrument(skip(req))]
pub async fn handle_add_targets_request(
    req: &Request,
) -> Result<AddTargetsToSendingResponse, Error> {
    let request: AddTargetsToSendingRequest = flexbuffers::from_slice(&req.message.payload)?;
    let sending_id_str = request.sending_id.to_string();

    // Get the template and other data from the sendings table
    let sending = sqlx::query!(
        r#"SELECT template_html, subject, data_sets, additional_data, reply_to, no_unsubscribe
           FROM sendings
           WHERE sending_id = ?"#,
        sending_id_str
    )
    .fetch_optional(db())
    .await?;

    let sending = sending.ok_or_else(|| anyhow!("Sending not found"))?;

    // Spawn background task to process recipients
    let sending_id_for_task = sending_id_str.clone();
    tokio::spawn(async move {
        if let Err(e) = process_recipients_background(
            sending_id_for_task.clone(),
            request.recipients,
            sending.template_html,
            sending.subject,
            sending.data_sets,
            sending.additional_data,
            sending.reply_to,
            sending.no_unsubscribe,
        )
        .await
        {
            tracing::error!(
                sending_id = %sending_id_for_task,
                error = %e,
                "Error processing additional recipients in background"
            );
        }
    });

    tracing::info!(
        sending_id = %request.sending_id,
        "Additional targets being processed in background"
    );

    Ok(AddTargetsToSendingResponse { success: true })
}
