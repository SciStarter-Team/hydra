//! Publishing a project on a staff reviewer's approval, and telling the
//! submitter about the review.
//!
//! Every way staff approve a project ends in [`publish`], so a project
//! published from its admin page and one published with the rest of an
//! import batch go live the same way, and their submitters hear the same
//! thing. Mirrors orrery's `Entity.publish_as_staff`, whose post-save hook
//! does the telling there.

use anyhow::Error;
use common::message::log::Housekeeping;
use common::{service::log_activity, shared::db};
use ulid::Ulid;

use crate::{
    entity::{Entity, EntityKind},
    peering::LoadStorePeered,
    project::field::{ProjectField, ProjectFieldValue},
    submission::{SubmissionKind, mark_published},
};

/// Publish a project on `reviewer`'s approval: its page goes live, it leaves
/// the review queue, and its submitter is told.
///
/// Telling the submitter is best effort: once the page is live the project
/// is published, whether or not the email could be queued.
#[tracing::instrument]
pub async fn publish(project_id: Ulid, reviewer: Ulid) -> Result<(), Error> {
    let project_id_str = project_id.to_string();

    let name = sqlx::query_scalar!(
        "SELECT name FROM projects WHERE project_id = ?",
        project_id_str
    )
    .fetch_one(db())
    .await?;

    Entity::publish_page(EntityKind::Project, project_id, &name).await?;

    ProjectField::new(project_id, ProjectFieldValue::Reviewed(true))
        .store()
        .await?;

    if let Err(err) = mark_published(SubmissionKind::Project, project_id).await {
        tracing::error!(%project_id, ?err, "Could not tell the submitter their project was published");
    }

    let _ = log_activity(
        None,
        Housekeeping::ProjectPublish,
        reviewer,
        Some(project_id),
        None,
    )
    .await;

    Ok(())
}
