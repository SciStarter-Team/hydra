//! An exchange's engagement figures, for its data page.
//!
//! What they count is shared with orrery's exchange data page
//! (`get_data_engagement` in orrery's src/module/exchange.rs), whose events
//! are carried into the same `exchange_events` table, and the two must agree.

use anyhow::Result;
use common::message::log::{ExchangeEngagementRequest, ExchangeEngagementResponse, ProjectCount};
use datafusion::arrow::array::{Array, Int32Array, Int64Array, StringArray};
use datafusion::prelude::SessionContext;

/// The figures `request` asks for, read through `ctx`.
pub async fn engagement_stats(
    ctx: &SessionContext,
    request: &ExchangeEngagementRequest,
) -> Result<ExchangeEngagementResponse> {
    // Ids are interpolated into DataFusion SQL literals below. They are always
    // ULIDs, so require them to be alphanumeric; this rejects any quote or
    // metacharacter and closes the injection surface regardless of the caller.
    let alphanumeric = |id: &str| !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric());

    if !alphanumeric(&request.exchange_id) {
        anyhow::bail!("invalid exchange_id: {:?}", request.exchange_id);
    }
    if let Some(bad) = request.project_ids.iter().find(|id| !alphanumeric(id)) {
        anyhow::bail!("invalid project id: {:?}", bad);
    }

    let exchange = format!("exchange_id = '{}'", request.exchange_id);
    let year_filter = if let Some(year) = request.year {
        format!("AND CAST(SUBSTRING(\"when\", 1, 4) AS INTEGER) = {}", year)
    } else {
        String::new()
    };

    // What these count is shared with orrery's exchange data page
    // (`get_data_engagement` in orrery's src/module/exchange.rs), whose events
    // are carried into this table, and the two must agree:
    //
    // - views: views of the exchange's home page and its project pages;
    // - visits: distinct interactions among those views;
    // - visitors: distinct devices;
    // - saves, and referrals (following a project or event out to its site);
    // - contributions: summed repetitions on the projects it lists, which
    //   `participation` already holds as one row apiece.
    //
    // Visits and visitors count only events carrying the analytics ids, which
    // orrery's earliest events lack; `tracked_since` says from when they run.
    const PAGE_VIEW: &str = "action = 'view' AND page IN ('home', 'project')";

    let exchange_visits = execute_count_query(
        &ctx,
        &format!(
            "SELECT COUNT(DISTINCT interaction_id) as count FROM exchange_events_realtime \
             WHERE {exchange} AND {PAGE_VIEW} AND interaction_id <> '' {year_filter}"
        ),
    )
    .await?;

    let visitors = execute_count_query(
        &ctx,
        &format!(
            "SELECT COUNT(DISTINCT device_id) as count FROM exchange_events_realtime \
             WHERE {exchange} AND device_id <> '' {year_filter}"
        ),
    )
    .await?;

    let views = execute_count_query(
        &ctx,
        &format!(
            "SELECT COUNT(*) as count FROM exchange_events_realtime \
             WHERE {exchange} AND {PAGE_VIEW} {year_filter}"
        ),
    )
    .await?;

    let saves = execute_count_query(
        &ctx,
        &format!(
            "SELECT COUNT(*) as count FROM exchange_events_realtime \
             WHERE {exchange} AND action = 'save' {year_filter}"
        ),
    )
    .await?;

    let referrals = execute_count_query(
        &ctx,
        &format!(
            "SELECT COUNT(*) as count FROM exchange_events_realtime \
             WHERE {exchange} AND action = 'visit' {year_filter}"
        ),
    )
    .await?;

    let contributions = if request.project_ids.is_empty() {
        0
    } else {
        let listed = request
            .project_ids
            .iter()
            .map(|id| format!("'{id}'"))
            .collect::<Vec<_>>()
            .join(", ");

        execute_count_query(
            &ctx,
            &format!(
                "SELECT COUNT(*) as count FROM participation_realtime \
                 WHERE entity_id IN ({listed}) {year_filter}"
            ),
        )
        .await?
    };

    let available_years = execute_years_query(
        &ctx,
        &format!(
            "SELECT DISTINCT CAST(SUBSTRING(\"when\", 1, 4) AS INTEGER) as year \
             FROM exchange_events_realtime WHERE {exchange} ORDER BY year DESC"
        ),
    )
    .await?;

    let top = |action: &str| {
        format!(
            "SELECT page_id, COUNT(*) as count FROM exchange_events_realtime \
             WHERE {exchange} AND page = 'project' AND action = '{action}' \
             AND page_id IS NOT NULL {year_filter} \
             GROUP BY page_id ORDER BY count DESC LIMIT 10"
        )
    };
    let top_views = execute_top_query(&ctx, &top("view")).await?;
    let top_saves = execute_top_query(&ctx, &top("save")).await?;
    let top_referrals = execute_top_query(&ctx, &top("visit")).await?;

    let tracked_since = execute_string_query(
        &ctx,
        &format!(
            "SELECT MIN(\"when\") as since FROM exchange_events_realtime \
             WHERE {exchange} AND device_id <> ''"
        ),
    )
    .await?;

    Ok(ExchangeEngagementResponse {
        exchange_visits,
        visitors,
        views,
        saves,
        referrals,
        contributions,
        available_years,
        top_views,
        top_saves,
        top_referrals,
        tracked_since,
    })
}

/// The single string a query returns, if it returns one.
async fn execute_string_query(ctx: &SessionContext, query: &str) -> Result<Option<String>> {
    let df = ctx.sql(query).await?;
    let results = df.collect().await?;

    if results.is_empty() || results[0].num_rows() == 0 {
        return Ok(None);
    }

    let column = results[0].column(0);
    if column.is_null(0) {
        return Ok(None);
    }

    let column =
        datafusion::arrow::compute::cast(column, &datafusion::arrow::datatypes::DataType::Utf8)?;
    let strings = column
        .as_any()
        .downcast_ref::<StringArray>()
        .ok_or_else(|| anyhow::anyhow!("Expected StringArray"))?;

    Ok(Some(strings.value(0).to_string()))
}

async fn execute_count_query(ctx: &SessionContext, query: &str) -> Result<i32> {
    let df = ctx.sql(query).await?;
    let results = df.collect().await?;

    if results.is_empty() || results[0].num_rows() == 0 {
        return Ok(0);
    }

    let count_array = results[0]
        .column(0)
        .as_any()
        .downcast_ref::<Int64Array>()
        .ok_or_else(|| anyhow::anyhow!("Expected Int64Array"))?;

    Ok(count_array.value(0) as i32)
}

async fn execute_years_query(ctx: &SessionContext, query: &str) -> Result<Vec<i32>> {
    let df = ctx.sql(query).await?;
    let results = df.collect().await?;

    let mut years = Vec::new();
    for batch in results {
        let year_array = batch
            .column(0)
            .as_any()
            .downcast_ref::<Int32Array>()
            .ok_or_else(|| anyhow::anyhow!("Expected Int32Array"))?;

        for i in 0..batch.num_rows() {
            if !year_array.is_null(i) {
                years.push(year_array.value(i));
            }
        }
    }

    Ok(years)
}

async fn execute_top_query(ctx: &SessionContext, query: &str) -> Result<Vec<ProjectCount>> {
    let df = ctx.sql(query).await?;
    let results = df.collect().await?;

    let mut projects = Vec::new();
    for batch in results {
        let id_array = batch
            .column(0)
            .as_any()
            .downcast_ref::<StringArray>()
            .ok_or_else(|| anyhow::anyhow!("Expected StringArray for page_id"))?;

        let count_array = batch
            .column(1)
            .as_any()
            .downcast_ref::<Int64Array>()
            .ok_or_else(|| anyhow::anyhow!("Expected Int64Array for count"))?;

        for i in 0..batch.num_rows() {
            if !id_array.is_null(i) {
                projects.push(ProjectCount {
                    project_id: id_array.value(i).to_string(),
                    count: count_array.value(i) as i32,
                });
            }
        }
    }

    Ok(projects)
}
