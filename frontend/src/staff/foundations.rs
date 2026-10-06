//! GET `/staff/foundations-completions` — how many people complete the
//! Foundations training each month. The same page as orrery's.
//!
//! The report and its markup are shared with orrery byte-for-byte (see
//! `common::foundations` and `common::report_kit`); the user service counts
//! each completion's month from the progress records
//! (`user.foundations.monthly`). This file only reads the range from the
//! query string and serves the pages.

use common::{
    foundations::{page, report},
    message::dashboard::FoundationsCompletionsResponse,
    service::request,
};
use poem::{Response, handler, web::Query};

use super::report_range::{RangeQuery, generated, today};

/// GET `/staff/foundations-completions` — the page, which loads its report
/// from `data`.
#[handler]
pub async fn index(Query(query): Query<RangeQuery>) -> Response {
    Response::builder()
        .content_type("text/html; charset=utf-8")
        .body(page::shell(query.range(today(), page::DEFAULT_SPAN)))
}

/// GET `/staff/foundations-completions/data` — the chart and table for the
/// range.
#[handler]
#[tracing::instrument]
pub async fn data(Query(query): Query<RangeQuery>) -> Result<Response, anyhow::Error> {
    let range = query.range(today(), page::DEFAULT_SPAN);

    let response: FoundationsCompletionsResponse = request("user.foundations.monthly", ()).await?;

    let html = page::fragment(
        &report::view(
            &report::tally(range, &response.counts),
            report::completed_to_date(&response.counts),
        ),
        range,
        &generated(),
    );

    Ok(Response::builder()
        .content_type("text/html; charset=utf-8")
        .body(html))
}
