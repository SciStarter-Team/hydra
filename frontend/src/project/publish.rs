//! Publishing a project on a staff reviewer's approval, and telling the
//! submitter about the review.
//!
//! Every way staff approve a project ends in [`publish`], so a project
//! published from its admin page and one published with the rest of an
//! import batch go live the same way, and their submitters hear the same
//! thing. Mirrors orrery's `Entity.publish_as_staff`, whose post-save hook
//! does the telling there.

use anyhow::Error;
use common::message::log::PageChange;
use common::{
    service::{TriggerEvent, log_activity, trigger_email, trigger_email_to_address},
    shared::db,
};
use ulid::Ulid;

use crate::{
    contact::contact_person_email,
    entity::{Entity, EntityKind},
    peering::LoadStorePeered,
    project::field::{ProjectField, ProjectFieldValue},
};

/// Bulk-imported projects are managed by the importing account, so mail
/// about the submission itself has to go to the address the source system
/// supplied instead. Mirrors orrery's `Entity.IMPORT_ORIGINS`; the two lists
/// must stay identical.
pub const IMPORT_ORIGINS: [&str; 4] = [
    "CitSci.org",
    "API import by gregory.newman@colostate.edu",
    "Anecdata.org",
    "API import by dbailey@mdibl.org",
];

/// Where mail about a submission's review goes. Mirrors orrery's
/// `Entity.submitter_email`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Submitter {
    /// The account that created the project.
    Creator(Ulid),
    /// The contact address an import's source catalog supplied. The email
    /// still speaks for `creator`, the importing account, as orrery's does.
    Contact { creator: Ulid, address: String },
}

impl Submitter {
    /// Who to tell about the review of `project_id`, or None when there is
    /// nobody: a creator that is not an account, or an import whose catalog
    /// supplied no contact address.
    pub async fn of(project_id: Ulid) -> Result<Option<Submitter>, Error> {
        let project_id_str = project_id.to_string();

        let project = sqlx::query!(
            "SELECT page_creator_id, origin FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;

        let Ok(creator) = project.page_creator_id.parse::<Ulid>() else {
            tracing::warn!(
                %project_id,
                page_creator_id = project.page_creator_id,
                "Project's creator is not an account, so there is nobody to tell about its review"
            );
            return Ok(None);
        };

        if !IMPORT_ORIGINS.contains(&project.origin.as_str()) {
            return Ok(Some(Submitter::Creator(creator)));
        }

        let address = contact_person_email(project_id).await?;

        if address.is_empty() {
            tracing::warn!(
                %project_id,
                origin = project.origin,
                "Imported project has no contact address, so there is nobody to tell about its review"
            );
            return Ok(None);
        }

        Ok(Some(Submitter::Contact { creator, address }))
    }

    pub async fn notify(
        &self,
        trigger: TriggerEvent,
        merge_fields: &[(String, String)],
    ) -> Result<(), Error> {
        match self {
            Submitter::Creator(creator) => {
                trigger_email(trigger, *creator, Some(merge_fields)).await
            }
            Submitter::Contact { creator, address } => {
                trigger_email_to_address(trigger, *creator, address, Some(merge_fields)).await
            }
        }
    }
}

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

    let slug = sqlx::query_scalar!(
        "SELECT slug FROM entities WHERE kind = 'Project' AND target = ?",
        project_id_str
    )
    .fetch_one(db())
    .await?;

    let told = match Submitter::of(project_id).await {
        Ok(Some(submitter)) => {
            submitter
                .notify(
                    TriggerEvent::ProjectPublished,
                    &[
                        ("project_name".to_string(), name),
                        ("project_id".to_string(), project_id_str),
                        (
                            "project_link".to_string(),
                            format!("https://{}/projects/{}", env!("DOMAIN"), slug),
                        ),
                    ],
                )
                .await
        }
        Ok(None) => Ok(()),
        Err(err) => Err(err),
    };

    if let Err(err) = told {
        tracing::error!(%project_id, ?err, "Could not tell the submitter their project was published");
    }

    let _ = log_activity(
        None,
        PageChange::ProjectPublish,
        reviewer,
        Some(project_id),
        None,
    )
    .await;

    Ok(())
}
