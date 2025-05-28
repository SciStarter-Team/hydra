use std::sync::Arc;

use common::message::user::BasicInfo;
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

pub struct AuthMiddleware;

pub struct AuthMiddlewareEndpoint<E> {
    wrapped: E,
}

impl<E: Endpoint> Middleware<E> for AuthMiddleware {
    type Output = AuthMiddlewareEndpoint<E>;

    fn transform(&self, ep: E) -> Self::Output {
        AuthMiddlewareEndpoint { wrapped: ep }
    }
}

impl<E: Endpoint> Endpoint for AuthMiddlewareEndpoint<E> {
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
                    common::service::request("users.basic_info.by_username", username).await?;

                if let Some(info) = &basic_info {
                    session.remove("auth_username");
                    session.set("basic_info", info);
                }
            }
        }

        let extensions = req.extensions_mut();
        extensions.insert(SessionSubject(Arc::new(subject)));
        extensions.insert(basic_info);

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
fn session_subject(session: &Session) -> String {
    if let Some(subj) = session.get::<String>("subject") {
        subj
    } else {
        let subj = Ulid::new().to_string();
        session.set("subject", &subj);
        subj
    }
}
