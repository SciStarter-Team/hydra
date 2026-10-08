use anyhow::Result;
use common::{
    message::log::{ExchangeEngagementRequest, ExchangeEngagementResponse, ExchangeEvent},
    service::decode_structured_message,
};

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

    let database = DATABASE
        .read()
        .as_ref()
        .expect("Database not initialized")
        .clone();
    database.exchange_events.insert(row).await?;

    Ok(())
}

pub async fn get_engagement_stats(
    req: &async_nats::service::Request,
) -> Result<ExchangeEngagementResponse> {
    let request: ExchangeEngagementRequest = decode_structured_message(req.message.clone())?;

    let database = DATABASE
        .read()
        .as_ref()
        .expect("Database not initialized")
        .clone();

    log_service::exchange_engagement::engagement_stats(&database.context(), &request).await
}
