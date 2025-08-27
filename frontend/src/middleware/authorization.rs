use std::sync::Arc;

use common::message::user::{BasicInfo, UserInfo};
use poem::{Endpoint, FromRequest, Middleware, error::ResponseError, session::Session};
use ulid::Ulid;

#[derive(Debug, Clone)]
pub struct SessionSubject(Arc<String>);

impl<'a> FromRequest<'a> for SessionSubject {
    async fn from_request(
        req: &'a poem::Request,
        _body: &mut poem::RequestBody,
    ) -> poem::Result<Self> {
        Ok(req
            .extensions()
            .get::<SessionSubject>()
            .cloned()
            .expect("AuthMiddleware must be installed to use SessionSubject"))
    }
}

impl AsRef<str> for SessionSubject {
    fn as_ref(&self) -> &str {
        self.0.as_str()
    }
}

impl From<SessionSubject> for String {
    fn from(value: SessionSubject) -> Self {
        value.0.to_string()
    }
}

/// Extractor for the current authenticated user
#[derive(Clone)]
pub struct Authenticated(pub Option<Arc<BasicInfo>>);

impl Default for Authenticated {
    fn default() -> Self {
        Authenticated(None)
    }
}

impl UserInfo for Authenticated {
    fn user_id(&self) -> Option<Ulid> {
        self.0.as_ref().and_then(|i| i.user_id())
    }

    fn handle(&self) -> &str {
        self.0.as_ref().map(|i| i.handle()).unwrap_or_default()
    }

    fn is_staff(&self) -> bool {
        self.0.as_ref().map(|i| i.is_staff()).unwrap_or(false)
    }

    fn is_superuser(&self) -> bool {
        self.0.as_ref().map(|i| i.is_superuser()).unwrap_or(false)
    }
}

impl<'a> FromRequest<'a> for Authenticated {
    async fn from_request(
        req: &'a poem::Request,
        _body: &mut poem::RequestBody,
    ) -> poem::Result<Self> {
        Ok(req
            .extensions()
            .get::<Authenticated>()
            .cloned()
            .expect("AuthMiddleware must be installed to use SessionSubject"))
    }
}

pub struct AuthMiddleware;

pub struct AuthEndpoint<E> {
    wrapped: E,
}

impl<E: Endpoint> Middleware<E> for AuthMiddleware {
    type Output = AuthEndpoint<E>;

    fn transform(&self, ep: E) -> Self::Output {
        AuthEndpoint { wrapped: ep }
    }
}

impl<E: Endpoint> Endpoint for AuthEndpoint<E> {
    type Output = E::Output;

    async fn call(&self, mut req: poem::Request) -> poem::Result<Self::Output> {
        let Some(session) = req.extensions().get::<Session>() else {
            eprintln!("AuthMiddleware: Session not found in request");
            return self.wrapped.call(req).await;
        };

        let subject = session_subject(session);
        let mut basic_info = session.get::<BasicInfo>("basic_info");

        if basic_info.is_none() {
            if let Some(username) = session.get::<String>("auth_username") {
                basic_info =
                    common::service::request("user.basic_info.by_username", username).await?;

                if let Some(info) = &basic_info {
                    session.remove("auth_username");
                    session.set("basic_info", info);
                }
            }
        }

        let extensions = req.extensions_mut();
        extensions.insert(SessionSubject(Arc::new(subject)));
        extensions.insert(Authenticated(basic_info.map(|i| Arc::new(i))));

        self.wrapped.call(req).await
    }
}

/// Return an instance as an error when the current request is for a
/// resource that the current user does not have permission for.
#[derive(Debug)]
pub struct NotAuthorized {
    target: String,
}

impl NotAuthorized {
    fn new(target: impl AsRef<str>) -> NotAuthorized {
        NotAuthorized {
            target: urlencoding::encode(target.as_ref()).to_string(),
        }
    }

    pub fn with_path(path: impl AsRef<str>) -> NotAuthorized {
        NotAuthorized::new(path)
    }

    pub fn with_path_and_query(path: impl AsRef<str>, query: impl AsRef<str>) -> NotAuthorized {
        NotAuthorized::new(format!("{}?{}", path.as_ref(), query.as_ref()))
    }

    pub fn target(&self) -> &str {
        self.target.as_str()
    }
}

impl std::fmt::Display for NotAuthorized {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Not authorized to access {}", &self.target)
    }
}

impl std::error::Error for NotAuthorized {}

impl ResponseError for NotAuthorized {
    fn status(&self) -> reqwest::StatusCode {
        poem::http::StatusCode::UNAUTHORIZED
    }
}

/// Ensures that the session contains a unique NATS subject, and
/// returns it
#[tracing::instrument]
fn session_subject(session: &Session) -> String {
    if let Some(subj) = session.get::<String>("subject") {
        subj
    } else {
        let subj = Ulid::new().to_string();
        session.set("subject", &subj);
        subj
    }
}

/// Ensures that the current user is logged in to a staff account, or
/// redirects them to the login page.
pub struct StaffOnly;

pub struct StaffOnlyEndpoint<E> {
    wrapped: E,
}

impl<E: Endpoint> Middleware<E> for StaffOnly {
    type Output = StaffOnlyEndpoint<E>;

    fn transform(&self, ep: E) -> Self::Output {
        StaffOnlyEndpoint { wrapped: ep }
    }
}

impl<E: Endpoint> Endpoint for StaffOnlyEndpoint<E> {
    type Output = E::Output;

    async fn call(&self, req: poem::Request) -> poem::Result<Self::Output> {
        staff_only(&req)?;
        self.wrapped.call(req).await
    }
}

/// Use staff_only()? as a quick way to enfore staff-only access
/// restrictions in handlers.
///
/// StaffOnly enforces the same rule, so the normal use case for
/// calling this directly is when the handler logic has both
/// staff-only and non-staff code paths.
#[tracing::instrument]
pub fn staff_only(req: &poem::Request) -> Result<(), NotAuthorized> {
    let Some(user_info) = req.extensions().get::<Authenticated>() else {
        let uri = req.uri();
        if let Some(query) = uri.query() {
            return Err(NotAuthorized::with_path_and_query(uri.path(), query));
        } else {
            return Err(NotAuthorized::with_path(uri.path()));
        }
    };

    if !user_info.is_staff() {
        let uri = req.uri();
        if let Some(query) = uri.query() {
            return Err(NotAuthorized::with_path_and_query(uri.path(), query));
        } else {
            return Err(NotAuthorized::with_path(uri.path()));
        }
    }

    Ok(())
}

/// Ensures that the current user is logged in to a staff account, or
/// redirects them to the login page.
pub struct AuthOnly;

pub struct AuthOnlyEndpoint<E> {
    wrapped: E,
}

impl<E: Endpoint> Middleware<E> for AuthOnly {
    type Output = AuthOnlyEndpoint<E>;

    fn transform(&self, ep: E) -> Self::Output {
        AuthOnlyEndpoint { wrapped: ep }
    }
}

impl<E: Endpoint> Endpoint for AuthOnlyEndpoint<E> {
    type Output = E::Output;

    async fn call(&self, req: poem::Request) -> poem::Result<Self::Output> {
        auth_only(&req)?;
        self.wrapped.call(req).await
    }
}

/// Use auth_only()? as a quick way to enfore logged-in-only access
/// restrictions in handlers.
#[tracing::instrument]
pub fn auth_only(req: &poem::Request) -> Result<(), NotAuthorized> {
    let Some(_user_info) = req.extensions().get::<Authenticated>() else {
        let uri = req.uri();
        if let Some(query) = uri.query() {
            return Err(NotAuthorized::with_path_and_query(uri.path(), query));
        } else {
            return Err(NotAuthorized::with_path(uri.path()));
        }
    };

    Ok(())
}
