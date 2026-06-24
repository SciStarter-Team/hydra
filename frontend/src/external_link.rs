use crate::middleware::analytics::{Device, Interaction};
use crate::middleware::authorization::Authenticated;
use anyhow::Context;
use common::message::log::{ExternalLinkClickthroughRequest, ExternalLinkClickthroughResponse};
use common::service::{log_clickthrough, request};
use poem::{
    Body, IntoResponse, Response, handler,
    http::StatusCode,
    web::{Json, Path, RealIp},
};
use serde::Deserialize;
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
    let response: ExternalLinkClickthroughResponse = request("log.links.clickthrough", &req_data)
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

/// Body of an outbound-link click beacon.
#[derive(Debug, Deserialize)]
pub struct ClickthroughBeacon {
    /// Absolute destination URL the user clicked.
    pub to_url: String,
    /// Path of the page the click originated from.
    pub from_path: String,
}

/// Record an outbound external-link click reported by `static/js/clickthrough.js`.
///
/// This replaces the old server-side href rewriting. Links stay direct; a
/// delegated `sendBeacon` handler posts here on external-link clicks. It records
/// the same `clickthroughs` data the `/-link` redirect path produces — IP,
/// geolocation, device, interaction and user are resolved server-side; only the
/// destination URL and originating path come from the client (the URL is
/// validated to be an absolute http(s) link).
#[handler]
pub async fn record_clickthrough(
    Authenticated(user_info): Authenticated,
    RealIp(ip_address): RealIp,
    device: Device,
    interaction: Interaction,
    Json(beacon): Json<ClickthroughBeacon>,
) -> Result<impl IntoResponse, poem::Error> {
    if !(beacon.to_url.starts_with("http://") || beacon.to_url.starts_with("https://")) {
        return Ok(StatusCode::BAD_REQUEST);
    }

    let ip_str = ip_address
        .unwrap_or_else(|| IpAddr::from([0, 0, 0, 0]))
        .to_string();

    if let Err(e) = log_clickthrough(
        None, // when - current time
        ip_str,
        String::from(device),
        String::from(interaction),
        user_info.as_ref().map(|u| u.user_id),
        beacon.from_path,
        beacon.to_url,
        "external_link",
        None, // program_id
        None, // geolocation - resolved from IP
    )
    .await
    {
        error!("Failed to record clickthrough beacon: {}", e);
    }

    Ok(StatusCode::NO_CONTENT)
}
