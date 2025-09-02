use anyhow::Result;
use common::{message::log::AnalyticsQuery, service::decode_structured_message};

use log_service::{
    analytics::{get_clickthrough_analytics, get_join_analytics},
    cache::{update_user_emails_cache, update_user_tags_cache},
    health::{HealthResponse},
};
use std::sync::Arc;

// Import the global database instance from main
use crate::DATABASE;

pub async fn handle_activity_log(
    req: &async_nats::service::Request,
) -> Result<()> {
    let database = DATABASE.get().expect("Database not initialized");
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

    database.activity_log.insert(row).await?;

    Ok(())
}

pub async fn handle_page_view(
    req: &async_nats::service::Request,
) -> Result<()> {
    let database = DATABASE.get().expect("Database not initialized");
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

    database.page_views.insert(row).await?;

    Ok(())
}

pub async fn handle_clickthrough(
    req: &async_nats::service::Request,
) -> Result<()> {
    let database = DATABASE.get().expect("Database not initialized");
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

    database.clickthroughs.insert(row).await?;

    Ok(())
}

pub async fn handle_join(
    req: &async_nats::service::Request,
) -> Result<()> {
    let database = DATABASE.get().expect("Database not initialized");
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

    database.join_table.insert(row).await?;

    Ok(())
}

pub async fn handle_clickthrough_analytics(
    req: &async_nats::service::Request,
) -> Result<()> {
    let database = DATABASE.get().expect("Database not initialized");
    let query: AnalyticsQuery = decode_structured_message(req.message.clone())?;

    match get_clickthrough_analytics(Arc::clone(database), query).await {
        Ok(response) => {
            let response_data = flexbuffers::to_vec(&response)?;
            req.respond(Ok(response_data.into())).await?;
        }
        Err(e) => {
            eprintln!("Error in clickthrough analytics: {}", e);
            let error_response = format!("Analytics error: {}", e);
            let response_data = flexbuffers::to_vec(&error_response)?;
            req.respond(Ok(response_data.into())).await?; // Send as Ok with error message
        }
    }

    Ok(())
}

pub async fn handle_join_analytics(
    req: &async_nats::service::Request,
) -> Result<()> {
    let database = DATABASE.get().expect("Database not initialized");
    let query: AnalyticsQuery = decode_structured_message(req.message.clone())?;

    match get_join_analytics(Arc::clone(database), query).await {
        Ok(response) => {
            let response_data = flexbuffers::to_vec(&response)?;
            req.respond(Ok(response_data.into())).await?;
        }
        Err(e) => {
            eprintln!("Error in join analytics: {}", e);
            let error_response = format!("Analytics error: {}", e);
            let response_data = flexbuffers::to_vec(&error_response)?;
            req.respond(Ok(response_data.into())).await?; // Send as Ok with error message
        }
    }

    Ok(())
}

pub async fn handle_user_tags_update(
    req: &async_nats::service::Request,
) -> Result<()> {
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

pub async fn handle_user_emails_update(
    req: &async_nats::service::Request,
) -> Result<()> {
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

pub async fn handle_health_check(req: &async_nats::service::Request) -> Result<HealthResponse, anyhow::Error> {
    let database = DATABASE.get().expect("Database not initialized");
    log_service::health::health_check(Arc::clone(database), req).await
}
