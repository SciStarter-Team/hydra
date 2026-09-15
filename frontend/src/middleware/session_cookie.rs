/// The session cookie: its attributes, and the middleware that finishes the
/// `Set-Cookie` headers poem's `ServerSession` writes for it.
///
/// Two things need finishing:
///
/// 1. **Removal.** When a session is purged, poem removes the cookie with a
///    bare `name=; Max-Age=0` that carries none of the cookie's attributes. A
///    browser refuses a `__Host-` cookie without `Secure` and `Path=/`, so in
///    production that removal was silently ignored and the browser kept
///    presenting the purged session ID. [SessionCookieMiddleware] gives every
///    removal the same attributes the cookie was set with.
///
/// 2. **Partitioning.** Modern browsers block third-party cookies in iframes by
///    default. To allow authenticated sessions in embedded contexts (e.g.,
///    exchanges in iframes), routes marked with [EnablePartitionedCookies] also
///    get a `SameSite=None; Secure; Partitioned` (CHIPS) variant of the session
///    cookie, alongside the standard one. The browser stores both variants and
///    automatically sends the appropriate one based on context (top-level vs
///    iframe).
use poem::{
    Endpoint, IntoResponse, Middleware, Request, Response,
    http::{HeaderValue, header::SET_COOKIE},
    session::{CookieConfig, ServerSession},
    web::cookie::{Cookie, SameSite},
};

use crate::session::UserServiceSessionStorage;

#[cfg(debug_assertions)]
pub const SESSION_COOKIE: &'static str = "session-id";

#[cfg(not(debug_assertions))]
pub const SESSION_COOKIE: &'static str = "__Host-session-id";

const SESSION_COOKIE_PATH: &str = "/";
const SESSION_COOKIE_SAME_SITE: SameSite = SameSite::Lax;
const SESSION_COOKIE_MAX_AGE: std::time::Duration =
    std::time::Duration::from_secs(60 * 60 * 24 * 365);

/// The shared server-side session middleware configuration. Every route tree
/// that needs the session cookie uses it, so that they all agree on the
/// cookie's settings.
pub fn server_session() -> ServerSession<UserServiceSessionStorage> {
    ServerSession::new(
        CookieConfig::default()
            .name(SESSION_COOKIE)
            .http_only(true)
            .secure(true)
            .same_site(SESSION_COOKIE_SAME_SITE)
            .path(SESSION_COOKIE_PATH)
            .max_age(SESSION_COOKIE_MAX_AGE),
        UserServiceSessionStorage,
    )
}

/// Gives a removal of the session cookie the attributes [server_session] sets
/// the cookie with. A cookie is only replaced by one that matches it, and a
/// `__Host-` cookie is only accepted at all with `Secure` and `Path=/`.
fn complete_removal(cookie: &mut Cookie) {
    cookie.set_http_only(true);
    cookie.set_secure(true);
    cookie.set_same_site(SESSION_COOKIE_SAME_SITE);
    cookie.set_path(SESSION_COOKIE_PATH);
}

/// Marker to indicate a response needs partitioned cookies
#[derive(Clone)]
struct NeedsPartitionedCookies;

/// Inner middleware: Mark responses that need partitioned cookies
/// Apply this to routes that should work in iframes (e.g., exchange routes)
pub struct EnablePartitionedCookies;

pub struct EnablePartitionedCookiesEndpoint<E> {
    wrapped: E,
}

impl<E: Endpoint> Middleware<E> for EnablePartitionedCookies {
    type Output = EnablePartitionedCookiesEndpoint<E>;

    fn transform(&self, ep: E) -> Self::Output {
        EnablePartitionedCookiesEndpoint { wrapped: ep }
    }
}

impl<E: Endpoint> Endpoint for EnablePartitionedCookiesEndpoint<E> {
    type Output = Response;

    async fn call(&self, req: Request) -> poem::Result<Self::Output> {
        let output = self.wrapped.call(req).await?;
        let mut resp = output.into_response();

        // Mark this response as needing partitioned cookies
        resp.extensions_mut().insert(NeedsPartitionedCookies);

        Ok(resp)
    }
}

/// Outer middleware: completes session cookie removals, and adds partitioned
/// cookies when the marker is set. Apply this outside [server_session], whose
/// `Set-Cookie` headers it rewrites.
pub struct SessionCookieMiddleware;

pub struct SessionCookieEndpoint<E> {
    wrapped: E,
}

impl<E: Endpoint> Middleware<E> for SessionCookieMiddleware {
    type Output = SessionCookieEndpoint<E>;

    fn transform(&self, ep: E) -> Self::Output {
        SessionCookieEndpoint { wrapped: ep }
    }
}

impl<E: Endpoint> Endpoint for SessionCookieEndpoint<E> {
    type Output = Response;

    async fn call(&self, req: Request) -> poem::Result<Self::Output> {
        let output = self.wrapped.call(req).await?;
        let mut resp = output.into_response();

        let needs_partitioned = resp.extensions().get::<NeedsPartitionedCookies>().is_some();
        let headers = resp.headers_mut();

        // Pull out every Set-Cookie header so we can re-emit them in a
        // controlled order.
        let all: Vec<_> = headers.get_all(SET_COOKIE).iter().cloned().collect();
        headers.remove(SET_COOKIE);

        let mut others = Vec::new();
        let mut session_lax = Vec::new();
        let mut session_partitioned = Vec::new();

        for value in all {
            let session_cookie = value
                .to_str()
                .ok()
                .and_then(|s| Cookie::parse(s).ok())
                .filter(|cookie| cookie.name() == SESSION_COOKIE);

            let Some(mut cookie) = session_cookie else {
                others.push(value);
                continue;
            };

            let is_removal = cookie.max_age().is_some_and(|age| age.is_zero());

            if is_removal {
                complete_removal(&mut cookie);
            }

            let lax = if is_removal {
                HeaderValue::from_str(&cookie.to_string()).ok()
            } else {
                Some(value)
            };

            if needs_partitioned {
                // Build the SameSite=None; Partitioned variant for the
                // iframe (CHIPS) context.
                cookie.set_same_site(SameSite::None);
                cookie.set_secure(true);
                cookie.set_partitioned(true);

                if let Ok(v) = HeaderValue::from_str(&cookie.to_string()) {
                    session_partitioned.push(v);
                }
            }

            session_lax.extend(lax);
        }

        // Re-emit: unrelated cookies first, then the SameSite=None;
        // Partitioned session cookie, and finally the SameSite=Lax session
        // cookie LAST.
        //
        // On browsers that support CHIPS, the Partitioned cookie is stored
        // under a separate partition key and coexists with the unpartitioned
        // Lax cookie, so order is irrelevant. On browsers that ignore
        // `Partitioned`, two cookies with the same name collapse to
        // last-write-wins; emitting the Lax variant last keeps the session
        // cookie SameSite=Lax globally instead of silently downgrading it to
        // SameSite=None (which would strip CSRF protection app-wide). Those
        // browsers block third-party cookies in iframes anyway, so losing the
        // None variant there costs nothing.
        for v in others {
            headers.append(SET_COOKIE, v);
        }
        for v in session_partitioned {
            headers.append(SET_COOKIE, v);
        }
        for v in session_lax {
            headers.append(SET_COOKIE, v);
        }

        Ok(resp)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use poem::{
        EndpointExt, handler, middleware::CookieJarManager, test::TestClient,
        web::cookie::CookieJar,
    };

    /// What `ServerSession` does with the cookie of a purged session.
    #[handler]
    fn remove(cookie_jar: &CookieJar) {
        cookie_jar.remove(SESSION_COOKIE);
    }

    async fn session_set_cookies(app: impl Endpoint) -> Vec<Cookie> {
        let resp = TestClient::new(app)
            .get("/")
            .header("cookie", format!("{SESSION_COOKIE}=purged"))
            .send()
            .await;

        resp.0
            .headers()
            .get_all(SET_COOKIE)
            .iter()
            .map(|v| Cookie::parse(v.to_str().unwrap()).unwrap())
            .filter(|cookie| cookie.name() == SESSION_COOKIE)
            .collect()
    }

    fn assert_complete_removal(cookie: &Cookie) {
        assert_eq!(cookie.value_str(), "");
        assert!(cookie.max_age().is_some_and(|age| age.is_zero()));
        assert!(cookie.secure());
        assert!(cookie.http_only());
        assert_eq!(cookie.path(), Some(SESSION_COOKIE_PATH));
    }

    #[tokio::test]
    async fn a_removal_carries_the_attributes_the_cookie_was_set_with() {
        let app = poem::get(remove)
            .with(CookieJarManager::new())
            .with(SessionCookieMiddleware);

        let cookies = session_set_cookies(app).await;

        assert_eq!(cookies.len(), 1);
        assert_complete_removal(&cookies[0]);
        assert_eq!(cookies[0].same_site(), Some(SESSION_COOKIE_SAME_SITE));
        assert!(!cookies[0].partitioned());
    }

    #[tokio::test]
    async fn a_partitioned_route_removes_both_variants() {
        let app = poem::get(remove)
            .with(EnablePartitionedCookies)
            .with(CookieJarManager::new())
            .with(SessionCookieMiddleware);

        let cookies = session_set_cookies(app).await;

        assert_eq!(cookies.len(), 2);
        cookies.iter().for_each(assert_complete_removal);

        let (partitioned, lax): (Vec<_>, Vec<_>) =
            cookies.iter().partition(|cookie| cookie.partitioned());
        assert_eq!(partitioned.len(), 1);
        assert_eq!(partitioned[0].same_site(), Some(SameSite::None));
        assert_eq!(lax.len(), 1);
        assert_eq!(lax[0].same_site(), Some(SESSION_COOKIE_SAME_SITE));

        // Last, so that a browser ignoring `Partitioned` keeps the Lax cookie.
        assert!(!cookies[1].partitioned());
    }
}
