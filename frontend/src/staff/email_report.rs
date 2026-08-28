//! GET `/staff/email-report` — what each mass email send cost us, over a date
//! range. Ported from orrery's `email_report`.
//!
//! orrery could only guess at the cost: it counted every bounce and
//! unsubscribe recorded in the three days after a send and attributed them all
//! to that send, which double-counts whenever two sends land in the same week.
//! hydra's emailer records bounces, complaints and unsubscribes against the
//! sending that caused them, so the columns here are attributed rather than
//! inferred, and the report says so.
//!
//! With no dates the endpoint answers with the date form, as orrery's did.

use common::{
    message::email::{ListSendingsRequest, ListSendingsResponse},
    service::request,
};
use jiff::ToSpan;
use poem::{Response, handler, web::Query};
use serde::Deserialize;

use super::csv_field;

/// A single request can pull a whole year of sends; the cap is high enough
/// that no realistic range is truncated, and present so a malformed range
/// cannot ask the emailer for everything at once.
const MAX_SENDINGS: u32 = 10_000;

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct ReportQuery {
    begin: String,
    end: String,
}

/// GET `/staff/email-report` — the CSV, or the date form when either date is
/// missing.
#[handler]
#[tracing::instrument]
pub async fn email_report(Query(query): Query<ReportQuery>) -> Result<Response, anyhow::Error> {
    if query.begin.is_empty() || query.end.is_empty() {
        return Ok(form(None));
    }

    let (begin_date, end_date) = match (
        query.begin.parse::<jiff::civil::Date>(),
        query.end.parse::<jiff::civil::Date>(),
    ) {
        (Ok(begin), Ok(end)) => (begin, end),
        _ => return Ok(form(Some("Both dates must be given as YYYY-MM-DD."))),
    };

    if end_date < begin_date {
        return Ok(form(Some("The end date is before the begin date.")));
    }

    // Both endpoints name whole days, so the window runs to the start of the
    // day after the end date.
    let created_from = format!("{begin_date}T00:00:00Z");
    let created_to = format!("{}T00:00:00Z", end_date.checked_add(1.day())?);

    let response: ListSendingsResponse = request(
        "emailer.analytics.list_sendings",
        ListSendingsRequest {
            page: 1,
            per_page: MAX_SENDINGS,
            // Every send counts, including the one-to-one ones the queue page
            // filters out: the report is about what left the building.
            min_queued: None,
            created_from: Some(created_from),
            created_to: Some(created_to),
        },
    )
    .await?;

    let mut csv = String::from(
        "subject,send initiated,# queued,# sent,# skipped,bounces,complaints,unsubscribes\n",
    );

    for sending in &response.sendings {
        csv.push_str(&format!(
            "{},{},{},{},{},{},{},{}\n",
            csv_field(&sending.subject),
            csv_field(&sending.created_at.to_string()),
            sending.total_queued,
            sending.messages_sent,
            sending.messages_skipped,
            sending.messages_bounced,
            sending.complaints,
            sending.unsubscribes,
        ));
    }

    Ok(Response::builder()
        .content_type("text/csv")
        .header(
            "Content-Disposition",
            format!("attachment; filename=\"email_{begin_date}_{end_date}.csv\""),
        )
        .body(csv))
}

fn form(error: Option<&str>) -> Response {
    let mut html = String::from(
        "<!doctype html><html><head><meta charset=\"utf-8\">\
         <title>Email Report</title>\
         <style>body{font-family:sans-serif;margin:2rem;max-width:640px;}\
         label{display:block;margin:.5rem 0;}</style></head>\
         <body><h1>Email report</h1>\
         <p>Every mass send initiated between the two dates, with the bounces, \
         complaints and unsubscribes attributed to it.</p>",
    );

    if let Some(error) = error {
        html.push_str(&format!(
            "<p style=\"color:#b00\">{}</p>",
            crate::component::html_escape(error)
        ));
    }

    html.push_str(
        "<form method=\"get\">\
         <label>Begin date <input type=\"date\" name=\"begin\" required></label>\
         <label>End date <input type=\"date\" name=\"end\" required></label>\
         <p><button type=\"submit\">Export CSV</button></p>\
         </form></body></html>",
    );

    Response::builder()
        .content_type("text/html; charset=utf-8")
        .body(html)
}
