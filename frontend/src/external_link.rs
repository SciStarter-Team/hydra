use crate::middleware::analytics::{Device, Interaction};
use crate::middleware::authorization::Authenticated;
use anyhow::Context;
use common::message::log::{ExternalLinkClickthroughRequest, ExternalLinkClickthroughResponse};
use common::service::request;
use poem::{
    Body, IntoResponse, Response, handler,
    http::StatusCode,
    web::{Path, RealIp},
};
use std::net::IpAddr;
use tracing::error;
use ulid::Ulid;

#[handler]
pub async fn handle_external_link(
    Authenticated(user_info): Authenticated,
    RealIp(ip_address): RealIp,
    device: Device,
    interaction: Interaction,
    Path(link_id): Path<Ulid>,
) -> Result<impl IntoResponse, poem::Error> {
    let ip_str = ip_address
        .unwrap_or_else(|| IpAddr::from([0, 0, 0, 0]))
        .to_string();

    let req_data = ExternalLinkClickthroughRequest {
        link_id,
        when: None, // Use current time
        ip_address: ip_str,
        device_id: device.into(),
        interaction_id: interaction.into(),
        user_id: user_info.as_ref().map(|u| u.user_id),
        link_type: "external_link".to_string(),
        program_id: None,
        geolocation: None, // Will be resolved from IP by log service
    };

    // Call log service to record clickthrough and get URL
    let response: ExternalLinkClickthroughResponse =
        request("log.links.clickthrough", &req_data)
            .await
            .context("Failed to record external link clickthrough")
            .map_err(|e| {
                error!("External link clickthrough error: {}", e);
                poem::Error::from_string("Link not found", StatusCode::NOT_FOUND)
            })?;

    // If URL found, redirect to it
    if let Some(url) = response.url {
        Ok(Response::builder()
            .status(StatusCode::FOUND)
            .header("Location", url)
            .body(Body::empty()))
    } else {
        Err(poem::Error::from_string(
            "Link not found",
            StatusCode::NOT_FOUND,
        ))
    }
}
