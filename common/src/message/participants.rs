//! A project's Participant Manager: who has joined or saved the project, for
//! the people who manage it, as orrery's Volunteer Manager listed them.
//!
//! The user service holds the roster and the stars managers put on it, the
//! log service what each participant has contributed, and the forum service
//! when the project last wrote to each of them.

use serde::{Deserialize, Serialize};
use ulid::Ulid;

/// How someone came to be on a project's roster. Someone who has both
/// joined and saved it is listed once, as having joined.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ParticipantKind {
    Joined,
    Saved,
}

impl ParticipantKind {
    pub const ALL: [ParticipantKind; 2] = [ParticipantKind::Joined, ParticipantKind::Saved];

    pub fn key(self) -> &'static str {
        match self {
            ParticipantKind::Joined => "joined",
            ParticipantKind::Saved => "saved",
        }
    }

    pub fn from_key(key: &str) -> Option<ParticipantKind> {
        ParticipantKind::ALL
            .into_iter()
            .find(|kind| kind.key() == key)
    }

    pub fn label(self) -> &'static str {
        match self {
            ParticipantKind::Joined => "Joined",
            ParticipantKind::Saved => "Saved",
        }
    }
}

/// `user.roster.list`: one page of a project's roster, most recent first.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RosterRequest {
    pub project_id: Ulid,
    /// Only those who came this way, or everyone.
    pub kind: Option<ParticipantKind>,
    /// Only the starred, or only the unstarred, or everyone.
    pub starred: Option<bool>,
    /// Only those whose name or handle contains this; empty for everyone.
    pub search: String,
    pub offset: u32,
    pub limit: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RosterResponse {
    /// How many match, on every page.
    pub total: u32,
    pub participants: Vec<Participant>,
}

/// One person on a project's roster.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Participant {
    pub user_id: Ulid,
    pub kind: ParticipantKind,
    pub name: String,
    /// When they joined or saved the project.
    pub since: String,
    pub starred: bool,
    /// When they last agreed to the project's consent statement.
    pub consented: Option<String>,
}

/// `user.roster.star`: star or unstar someone on a project's roster.
/// Answered with whether they are on it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StarRequest {
    pub project_id: Ulid,
    pub user_id: Ulid,
    pub kind: ParticipantKind,
    pub starred: bool,
}

/// `user.roster.members`: which of `user_ids` are on the project's roster,
/// the only people its managers may write to from it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RosterMembersRequest {
    pub project_id: Ulid,
    pub user_ids: Vec<Ulid>,
}

/// `log.project_participants`: what each of `user_ids` has contributed to
/// the project, as of the daily stats snapshot. Those who have contributed
/// nothing are left out.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContributionsRequest {
    pub project_id: Ulid,
    pub user_ids: Vec<Ulid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParticipantContributions {
    pub user_id: Ulid,
    pub contributions: i64,
    /// When they last contributed, RFC 3339.
    pub last: String,
}

/// `forum.messaging.last_contacted`: when `from_id` last wrote to each of
/// `to_ids`. Those it never wrote to are left out.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LastContactedRequest {
    pub from_id: Ulid,
    pub to_ids: Vec<Ulid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LastContacted {
    pub user_id: Ulid,
    pub when: String,
}
