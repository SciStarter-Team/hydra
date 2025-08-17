use anyhow::Result;
use common::{message::log::AnalyticsQuery, service::decode_structured_message};

use std::sync::Arc;
use log_service::{
    Database,
    analytics::{get_clickthrough_analytics, get_join_analytics},
    cache::{update_user_tags_cache, update_user_emails_cache}
};

// LogEntry and database_writer removed - migrated to new Table<T> system

// Handler function that takes database parameter
pub async fn handle_activity_log(database: Arc<Database>, req: &async_nats::service::Request) -> Result<()> {
    let (when, action, user_id, target, extra): (String, String, Option<String>, Option<String>, Option<String>) = 
        decode_structured_message(req.message.clone())?;
    
    // Use new table system
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

pub async fn handle_page_view(database: Arc<Database>, req: &async_nats::service::Request) -> Result<()> {
    let (when, ip_address, session_id, interaction_id, user_id, path, referrer, query, geolocation): 
        (String, String, String, String, Option<String>, String, Option<String>, Option<String>, String) = 
        decode_structured_message(req.message.clone())?;
    
    // Use new table system
    use log_service::database::tables::PageViewRow;
    
    let row = PageViewRow {
        when,
        ip_address,
        session_id,
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

pub async fn handle_clickthrough(database: Arc<Database>, req: &async_nats::service::Request) -> Result<()> {
    let (when, ip_address, session_id, interaction_id, user_id, from_path, to_url, link_type, program_id, geolocation): 
        (String, String, String, String, Option<String>, String, String, String, Option<String>, String) = 
        decode_structured_message(req.message.clone())?;
    
    // Use new table system
    use log_service::database::tables::ClickthroughRow;
    
    let row = ClickthroughRow {
        when,
        ip_address,
        session_id,
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

pub async fn handle_user_tags_update(database: Arc<Database>, req: &async_nats::service::Request) -> Result<()> {
    let (user_id, tags): (String, Vec<String>) = decode_structured_message(req.message.clone())?;
    
    // Use new table system for logging/analytics
    use log_service::database::tables::UserTagsRow;
    
    let row = UserTagsRow {
        user_id: user_id.clone(),
        tags: tags.clone(),
    };
    
    database.user_tags.insert(row).await?;
    
    // Update cache using new SQLite-based cache system
    for _attempt in 0..3 {
        if update_user_tags_cache(user_id.clone(), tags.clone()).await.is_ok() {
            break;
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
    }
    
    Ok(())
}

pub async fn handle_join(database: Arc<Database>, req: &async_nats::service::Request) -> Result<()> {
    let (when, entity_id, user_id, active, consented, contacted, notified, activity, reason): 
        (String, String, String, bool, Option<String>, Option<String>, bool, String, String) = 
        decode_structured_message(req.message.clone())?;
    
    // Use new table system
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

pub async fn handle_clickthrough_analytics(database: Arc<Database>, req: &async_nats::service::Request) -> Result<()> {
    let query: AnalyticsQuery = decode_structured_message(req.message.clone())?;
    
    // Use the new DataFusion-based analytics
    match get_clickthrough_analytics(Arc::clone(&database), query).await {
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

pub async fn handle_join_analytics(database: Arc<Database>, req: &async_nats::service::Request) -> Result<()> {
    let query: AnalyticsQuery = decode_structured_message(req.message.clone())?;
    
    // Use the new DataFusion-based analytics
    match get_join_analytics(Arc::clone(&database), query).await {
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

pub async fn handle_user_emails_update(_database: Arc<Database>, req: &async_nats::service::Request) -> Result<()> {
    let (user_id, emails): (String, Vec<String>) = decode_structured_message(req.message.clone())?;
    
    // Update cache using SQLite-based cache system with retry logic
    for _attempt in 0..3 {
        if update_user_emails_cache(user_id.clone(), emails.clone()).await.is_ok() {
            break;
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
    }
    
    Ok(())
}