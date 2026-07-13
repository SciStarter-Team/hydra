//! Staff review queue for direct messages. Every in-app message is held until
//! a staff member approves it; this page lists the pending messages and lets
//! staff approve (deliver) or reject them. Modeled on the ambassador contact
//! queue (`admin::ambassadors`). Nested under the already-`StaffOnly`
//! `admin::routes()`.

use crate::component::{AdminLayout, Component};
use crate::middleware::authorization::Authenticated;
use common::message::forum::{ReviewMessageDecision, ReviewMessageRow};
use common::service::request;
use poem::{
    Endpoint, Route, get, handler, post,
    web::{Form, Path},
};
use sailfish::TemplateOnce;
use serde::Deserialize;
use ulid::Ulid;

#[derive(TemplateOnce)]
#[template(path = "admin/message_review.stpl.html")]
struct MessageReviewPage {
    rows: Vec<ReviewMessageRow>,
}

/// One pending-message `<tr>`, rendered inside the page.
#[derive(TemplateOnce)]
#[template(path = "admin/message_review_row.stpl.html")]
pub struct ReviewRow {
    pub row: ReviewMessageRow,
}

pub fn routes() -> impl Endpoint {
    Route::new()
        .at("/", get(index))
        .at("/review/:id", post(review))
}

#[handler]
async fn index(Authenticated(user_info): Authenticated) -> Result<poem::Response, anyhow::Error> {
    let rows: Vec<ReviewMessageRow> = request("forum.messaging.pending_list", ()).await?;
    let page = MessageReviewPage { rows };
    let prepared = AdminLayout::new("Message Review", page)
        .with_sidebar_active("messages")
        .with_breadcrumb("Message Review", Some("/-admin/messages"))
        .prepare(user_info)
        .await?;
    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(prepared.render_once()?))
}

#[derive(Deserialize)]
struct ReviewForm {
    /// "accept" delivers the message; anything else rejects it.
    decision: String,
}

#[handler]
async fn review(
    Path(id): Path<String>,
    Authenticated(_user_info): Authenticated,
    Form(form): Form<ReviewForm>,
) -> Result<poem::Response, anyhow::Error> {
    let post_id = Ulid::from_string(&id)?;
    let accept = form.decision == "accept";
    let _: () = request(
        "forum.messaging.review",
        ReviewMessageDecision { post_id, accept },
    )
    .await?;

    // The message is no longer pending; replace its row with a confirmation.
    let label = if accept {
        "Approved &amp; delivered"
    } else {
        "Rejected"
    };
    let html = format!(
        "<tr class=\"border-t border-gray-100\">\
           <td colspan=\"5\" class=\"px-2 py-2 text-gray-500\">{}</td>\
         </tr>",
        label
    );
    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(html))
}
