//! GET `/staff/signup-referrals` — where new accounts come from, month by
//! month. The same page as orrery's.
//!
//! The report and its markup are shared with orrery byte-for-byte (see
//! `common::referrals` and `common::report_kit`); the user service decides
//! which accounts count and tallies them (`user.referrals.monthly`). This
//! file only reads the range from the query string and serves the pages.

use common::{
    message::dashboard::{ReferralsRequest, ReferralsResponse},
    referrals::{page, report},
    service::request,
};
use poem::{Response, handler, web::Query};

use super::report_range::{RangeQuery, generated, today};

/// GET `/staff/signup-referrals` — the page, which loads its report from
/// `data`.
#[handler]
pub async fn index(Query(query): Query<RangeQuery>) -> Response {
    Response::builder()
        .content_type("text/html; charset=utf-8")
        .body(page::shell(query.range(today(), page::DEFAULT_SPAN)))
}

/// GET `/staff/signup-referrals/data` — the chart and tables for the range.
#[handler]
#[tracing::instrument]
pub async fn data(Query(query): Query<RangeQuery>) -> Result<Response, anyhow::Error> {
    let range = query.range(today(), page::DEFAULT_SPAN);

    let response: ReferralsResponse = request(
        "user.referrals.monthly",
        ReferralsRequest {
            first: range.first,
            last: range.last,
        },
    )
    .await?;

    let html = page::fragment(&report::view(&response.months), range, &generated());

    Ok(Response::builder()
        .content_type("text/html; charset=utf-8")
        .body(html))
}
