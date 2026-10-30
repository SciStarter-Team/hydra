pub mod ala;
pub mod all;
pub mod choice_ids;
pub mod field;
pub mod field_source;
pub mod inaturalist;
pub mod investigator;
pub mod linked_platform;
pub mod location;
pub mod publish;
pub mod reference;

/// Tags whose name begins with `_` (e.g. `_nasa`) are internal/system tags used
/// for filtering and attribution. They are hidden from user-facing displays,
/// though they remain in the data model and in editor-facing views.
pub fn is_hidden_tag(tag: &str) -> bool {
    tag.starts_with('_')
}

#[derive(
    Clone, Copy, Default, Debug, PartialEq, sqlx::Type, serde::Serialize, serde::Deserialize,
)]
pub enum ProjectStatus {
    Pending,
    #[default]
    Active,
    #[sqlx(rename = "Periodically Active")]
    PeriodicallyActive,
    Hiatus,
    Complete,
    Hidden,
    Expired,
}

impl ProjectStatus {
    /// The statuses a search lists when it is asked to include inactive
    /// projects.
    ///
    /// Every status that describes how far along a project is belongs here.
    /// Hidden does not: it is a decision that the project is not to be
    /// listed at all, and it is also where the orrery sync puts any status
    /// it does not recognise -- orrery's legacy 999 "deleted" marker among
    /// them. Mirrors `ProjectStatus::LISTED_WHEN_INACTIVE` in orrery's
    /// src/common.rs, which has no Expired.
    pub const LISTED_WHEN_INACTIVE: [ProjectStatus; 6] = [
        ProjectStatus::Pending,
        ProjectStatus::Active,
        ProjectStatus::PeriodicallyActive,
        ProjectStatus::Hiatus,
        ProjectStatus::Complete,
        ProjectStatus::Expired,
    ];

    /// The statuses the project finder lists unless asked to show finished
    /// projects as well, when it lists [`Self::LISTED_WHEN_INACTIVE`].
    ///
    /// Every status a project can still be joined in. Mirrors the default
    /// `project__status__in=(0, 1, 2, 3)` of orrery's `find_projects`.
    pub const LISTED_BY_FINDER: [ProjectStatus; 4] = [
        ProjectStatus::Pending,
        ProjectStatus::Active,
        ProjectStatus::PeriodicallyActive,
        ProjectStatus::Hiatus,
    ];

    /// The status's name, as the database and orrery spell it.
    pub fn label(self) -> &'static str {
        match self {
            ProjectStatus::Pending => "Pending",
            ProjectStatus::Active => "Active",
            ProjectStatus::PeriodicallyActive => "Periodically Active",
            ProjectStatus::Hiatus => "Hiatus",
            ProjectStatus::Complete => "Complete",
            ProjectStatus::Hidden => "Hidden",
            ProjectStatus::Expired => "Expired",
        }
    }

    /// orrery's number for the status (`Project.status`), or None for
    /// Expired, which orrery has no status for: it reads a project's end
    /// date instead.
    pub fn orrery_code(self) -> Option<i64> {
        match self {
            ProjectStatus::Pending => Some(0),
            ProjectStatus::Active => Some(1),
            ProjectStatus::PeriodicallyActive => Some(2),
            ProjectStatus::Hiatus => Some(3),
            ProjectStatus::Complete => Some(4),
            ProjectStatus::Hidden => Some(5),
            ProjectStatus::Expired => None,
        }
    }
}

/// Narrow a query over `projects p` to the projects that may be listed, and
/// to those of them whose status is one of `statuses`.
///
/// A project flagged, marked a duplicate or marked a test is never listed,
/// whatever its status: not by the finder, not on an exchange, not in the
/// sitemap, not by the public MCP tools, and not among the catalogue figures
/// of `/data`. Orrery's `find_projects` and exchange search apply the same
/// rule. Publication is what keeps a submission out until it is approved, so
/// the draft flag -- left set on some long-published projects, and not
/// cleared by exchange approval -- is not consulted.
pub fn push_listing_rule(qb: &mut sqlx::QueryBuilder<sqlx::Sqlite>, statuses: &[ProjectStatus]) {
    qb.push(" AND p.flagged = 0 AND p.duplicate = 0 AND p.is_test = 0");
    qb.push(" AND p.status IN (");
    let mut separated = qb.separated(", ");
    for status in statuses {
        separated.push_bind(*status);
    }
    qb.push(")");
}

#[derive(
    Clone, Copy, Default, Debug, PartialEq, sqlx::Type, serde::Serialize, serde::Deserialize,
)]
pub enum ProjectPromotionTime {
    #[default]
    #[sqlx(rename = "Promote at any time")]
    PromoteAtAnyTime,
    #[sqlx(rename = "Do not promote")]
    DoNotPromote,
    #[sqlx(rename = "Prefer promotion on the target date")]
    PreferOnTargetDate,
}

/// Whether reviewers are offered a rejection reason on a project, picking
/// which emails the submitter (`TriggerEvent::ProjectRejected`). The two go
/// together: the picker does nothing but send that email.
///
/// Mirrors orrery's `SUBMISSION_REJECTION_NOTICES` setting, and like it is
/// off. What was sent while it was on stays recorded on each project.
pub const SUBMISSION_REJECTION_NOTICES: bool = false;

/// Why a reviewer declined to publish a submission.
///
/// Mirrored in orrery as `Entity.RejectionReason` (core/models.py) — the two
/// must stay in sync, [`ProjectRejectionReason::message`] text included, since
/// they are the same decision presented to the same submitters.
#[derive(
    Clone, Copy, Default, Debug, PartialEq, Eq, sqlx::Type, serde::Serialize, serde::Deserialize,
)]
pub enum ProjectRejectionReason {
    #[default]
    None,
    Participation,
    Science,
    Spam,
}

impl ProjectRejectionReason {
    /// The sentence the submitter reads in the rejection email.
    pub fn message(&self) -> &'static str {
        match self {
            Self::None => "",
            Self::Participation => "The volunteer participation opportunity is missing or unclear.",
            Self::Science => "The citizen science component is missing or unclear.",
            Self::Spam => {
                "Typos or other issues caused the submission to be flagged as possible spam."
            }
        }
    }

    /// The value the admin form posts for this reason.
    pub fn form_value(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Participation => "participation",
            Self::Science => "science",
            Self::Spam => "spam",
        }
    }

    pub fn from_form_value(value: &str) -> Self {
        match value {
            "participation" => Self::Participation,
            "science" => Self::Science,
            "spam" => Self::Spam,
            _ => Self::None,
        }
    }
}

#[derive(
    Clone, Copy, Default, Debug, PartialEq, sqlx::Type, serde::Serialize, serde::Deserialize,
)]
pub enum ProjectParticipationDegree {
    #[default]
    Collegial,
    #[sqlx(rename = "Co-Created")]
    CoCreated,
    Collaborative,
    Contributory,
    Contractual,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The wording a submitter reads. It is duplicated in orrery's
    /// `Entity.RejectionReason` (orrery/core/models.py); if you change a
    /// sentence here, change it there in the same breath.
    #[test]
    fn every_reason_has_the_agreed_wording() {
        assert_eq!(
            ProjectRejectionReason::Participation.message(),
            "The volunteer participation opportunity is missing or unclear."
        );
        assert_eq!(
            ProjectRejectionReason::Science.message(),
            "The citizen science component is missing or unclear."
        );
        assert_eq!(
            ProjectRejectionReason::Spam.message(),
            "Typos or other issues caused the submission to be flagged as possible spam."
        );
    }

    #[test]
    fn not_rejected_says_nothing() {
        assert_eq!(ProjectRejectionReason::None.message(), "");
    }

    #[test]
    fn form_values_round_trip() {
        for reason in [
            ProjectRejectionReason::None,
            ProjectRejectionReason::Participation,
            ProjectRejectionReason::Science,
            ProjectRejectionReason::Spam,
        ] {
            assert_eq!(
                ProjectRejectionReason::from_form_value(reason.form_value()),
                reason
            );
        }
    }

    /// A picker posting something unexpected must not silently reject a
    /// submission for a reason nobody chose.
    #[test]
    fn an_unknown_form_value_is_not_a_rejection() {
        assert_eq!(
            ProjectRejectionReason::from_form_value("wat"),
            ProjectRejectionReason::None
        );
    }
}
