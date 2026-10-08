//! What an exchange records about how it is used, for its data page.
//!
//! Each record names a page and what was done on it, spelled the way
//! orrery's `core_exchangeevent` spells them, because orrery's history is
//! carried into the same table and both are read by the same queries.
//! `page_id` is the ULID of the project or event concerned.

use anyhow::Error;
use common::{message::log::ExchangeEvent, service::send, timestring::TimeString};
use ulid::Ulid;

/// The page an exchange event happened on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackedPage {
    Home,
    Results,
    Project,
    Event,
    Org,
    Data,
}

impl TrackedPage {
    pub fn as_str(self) -> &'static str {
        match self {
            TrackedPage::Home => "home",
            TrackedPage::Results => "results",
            TrackedPage::Project => "project",
            TrackedPage::Event => "event",
            TrackedPage::Org => "org",
            TrackedPage::Data => "data",
        }
    }
}

/// What was done.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackedAction {
    View,
    Search,
    Save,
    Vote,
    /// Followed a project or event out to its own website -- a referral.
    Visit,
    /// Asked to follow it out, but it has no website to go to.
    VisitMissing,
    Approve,
    Reject,
}

impl TrackedAction {
    pub fn as_str(self) -> &'static str {
        match self {
            TrackedAction::View => "view",
            TrackedAction::Search => "search",
            TrackedAction::Save => "save",
            TrackedAction::Vote => "vote",
            TrackedAction::Visit => "visit",
            TrackedAction::VisitMissing => "visit-missing",
            TrackedAction::Approve => "approve",
            TrackedAction::Reject => "reject",
        }
    }
}

/// Record one exchange event. Everyone is recorded, signed in or not; the
/// device and interaction ids are what tell visitors and visits apart.
#[allow(clippy::too_many_arguments)]
pub async fn record(
    exchange_id: Ulid,
    user_id: Option<Ulid>,
    device: impl AsRef<str>,
    interaction: impl AsRef<str>,
    page: TrackedPage,
    page_id: Option<Ulid>,
    action: TrackedAction,
    extra: Option<String>,
) -> Result<(), Error> {
    send(
        "log.exchange_event",
        ExchangeEvent {
            event_id: Ulid::new().to_string(),
            exchange_id: exchange_id.to_string(),
            user_id: user_id.map(|u| u.to_string()),
            device_id: device.as_ref().to_string(),
            interaction_id: interaction.as_ref().to_string(),
            when_occurred: TimeString::now().to_string(),
            page: page.as_str().to_string(),
            page_id: page_id.map(|id| id.to_string()),
            action: action.as_str().to_string(),
            extra,
        },
    )
    .await
}
