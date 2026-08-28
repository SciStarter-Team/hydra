//! The Ambassador funnel exports under `/staff/ambassadors/`, ported from
//! orrery's `src/staff/ambassadors.rs`.
//!
//! Four of them name a single stage; the fifth, the Champions list, spans every
//! stage where Champion training is done and the route to Ambassador is still
//! open. They are all the same query against different stages, so they share one
//! handler and differ only in which stages they name.
//!
//! The application browser and its export live at `/-admin/ambassadors`, which
//! is where orrery's `/staff/ambassadors/applications` went.

use common::{
    message::dashboard::{AmbassadorStage, AmbassadorStageRow, AmbassadorStagesRequest},
    service::request,
};
use poem::{Response, handler, web::Query};
use serde::Deserialize;

use super::csv_field;

/// Stages where Champion training is done and the person could still become an
/// Ambassador.
///
/// The stages themselves are [`AmbassadorStage`], shared with the user service
/// that derives them; what belongs here is only which of them each CSV covers.
const OPEN: &[AmbassadorStage] = &[
    AmbassadorStage::Tutorial,
    AmbassadorStage::Application,
    AmbassadorStage::Verified,
];

/// Stages where Champion training is done but the route to Ambassador has
/// closed. Kept out of the default export: these are people who were turned
/// down, and a recruiting list should not reach them by accident.
const CLOSED: &[AmbassadorStage] = &[
    AmbassadorStage::International,
    AmbassadorStage::Rejected,
    AmbassadorStage::Failed,
];

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct ChampionsQuery {
    /// Add the people whose route to Ambassador has closed — rejected,
    /// screening failed, or outside the countries the program covers.
    include_closed: bool,
}

/// Fetch everyone at `stages` and render the export.
async fn export(stages: &[AmbassadorStage], filename: &str) -> Result<Response, anyhow::Error> {
    let rows: Vec<AmbassadorStageRow> = request(
        "user.ambassador.stages",
        AmbassadorStagesRequest {
            stages: stages.to_vec(),
        },
    )
    .await?;

    let mut csv = String::from("first_name,last_name,handle,email,status,since\n");

    for row in rows {
        csv.push_str(&format!(
            "{},{},{},{},{},{}\n",
            csv_field(&row.first_name),
            csv_field(&row.last_name),
            csv_field(&row.handle),
            csv_field(&row.email),
            csv_field(row.stage.label()),
            csv_field(&row.since),
        ));
    }

    Ok(Response::builder()
        .content_type("text/csv")
        .header(
            "Content-Disposition",
            format!("attachment; filename=\"{filename}\""),
        )
        .body(csv))
}

/// GET `/staff/ambassadors/ambassador_tutorial.csv` — finished Champion
/// training, has not applied.
#[handler]
#[tracing::instrument]
pub async fn tutorial() -> Result<Response, anyhow::Error> {
    export(&[AmbassadorStage::Tutorial], "ambassador_tutorial.csv").await
}

/// GET `/staff/ambassadors/ambassador_application.csv` — applied, not yet
/// verified.
#[handler]
#[tracing::instrument]
pub async fn application() -> Result<Response, anyhow::Error> {
    export(
        &[AmbassadorStage::Application],
        "ambassador_application.csv",
    )
    .await
}

/// GET `/staff/ambassadors/ambassador_verified.csv` — application verified,
/// not yet an Ambassador.
#[handler]
#[tracing::instrument]
pub async fn verified() -> Result<Response, anyhow::Error> {
    export(&[AmbassadorStage::Verified], "ambassador_verified.csv").await
}

/// GET `/staff/ambassadors/ambassador_certified.csv` — the Ambassadors.
#[handler]
#[tracing::instrument]
pub async fn certified() -> Result<Response, anyhow::Error> {
    export(&[AmbassadorStage::Passed], "ambassador_certified.csv").await
}

/// GET `/staff/ambassadors/champions_not_yet_ambassadors.csv` — everyone who
/// finished Champion training and has not become an Ambassador.
///
/// By default only those whose route is still open; `?include_closed=true`
/// adds the people who were turned down.
#[handler]
#[tracing::instrument]
pub async fn champions(Query(query): Query<ChampionsQuery>) -> Result<Response, anyhow::Error> {
    let mut stages = OPEN.to_vec();

    if query.include_closed {
        stages.extend_from_slice(CLOSED);
    }

    export(&stages, "champions_not_yet_ambassadors.csv").await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_and_closed_are_disjoint_and_exclude_ambassadors() {
        for stage in OPEN {
            assert!(!CLOSED.contains(stage), "{stage:?} is in both");
            assert_ne!(
                *stage,
                AmbassadorStage::Passed,
                "an Ambassador is not a prospect"
            );
        }
        for stage in CLOSED {
            assert_ne!(*stage, AmbassadorStage::Passed);
        }
    }

    #[test]
    fn champions_covers_every_non_ambassador_stage_when_closed_is_included() {
        let mut covered: Vec<AmbassadorStage> = OPEN.to_vec();
        covered.extend_from_slice(CLOSED);

        for stage in AmbassadorStage::ALL {
            if *stage == AmbassadorStage::Passed {
                continue;
            }
            assert!(covered.contains(stage), "{stage:?} is in no Champions list");
        }
    }
}
