//! Joining people to the projects their contributions are credited to.
//!
//! A contribution credited to a project is the best evidence there is that
//! its contributor takes part in the project, so it joins them to it
//! ([`JoinSource::Participation`]). Only rows that report more than joining
//! count: a row that only says someone joined is not a contribution.
//!
//! Only the primary server does this. Before cutover the joins are orrery's,
//! copied here by sync, and the figures read from them stay orrery's.

use std::collections::HashSet;

use common::message::user::{JoinSource, ProjectJoinRequest, ProjectJoinResponse};
use log_service::database::tables::ParticipationRow;
use ulid::Ulid;

/// The contributors one run of an importer has already joined to a project,
/// so that a run crediting someone many times asks once.
#[derive(Default)]
pub struct ContributorJoins {
    joined: HashSet<(Ulid, String)>,
}

impl ContributorJoins {
    /// Join the account `row` credits to the project it credits, unless
    /// this run already has or the row is not a contribution.
    ///
    /// Not fatal: the contribution is recorded either way, and the next one
    /// the person makes asks again.
    pub async fn join(&mut self, row: &ParticipationRow) {
        if !common::primary::PRIMARY || row.joins_project() {
            return;
        }

        let Some(user_id) = row.user_id.as_deref() else {
            return;
        };

        let user_id: Ulid = match user_id.parse() {
            Ok(user_id) => user_id,
            Err(e) => {
                eprintln!("Contribution credited to unreadable account {user_id:?}: {e}");
                return;
            }
        };

        if !self.joined.insert((user_id, row.entity_id.clone())) {
            return;
        }

        if let Err(e) = common::service::request::<ProjectJoinResponse>(
            "user.joins.create",
            ProjectJoinRequest {
                user_id,
                project_id: row.entity_id.clone(),
                source: JoinSource::Participation,
            },
        )
        .await
        {
            eprintln!(
                "Contribution by {user_id} did not join them to {}: {e}",
                row.entity_id
            );
        }
    }
}

/// [`ContributorJoins::join`], for a single contribution.
pub async fn join_contributor(row: &ParticipationRow) {
    ContributorJoins::default().join(row).await
}
