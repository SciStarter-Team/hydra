use anyhow::{Error, anyhow};
use async_nats::{HeaderMap, Message, subject::ToSubject};
use errors::DecodeError;
use futures::StreamExt;
use serde::de::DeserializeOwned;
use std::time::Duration;
use tracing::{debug, warn};
use ulid::Ulid;

use crate::{shared::nats, timestring::TimeString, ulid::AsUlid};

/// Handler helper for duplex request-response service endpoints
#[macro_export]
macro_rules! handler {
    ($group:ident, $name:literal, $fut:expr) => {
        $group
            .endpoint($name)
            .await
            .expect("Unable to create endpoint")
            .for_each_concurrent(None, |req| async move {
                match $fut(&req).await {
                    Ok(val) => {
                        let Ok(payload) = flexbuffers::to_vec(val) else {
                            if let Err(x) = req
                                .respond(Err(common::service::errors::service_error(
                                    "Error while encoding response payload",
                                    common::service::errors::ErrorCode::RequestError,
                                )))
                                .await
                            {
                                eprintln!("{}: {:?}", x.to_string(), x);
                            }
                            return;
                        };

                        if let Err(x) = req.respond(Ok(payload.into())).await {
                            eprintln!("{}: {:?}", x.to_string(), x);
                        }
                    }
                    Err(err) => {
                        if let Err(x) = req
                            .respond(Err(common::service::errors::service_error(
                                err.to_string(),
                                common::service::errors::ErrorCode::HandlerError,
                            )))
                            .await
                        {
                            eprintln!("{}: {:?}", x.to_string(), x);
                        }
                    }
                }
            })
    };
}

/// Handler helper for simplex service endpoints
#[macro_export]
macro_rules! recv_handler {
    ($group:ident, $name:literal, $fut:expr) => {
        $group
            .endpoint($name)
            .await
            .expect("Unable to create endpoint")
            .for_each_concurrent(None, |req| async move {
                match $fut(&req).await {
                    Ok(_) => {}
                    Err(err) => {
                        eprintln!("{}: {:?}", err.to_string(), err);
                    }
                }
            })
    };
}

/// Publish a structured data message through NATS, without waiting
/// for a reply.
pub async fn send(
    subject: impl ToSubject,
    message: impl serde::Serialize,
) -> Result<(), anyhow::Error> {
    let payload = flexbuffers::to_vec(message)?;

    nats().publish(subject, payload.into()).await?;

    Ok(())
}

#[macro_export]
macro_rules! nats_headers {
    ( $($name: literal : $value: expr ),* $(,)?) => {
        {
            use async_nats::header::{IntoHeaderName, IntoHeaderValue};
            let mut headers = async_nats::header::HeaderMap::new();
            $(headers.append($name.into_header_name(), $value.into_header_value()));+;
            headers
        }
    };
}

/// Publish a structured data message with headers through NATS,
/// without waiting for a reply.
pub async fn send_with_headers(
    subject: impl ToSubject,
    headers: HeaderMap,
    message: impl serde::Serialize,
) -> Result<(), anyhow::Error> {
    let payload = flexbuffers::to_vec(message)?;

    nats()
        .publish_with_headers(subject, headers, payload.into())
        .await?;

    Ok(())
}

/// Transforms an [`async_nats::Message`] (or anything which is
/// `Into<Message>` such as [`async_nats::jetream::Message`]) into
/// structured data.
pub fn decode_structured_message<R>(msg: impl Into<Message>) -> Result<R, anyhow::Error>
where
    R: DeserializeOwned,
{
    let msg = msg.into();
    Ok(flexbuffers::from_slice(&msg.payload)?)
}

/// Receive a message through NATS, and decode it into structured data.
///
/// This creates a subscription, waits for a single message, decodes
/// the payload, and returns it. That's useful for one-offs, but doing
/// it in a loop means you're *not* subscribed during the rest of the
/// loop body, and risks missing messages.
///
/// If you need to listen for and process multiple messages in a loop,
/// and you're not doing something which should use the NATS Service
/// API instead, create a subscription and apply
/// [decode_structured_message] to the messages it
/// produces. [`StreamExt::map`] can help streamline that.
pub async fn recv<R>(
    subject: impl ToSubject,
    queue: impl Into<Option<String>>,
) -> Result<R, anyhow::Error>
where
    R: DeserializeOwned,
{
    let mut subs = if let Some(q) = queue.into() {
        nats().queue_subscribe(subject, q).await
    } else {
        nats().subscribe(subject).await
    }?;

    subs.unsubscribe_after(1).await?;

    let Some(msg) = subs.next().await else {
        return Err(anyhow!("Subscription ended prematurely"));
    };

    decode_structured_message(msg)
}

/// Publish a structured data message through NATS, wait for a reply,
/// and decode it into structured data.
///
/// This uses the [NATS Service API] mechanism and is meant for
/// bidirectional communication with services.
///
/// [NATS Service API]: https://docs.rs/async-nats/latest/async_nats/service/struct.Service.html
pub async fn request<R>(
    subject: impl ToSubject,
    message: impl serde::Serialize,
) -> Result<R, anyhow::Error>
where
    R: DeserializeOwned,
{
    let payload = flexbuffers::to_vec(message)?;

    let msg = nats()
        .request(subject, payload.into())
        .await?
        .decode_error()?;

    decode_structured_message(msg)
}

/// Publish a structured data message through NATS, wait for a reply with timeout,
/// and decode it into structured data.
///
/// This uses the [NATS Service API] mechanism and is meant for
/// bidirectional communication with services where you need timeout control.
///
/// [NATS Service API]: https://docs.rs/async-nats/latest/async_nats/service/struct.Service.html
pub async fn request_timeout<R>(
    subject: impl ToSubject,
    message: impl serde::Serialize,
    timeout: std::time::Duration,
) -> Result<R, anyhow::Error>
where
    R: DeserializeOwned,
{
    let payload = flexbuffers::to_vec(message)?;

    // Use tokio::time::timeout to enforce our timeout, not NATS client's default
    let msg = tokio::time::timeout(timeout, nats().request(subject, payload.into()))
        .await
        .map_err(|_| anyhow!("request timed out: deadline has elapsed"))?
        .map_err(|e| anyhow!("NATS request failed: {}", e))?
        .decode_error()?;

    decode_structured_message(msg)
}

/// Publish a structured data message through NATS, wait for a reply,
/// and decode it into structured data with infinite retry on failure.
///
/// This function will retry indefinitely with exponential backoff when requests fail.
/// It's useful for critical operations that must eventually succeed, especially during
/// service startup when dependencies may not be immediately available.
///
/// The retry delay starts at 100ms and doubles on each failure, up to a maximum
/// of 30 seconds between attempts.
///
/// Note: This function will never return an Err - it retries indefinitely until success.
/// However, it returns Result<R, anyhow::Error> for API compatibility with the regular
/// request function.
pub async fn request_with_retry<R>(
    subject: impl ToSubject,
    message: impl serde::Serialize + Clone,
) -> Result<R, anyhow::Error>
where
    R: DeserializeOwned,
{
    let mut retry_count = 0u64;
    let mut delay = Duration::from_millis(100);
    let subject_str = subject.to_subject();

    loop {
        match request(subject_str.clone(), message.clone()).await {
            Ok(response) => {
                if retry_count > 0 {
                    debug!(
                        subject = %subject_str,
                        retry_count,
                        "Request succeeded after retries"
                    );
                }
                return Ok(response);
            }
            Err(e) => {
                retry_count += 1;

                // Log first few attempts as warnings, then reduce to debug to avoid spam
                if retry_count <= 3 {
                    warn!(
                        subject = %subject_str,
                        retry_count,
                        delay_ms = delay.as_millis(),
                        error = %e,
                        "Request failed, retrying"
                    );
                } else if retry_count % 10 == 0 {
                    // Log every 10th attempt to show we're still trying
                    warn!(
                        subject = %subject_str,
                        retry_count,
                        delay_ms = delay.as_millis(),
                        "Request still failing after {} attempts",
                        retry_count
                    );
                } else {
                    debug!(
                        subject = %subject_str,
                        retry_count,
                        error = %e,
                        "Request failed, retrying"
                    );
                }

                tokio::time::sleep(delay).await;

                // Exponential backoff, capped at 30 seconds
                delay = std::cmp::min(delay * 2, Duration::from_secs(30));
            }
        }
    }
}

/// A convenience wrapper around [`common::service::send`] for sending
/// activity logging messages.
pub fn log_activity(
    when: impl Into<Option<TimeString>>,
    action: impl Into<String>,
    user_id: impl Into<Option<Ulid>>,
    target_id: impl Into<Option<Ulid>>,
    extra: impl Into<Option<String>>,
) -> impl Future<Output = Result<(), Error>> {
    let when: String = when.into().unwrap_or_else(|| TimeString::now()).into();
    let action: String = action.into();
    let user_id: Option<String> = user_id
        .into()
        .and_then(|u| u.as_ulid().ok())
        .map(|u| u.to_string());
    let target_id: Option<String> = target_id
        .into()
        .and_then(|t| t.as_ulid().ok())
        .map(|t| t.to_string());
    let extra: Option<String> = extra.into().map(|e| e.into());
    send("log.activity", (when, action, user_id, target_id, extra))
}

/// A convenience wrapper around [`common::service::send`] for sending
/// page view logging messages. Automatically resolves geolocation from IP address if not provided.
pub async fn log_pageview(
    when: impl Into<Option<TimeString>>,
    ip_address: impl Into<String>,
    device_id: impl Into<String>,
    interaction_id: impl Into<String>,
    user_id: impl Into<Option<Ulid>>,
    path: impl Into<String>,
    referrer: impl Into<Option<String>>,
    query: impl Into<Option<String>>,
    geolocation: impl Into<Option<String>>,
) -> Result<(), Error> {
    use crate::message::location::LocationInfo;

    let when: String = when.into().unwrap_or_else(|| TimeString::now()).into();
    let ip_address: String = ip_address.into();
    let device_id: String = device_id.into();
    let interaction_id: String = interaction_id.into();
    let user_id: Option<String> = user_id
        .into()
        .and_then(|u| u.as_ulid().ok())
        .map(|u| u.to_string());
    let path: String = path.into();
    let referrer: Option<String> = referrer.into();
    let query: Option<String> = query.into();

    // Use provided geolocation or resolve from IP address
    let geolocation = match geolocation.into() {
        Some(geo) => geo,
        None => {
            let location_info: LocationInfo = request("location.ip", &ip_address)
                .await
                .unwrap_or_default();
            location_info
                .as_point()
                .map(|p| p.to_string())
                .unwrap_or_else(|| r#"{"type":"Point","coordinates":[0.0,0.0]}"#.to_string())
        }
    };

    send(
        "log.pageview",
        (
            when,
            ip_address,
            device_id,
            interaction_id,
            user_id,
            path,
            referrer,
            query,
            geolocation,
        ),
    )
    .await
}

/// A convenience wrapper around [`common::service::send`] for sending
/// clickthrough logging messages. Automatically resolves geolocation from IP address if not provided.
pub async fn log_clickthrough(
    when: impl Into<Option<TimeString>>,
    ip_address: impl Into<String>,
    device_id: impl Into<String>,
    interaction_id: impl Into<String>,
    user_id: impl Into<Option<Ulid>>,
    from_path: impl Into<String>,
    to_url: impl Into<String>,
    link_type: impl Into<String>,
    program_id: impl Into<Option<String>>,
    geolocation: impl Into<Option<String>>,
) -> Result<(), Error> {
    use crate::message::location::LocationInfo;

    let when: String = when.into().unwrap_or_else(|| TimeString::now()).into();
    let ip_address: String = ip_address.into();
    let device_id: String = device_id.into();
    let interaction_id: String = interaction_id.into();
    let user_id: Option<String> = user_id
        .into()
        .and_then(|u| u.as_ulid().ok())
        .map(|u| u.to_string());
    let from_path: String = from_path.into();
    let to_url: String = to_url.into();
    let link_type: String = link_type.into();
    let program_id: Option<String> = program_id.into();

    // Use provided geolocation or resolve from IP address
    let geolocation = match geolocation.into() {
        Some(geo) => geo,
        None => {
            let location_info: LocationInfo = request("location.ip", &ip_address)
                .await
                .unwrap_or_default();
            location_info
                .as_point()
                .map(|p| p.to_string())
                .unwrap_or_else(|| r#"{"type":"Point","coordinates":[0.0,0.0]}"#.to_string())
        }
    };

    send(
        "log.clickthrough",
        (
            when,
            ip_address,
            device_id,
            interaction_id,
            user_id,
            from_path,
            to_url,
            link_type,
            program_id,
            geolocation,
        ),
    )
    .await
}

/// A convenience wrapper around [`common::service::send`] for recording
/// social-share button clicks.
///
/// A share is an action, not a clickthrough, so it is logged to its own
/// `share_events` table rather than the clickthrough pipeline. This lets us
/// answer "how many times was each share option clicked in a given timeframe"
/// without polluting external-link clickthrough analytics.
pub fn log_share(
    when: impl Into<Option<TimeString>>,
    target_id: Ulid,
    platform: crate::message::log::SharePlatform,
    user_id: impl Into<Option<Ulid>>,
    device_id: impl Into<String>,
    interaction_id: impl Into<String>,
) -> impl Future<Output = Result<(), Error>> {
    let when: String = when.into().unwrap_or_else(|| TimeString::now()).into();
    let target_id: String = target_id.to_string();
    let platform: String = platform.to_string();
    let user_id: Option<String> = user_id
        .into()
        .and_then(|u| u.as_ulid().ok())
        .map(|u| u.to_string());
    let device_id: String = device_id.into();
    let interaction_id: String = interaction_id.into();

    send(
        "log.share",
        (
            when,
            target_id,
            platform,
            user_id,
            device_id,
            interaction_id,
        ),
    )
}

/// A convenience wrapper around [`common::service::request`] for sending
/// emails via the emailer service.
///
/// Returns the sending_id which can be used to track the email delivery status.
///
/// # Example
///
/// ```no_run
/// use common::message::email::EmailRecipient;
/// use ulid::Ulid;
///
/// let sending_id = send_email(
///     vec![EmailRecipient::UserId(user_id)],
///     "Welcome to SciStarter",
///     "<h1>Welcome!</h1><p>Thanks for joining.</p>",
///     vec!["user_profile".to_string()],
///     None,
/// ).await?;
/// ```
pub async fn send_email(
    recipients: impl Into<Vec<crate::message::email::EmailRecipient>>,
    subject: impl Into<String>,
    template_html: impl Into<String>,
    data_sets: impl Into<Vec<String>>,
    additional_data: impl Into<Option<Vec<(String, String)>>>,
) -> Result<Ulid, Error> {
    use crate::message::email::{SendEmailRequest, SendEmailResponse};

    let email_request = SendEmailRequest {
        template_html: template_html.into(),
        subject: subject.into(),
        recipients: recipients.into(),
        data_sets: data_sets.into(),
        additional_data: additional_data.into(),
    };

    let response: SendEmailResponse = request("emailer.send", email_request).await?;
    Ok(response.sending_id)
}

/// A convenience wrapper around [`common::service::send`] for logging
/// email events to the log service.
///
/// Event types: "sent", "bounce", "complaint", "open", "unsubscribe"
pub fn log_email_event(
    when: impl Into<Option<TimeString>>,
    event_type: impl Into<String>,
    sending_id: impl Into<Ulid>,
    email_address: impl Into<String>,
    user_id: impl Into<Option<Ulid>>,
    reason: impl Into<Option<String>>,
) -> impl Future<Output = Result<(), Error>> {
    let when: String = when.into().unwrap_or_else(|| TimeString::now()).into();
    let event_type: String = event_type.into();
    let sending_id: String = sending_id.into().to_string();
    let email_address: String = email_address.into();
    let user_id: Option<String> = user_id
        .into()
        .and_then(|u| u.as_ulid().ok())
        .map(|u| u.to_string());
    let reason: Option<String> = reason.into();

    send(
        "log.email_event",
        (when, event_type, sending_id, email_address, user_id, reason),
    )
}

pub mod errors {
    use std::str::FromStr;

    use async_nats::service::error::Error;
    use num_enum::{IntoPrimitive, TryFromPrimitive};

    #[derive(IntoPrimitive, TryFromPrimitive)]
    #[repr(usize)]
    pub enum ErrorCode {
        Unknown,
        RequestError,
        HandlerError,
    }

    impl FromStr for ErrorCode {
        type Err = anyhow::Error;

        fn from_str(s: &str) -> Result<Self, Self::Err> {
            let code: usize = s.parse()?;
            Ok(ErrorCode::try_from(code)?)
        }
    }

    pub fn service_error(status: impl Into<String>, code: ErrorCode) -> Error {
        Error {
            status: status.into(),
            code: code.into(),
        }
    }

    pub trait DecodeError {
        fn decode_error(self) -> Result<async_nats::Message, async_nats::service::error::Error>;
    }

    impl DecodeError for async_nats::Message {
        fn decode_error(self) -> Result<async_nats::Message, async_nats::service::error::Error> {
            if let Some(headers) = self.headers.as_ref() {
                if let Some(val) = headers.get("Nats-Service-Error") {
                    Err(service_error(
                        val.as_str(),
                        headers
                            .get("Nats-Service-Error-Code")
                            .and_then(|v| v.as_str().parse().ok())
                            .unwrap_or(ErrorCode::Unknown),
                    ))
                } else {
                    Ok(self)
                }
            } else {
                Ok(self)
            }
        }
    }
}

/// Email trigger events
///
/// Each variant represents a specific event that can trigger an automated email.
/// To add a new trigger, simply add a new variant to this enum.
///
/// The string representation (Display/AsRefStr) is the trigger name stored in the database
/// and used for NATS communication.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    strum::Display,
    strum::EnumString,
    strum::EnumIter,
    strum::AsRefStr,
)]
#[strum(serialize_all = "kebab-case")]
pub enum TriggerEvent {
    /// Sent when a new user completes signup
    UserWelcome,

    /// Sent when a project is submitted for review
    ProjectSubmitted,

    /// Sent when a project is approved
    ProjectApproved,

    /// Sent when the Foundations of Citizen Science badge is earned
    BadgeFoundations,

    /// Sent when the Libraries as Hubs for Citizen Science badge is earned
    BadgeLibrariesHubs,

    /// Sent when the Building Data Literacy badge is earned
    BadgeDataLiteracy,

    /// Sent when the Teaching in Higher Education badge is earned
    BadgeHigherEd,

    /// Sent when the Data Ethics for Practitioners badge is earned
    BadgeDataEthics,

    /// Sent when the Ideal Participatory Science badge is earned
    BadgeIdeal,

    /// Sent when the SciStarter Ambassador badge is earned
    BadgeAmbassador,

    /// Sent when Foundations tutorial is completed
    FoundationsTutorial,

    /// Sent when Squirrel Mapper project is completed (Foundations)
    FoundationsSquirrel,

    /// Sent when Stall Catchers project is completed (Foundations)
    FoundationsStall,

    /// Sent when Debris Tracker project is completed (Foundations)
    FoundationsDebris,

    /// Sent when Libraries tutorial is completed
    LibrariesHubsTutorial,

    /// Sent when Libraries supplemental signup is completed
    LibrariesHubsSupplemental,

    /// Sent when a user joins a community
    CommunityJoin,

    /// Sent when a user leaves a community
    CommunityLeave,

    /// Sent when a user earns a certificate
    CertificateEarned,

    /// Sent when a user needs to verify their email address
    EmailValidation,

    /// Sent when a project is published
    ProjectPublished,

    /// Sent when a new project is submitted for review (staff notification)
    ProjectCreatedStaff,

    /// Sent when a new forum post is created
    ForumNewPost,

    /// Sent when someone replies to a forum post
    ForumPostReply,

    /// Sent when a forum post is edited
    ForumPostEdited,

    /// Sent when a user makes their first contribution to a project
    FirstContribution,

    /// Sent to confirm a people finder request was submitted
    PeopleFinderConfirmation,

    /// Sent when a new people finder request needs approval
    PeopleFinderNewRequest,

    /// Sent when an Acts of Science Connected application is submitted
    ActsOfScienceApplication,

    /// Sent to confirm Acts of Science participation
    ActsOfScienceConfirmation,

    /// Generic notification sent to staff
    StaffNotification,
}

impl TriggerEvent {
    /// Get a human-readable description of this trigger
    pub fn description(&self) -> &'static str {
        match self {
            Self::UserWelcome => "When a new user completes signup",
            Self::ProjectSubmitted => "When a project is submitted for review",
            Self::ProjectApproved => "When a project is approved",
            Self::BadgeFoundations => "When the Foundations of Citizen Science badge is earned",
            Self::BadgeLibrariesHubs => {
                "When the Libraries as Hubs for Citizen Science badge is earned"
            }
            Self::BadgeDataLiteracy => "When the Building Data Literacy badge is earned",
            Self::BadgeHigherEd => "When the Teaching in Higher Education badge is earned",
            Self::BadgeDataEthics => "When the Data Ethics for Practitioners badge is earned",
            Self::BadgeIdeal => "When the Ideal Participatory Science badge is earned",
            Self::BadgeAmbassador => "When the SciStarter Ambassador badge is earned",
            Self::FoundationsTutorial => "When Foundations tutorial is completed",
            Self::FoundationsSquirrel => "When Squirrel Mapper project is completed (Foundations)",
            Self::FoundationsStall => "When Stall Catchers project is completed (Foundations)",
            Self::FoundationsDebris => "When Debris Tracker project is completed (Foundations)",
            Self::LibrariesHubsTutorial => "When Libraries tutorial is completed",
            Self::LibrariesHubsSupplemental => "When Libraries supplemental signup is completed",
            Self::CommunityJoin => "When a user joins a community",
            Self::CommunityLeave => "When a user leaves a community",
            Self::CertificateEarned => "When a user earns a certificate",
            Self::EmailValidation => "When a user needs to verify their email address",
            Self::ProjectPublished => "When a project is published",
            Self::ProjectCreatedStaff => {
                "When a new project is submitted for review (staff notification)"
            }
            Self::ForumNewPost => "When a new forum post is created",
            Self::ForumPostReply => "When someone replies to a forum post",
            Self::ForumPostEdited => "When a forum post is edited",
            Self::FirstContribution => "When a user makes their first contribution to a project",
            Self::PeopleFinderConfirmation => "When a people finder request is submitted",
            Self::PeopleFinderNewRequest => "When a new people finder request needs approval",
            Self::ActsOfScienceApplication => {
                "When an Acts of Science Connected application is submitted"
            }
            Self::ActsOfScienceConfirmation => "When Acts of Science participation is confirmed",
            Self::StaffNotification => "Generic notification sent to staff",
        }
    }

    /// Get the category for this trigger (e.g., "user", "project")
    pub fn category(&self) -> &'static str {
        match self {
            Self::UserWelcome | Self::EmailValidation | Self::FirstContribution => "user",
            Self::ProjectSubmitted
            | Self::ProjectApproved
            | Self::ProjectPublished
            | Self::ProjectCreatedStaff => "project",
            Self::BadgeFoundations
            | Self::BadgeLibrariesHubs
            | Self::BadgeDataLiteracy
            | Self::BadgeHigherEd
            | Self::BadgeDataEthics
            | Self::BadgeIdeal
            | Self::BadgeAmbassador => "badge",
            Self::FoundationsTutorial
            | Self::FoundationsSquirrel
            | Self::FoundationsStall
            | Self::FoundationsDebris
            | Self::LibrariesHubsTutorial
            | Self::LibrariesHubsSupplemental => "workflow",
            Self::CommunityJoin | Self::CommunityLeave => "community",
            Self::CertificateEarned => "certificate",
            Self::ForumNewPost | Self::ForumPostReply | Self::ForumPostEdited => "forum",
            Self::PeopleFinderConfirmation | Self::PeopleFinderNewRequest => "messaging",
            Self::ActsOfScienceApplication | Self::ActsOfScienceConfirmation => "acts",
            Self::StaffNotification => "staff",
        }
    }
}

/// Fire an email trigger event with a user and optional additional data
///
/// This is a fire-and-forget operation that doesn't wait for email processing.
/// The user_id is treated as a first-class parameter and will be available as a merge field.
///
/// # Arguments
/// * `trigger` - The trigger event to fire
/// * `user_id` - The user this event is for (will receive the email)
/// * `additional_data` - Optional extra merge fields for the email template
///
/// # Example
/// ```no_run
/// use ulid::Ulid;
/// use common::service::{trigger_email, TriggerEvent};
///
/// # async fn example(user_id: Ulid) -> Result<(), anyhow::Error> {
/// trigger_email(
///     TriggerEvent::UserWelcome,
///     user_id,
///     Some(&[
///         ("username".to_string(), "alice".to_string()),
///     ])
/// ).await?;
/// # Ok(())
/// # }
/// ```
pub async fn trigger_email(
    trigger: TriggerEvent,
    user_id: ulid::Ulid,
    additional_data: Option<&[(String, String)]>,
) -> Result<(), anyhow::Error> {
    send(
        "emailer.trigger",
        crate::message::email::TriggerEmailRequest {
            trigger_name: trigger.as_ref().to_string(),
            user_id,
            additional_data: additional_data.map(|slice| slice.to_vec()),
        },
    )
    .await
}
