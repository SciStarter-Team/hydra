use anyhow::anyhow;
use async_nats::{HeaderMap, Message, subject::ToSubject};
use errors::DecodeError;
use futures::StreamExt;
use serde::de::DeserializeOwned;

use crate::shared::nats;

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
