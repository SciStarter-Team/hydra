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
            let headers = resp.headers_mut();

            // Collect existing Set-Cookie values that need modification
            let mut modified_cookies = Vec::new();

            for (_name, value) in headers.iter() {
                if let Ok(cookie_str) = value.to_str() {
                    // Only modify session cookies, not CSRF or other cookies
                    if cookie_str.starts_with("session-id=")
                        || cookie_str.starts_with("__Host-session-id=")
                    {
                        let mut modified = cookie_str.to_string();

                        // Replace SameSite=Lax with SameSite=None
                        modified = modified.replace("SameSite=Lax", "SameSite=None");

                        // Add Partitioned attribute if not present
                        if !modified.contains("Partitioned") {
                            modified.push_str("; Partitioned");
                        }

                        modified_cookies.push(modified);
                    }
                }
            }

            // Add partitioned versions alongside the originals
            // Browsers will store both variants of the same cookie name:
            // - session-id: SameSite=Lax for normal (non-iframe) contexts
            // - session-id: SameSite=None; Partitioned for iframe contexts
            for cookie in modified_cookies {
                if let Ok(value) = poem::http::HeaderValue::from_str(&cookie) {
                    headers.append(poem::http::header::SET_COOKIE, value);
                }
            }
        }

        Ok(resp)
    }
}
