//! Turning a [`poem::Error`] into a response.
//!
//! This lives in a middleware rather than in [`poem::EndpointExt::catch_all_error`]
//! because the shape of the response depends on the request: htmx 4 swaps error
//! responses like any other, so a failed htmx request needs a body that belongs
//! somewhere on the page, while a plain navigation needs a whole page.

use poem::{Endpoint, IntoResponse, Middleware, Response, web::Redirect};

use super::authorization::NotAuthorized;
use crate::component::Notification;

/// Renders every error raised below it, in a shape that suits the request.
pub struct ErrorPages;

pub struct ErrorPagesEndpoint<E> {
    wrapped: E,
}

impl<E: Endpoint> Middleware<E> for ErrorPages {
    type Output = ErrorPagesEndpoint<E>;

    fn transform(&self, ep: E) -> Self::Output {
        ErrorPagesEndpoint { wrapped: ep }
    }
}

impl<E: Endpoint> Endpoint for ErrorPagesEndpoint<E> {
    type Output = Response;

    async fn call(&self, req: poem::Request) -> poem::Result<Self::Output> {
        // htmx 4 sends this on every request it makes.
        let from_htmx = req.headers().contains_key("HX-Request");

        match self.wrapped.call(req).await {
            Ok(output) => Ok(output.into_response()),
            Err(err) => Ok(dispatch(from_htmx, err).await),
        }
    }
}

#[tracing::instrument]
async fn dispatch(from_htmx: bool, err: poem::Error) -> Response {
    use poem::http::StatusCode;

    // A 401 answers with a redirect to the login page either way: htmx follows
    // it, and the login page it lands on replaces the target, which is what a
    // session that expired mid-interaction should produce.
    if err.status() == StatusCode::UNAUTHORIZED {
        return not_authorized(err);
    }

    if err.status() == StatusCode::INTERNAL_SERVER_ERROR {
        tracing::error!(?err);
    }

    if from_htmx {
        let status = err.status();
        let raised = err.to_string();
        // `Error::from_status` carries no message and renders as the status
        // itself ("404 Not Found"), which says nothing the title will not;
        // treat that as "no message" so the default for the status is used.
        let message = if raised == status.to_string() {
            ""
        } else {
            raised.as_str()
        };
        return error_response(status, message);
    }

    match err.status() {
        StatusCode::NOT_FOUND => page(StatusCode::NOT_FOUND, "Not Found"),
        StatusCode::FORBIDDEN => page(StatusCode::FORBIDDEN, "Not Allowed"),
        StatusCode::INTERNAL_SERVER_ERROR => {
            page(StatusCode::INTERNAL_SERVER_ERROR, "Internal Server Error")
        }
        _ => err.into_response(),
    }
}

/// Marks a response as a message to be placed rather than swapped in situ.
///
/// Deliberately not in htmx's `HX-` namespace: that belongs to htmx, which
/// gives its own meanings to headers there. The header is read by
/// static/js/notifications.js, which decides where the message goes: the nearest enclosing container that asked for one, or the
/// page-wide region when nothing did. Handlers do not choose -- they cannot
/// know which part of the page the request came from.
pub const NOTIFICATION_HEADER: &str = "X-Notification";

/// Renders an error as a message for the reader.
///
/// htmx 4 swaps error responses like any other, so a failed htmx request would
/// otherwise drop whatever came back into the element the request targeted --
/// replacing the very content the reader was working on with an apology. The
/// response is marked [`NOTIFICATION_HEADER`] instead, and placed by the
/// client.
///
/// Handlers that answer an htmx request with an error status call this rather
/// than writing a body of their own, so that every such error is delivered the
/// same way and built in one place. [`ErrorPages`] uses it for every error that
/// reaches it.
///
/// `message` is what the reader is told; pass `""` to accept the default for
/// the status. A server error always uses the default whatever is passed, since
/// its detail describes our failure rather than anything the reader can act on
/// and belongs in the log instead.
pub fn error_response(status: poem::http::StatusCode, message: &str) -> Response {
    use poem::http::StatusCode;

    let (title, default) = match status {
        StatusCode::NOT_FOUND => (
            "Not found",
            "That item no longer exists. Reload the page to see the current contents.",
        ),
        StatusCode::FORBIDDEN => ("Not allowed", "You do not have permission to do that."),
        _ if status.is_server_error() => (
            "Something went wrong",
            "The action could not be completed. Please try again.",
        ),
        _ => ("Something went wrong", "The action could not be completed."),
    };

    let message = if message.is_empty() || status.is_server_error() {
        default
    } else {
        message
    };

    notification_response(status, Notification::error(title, message))
}

/// Answers a request with a single notification and nothing else.
pub fn notification_response(
    status: poem::http::StatusCode,
    notification: Notification,
) -> Response {
    use sailfish::TemplateOnce;

    let body = notification.render_once().unwrap_or_else(|err| {
        tracing::error!(?err, "failed to render notification");
        String::new()
    });

    Response::builder()
        .status(status)
        .content_type("text/html; charset=utf-8")
        .header(NOTIFICATION_HEADER, "true")
        .body(body)
}

/// An error as a whole page, for a plain navigation.
// TODO: render the error page templates instead of these placeholders.
fn page(status: poem::http::StatusCode, body: &'static str) -> Response {
    Response::builder()
        .status(status)
        .content_type("text/html; charset=utf-8")
        .body(body)
}

fn not_authorized(err: poem::Error) -> Response {
    if let Ok(err) = err.downcast::<NotAuthorized>() {
        // The NotAuthorized type handles URL-encoding the target
        Redirect::see_other(format!("/login?unauthorized=true&next={}", err.target()))
            .into_response()
    } else {
        Redirect::see_other("/login?unauthorized=true").into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use poem::http::StatusCode;

    /// The dispatcher only recognises "no message" if a message-less poem error
    /// really does render as the status' own Display. Pin that.
    #[test]
    fn message_less_error_renders_as_its_status() {
        assert_eq!(
            poem::Error::from_status(StatusCode::NOT_FOUND).to_string(),
            StatusCode::NOT_FOUND.to_string()
        );
    }

    fn body_of(resp: Response) -> String {
        futures::executor::block_on(async { resp.into_body().into_string().await.unwrap() })
    }

    #[test]
    fn a_supplied_message_reaches_the_reader() {
        let body = body_of(error_response(StatusCode::NOT_FOUND, "No such community."));
        assert!(body.contains("No such community."), "{body}");
    }

    #[test]
    fn an_empty_message_falls_back_to_the_default() {
        let body = body_of(error_response(StatusCode::FORBIDDEN, ""));
        assert!(body.contains("You do not have permission"), "{body}");
    }

    /// Server-error detail describes our failure, not the reader's problem, so
    /// it must never be rendered even when a call site passes it.
    #[test]
    fn server_error_detail_is_never_shown() {
        let body = body_of(error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "connection pool exhausted at db.rs:412",
        ));
        assert!(!body.contains("db.rs"), "{body}");
        assert!(body.contains("could not be completed"), "{body}");
    }

    /// The response says only "this is a message". Where it goes is the
    /// client's call, because only the client can see which part of the page
    /// the request came from.
    #[test]
    fn the_response_is_marked_for_placement() {
        let resp = error_response(StatusCode::FORBIDDEN, "");
        assert_eq!(
            resp.headers()
                .get(NOTIFICATION_HEADER)
                .and_then(|v| v.to_str().ok()),
            Some("true")
        );
    }

    #[test]
    fn the_response_names_no_destination() {
        let body = body_of(error_response(StatusCode::NOT_FOUND, ""));
        assert!(!body.contains("hx-target"), "{body}");
        assert!(!body.contains("hx-partial"), "{body}");
        assert!(body.contains("ss-notification"), "{body}");
    }

    #[test]
    fn the_status_is_preserved() {
        assert_eq!(
            error_response(StatusCode::FORBIDDEN, "").status(),
            StatusCode::FORBIDDEN
        );
    }
}
