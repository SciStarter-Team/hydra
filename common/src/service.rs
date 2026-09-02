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

/// Why a request failed.
///
/// The distinction that matters to a caller is whether trying again could
/// possibly help. A service that answered and said "no" will say "no" again;
/// a message that never reached anyone might get through next time.
#[derive(Debug)]
pub enum RequestError {
    /// The service answered, and the answer was a failure. Deterministic: a
    /// retry sends the same message to the same handler and gets the same reply.
    Answered(async_nats::service::error::Error),
    /// Nobody answered -- no responders on the subject, a dropped connection, a
    /// reply that never arrived. Worth another attempt.
    Unanswered(Error),
    /// The message could not be encoded, or its reply could not be decoded.
    /// Retrying changes neither.
    Malformed(Error),
}

impl RequestError {
    /// Whether trying the same request again could give a different result.
    pub fn retryable(&self) -> bool {
        matches!(self, RequestError::Unanswered(_))
    }
}

impl std::fmt::Display for RequestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RequestError::Answered(e) => write!(f, "{}", e.status),
            RequestError::Unanswered(e) | RequestError::Malformed(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for RequestError {}

/// Publish a structured data message through NATS, wait for a reply, and decode
/// it into structured data, keeping the reason for any failure.
///
/// [`request`] is this with the reason flattened away; [`request_with_retry`]
/// needs the reason, because it decides whether to try again.
pub async fn request_typed<R>(
    subject: impl ToSubject,
    message: impl serde::Serialize,
) -> Result<R, RequestError>
where
    R: DeserializeOwned,
{
    let payload = flexbuffers::to_vec(message).map_err(|e| RequestError::Malformed(e.into()))?;

    let reply = nats()
        .request(subject, payload.into())
        .await
        .map_err(|e| RequestError::Unanswered(anyhow!("NATS request failed: {e}")))?;

    let msg = reply.decode_error().map_err(RequestError::Answered)?;

    decode_structured_message(msg).map_err(RequestError::Malformed)
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
    Ok(request_typed(subject, message).await?)
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

/// Publish a structured data message through NATS, wait for a reply, and decode
/// it into structured data, retrying while it is worth retrying.
///
/// Only a request that went **unanswered** is tried again -- no responders on
/// the subject, a dropped connection, a reply that never arrived. Those can
/// succeed on a later attempt, so the wait between attempts grows to
/// [`RETRY_MAX_DELAY`] and the whole run gives up at [`RETRY_DEADLINE`].
///
/// A request the service *answered* is not retried. The handler ran and
/// reported failure; sending the same message to the same handler will fail the
/// same way. Retrying it turns a fast, legible error into a caller that hangs
/// until something upstream times out -- which is how a duplicate email address
/// once became a request that never returned.
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
    let started = std::time::Instant::now();

    loop {
        match request_typed(subject_str.clone(), message.clone()).await {
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

            // The service answered. Whatever it said, it will say again.
            Err(err) if !err.retryable() => {
                debug!(subject = %subject_str, error = %err, "Request failed, not retryable");
                return Err(err.into());
            }

            Err(err) => {
                retry_count += 1;
                let elapsed = started.elapsed();

                if elapsed + delay > RETRY_DEADLINE {
                    warn!(
                        subject = %subject_str,
                        retry_count,
                        elapsed_ms = elapsed.as_millis(),
                        error = %err,
                        "Giving up: no answer within the retry deadline"
                    );
                    return Err(anyhow!(
                        "no answer from {subject_str} after {retry_count} attempts over {:?}: {err}",
                        elapsed
                    ));
                }

                // Log the first few attempts loudly, then only occasionally.
                if retry_count <= 3 {
                    warn!(
                        subject = %subject_str,
                        retry_count,
                        delay_ms = delay.as_millis(),
                        error = %err,
                        "Request unanswered, retrying"
                    );
                } else if retry_count % 10 == 0 {
                    warn!(
                        subject = %subject_str,
                        retry_count,
                        delay_ms = delay.as_millis(),
                        "Request still unanswered after {} attempts",
                        retry_count
                    );
                } else {
                    debug!(
                        subject = %subject_str,
                        retry_count,
                        error = %err,
                        "Request unanswered, retrying"
                    );
                }

                tokio::time::sleep(delay).await;
                delay = std::cmp::min(delay * 2, RETRY_MAX_DELAY);
            }
        }
    }
}

/// Longest wait between attempts at an unanswered request.
pub const RETRY_MAX_DELAY: Duration = Duration::from_secs(5);

/// How long [`request_with_retry`] keeps trying before giving up.
///
/// Every caller is serving a request of its own, so retrying past the point
/// where anyone is still waiting only holds a connection open. This is well
/// inside htmx's 60s default timeout, so the browser sees our error rather than
/// an aborted request.
pub const RETRY_DEADLINE: Duration = Duration::from_secs(20);

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
///     None,
/// ).await?;
/// ```
///
/// `reply_to` sets optional Reply-To addresses; pass `None` for the default
/// (replies go to the send-from address).
pub async fn send_email(
    recipients: impl Into<Vec<crate::message::email::EmailRecipient>>,
    subject: impl Into<String>,
    template_html: impl Into<String>,
    data_sets: impl Into<Vec<String>>,
    additional_data: impl Into<Option<Vec<(String, String)>>>,
    reply_to: impl Into<Option<Vec<String>>>,
) -> Result<Ulid, Error> {
    use crate::message::email::{SendEmailRequest, SendEmailResponse};

    let email_request = SendEmailRequest {
        template_html: template_html.into(),
        subject: subject.into(),
        recipients: recipients.into(),
        data_sets: data_sets.into(),
        additional_data: additional_data.into(),
        reply_to: reply_to.into(),
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

    /// Sent when a reviewer declines to publish a project, quoting the
    /// reason they picked
    ProjectRejected,

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

    /// Sent when the Data Ethics badge is earned by finishing the Spanish
    /// build of the tutorial. One badge for all three languages, so this
    /// differs from [`TriggerEvent::BadgeDataEthics`] only in the language it
    /// is written in.
    BadgeDataEthicsEs,

    /// Sent when the Data Ethics badge is earned by finishing the Portuguese
    /// build of the tutorial.
    BadgeDataEthicsPt,

    /// Sent when the Ideal Participatory Science badge is earned
    BadgeIdeal,

    /// Sent when the SciStarter Ambassador badge is earned
    BadgeAmbassador,

    /// Sent when the Citizen Science for K-12 Educators badge is earned
    BadgeK12,

    /// Sent when the Champion (ambassador) training tutorial is completed
    AmbassadorTrainingComplete,

    /// Sent to confirm an ambassador application was submitted
    AmbassadorApplicationSubmitted,

    /// Sent when an ambassador application is rejected
    AmbassadorRejected,

    /// Sent when an ambassador applicant is outside the US (international)
    AmbassadorInternational,

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

    /// Sent when an event is published. Its own event rather than a kind
    /// carried on `ProjectPublished`, because a trigger is chosen by name and
    /// a site may well want to say something different about an event.
    EventPublished,

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
            Self::ProjectRejected => "When a project is rejected by a reviewer",
            Self::BadgeFoundations => "When the Foundations of Citizen Science badge is earned",
            Self::BadgeLibrariesHubs => {
                "When the Libraries as Hubs for Citizen Science badge is earned"
            }
            Self::BadgeDataLiteracy => "When the Building Data Literacy badge is earned",
            Self::BadgeHigherEd => "When the Teaching in Higher Education badge is earned",
            Self::BadgeDataEthics => "When the Data Ethics for Practitioners badge is earned",
            Self::BadgeDataEthicsEs => {
                "When the Data Ethics badge is earned in Spanish (badge email in Spanish)"
            }
            Self::BadgeDataEthicsPt => {
                "When the Data Ethics badge is earned in Portuguese (badge email in Portuguese)"
            }
            Self::BadgeIdeal => "When the Ideal Participatory Science badge is earned",
            Self::BadgeAmbassador => "When the SciStarter Ambassador badge is earned",
            Self::BadgeK12 => "When the Citizen Science for K-12 Educators badge is earned",
            Self::AmbassadorTrainingComplete => "When the Champion training tutorial is completed",
            Self::AmbassadorApplicationSubmitted => "When an ambassador application is submitted",
            Self::AmbassadorRejected => "When an ambassador application is rejected",
            Self::AmbassadorInternational => "When an ambassador applicant is international",
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
            Self::EventPublished => "When an event is published",
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
            | Self::ProjectRejected
            | Self::ProjectPublished
            | Self::EventPublished
            | Self::ProjectCreatedStaff => "project",
            Self::BadgeFoundations
            | Self::BadgeLibrariesHubs
            | Self::BadgeDataLiteracy
            | Self::BadgeDataEthicsEs
            | Self::BadgeDataEthicsPt
            | Self::BadgeHigherEd
            | Self::BadgeDataEthics
            | Self::BadgeIdeal
            | Self::BadgeAmbassador
            | Self::BadgeK12 => "badge",
            Self::AmbassadorTrainingComplete
            | Self::AmbassadorApplicationSubmitted
            | Self::AmbassadorRejected
            | Self::AmbassadorInternational => "ambassador",
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

#[cfg(test)]
mod request_error_tests {
    use super::*;
    use crate::service::errors::{ErrorCode, service_error};

    /// The whole point: a service that answered and said no is not a transport
    /// failure, and must not be retried. A duplicate email address once became
    /// a request that never returned because these were treated alike.
    #[test]
    fn an_answered_failure_is_not_retried() {
        let err = RequestError::Answered(service_error(
            "Email address is already in use",
            ErrorCode::HandlerError,
        ));
        assert!(!err.retryable());
    }

    #[test]
    fn an_unanswered_request_is_retried() {
        assert!(RequestError::Unanswered(anyhow!("no responders")).retryable());
    }

    /// Neither encoding nor decoding gets better on a second attempt.
    #[test]
    fn a_malformed_message_is_not_retried() {
        assert!(!RequestError::Malformed(anyhow!("bad payload")).retryable());
    }

    /// The service's own words are what reach the caller, so the reason for a
    /// refusal survives the trip.
    #[test]
    fn an_answered_failure_carries_the_services_message() {
        let err = RequestError::Answered(service_error(
            "Email address is already in use",
            ErrorCode::HandlerError,
        ));
        assert_eq!(err.to_string(), "Email address is already in use");
        assert_eq!(
            Error::from(err).to_string(),
            "Email address is already in use"
        );
    }

    /// Retrying must not outlast the caller that is waiting on it.
    #[test]
    fn the_deadline_leaves_room_before_a_browser_gives_up() {
        assert!(RETRY_DEADLINE < Duration::from_secs(60));
        assert!(RETRY_MAX_DELAY < RETRY_DEADLINE);
    }
}
