//! A calendar year's site-wide engagement totals, for the `/data` page.
//!
//! Two sources answer the same question. Years the legacy site served are
//! answered from the `legacy_engagement` table, which holds that site's own
//! final tallies; every other year is counted from our logs. The split is per
//! year rather than per figure so that the four numbers shown together always
//! come from one accounting of the same year.

use anyhow::Result;
use common::{
    message::log::{AnnualEngagementRequest, AnnualEngagementResponse},
    service::decode_structured_message,
};
use datafusion::arrow::array::Int64Array;
use datafusion::prelude::SessionContext;
use jiff::{Timestamp, tz::TimeZone};

use crate::DATABASE;

pub async fn handle_annual_engagement(
    req: &async_nats::service::Request,
) -> Result<AnnualEngagementResponse> {
    let request: AnnualEngagementRequest = decode_structured_message(req.message.clone())?;

    let year = match request.year {
        Some(year) => year,
        None => last_complete_year()?,
    };

    if let Some(legacy) = legacy_engagement(year).await? {
        return Ok(legacy);
    }

    let database = DATABASE
        .read()
        .as_ref()
        .expect("Database not initialized")
        .clone();
    let ctx = database.context();

    Ok(AnnualEngagementResponse {
        year,
        views: count_in_year(&ctx, "page_views_realtime", year).await?,
        saves: count_in_year(&ctx, "bookmark_realtime", year).await?,
        referrals: count_in_year(&ctx, "join_table_realtime", year).await?,
        contributions: count_in_year(&ctx, "participation_realtime", year).await?,
    })
}

/// The most recent calendar year that has fully elapsed, in UTC.
fn last_complete_year() -> Result<i32> {
    Ok(i32::from(
        Timestamp::now().to_zoned(TimeZone::UTC).date().year() - 1,
    ))
}

/// The legacy site's tally for `year`, if it recorded one.
async fn legacy_engagement(year: i32) -> Result<Option<AnnualEngagementResponse>> {
    let row = sqlx::query!(
        r#"SELECT "year" as "year!: i32",
                  "views" as "views!: i64",
                  "saves" as "saves!: i64",
                  "referrals" as "referrals!: i64",
                  "contributions" as "contributions!: i64"
           FROM legacy_engagement WHERE "year" = ?"#,
        year
    )
    .fetch_optional(common::shared::db())
    .await?;

    Ok(row.map(|row| AnnualEngagementResponse {
        year: row.year,
        views: row.views,
        saves: row.saves,
        referrals: row.referrals,
        contributions: row.contributions,
    }))
}

/// How many rows `table` holds for `year`.
///
/// The table name is a literal from this module and the year an integer, so
/// neither can carry anything but itself into the SQL; DataFusion takes no
/// bind parameters here.
async fn count_in_year(ctx: &SessionContext, table: &str, year: i32) -> Result<i64> {
    let sql = format!(
        r#"SELECT COUNT(*) as count FROM {table} WHERE CAST(SUBSTRING("when", 1, 4) AS INTEGER) = {year}"#
    );

    let batches = ctx.sql(&sql).await?.collect().await?;

    if batches.is_empty() || batches[0].num_rows() == 0 {
        return Ok(0);
    }

    let counts = batches[0]
        .column(0)
        .as_any()
        .downcast_ref::<Int64Array>()
        .ok_or_else(|| anyhow::anyhow!("Expected Int64Array for count"))?;

    Ok(counts.value(0))
}
