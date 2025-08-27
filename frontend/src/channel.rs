use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use common::shared::nats;
use futures::StreamExt;
use poem::{
    Route,
    error::InternalServerError,
    handler,
    web::{
        Path,
        sse::{self, SSE},
    },
};

use crate::middleware::authorization::SessionSubject;

pub fn routes() -> Route {
    Route::new().at("/:channel", listen)
}

#[handler]
#[tracing::instrument]
pub async fn listen(subject: SessionSubject, Path(channel): Path<String>) -> poem::Result<SSE> {
    let subject = format!("{}.{}", subject.as_ref(), URL_SAFE_NO_PAD.encode(channel),);

    let subscription = nats()
        .subscribe(subject)
        .await
        .map_err(InternalServerError)?;

    Ok(SSE::new(
        subscription.map(|msg| match flexbuffers::from_slice::<Event>(&msg.payload) {
            Ok(evt) => evt.into(),
            Err(err) => sse::Event::message(err.to_string()).event_type("error"),
        }),
    )
    .keep_alive(std::time::Duration::from_secs(300)))
}

#[tracing::instrument(skip(to_session_subject, to_channel))]
pub async fn send(
    to_session_subject: impl AsRef<str>,
    to_channel: impl AsRef<str>,
    event: Event,
) -> Result<(), anyhow::Error> {
    let subject = format!(
        "{}.{}",
        to_session_subject.as_ref(),
        URL_SAFE_NO_PAD.encode(to_channel.as_ref()),
    );

    let body = flexbuffers::to_vec(event)?;

    nats().publish(subject, body.into()).await?;

    Ok(())
}

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

impl Into<sse::Event> for Event {
    fn into(self) -> sse::Event {
        sse::Event::message(self.data).event_type(self.event)
    }
}
