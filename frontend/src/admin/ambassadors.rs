//! Staff ambassador-application processing grid (top-level admin, staff-facing).
//!
//! A spreadsheet-style view of every ambassador application, mirroring orrery's
//! Django `AmbassadorApplicationAdmin` list-editable page. Each row is editable
//! inline; saving a row posts the review to `user.ambassador.review`, which
//! drives the native ambassador state machine (granting/revoking the Certified
//! Ambassador badge and directory membership) and returns the re-rendered row.
//!
//! NOTE: while ambassador state still syncs from orrery, orrery's Django admin
//! remains the authoritative processing tool; this page is verified but not the
//! live workflow until the hydra-native cutover (see
//! `user-service` `apply_ambassador_state` / `ambassador::apply_review_effects`).

use crate::component::{AdminLayout, Component};
use crate::middleware::authorization::Authenticated;
use common::message::dashboard::{
    AmbassadorAppStatus, AmbassadorApplicationRow, AmbassadorMessageReview, AmbassadorMessageRow,
    AmbassadorReviewUpdate, AmbassadorScreeningStatus,
};
use common::service::request;
use poem::{
    Endpoint, Route, get, handler, post,
    web::{Form, Path},
};
use sailfish::TemplateOnce;
use serde::Deserialize;
use ulid::Ulid;

#[derive(TemplateOnce)]
#[template(path = "admin/ambassadors.stpl.html")]
struct AmbassadorsPage {
    rows: Vec<AmbassadorApplicationRow>,
}

/// One editable `<tr>`, rendered both inside the page and standalone as the
/// response to an inline save.
#[derive(TemplateOnce)]
#[template(path = "admin/ambassadors_row.stpl.html")]
pub struct AmbassadorRow {
    pub row: AmbassadorApplicationRow,
}

#[derive(TemplateOnce)]
#[template(path = "admin/ambassador_messages.stpl.html")]
struct MessagesPage {
    rows: Vec<AmbassadorMessageRow>,
}

/// One contact-message row in the moderation queue, rendered both inside the
/// page and standalone as the response to an accept/reject.
#[derive(TemplateOnce)]
#[template(path = "admin/ambassador_message_row.stpl.html")]
pub struct MessageRow {
    pub row: AmbassadorMessageRow,
}

pub fn routes() -> impl Endpoint {
    Route::new()
        .at("/", get(index))
        .at("/save/:user_id", post(save))
        .at("/messages", get(messages))
        .at("/messages/review/:id", post(message_review))
}

#[handler]
async fn index(Authenticated(user_info): Authenticated) -> Result<poem::Response, anyhow::Error> {
    let rows: Vec<AmbassadorApplicationRow> = request("user.ambassador.list", ()).await?;
    let page = AmbassadorsPage { rows };
    let prepared = AdminLayout::new("Ambassador Applications", page)
        .with_sidebar_active("ambassadors")
        .with_breadcrumb("Ambassadors", Some("/-admin/ambassadors"))
        .prepare(user_info)
        .await?;
    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(prepared.render_once()?))
}

/// Inline row edit. Absent checkboxes deserialize to `None` (= unchecked);
/// empty date fields become `None`.
#[derive(Deserialize)]
struct ReviewForm {
    cohort: i64,
    app_status: i64,
    screening_status: i64,
    #[serde(default)]
    marked: Option<String>,
    #[serde(default)]
    archived: Option<String>,
    #[serde(default)]
    hidden: Option<String>,
    #[serde(default)]
    draft: Option<String>,
    #[serde(default)]
    verified: Option<String>,
    #[serde(default)]
    rejection_sent: Option<String>,
    #[serde(default)]
    welcome_sent: Option<String>,
    #[serde(default)]
    orientation: Option<String>,
    #[serde(default)]
    kit_sent: Option<String>,
    #[serde(default)]
    survey: Option<String>,
    #[serde(default)]
    handles: String,
    #[serde(default)]
    reviewers: String,
    #[serde(default)]
    notes: String,
}

fn checked(o: &Option<String>) -> bool {
    o.is_some()
}

fn nonempty(o: Option<String>) -> Option<String> {
    o.filter(|s| !s.trim().is_empty())
}

#[handler]
async fn save(
    Path(user_id): Path<String>,
    Authenticated(_user_info): Authenticated,
    Form(form): Form<ReviewForm>,
) -> Result<poem::Response, anyhow::Error> {
    let user_ulid = Ulid::from_string(&user_id)?;

    let update = AmbassadorReviewUpdate {
        user_id: user_ulid,
        marked: checked(&form.marked),
        archived: checked(&form.archived),
        hidden: checked(&form.hidden),
        draft: checked(&form.draft),
        cohort: form.cohort,
        app_status: AmbassadorAppStatus::from_i64(form.app_status),
        screening_status: AmbassadorScreeningStatus::from_i64(form.screening_status),
        verified: nonempty(form.verified),
        rejection_sent: checked(&form.rejection_sent),
        welcome_sent: checked(&form.welcome_sent),
        orientation: nonempty(form.orientation),
        kit_sent: nonempty(form.kit_sent),
        survey: checked(&form.survey),
        handles: form.handles,
        reviewers: form.reviewers,
        notes: form.notes,
    };
    let _: () = request("user.ambassador.review", update).await?;

    // Re-fetch this application to reflect derived badge/directory state.
    let rows: Vec<AmbassadorApplicationRow> = request("user.ambassador.list", ()).await?;
    let row = rows
        .into_iter()
        .find(|r| r.user_id == user_ulid)
        .ok_or_else(|| anyhow::anyhow!("Application not found after review"))?;

    let html = AmbassadorRow { row }.render_once()?;
    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(html))
}

/// The contact-an-ambassador moderation queue. Staff accept a message to deliver
/// it to the ambassador (with a staff copy); rejecting simply leaves it un-sent.
#[handler]
async fn messages(
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let rows: Vec<AmbassadorMessageRow> = request("user.ambassador.message_list", ()).await?;
    let page = MessagesPage { rows };
    let prepared = AdminLayout::new("Ambassador Messages", page)
        .with_sidebar_active("ambassadors")
        .with_breadcrumb("Ambassadors", Some("/-admin/ambassadors"))
        .with_breadcrumb("Messages", Some("/-admin/ambassadors/messages"))
        .prepare(user_info)
        .await?;
    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(prepared.render_once()?))
}

#[derive(Deserialize)]
struct MessageReviewForm {
    /// "accept" delivers the message; "reject" leaves it un-sent.
    decision: String,
}

#[handler]
async fn message_review(
    Path(id): Path<String>,
    Authenticated(_user_info): Authenticated,
    Form(form): Form<MessageReviewForm>,
) -> Result<poem::Response, anyhow::Error> {
    let message_id = Ulid::from_string(&id)?;
    let accepted = form.decision == "accept";
    let _: () = request(
        "user.ambassador.message_review",
        AmbassadorMessageReview {
            id: message_id,
            accepted,
        },
    )
    .await?;

    // Re-fetch to reflect delivery (accepted + sent timestamp).
    let rows: Vec<AmbassadorMessageRow> = request("user.ambassador.message_list", ()).await?;
    let row = rows
        .into_iter()
        .find(|r| r.id == message_id)
        .ok_or_else(|| anyhow::anyhow!("Message not found after review"))?;

    let html = MessageRow { row }.render_once()?;
    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(html))
}
