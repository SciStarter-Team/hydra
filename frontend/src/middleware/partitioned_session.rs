/// Middleware system for partitioned session cookies in iframe embedding
///
/// Modern browsers block third-party cookies in iframes by default. To allow
/// authenticated sessions in embedded contexts (e.g., exchanges in iframes),
/// we need to use:
/// - SameSite=None (allow cross-site usage)
/// - Secure (required with SameSite=None)
/// - Partitioned (CHIPS - Cookie Having Independent Partitioned State)
///
/// This is split into two middlewares:
///
/// 1. **EnablePartitionedCookies** (inner): Applied to routes that need iframe support.
///    Sets a marker on the response to indicate partitioned cookies should be added.
///
/// 2. **PartitionedSessionMiddleware** (outer): Wraps ServerSession at the top level.
///    Checks the response for the marker and if present, modifies Set-Cookie headers
///    to add partitioned versions alongside the standard cookies.
///
/// The browser stores both variants and automatically sends the appropriate one
/// based on context (top-level vs iframe).
use poem::{Endpoint, IntoResponse, Middleware, Request, Response};

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

/// Outer middleware: Add partitioned cookies when marker is set
/// Apply this at the top level, wrapping ServerSession
pub struct PartitionedSessionMiddleware;

pub struct PartitionedSessionEndpoint<E> {
    wrapped: E,
}

impl<E: Endpoint> Middleware<E> for PartitionedSessionMiddleware {
    type Output = PartitionedSessionEndpoint<E>;

    fn transform(&self, ep: E) -> Self::Output {
        PartitionedSessionEndpoint { wrapped: ep }
    }
}

impl<E: Endpoint> Endpoint for PartitionedSessionEndpoint<E> {
    type Output = Response;

    async fn call(&self, req: Request) -> poem::Result<Self::Output> {
        let output = self.wrapped.call(req).await?;
        let mut resp = output.into_response();

        // Check if the response was marked as needing partitioned cookies
        let needs_partitioned = resp.extensions().get::<NeedsPartitionedCookies>().is_some();

        if needs_partitioned {
            use poem::http::header::SET_COOKIE;
            let headers = resp.headers_mut();

            // Pull out every Set-Cookie header so we can re-emit them in a
            // controlled order.
            let all: Vec<_> = headers.get_all(SET_COOKIE).iter().cloned().collect();
            headers.remove(SET_COOKIE);

            let mut others = Vec::new();
            let mut session_lax = Vec::new();
            let mut session_partitioned = Vec::new();

            for value in all {
                let is_session = value
                    .to_str()
                    .map(|s| s.starts_with("session-id=") || s.starts_with("__Host-session-id="))
                    .unwrap_or(false);

                if is_session {
                    // Build the SameSite=None; Partitioned variant for the
                    // iframe (CHIPS) context.
                    if let Ok(s) = value.to_str() {
                        let mut modified = s.replace("SameSite=Lax", "SameSite=None");
                        if !modified.contains("Partitioned") {
                            modified.push_str("; Partitioned");
                        }
                        if let Ok(v) = poem::http::HeaderValue::from_str(&modified) {
                            session_partitioned.push(v);
                        }
                    }
                    session_lax.push(value);
                } else {
                    others.push(value);
                }
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
        }

        Ok(resp)
    }
}
