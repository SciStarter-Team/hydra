use std::sync::Arc;

use common::message::user::{BasicInfo, UserInfo};
use jiff::{ToSpan, Zoned};
use poem::{Endpoint, FromRequest, Middleware, session::Session};
use ulid::Ulid;

#[derive(Debug, Clone)]
pub struct Interaction(Arc<String>);

impl<'a> FromRequest<'a> for Interaction {
    async fn from_request(
        req: &'a poem::Request,
        _body: &mut poem::RequestBody,
    ) -> poem::Result<Self> {
        Ok(req
            .extensions()
            .get::<Interaction>()
            .cloned()
            .expect("AuthMiddleware must be installed to use Interaction"))
    }
}

impl AsRef<str> for Interaction {
    fn as_ref(&self) -> &str {
        self.0.as_str()
    }
}

impl From<Interaction> for String {
    fn from(value: Interaction) -> Self {
        value.0.to_string()
    }
}

/// Records views and interactions sequences
pub struct AnalyticsMiddleware;

pub struct AnalyticsMiddlewareEndpoint<E> {
    wrapped: E,
}

impl<E: Endpoint> Middleware<E> for AnalyticsMiddleware {
    type Output = AnalyticsMiddlewareEndpoint<E>;

    fn transform(&self, ep: E) -> Self::Output {
        AnalyticsMiddlewareEndpoint { wrapped: ep }
    }
}

impl<E: Endpoint> Endpoint for AnalyticsMiddlewareEndpoint<E> {
    type Output = E::Output;

    async fn call(&self, mut req: poem::Request) -> poem::Result<Self::Output> {
        let Some(session) = req.extensions().get::<Session>() else {
            eprintln!("AnalyticsMiddleware: Session not found in request");
            return self.wrapped.call(req).await;
        };

        let interaction = session_interaction(session);

        if req.method() == poem::http::Method::GET {
            let uri = req.uri();
            let path = uri.path();
            let query = uri.query();
            let user_info = req.extensions().get::<BasicInfo>();

            common::service::send("log.view", (path, query, &interaction, user_info.user_id()))
                .await?;
        }

        let extensions = req.extensions_mut();
        extensions.insert(Interaction(Arc::new(interaction)));

        self.wrapped.call(req).await
    }
}

/// Ensures that the session contains a unique interaction id, and
/// returns it
fn session_interaction(session: &Session) -> String {
    fn new(session: &Session) -> String {
        let interaction = Ulid::new().to_string();
        let expires = (Zoned::now().timestamp() + 1.hour()).to_string();
        session.set("interaction", (&interaction, &expires));
        interaction
    }

    if let Some((interaction, expires)) = session.get::<(String, String)>("interaction") {
        let Ok(expires) = expires.parse() else {
            return new(session);
        };

        let now = Zoned::now().timestamp();

        if now > expires {
            return new(session);
        }

        session.set("interaction", (&interaction, (now + 1.hour()).to_string()));
        interaction
    } else {
        new(session)
    }
}
