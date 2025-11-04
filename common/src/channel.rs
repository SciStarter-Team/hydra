//! Channel-based SSE (Server-Sent Events) messaging
//!
//! Provides a NATS-backed publish/subscribe system for real-time updates to web clients.
//! Supports both session-based and user-based channels.

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use ulid::Ulid;

/// Event for SSE channels
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct Event {
    pub event: String,
    pub data: String,
}

impl Event {
    pub fn new(event: impl Into<String>, data: impl Into<String>) -> Self {
        Event {
            event: event.into(),
            data: data.into(),
        }
    }
}

/// Send an event to a session-based channel
#[tracing::instrument(skip(to_session_subject, to_channel))]
pub async fn send(
    to_session_subject: impl AsRef<str>,
    to_channel: impl AsRef<str>,
    event: Event,
) -> Result<(), anyhow::Error> {
    let subject = format!(
        "channel.session.{}.{}",
        to_session_subject.as_ref(),
        URL_SAFE_NO_PAD.encode(to_channel.as_ref()),
    );

    let body = flexbuffers::to_vec(event)?;

    crate::shared::nats().publish(subject, body.into()).await?;

    Ok(())
}

/// Send an event to a user-based channel
#[tracing::instrument(skip(to_channel))]
pub async fn send_to_user(
    to_user: Ulid,
    to_channel: impl AsRef<str>,
    event: Event,
) -> Result<(), anyhow::Error> {
    let subject = format!(
        "channel.user.{}.{}",
        to_user,
        URL_SAFE_NO_PAD.encode(to_channel.as_ref()),
    );

    let body = flexbuffers::to_vec(event)?;

    crate::shared::nats().publish(subject, body.into()).await?;

    Ok(())
}
