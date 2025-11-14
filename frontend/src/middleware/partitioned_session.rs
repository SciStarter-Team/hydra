/// Middleware to set partitioned session cookies for iframe embedding
///
/// Modern browsers block third-party cookies in iframes by default. To allow
/// authenticated sessions in embedded contexts (e.g., exchanges in iframes),
/// we need to use:
/// - SameSite=None (allow cross-site usage)
/// - Secure (required with SameSite=None)
/// - Partitioned (CHIPS - Cookie Having Independent Partitioned State)
///
/// This middleware modifies Set-Cookie headers after the session middleware runs,
/// adding partitioned versions alongside the standard cookies.
///
/// Apply this to any routes that need to work when embedded in iframes.

use poem::{Endpoint, IntoResponse, Middleware, Response};

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

    async fn call(&self, req: poem::Request) -> poem::Result<Self::Output> {
        let output = self.wrapped.call(req).await?;
        let mut resp = output.into_response();

        // Modify Set-Cookie headers for session cookies
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
        // Browsers will store both:
        // - Original SameSite=Lax for normal (non-iframe) contexts
        // - Partitioned version for cross-site iframe contexts
        for cookie in modified_cookies {
            if let Ok(value) = poem::http::HeaderValue::from_str(&cookie) {
                headers.append(poem::http::header::SET_COOKIE, value);
            }
        }

        Ok(resp)
    }
}
