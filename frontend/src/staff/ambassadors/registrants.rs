//! GET/POST `/staff/ambassadors/registrants` — the staff page that finds the
//! Champions and Ambassadors among an event's Zoom registrants, ported from
//! orrery's `src/staff/ambassadors/registrants.rs`.
//!
//! Reading the exports and building the lists is in [`report`], which orrery
//! shares byte for byte; this module is the page, the upload, and the lookup
//! of each address's funnel standing.

mod report;

use common::{
    message::dashboard::{AmbassadorStage, AmbassadorStanding, AmbassadorStandingsRequest},
    service::request,
};
use poem::{Response, handler, http::StatusCode, web::Multipart};
use sailfish::TemplateOnce;

use self::report::{List, Reach, Registrants, Standing, Standings};
use super::OPEN;

/// Room for a batch of Zoom exports: a registration report with custom
/// questions runs to a few hundred bytes per registrant.
pub const UPLOAD_LIMIT: usize = 32 * 1024 * 1024;

#[derive(TemplateOnce)]
#[template(path = "staff/ambassador_registrants.stpl.html")]
struct Page {
    problem: Option<String>,
}

fn page(status: StatusCode, problem: Option<String>) -> Result<Response, anyhow::Error> {
    Ok(Response::builder()
        .status(status)
        .content_type("text/html; charset=utf-8")
        .body(Page { problem }.render_once()?))
}

#[handler]
#[tracing::instrument]
pub async fn form() -> Result<Response, anyhow::Error> {
    page(StatusCode::OK, None)
}

/// One uploaded export, as the form delivered it.
struct Upload {
    filename: String,
    bytes: Vec<u8>,
}

/// What the form sent.
struct Submission {
    uploads: Vec<Upload>,
    list: Option<List>,
    include_closed: bool,
}

/// The form's fields, or a message for staff when the upload did not arrive
/// whole.
async fn read_submission(mut multipart: Multipart) -> Result<Submission, String> {
    let incomplete = |err| format!("The upload did not arrive complete ({err}). Please try again.");

    let mut submission = Submission {
        uploads: Vec::new(),
        list: None,
        include_closed: false,
    };

    while let Some(field) = multipart.next_field().await.map_err(incomplete)? {
        let name = field.name().unwrap_or_default().to_owned();
        let filename = field.file_name().unwrap_or_default().to_owned();
        let bytes = field.bytes().await.map_err(incomplete)?;

        match name.as_str() {
            // A file input left empty still sends a part, with no name and
            // no content.
            "exports" if !bytes.is_empty() => submission.uploads.push(Upload { filename, bytes }),
            "list" => submission.list = List::from_value(&String::from_utf8_lossy(&bytes)),
            "include_closed" => submission.include_closed = true,
            _ => {}
        }
    }

    Ok(submission)
}

/// POST `/staff/ambassadors/registrants` — one of the two lists, as a CSV
/// download. Input staff can correct re-renders the form with a message and a
/// 400, rather than raising an error.
#[handler]
#[tracing::instrument(skip(multipart))]
pub async fn download(multipart: Multipart) -> Result<Response, anyhow::Error> {
    let submission = match read_submission(multipart).await {
        Ok(submission) => submission,
        Err(problem) => return page(StatusCode::BAD_REQUEST, Some(problem)),
    };

    let Some(list) = submission.list else {
        return page(
            StatusCode::BAD_REQUEST,
            Some("Choose which list to download.".to_owned()),
        );
    };

    if submission.uploads.is_empty() {
        return page(
            StatusCode::BAD_REQUEST,
            Some("Choose at least one registrant export from Zoom.".to_owned()),
        );
    }

    let mut registrants = Registrants::default();

    for upload in &submission.uploads {
        if let Err(problem) = registrants.add_export(&upload.filename, &upload.bytes) {
            return page(StatusCode::BAD_REQUEST, Some(problem));
        }
    }

    let standings = fetch_standings(registrants.addresses()).await?;
    let rows = registrants.list(list, submission.include_closed, &standings);

    Ok(Response::builder()
        .content_type("text/csv")
        .header(
            "Content-Disposition",
            format!("attachment; filename=\"{}\"", list.filename()),
        )
        .body(report::csv(&rows)?))
}

/// How far along the funnel a stage is, as the lists count it.
fn reach(stage: AmbassadorStage) -> Reach {
    if stage == AmbassadorStage::Passed {
        Reach::Ambassador
    } else if OPEN.contains(&stage) {
        Reach::Open
    } else {
        Reach::Closed
    }
}

/// The funnel standing of each address that belongs to an account past the
/// start of the funnel, from the user service.
async fn fetch_standings(addresses: Vec<String>) -> Result<Standings, anyhow::Error> {
    let found: Vec<AmbassadorStanding> = request(
        "user.ambassador.standings",
        AmbassadorStandingsRequest { addresses },
    )
    .await?;

    let mut standings = Standings::default();

    for AmbassadorStanding { address, person } in found {
        standings.insert(
            &address,
            Standing {
                reach: reach(person.stage),
                status: person.stage.label().to_owned(),
                handle: person.handle,
                account_name: format!("{} {}", person.first_name, person.last_name)
                    .trim()
                    .to_owned(),
                since: person.since,
            },
        );
    }

    Ok(standings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::staff::ambassadors::CLOSED;
    use assert2::check;

    #[test]
    fn stages_reach_as_the_funnel_exports_count_them() {
        check!(reach(AmbassadorStage::Passed) == Reach::Ambassador);

        for &stage in OPEN {
            check!(reach(stage) == Reach::Open, "{stage:?}");
        }

        for &stage in CLOSED {
            check!(reach(stage) == Reach::Closed, "{stage:?}");
        }
    }
}
