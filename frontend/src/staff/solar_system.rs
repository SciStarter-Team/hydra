//! GET `/staff/solar-system-ambassadors` — how far NASA's Solar System
//! Ambassadors have got through SciStarter's training. Ported from orrery's
//! `solar_system_ambassador_progress`.
//!
//! NASA sends over a roster; the accounts on it carry the
//! [`SOLAR_SYSTEM_AMBASSADOR`] tag. This report goes back to NASA saying which
//! of them made an account, finished Foundations, and became a Champion.

use common::{
    message::user::{TagCohortMember, TagCohortRequest},
    service::request,
};
use poem::{Response, handler};

use super::csv_field;

/// The tag NASA's roster leaves on an account.
const SOLAR_SYSTEM_AMBASSADOR: &str = "solar-system-ambassador";

/// The tag awarded for finishing Foundations.
const FOUNDATIONS_BADGE: &str = "foundations-badge";

/// GET `/staff/solar-system-ambassadors` — the CSV.
#[handler]
#[tracing::instrument]
pub async fn solar_system_ambassadors() -> Result<Response, anyhow::Error> {
    let members: Vec<TagCohortMember> = request(
        "user.tags.cohort",
        TagCohortRequest {
            tag: SOLAR_SYSTEM_AMBASSADOR.to_string(),
        },
    )
    .await?;

    let mut csv =
        String::from("email_address,account_made,foundations_training,ambassador_training\n");

    for member in &members {
        let has = |tag: &str| member.tags.iter().any(|t| t == tag);
        let foundations = has(FOUNDATIONS_BADGE);
        let champion = common::tag::CHAMPION_TRAINING_ANY
            .iter()
            .any(|tag| has(tag));

        csv.push_str(&format!(
            "{},{},{},{}\n",
            csv_field(&member.email),
            csv_field(&member.date_joined),
            foundations,
            champion,
        ));
    }

    Ok(Response::builder()
        .content_type("text/csv")
        .header("Content-Disposition", "attachment; filename=\"SSA.csv\"")
        .body(csv))
}
