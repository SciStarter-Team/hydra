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
    message::dashboard::{AmbassadorStageRow, AmbassadorStagesRequest},
    service::request,
};
use poem::{Response, handler, web::Query};
use serde::Deserialize;

use super::csv_field;

/// A stage of the Ambassador funnel.
///
/// The state machine's own names, paired with what staff call them. Everything
/// below the tutorial stage has finished Champion training; `Passed` is the
/// only stage that means "is an Ambassador".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Tutorial,
    Application,
    Verified,
    Passed,
    International,
    Rejected,
    Failed,
}

impl Stage {
    /// Stages where Champion training is done and the person could still
    /// become an Ambassador.
    const OPEN: &'static [Stage] = &[Stage::Tutorial, Stage::Application, Stage::Verified];

    /// Stages where Champion training is done but the route to Ambassador has
    /// closed. Kept out of the default export: these are people who were turned
    /// down, and a recruiting list should not reach them by accident.
    const CLOSED: &'static [Stage] = &[Stage::International, Stage::Rejected, Stage::Failed];

    const ALL: &'static [Stage] = &[
        Stage::Tutorial,
        Stage::Application,
        Stage::Verified,
        Stage::Passed,
        Stage::International,
        Stage::Rejected,
        Stage::Failed,
    ];

    /// The name this stage carries in `ambassador_progress.state`.
    fn state(self) -> &'static str {
        match self {
            Stage::Tutorial => "tutorial",
            Stage::Application => "application",
            Stage::Verified => "verified",
            Stage::Passed => "passed",
            Stage::International => "international",
            Stage::Rejected => "rejected",
            Stage::Failed => "failed",
        }
    }

    /// What staff call this stage.
    fn label(self) -> &'static str {
        match self {
            Stage::Tutorial => "Champion training complete",
            Stage::Application => "Application submitted",
            Stage::Verified => "Application verified",
            Stage::Passed => "Ambassador",
            Stage::International => "International",
            Stage::Rejected => "Rejected",
            Stage::Failed => "Screening failed",
        }
    }

    fn from_state(state: &str) -> Option<Stage> {
        Stage::ALL.iter().copied().find(|s| s.state() == state)
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct ChampionsQuery {
    /// Add the people whose route to Ambassador has closed — rejected,
    /// screening failed, or outside the countries the program covers.
    include_closed: bool,
}

/// Fetch everyone at `stages` and render the export.
async fn export(stages: &[Stage], filename: &str) -> Result<Response, anyhow::Error> {
    let rows: Vec<AmbassadorStageRow> = request(
        "user.ambassador.stages",
        AmbassadorStagesRequest {
            states: stages.iter().map(|s| s.state().to_owned()).collect(),
        },
    )
    .await?;

    let mut csv = String::from("first_name,last_name,handle,email,status,since\n");

    for row in rows {
        // An unrecognized state is reported as it is stored rather than
        // dropped, so a stage added to the state machine and not yet to this
        // list still shows up in the export.
        let status = Stage::from_state(&row.state)
            .map(Stage::label)
            .unwrap_or(&row.state)
            .to_string();

        csv.push_str(&format!(
            "{},{},{},{},{},{}\n",
            csv_field(&row.first_name),
            csv_field(&row.last_name),
            csv_field(&row.handle),
            csv_field(&row.email),
            csv_field(&status),
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
    export(&[Stage::Tutorial], "ambassador_tutorial.csv").await
}

/// GET `/staff/ambassadors/ambassador_application.csv` — applied, not yet
/// verified.
#[handler]
#[tracing::instrument]
pub async fn application() -> Result<Response, anyhow::Error> {
    export(&[Stage::Application], "ambassador_application.csv").await
}

/// GET `/staff/ambassadors/ambassador_verified.csv` — application verified,
/// not yet an Ambassador.
#[handler]
#[tracing::instrument]
pub async fn verified() -> Result<Response, anyhow::Error> {
    export(&[Stage::Verified], "ambassador_verified.csv").await
}

/// GET `/staff/ambassadors/ambassador_certified.csv` — the Ambassadors.
#[handler]
#[tracing::instrument]
pub async fn certified() -> Result<Response, anyhow::Error> {
    export(&[Stage::Passed], "ambassador_certified.csv").await
}

/// GET `/staff/ambassadors/champions_not_yet_ambassadors.csv` — everyone who
/// finished Champion training and has not become an Ambassador.
///
/// By default only those whose route is still open; `?include_closed=true`
/// adds the people who were turned down.
#[handler]
#[tracing::instrument]
pub async fn champions(Query(query): Query<ChampionsQuery>) -> Result<Response, anyhow::Error> {
    let mut stages = Stage::OPEN.to_vec();

    if query.include_closed {
        stages.extend_from_slice(Stage::CLOSED);
    }

    export(&stages, "champions_not_yet_ambassadors.csv").await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_stage_round_trips_through_its_state_name() {
        for stage in Stage::ALL {
            assert_eq!(Stage::from_state(stage.state()), Some(*stage));
        }
    }

    #[test]
    fn open_and_closed_are_disjoint_and_exclude_ambassadors() {
        for stage in Stage::OPEN {
            assert!(!Stage::CLOSED.contains(stage), "{stage:?} is in both");
            assert_ne!(*stage, Stage::Passed, "an Ambassador is not a prospect");
        }
        for stage in Stage::CLOSED {
            assert_ne!(*stage, Stage::Passed);
        }
    }

    #[test]
    fn champions_covers_every_non_ambassador_stage_when_closed_is_included() {
        let mut covered: Vec<Stage> = Stage::OPEN.to_vec();
        covered.extend_from_slice(Stage::CLOSED);

        for stage in Stage::ALL {
            if *stage == Stage::Passed {
                continue;
            }
            assert!(covered.contains(stage), "{stage:?} is in no Champions list");
        }
    }

    #[test]
    fn an_unknown_state_has_no_stage() {
        assert!(Stage::from_state("initial").is_none());
    }
}
