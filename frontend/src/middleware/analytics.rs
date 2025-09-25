use std::{net::IpAddr, sync::Arc};

use common::{message::user::UserInfo, service::log_pageview};
use jiff::{ToSpan, Zoned};
use poem::{Endpoint, FromRequest, Middleware, session::Session, web::RealIp};
use ulid::Ulid;

use super::authorization::Authenticated;

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
            .expect("AnalyticsMiddleware must be installed to use Interaction"))
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

#[derive(Debug, Clone)]
pub struct Device(Arc<String>);

impl<'a> FromRequest<'a> for Device {
    async fn from_request(
        req: &'a poem::Request,
        _body: &mut poem::RequestBody,
    ) -> poem::Result<Self> {
        Ok(req
            .extensions()
            .get::<Device>()
            .cloned()
            .expect("AnalyticsMiddleware must be installed to use Device"))
    }
}

impl AsRef<str> for Device {
    fn as_ref(&self) -> &str {
        self.0.as_str()
    }
}

impl From<Device> for String {
    fn from(value: Device) -> Self {
        value.0.to_string()
    }
}

/// Records views and interaction sequences
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
            tracing::error!("AnalyticsMiddleware: Session not found in request");
            return self.wrapped.call(req).await;
        };

        let device = session_device(session);
        let interaction = session_interaction(session);

        let uri = req.uri();
        let path = uri.path();
        let query = uri.query().map(|q| q.to_string());
        let user_info = req
            .extensions()
            .get::<Authenticated>()
            .cloned()
            .unwrap_or_default();

        let ip_address = RealIp::from_request_without_body(&req)
            .await
            .map(|RealIp(ip)| ip)
            .unwrap_or_default()
            .unwrap_or_else(|| IpAddr::from([0, 0, 0, 0]))
            .to_string();

        // Extract referrer header
        let referrer = req
            .headers()
            .get("referer")
            .and_then(|h| h.to_str().ok())
            .map(|s| s.to_string());

        // Log the page view
        if let Err(e) = log_pageview(
            None, // when - let it use current time
            ip_address,
            &device,
            &interaction,
            user_info.user_id(),
            path,
            referrer,
            query,
            None, // geolocation - let it resolve from IP
        )
        .await
        {
            tracing::warn!("Failed to log page view: {}", e);
        }

        let extensions = req.extensions_mut();
        extensions.insert(Device(Arc::new(device)));
        extensions.insert(Interaction(Arc::new(interaction)));

        self.wrapped.call(req).await
    }
}

/// Ensures that the session contains a unique interaction id, and
/// returns it
#[tracing::instrument]
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

/// Ensures that the session contains a unique device id, and
/// returns it
#[tracing::instrument]
fn session_device(session: &Session) -> String {
    if let Some(device) = session.get::<String>("device") {
        device
    } else {
        let device = Ulid::new().to_string();
        session.set("device", &device);
        device
    }
}
