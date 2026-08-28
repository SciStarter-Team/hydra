//! GET/POST `/staff/ax4gr27` — become another user. Ported from orrery's
//! `masquerade`, at the same obscure path so existing staff bookmarks work.
//!
//! Support work sometimes needs to see exactly what one account sees. orrery
//! allowed that and recorded nothing; hydra records every use in the user
//! service's `masquerades` table before the session changes hands, so anything
//! done afterwards can be traced back to the staff member who did it.
//!
//! Two things follow the identity change. The session id is rotated, as it is
//! on an ordinary login, so the new identity does not inherit the old
//! session's id. And any `ss-token` the browser is holding is cleared, because
//! it still names the staff member and would otherwise contradict the session.

use common::{
    message::user::{BasicInfo, MasqueradeRequest},
    service::request,
};
use poem::{
    IntoResponse, Response, handler,
    session::Session,
    web::{Form, Redirect},
};
use serde::Deserialize;

use crate::component::html_escape;
use crate::middleware::authorization::Authenticated;

/// The field name orrery used, kept so a staff member's saved form still
/// submits something the endpoint understands.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct MasqueradeForm {
    ax4gr27: String,
}

/// GET `/staff/ax4gr27` — the form.
#[handler]
pub async fn masquerade_form() -> Response {
    page(None)
}

/// POST `/staff/ax4gr27` — become the account owning the submitted address.
#[handler]
#[tracing::instrument(skip(session))]
pub async fn masquerade(
    session: &Session,
    Authenticated(user_info): Authenticated,
    Form(form): Form<MasqueradeForm>,
) -> Result<Response, anyhow::Error> {
    let staff = user_info.ok_or_else(|| anyhow::anyhow!("Not authenticated"))?;

    let email = form.ax4gr27.trim().to_string();

    if email.is_empty() {
        return Ok(page(Some(
            "Enter the email address of the account to become.",
        )));
    }

    let target: Option<BasicInfo> = request(
        "user.admin.masquerade",
        MasqueradeRequest {
            staff_user_id: staff.user_id.to_string(),
            email: email.clone(),
        },
    )
    .await?;

    let Some(target) = target else {
        return Ok(page(Some(&format!("No account has the address {email}."))));
    };

    // Rotate the session id on this identity change, exactly as an ordinary
    // login does, so the staff session's id is not carried into the new one.
    session.renew();
    session.set("basic_info", &target);

    Ok(Redirect::see_other("/").into_response())
}

fn page(error: Option<&str>) -> Response {
    let mut html = String::from(
        "<!doctype html><html><head><meta charset=\"utf-8\">\
         <title>Masquerade</title>\
         <style>body{font-family:sans-serif;margin:2rem;max-width:560px;}\
         label{display:block;margin:.5rem 0;}\
         .warn{background:#fff6e5;border:1px solid #e0b558;padding:.75rem;}</style>\
         </head><body>\
         <script>localStorage.removeItem(\"ss-token\");</script>\
         <h1>Become another user</h1>\
         <p class=\"warn\">This is recorded. Every use is written to the \
         masquerade log with your account and the account you become.</p>",
    );

    if let Some(error) = error {
        html.push_str(&format!(
            "<p style=\"color:#b00\">{}</p>",
            html_escape(error)
        ));
    }

    html.push_str(
        "<form method=\"post\">\
         <label>Email address of the account\
         <input type=\"email\" name=\"ax4gr27\" required autocomplete=\"off\"></label>\
         <p><button type=\"submit\">Become this user</button></p>\
         </form></body></html>",
    );

    Response::builder()
        .content_type("text/html; charset=utf-8")
        .body(html)
}
