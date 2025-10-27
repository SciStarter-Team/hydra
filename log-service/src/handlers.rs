use anyhow::Result;
use common::{
    message::log::{
        AffiliateParticipantsRequest, AffiliateParticipantsResponse, AnalyticsQuery,
        AnalyticsResponse, GoUrlStatsRequest, GoUrlStatsResponse, RecentActivityRequest,
        RecentActivityResponse,
    },
    service::decode_structured_message,
};
use datafusion::prelude::*;
use ulid::Ulid;

use log_service::{
    analytics::{
        get_clickthrough_analytics_realtime, get_go_url_stats,
        get_participation_analytics_realtime, get_recent_activity,
    },
    cache::{update_user_emails_cache, update_user_tags_cache},
    health::HealthResponse,
};
// Import the global database instance from main
use crate::DATABASE;

pub async fn handle_activity_log(req: &async_nats::service::Request) -> Result<()> {
    let (when, action, user_id, target, extra): (
        String,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
    ) = decode_structured_message(req.message.clone())?;

    use log_service::database::tables::ActivityLogRow;

    let row = ActivityLogRow {
        when,
        action,
        user_id,
        target,
        extra,
    };

    // Clone database reference and drop it immediately after use
    {
        let database = DATABASE
            .read()
            .as_ref()
            .expect("Database not initialized")
            .clone();
        database.activity_log.insert(row).await?
    };

    Ok(())
}

pub async fn handle_page_view(req: &async_nats::service::Request) -> Result<()> {
    let (when, ip_address, device_id, interaction_id, user_id, path, referrer, query, geolocation): (String, String, String, String, Option<String>, String, Option<String>, Option<String>, String) = decode_structured_message(req.message.clone())?;

    use log_service::database::tables::PageViewRow;

    let row = PageViewRow {
        when,
        ip_address,
        device_id,
        interaction_id,
        user_id,
        path,
        referrer,
        query,
        geolocation,
    };

    // Clone database reference and drop it immediately after use
    {
        let database = DATABASE
            .read()
            .as_ref()
            .expect("Database not initialized")
            .clone();
        database.page_views.insert(row).await?
    };

    Ok(())
}

pub async fn handle_clickthrough(req: &async_nats::service::Request) -> Result<()> {
    let (
        when,
        ip_address,
        device_id,
        interaction_id,
        user_id,
        from_path,
        to_url,
        link_type,
        program_id,
        geolocation,
    ): (
        String,
        String,
        String,
        String,
        Option<String>,
        String,
        String,
        String,
        Option<String>,
        String,
    ) = decode_structured_message(req.message.clone())?;

    use log_service::database::tables::ClickthroughRow;

    let row = ClickthroughRow {
        when,
        ip_address,
        device_id,
        interaction_id,
        user_id,
        from_path,
        to_url,
        link_type,
        program_id,
        geolocation,
    };

    // Clone database reference and drop it immediately after use
    {
        let database = DATABASE
            .read()
            .as_ref()
            .expect("Database not initialized")
            .clone();
        database.clickthroughs.insert(row).await?
    };

    Ok(())
}

pub async fn handle_join(req: &async_nats::service::Request) -> Result<()> {
    let (when, entity_id, user_id, active, consented, contacted, notified, activity, reason): (
        String,
        String,
        String,
        bool,
        Option<String>,
        Option<String>,
        bool,
        String,
        String,
    ) = decode_structured_message(req.message.clone())?;

    use log_service::database::tables::JoinRow;

    let row = JoinRow {
        when,
        entity_id,
        user_id,
        active,
        consented,
        contacted,
        explicit: true, // Live joins are always explicit
        notified,
        activity,
        reason,
    };

    // Clone database reference and drop it immediately after use
    {
        let database = DATABASE
            .read()
            .as_ref()
            .expect("Database not initialized")
            .clone();
        database.join_table.insert(row).await?
    };

    Ok(())
}

pub async fn handle_clickthrough_analytics(
    req: &async_nats::service::Request,
) -> Result<AnalyticsResponse, anyhow::Error> {
    let query: AnalyticsQuery = decode_structured_message(req.message.clone())?;

    // Clone database reference and use it in scope
    let database = DATABASE
        .read()
        .as_ref()
        .expect("Database not initialized")
        .clone();
    get_clickthrough_analytics_realtime(database, query)
        .await
        .map_err(Into::into)
}

pub async fn handle_participation_analytics(
    req: &async_nats::service::Request,
) -> Result<AnalyticsResponse, anyhow::Error> {
    let query: AnalyticsQuery = decode_structured_message(req.message.clone())?;

    // Clone database reference and use it in scope
    let database = DATABASE
        .read()
        .as_ref()
        .expect("Database not initialized")
        .clone();
    get_participation_analytics_realtime(database, query)
        .await
        .map_err(Into::into)
}

pub async fn handle_user_tags_update(req: &async_nats::service::Request) -> Result<()> {
    let (user_id, tags): (String, Vec<String>) = decode_structured_message(req.message.clone())?;

    loop {
        if update_user_tags_cache(user_id.clone(), tags.clone())
            .await
            .is_ok()
        {
            break;
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
    }

    Ok(())
}

pub async fn handle_user_emails_update(req: &async_nats::service::Request) -> Result<()> {
    let (user_id, emails): (String, Vec<String>) = decode_structured_message(req.message.clone())?;

    // Update cache using SQLite-based cache system with retry logic
    loop {
        if update_user_emails_cache(user_id.clone(), emails.clone())
            .await
            .is_ok()
        {
            break;
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
    }

    Ok(())
}

pub async fn handle_health_check(
    req: &async_nats::service::Request,
) -> Result<HealthResponse, anyhow::Error> {
    // Clone database reference and use it in scope
    let database = DATABASE
        .read()
        .as_ref()
        .expect("Database not initialized")
        .clone();
    log_service::health::health_check(database, req).await
}

pub async fn handle_recent_activity(
    req: &async_nats::service::Request,
) -> Result<RecentActivityResponse, anyhow::Error> {
    let request: RecentActivityRequest = decode_structured_message(req.message.clone())?;

    // Clone database reference and use it in scope
    let database = DATABASE
        .read()
        .as_ref()
        .expect("Database not initialized")
        .clone();
    get_recent_activity(database, request)
        .await
        .map_err(Into::into)
}

pub async fn handle_go_url_stats(
    req: &async_nats::service::Request,
) -> Result<GoUrlStatsResponse, anyhow::Error> {
    let request: GoUrlStatsRequest = decode_structured_message(req.message.clone())?;

    // Clone database reference and use it in scope
    let database = DATABASE
        .read()
        .as_ref()
        .expect("Database not initialized")
        .clone();
    get_go_url_stats(database, request)
        .await
        .map_err(Into::into)
}

pub async fn handle_email_event(req: &async_nats::service::Request) -> Result<()> {
    let (when, event_type, sending_id, email_address, user_id, reason): (
        String,
        String,
        String,
        String,
        Option<String>,
        Option<String>,
    ) = decode_structured_message(req.message.clone())?;

    use log_service::database::tables::EmailEventRow;

    let row = EmailEventRow {
        when,
        event_type,
        sending_id,
        email_address,
        user_id,
        reason,
    };

    // Clone database reference and drop it immediately after use
    {
        let database = DATABASE
            .read()
            .as_ref()
            .expect("Database not initialized")
            .clone();
        database.email_events.insert(row).await?
    };

    Ok(())
}

pub async fn handle_affiliate_participation(
    req: &async_nats::service::Request,
) -> Result<AffiliateParticipantsResponse, anyhow::Error> {
    let request: AffiliateParticipantsRequest = decode_structured_message(req.message.clone())?;

    // Clone database reference and use it in scope
    let database = DATABASE
        .read()
        .as_ref()
        .expect("Database not initialized")
        .clone();

    let ctx = database.context();

    // Convert ULID and TimeString to strings for the query
    let affiliate_id_str = request.affiliate_id.to_string();
    let start_str = request.start.to_string();
    let end_str = request.end.to_string();

    // Query participation table for users who participated in this affiliate within the date range
    let df = ctx
        .table("participation")
        .await?
        .filter(col("entity_id").eq(lit(&affiliate_id_str)))?
        .filter(col("when").gt_eq(lit(&start_str)))?
        .filter(col("when").lt_eq(lit(&end_str)))?
        .filter(col("user_id").is_not_null())?
        .select(vec![col("user_id")])?
        .distinct()?;

    let batches = df.collect().await?;

    // Extract user IDs from the result batches
    let mut user_ids = Vec::new();
    use datafusion::arrow::array::{Array, StringArray};

    for batch in batches {
        if batch.num_rows() == 0 {
            continue;
        }

        let user_id_array = batch
            .column(0)
            .as_any()
            .downcast_ref::<StringArray>()
            .expect("user_id column should be StringArray");

        for i in 0..user_id_array.len() {
            if !user_id_array.is_null(i) {
                if let Ok(user_id) = user_id_array.value(i).parse::<Ulid>() {
                    user_ids.push(user_id);
                }
            }
        }
    }

    Ok(AffiliateParticipantsResponse { user_ids })
}
