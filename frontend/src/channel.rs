use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use common::shared::nats;
use futures::StreamExt;
use poem::{
    Route,
    error::InternalServerError,
    handler,
    http::StatusCode,
    web::{
        Path,
        sse::{self, SSE},
    },
};

use crate::middleware::authorization::{Authenticated, SessionSubject};

// Re-export common channel functions
pub use common::channel::{send, send_to_user, Event};

pub fn routes() -> Route {
    Route::new()
        .at("/:channel", listen)
        .at("/me/:channel", listen_by_user)
}

#[handler]
#[tracing::instrument]
pub async fn listen(subject: SessionSubject, Path(channel): Path<String>) -> poem::Result<SSE> {
    let subject = format!(
        "channel.session.{}.{}",
        subject.as_ref(),
        URL_SAFE_NO_PAD.encode(channel),
    );

    let subscription = nats()
        .subscribe(subject)
        .await
        .map_err(InternalServerError)?;

    Ok(SSE::new(
        subscription.map(|msg| match flexbuffers::from_slice::<Event>(&msg.payload) {
            Ok(evt) => event_to_sse(evt),
            Err(err) => sse::Event::message(err.to_string()).event_type("error"),
        }),
    )
    .keep_alive(std::time::Duration::from_secs(300)))
}

#[handler]
#[tracing::instrument]
pub async fn listen_by_user(
    Authenticated(basic_info): Authenticated,
    Path(channel): Path<String>,
) -> poem::Result<SSE> {
    let Some(basic_info) = basic_info else {
        return Err(poem::Error::from_status(StatusCode::NOT_FOUND));
    };

    let subject = format!(
        "channel.user.{}.{}",
        basic_info.user_id,
        URL_SAFE_NO_PAD.encode(channel),
    );

    let subscription = nats()
        .subscribe(subject)
        .await
        .map_err(InternalServerError)?;

    Ok(SSE::new(
        subscription.map(|msg| match flexbuffers::from_slice::<Event>(&msg.payload) {
            Ok(evt) => event_to_sse(evt),
            Err(err) => sse::Event::message(err.to_string()).event_type("error"),
        }),
    )
    .keep_alive(std::time::Duration::from_secs(300)))
}

fn event_to_sse(evt: Event) -> sse::Event {
    sse::Event::message(evt.data).event_type(evt.event)
}
