use anyhow::Result;
use common::{
    message::log::{ExchangeEngagementRequest, ExchangeEngagementResponse, ExchangeEvent, ProjectCount},
    service::decode_structured_message,
};
use datafusion::prelude::SessionContext;
use datafusion::arrow::array::{Array, Int32Array, Int64Array, StringArray};

use crate::DATABASE;
use log_service::database::tables::ExchangeEventRow;

pub async fn record_exchange_event(req: &async_nats::service::Request) -> Result<()> {
    let event: ExchangeEvent = decode_structured_message(req.message.clone())?;

    // Write to exchange_events table
    let row = ExchangeEventRow {
        when: event.when_occurred.clone(),
        event_id: event.event_id.clone(),
        exchange_id: event.exchange_id.clone(),
        device_id: event.device_id.clone(),
        interaction_id: event.interaction_id.clone(),
        user_id: event.user_id.clone(),
        page: event.page.clone(),
        page_id: event.page_id.clone(),
        action: event.action.clone(),
        extra: event.extra.clone(),
    };

    let database = DATABASE.read().as_ref().expect("Database not initialized").clone();
    database.exchange_events.insert(row).await?;

    Ok(())
}

pub async fn get_engagement_stats(req: &async_nats::service::Request) -> Result<ExchangeEngagementResponse> {
    let request: ExchangeEngagementRequest = decode_structured_message(req.message.clone())?;
    let database = DATABASE.read().as_ref().expect("Database not initialized").clone();
    let ctx = database.context();

    // Build year filter for SQL queries
    let year_filter = if let Some(year) = request.year {
        format!("AND CAST(SUBSTRING(\"when\", 1, 4) AS INTEGER) = {}", year)
    } else {
        String::new()
    };

    // Query 1: Exchange visits (distinct interactions viewing the home page)
    let exchange_visits_query = format!(
        "SELECT COUNT(DISTINCT interaction_id) as count FROM exchange_events \
         WHERE exchange_id = '{}' AND page = 'home' AND action = 'view' {}",
        request.exchange_id, year_filter
    );
    let exchange_visits = execute_count_query(&ctx, &exchange_visits_query).await?;

    // Query 2: Unique visitors (distinct devices)
    let visitors_query = format!(
        "SELECT COUNT(DISTINCT device_id) as count FROM exchange_events \
         WHERE exchange_id = '{}' {}",
        request.exchange_id, year_filter
    );
    let visitors = execute_count_query(&ctx, &visitors_query).await?;

    // Query 3: Project page views
    let views_query = format!(
        "SELECT COUNT(*) as count FROM exchange_events \
         WHERE exchange_id = '{}' AND page = 'project' AND action = 'view' {}",
        request.exchange_id, year_filter
    );
    let views = execute_count_query(&ctx, &views_query).await?;

    // Query 4: Saves
    let saves_query = format!(
        "SELECT COUNT(*) as count FROM exchange_events \
         WHERE exchange_id = '{}' AND action = 'save' {}",
        request.exchange_id, year_filter
    );
    let saves = execute_count_query(&ctx, &saves_query).await?;

    // Query 5: System-wide clickthroughs (can't reliably attribute to exchange)
    let clickthroughs_query = if let Some(year) = request.year {
        format!(
            "SELECT COUNT(*) as count FROM clickthroughs \
             WHERE CAST(SUBSTRING(\"when\", 1, 4) AS INTEGER) = {}",
            year
        )
    } else {
        "SELECT COUNT(*) as count FROM clickthroughs".to_string()
    };
    let clickthroughs = execute_count_query(&ctx, &clickthroughs_query).await?;

    // Query 6: System-wide contributions (can't reliably attribute to exchange)
    let contributions_query = if let Some(year) = request.year {
        format!(
            "SELECT COUNT(*) as count FROM participation \
             WHERE CAST(SUBSTRING(\"when\", 1, 4) AS INTEGER) = {}",
            year
        )
    } else {
        "SELECT COUNT(*) as count FROM participation".to_string()
    };
    let contributions = execute_count_query(&ctx, &contributions_query).await?;

    // Query 7: Available years
    let years_query = format!(
        "SELECT DISTINCT CAST(SUBSTRING(\"when\", 1, 4) AS INTEGER) as year \
         FROM exchange_events WHERE exchange_id = '{}' ORDER BY year DESC",
        request.exchange_id
    );
    let available_years = execute_years_query(&ctx, &years_query).await?;

    // Query 8: Top 10 projects by views
    let top_views_query = format!(
        "SELECT page_id, COUNT(*) as count FROM exchange_events \
         WHERE exchange_id = '{}' AND page = 'project' AND action = 'view' \
         AND page_id IS NOT NULL {} \
         GROUP BY page_id ORDER BY count DESC LIMIT 10",
        request.exchange_id, year_filter
    );
    let top_views = execute_top_query(&ctx, &top_views_query).await?;

    // Query 9: Top 10 projects by saves
    let top_saves_query = format!(
        "SELECT page_id, COUNT(*) as count FROM exchange_events \
         WHERE exchange_id = '{}' AND action = 'save' \
         AND page_id IS NOT NULL {} \
         GROUP BY page_id ORDER BY count DESC LIMIT 10",
        request.exchange_id, year_filter
    );
    let top_saves = execute_top_query(&ctx, &top_saves_query).await?;

    Ok(ExchangeEngagementResponse {
        exchange_visits,
        visitors,
        views,
        saves,
        clickthroughs,
        contributions,
        available_years,
        top_views,
        top_saves,
    })
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
