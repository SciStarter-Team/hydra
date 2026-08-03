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
use crate::form::QsQuery;
use crate::middleware::authorization::Authenticated;
use common::message::dashboard::{
    AmbassadorAppStatus, AmbassadorApplicationRow, AmbassadorExportRow, AmbassadorFilters,
    AmbassadorListRequest, AmbassadorListResponse, AmbassadorMessageReview, AmbassadorMessageRow,
    AmbassadorReviewUpdate, AmbassadorScreeningStatus, DatePresence, TriState,
};
use common::service::request;
use poem::{
    Endpoint, Route, get, handler, post,
    web::{Form, Path},
};
use sailfish::TemplateOnce;
use serde::{Deserialize, Serialize};
use ulid::Ulid;

/// Rows per page, matching orrery's staff browser.
const PAGE_SIZE: i64 = 25;

/// The filter form's state, as it arrives in the query string. Multi-selects
/// use `name[]`, the only spelling `serde_qs` collects into a list.
#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct GridQuery {
    #[serde(default)]
    pub archived: TriState,
    #[serde(default)]
    pub draft: TriState,
    #[serde(default)]
    pub hidden: TriState,
    #[serde(default)]
    pub marked: TriState,
    #[serde(default)]
    pub welcome_sent: TriState,
    #[serde(default)]
    pub rejection_sent: TriState,
    #[serde(default)]
    pub survey: TriState,
    #[serde(default)]
    pub citscimonth: TriState,
    #[serde(default)]
    pub background_check: TriState,
    #[serde(default)]
    pub conduct: TriState,
    #[serde(default)]
    pub in_directory: TriState,

    #[serde(default)]
    pub cohort: Vec<i64>,
    #[serde(default)]
    pub app_status: Vec<i64>,
    #[serde(default)]
    pub screening_status: Vec<i64>,

    #[serde(default)]
    pub verified_presence: DatePresence,
    #[serde(default)]
    pub orientation_presence: DatePresence,
    #[serde(default)]
    pub kit_sent_presence: DatePresence,
    #[serde(default)]
    pub renewal_date_presence: DatePresence,

    #[serde(default)]
    pub submitted_from: String,
    #[serde(default)]
    pub submitted_to: String,
    #[serde(default)]
    pub verified_from: String,
    #[serde(default)]
    pub verified_to: String,
    #[serde(default)]
    pub renewal_date_from: String,
    #[serde(default)]
    pub renewal_date_to: String,

    #[serde(default)]
    pub search: String,
    #[serde(default)]
    pub city: String,
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub phone: String,
    #[serde(default)]
    pub tee_size: String,
    #[serde(default)]
    pub gender: String,
    #[serde(default)]
    pub ethnicity: String,
    #[serde(default)]
    pub handles: String,
    #[serde(default)]
    pub reviewers: String,
    #[serde(default)]
    pub notes: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<i64>,
}

impl GridQuery {
    fn page_number(&self) -> i64 {
        self.page.unwrap_or(1).max(1)
    }

    fn as_filters(&self) -> AmbassadorFilters {
        AmbassadorFilters {
            user_id: None,
            archived: self.archived,
            draft: self.draft,
            hidden: self.hidden,
            marked: self.marked,
            welcome_sent: self.welcome_sent,
            rejection_sent: self.rejection_sent,
            survey: self.survey,
            citscimonth: self.citscimonth,
            background_check: self.background_check,
            conduct: self.conduct,
            in_directory: self.in_directory,
            cohort: self.cohort.clone(),
            app_status: self.app_status.clone(),
            screening_status: self.screening_status.clone(),
            verified_presence: self.verified_presence,
            orientation_presence: self.orientation_presence,
            kit_sent_presence: self.kit_sent_presence,
            renewal_date_presence: self.renewal_date_presence,
            submitted_from: self.submitted_from.clone(),
            submitted_to: self.submitted_to.clone(),
            verified_from: self.verified_from.clone(),
            verified_to: self.verified_to.clone(),
            renewal_date_from: self.renewal_date_from.clone(),
            renewal_date_to: self.renewal_date_to.clone(),
            search: self.search.clone(),
            city: self.city.clone(),
            state: self.state.clone(),
            phone: self.phone.clone(),
            tee_size: self.tee_size.clone(),
            gender: self.gender.clone(),
            ethnicity: self.ethnicity.clone(),
            handles: self.handles.clone(),
            reviewers: self.reviewers.clone(),
            notes: self.notes.clone(),
        }
    }
}

#[derive(TemplateOnce)]
#[template(path = "admin/ambassadors.stpl.html")]
struct AmbassadorsPage {
    rows: Vec<AmbassadorApplicationRow>,
    filters: GridQuery,
    total: i64,
    page: i64,
    total_pages: i64,
    has_prev: bool,
    has_next: bool,
    prev_page: i64,
    next_page: i64,
    app_statuses: &'static [AmbassadorAppStatus],
    screening_statuses: &'static [AmbassadorScreeningStatus],
    query_string: String,
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
        .at("/export.csv", get(export_csv))
        .at("/save/:user_id", post(save))
        .at("/messages", get(messages))
        .at("/messages/review/:id", post(message_review))
}

#[handler]
async fn index(
    Authenticated(user_info): Authenticated,
    QsQuery(query): QsQuery<GridQuery>,
) -> Result<poem::Response, anyhow::Error> {
    let requested_page = query.page_number();

    let response: AmbassadorListResponse = request(
        "user.ambassador.list",
        AmbassadorListRequest {
            filters: query.as_filters(),
            page: requested_page,
            per_page: PAGE_SIZE,
        },
    )
    .await?;

    let total_pages = if response.total == 0 {
        1
    } else {
        (response.total + PAGE_SIZE - 1) / PAGE_SIZE
    };
    let page_number = requested_page.min(total_pages);

    // Preserved across pagination and export links so a filtered view
    // survives both. `page` is dropped so those links can supply their own.
    let query_string = {
        let mut without_page = query.clone();
        without_page.page = None;
        serde_qs::to_string(&without_page).unwrap_or_default()
    };

    let page = AmbassadorsPage {
        rows: response.rows,
        filters: query,
        total: response.total,
        page: page_number,
        total_pages,
        has_prev: page_number > 1,
        has_next: page_number < total_pages,
        prev_page: (page_number - 1).max(1),
        next_page: (page_number + 1).min(total_pages),
        app_statuses: AmbassadorAppStatus::all(),
        screening_statuses: AmbassadorScreeningStatus::all(),
        query_string,
    };
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
    let response: AmbassadorListResponse = request(
        "user.ambassador.list",
        AmbassadorListRequest {
            filters: AmbassadorFilters {
                user_id: Some(user_ulid),
                ..AmbassadorFilters::default()
            },
            page: 1,
            per_page: 1,
        },
    )
    .await?;
    let row = response
        .rows
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("Application not found after review"))?;

    let html = AmbassadorRow { row }.render_once()?;
    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(html))
}

/// CSV of every application matching the current filters, with the applicant's
/// long-form answers. Mirrors orrery's export; the filter form links here with
/// its own query string, so what you see is what you get.
#[handler]
async fn export_csv(
    Authenticated(_user_info): Authenticated,
    QsQuery(query): QsQuery<GridQuery>,
) -> Result<poem::Response, anyhow::Error> {
    let rows: Vec<AmbassadorExportRow> = request(
        "user.ambassador.export",
        AmbassadorListRequest {
            filters: query.as_filters(),
            page: 1,
            per_page: 0,
        },
    )
    .await?;

    let mut writer = csv::WriterBuilder::new()
        .terminator(csv::Terminator::CRLF)
        .has_headers(true)
        .from_writer(vec![]);

    for row in rows {
        writer.serialize(row)?;
    }

    Ok(poem::Response::builder()
        .content_type("text/csv")
        .header(
            "Content-Disposition",
            "attachment; filename=\"ambassador_applications.csv\"",
        )
        .body(writer.into_inner()?))
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
