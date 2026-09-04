//! `/-api/me/…` — small JSON answers about the current user, for pages that
//! decide what to show on the client.

use anyhow::Error;
use common::message::user::{ProgramStatusRequest, ProgramStatusResponse};
use common::service::request as nats_request;
use poem::{IntoResponse, Response, handler, web::Json};
use serde::Serialize;

use crate::middleware::authorization::Authenticated;

/// Where the current user stands in the Citizen Science Champion and
/// SciStarter Ambassador programs.
///
/// The shape is orrery's, field for field: the two resource pages that read it
/// are ported CMS pages carrying their own copy of the fetch, and they have to
/// keep working against whichever site answers them.
#[derive(Serialize)]
struct ProgramStatus {
    authenticated: bool,
    has_champion_cert: bool,
    is_ambassador: bool,
}

/// GET `/-api/me/program-status`
///
/// A signed-out visitor, or a lookup that fails, answers `false` to everything
/// -- the gates on those pages then show their signed-out branch, so a fault
/// here hides the resources rather than leaking them.
#[handler]
pub async fn program_status(Authenticated(user_info): Authenticated) -> Response {
    let Some(user) = user_info else {
        return Json(ProgramStatus {
            authenticated: false,
            has_champion_cert: false,
            is_ambassador: false,
        })
        .into_response();
    };

    let status: Result<ProgramStatusResponse, Error> = nats_request(
        "user.certificates.program_status",
        ProgramStatusRequest {
            user_id: user.user_id,
        },
    )
    .await;

    let status = status.unwrap_or_else(|error| {
        eprintln!("Program status for {} unavailable: {error}", user.user_id);
        ProgramStatusResponse {
            has_champion_cert: false,
            is_ambassador: false,
        }
    });

    Json(ProgramStatus {
        authenticated: true,
        has_champion_cert: status.has_champion_cert,
        is_ambassador: status.is_ambassador,
    })
    .into_response()
}
