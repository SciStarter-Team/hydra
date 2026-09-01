//! Which projects advance the Foundations of Citizen Science module.
//!
//! Participating in one of the module's projects is a step of the module, so
//! recording that participation has to advance the account's position in it.
//! Orrery raises exactly these three signals from inside its own
//! `record_event`, for exactly these three project slugs; this is the same
//! rule at the same point, since every participation hydra records converges
//! on [`crate::handlers::handle_project_participation_record`].

use anyhow::Error;
use common::message::user::{WorkflowProgressRequest, WorkflowProgressResponse, WorkflowType};
use common::service::request;

use crate::project_ids;

/// The module's projects, each with the step it completes. Any two of the
/// three finish the module's project requirement.
const PROJECT_STEPS: [(&str, &str); 3] = [
    (project_ids::SQUIRREL, "squirrel"),
    (project_ids::STALL, "stall"),
    (project_ids::DEBRIS, "debris"),
];

/// Advance the Foundations module for `user_id` if `project_id` is one of its
/// projects, and report whether it was.
///
/// The module's own project page is not one of them, so the contribution the
/// user service records when the module completes arrives back here and stops.
pub async fn advance_for_participation(user_id: &str, project_id: &str) -> Result<bool, Error> {
    let Some(slug) = project_ids::slug_of(project_id).await? else {
        return Ok(false);
    };

    let Some((_, step)) = PROJECT_STEPS.iter().find(|(known, _)| *known == slug) else {
        return Ok(false);
    };

    let _: WorkflowProgressResponse = request(
        "user.workflow.progress",
        WorkflowProgressRequest {
            user_id: user_id.to_string(),
            workflow: WorkflowType::Foundations,
            step: step.to_string(),
        },
    )
    .await?;

    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The steps this service sends are matched by their spelling in
    /// user-service's `update_foundations_progress`, and counted by their
    /// columns in `check_foundations_badge`. Neither side can import the
    /// other's spelling, so a rename would just stop advancing anybody's
    /// module, silently and only in production. Pinned here instead.
    ///
    /// Orrery raises the same three signals for the same three slugs, in
    /// `orrery/affiliate/tasks.py::record_event`. The two codebases have to
    /// agree, or an account's module state depends on which one recorded its
    /// contribution.
    #[test]
    fn the_module_advances_on_three_projects_by_their_orrery_names() {
        assert_eq!(
            PROJECT_STEPS,
            [
                ("project-squirrel", "squirrel"),
                ("stall-catchers-by-eyesonalz", "stall"),
                ("marine-debris-tracker", "debris"),
            ]
        );
    }

    /// Two of the three finish the module's project requirement, so a fourth
    /// entry here would change what the badge asks for without touching the
    /// rule that grants it.
    #[test]
    fn no_project_names_the_same_step_as_another() {
        for (index, (_, step)) in PROJECT_STEPS.iter().enumerate() {
            assert!(
                !PROJECT_STEPS[..index]
                    .iter()
                    .any(|(_, other)| other == step),
                "two projects both complete the `{step}` step"
            );
        }
    }
}
