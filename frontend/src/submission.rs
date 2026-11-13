//! A project or event's way onto SciStarter, and the mail about it.
//!
//! When something is first created, [`announce`] tells whoever will review
//! it -- SciStarter's reviewers, or the people of the exchange it came
//! through -- and tells its submitter it was received. Whatever publishes it,
//! then or later, goes through [`mark_published`], which records when it was
//! first made public and tells the submitter that one time only.
//!
//! Every one of those notices carries the merge fields [`notice_fields`]
//! gives, so a field one template uses can be used in any of them. This
//! mirrors orrery's `entity_post_save` and `Entity.notice_context`, where the
//! templates hydra's copies came from were written; the two sets of fields
//! must stay alike, so that a template edited in one works in the other.

use anyhow::Error;
use common::{
    message::user::{UserEmailInfo, UserEmailInfoRequest},
    service::{TriggerEvent, request, trigger_email, trigger_email_to_address},
    shared::db,
    timestring::TimeString,
};
use jiff::Timestamp;
use ulid::Ulid;

use crate::{
    contact::contact_person_email,
    entity::{Entity, EntityKind, EntityTarget},
    event::field::{EventField, EventFieldValue},
    exchange::{SubmissionExchange, access},
    peering::LoadStorePeered,
    project::field::{ProjectField, ProjectFieldValue},
};

/// What can be submitted to SciStarter and wait for review.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubmissionKind {
    Project,
    Event,
}

impl SubmissionKind {
    /// The kind of submission an entity of `kind` is the page of, if any.
    pub fn of(kind: EntityKind) -> Option<SubmissionKind> {
        match kind {
            EntityKind::Project => Some(SubmissionKind::Project),
            EntityKind::Event => Some(SubmissionKind::Event),
            _ => None,
        }
    }

    pub fn entity_kind(self) -> EntityKind {
        match self {
            SubmissionKind::Project => EntityKind::Project,
            SubmissionKind::Event => EntityKind::Event,
        }
    }

    /// What orrery's `EntityType.name` calls it, which a notice gives as
    /// `project_type`.
    pub fn type_name(self) -> &'static str {
        match self {
            SubmissionKind::Project => "Project",
            SubmissionKind::Event => "Event",
        }
    }

    /// Told to the submitter when it arrives to wait for review.
    fn submitted_trigger(self) -> TriggerEvent {
        match self {
            SubmissionKind::Project => TriggerEvent::ProjectSubmitted,
            SubmissionKind::Event => TriggerEvent::EventSubmitted,
        }
    }

    /// Told to the submitter when it is published.
    fn published_trigger(self) -> TriggerEvent {
        match self {
            SubmissionKind::Project => TriggerEvent::ProjectPublished,
            SubmissionKind::Event => TriggerEvent::EventPublished,
        }
    }

    /// Its page in the Engine Room, where staff review it.
    fn admin_link(self, id: Ulid) -> String {
        let section = match self {
            SubmissionKind::Project => "projects",
            SubmissionKind::Event => "events",
        };

        format!("https://{}/-admin/{section}/{id}", env!("DOMAIN"))
    }
}

/// How a new project or event reached SciStarter, which decides who hears
/// about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Submission {
    /// A person submitted it through a form. They are told it was received,
    /// and its reviewers hear of it.
    Submitted,
    /// It arrived from an imported catalog. The reviewers hear of it.
    Imported,
    /// A partner's API key added it. The reviewers hear of it in their own
    /// words, naming the key's account (`key_email`), unless that account is
    /// set not to tell them. The account itself is not told, as orrery does
    /// not tell it.
    Api {
        key_email: String,
        notify_reviewers: bool,
    },
}

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
    /// The account that created it.
    Creator(Ulid),
    /// The contact address an import's source catalog supplied. The email
    /// still speaks for `creator`, the importing account, as orrery's does.
    Contact { creator: Ulid, address: String },
}

impl Submitter {
    /// Who to tell about the review of the project or event `id`, or None
    /// when there is nobody: a creator that is not an account, or an import
    /// whose catalog supplied no contact address.
    ///
    /// Only projects keep a contact list, so only a project can be an
    /// import's; the catalogs in [`IMPORT_ORIGINS`] supply projects.
    pub async fn of(kind: SubmissionKind, id: Ulid) -> Result<Option<Submitter>, Error> {
        let id_str = id.to_string();

        let (page_creator_id, origin) = match kind {
            SubmissionKind::Project => {
                let row = sqlx::query!(
                    "SELECT page_creator_id, origin FROM projects WHERE project_id = ?",
                    id_str
                )
                .fetch_one(db())
                .await?;
                (row.page_creator_id, row.origin)
            }
            SubmissionKind::Event => {
                let row = sqlx::query!(
                    "SELECT page_creator_id, origin FROM events WHERE event_id = ?",
                    id_str
                )
                .fetch_one(db())
                .await?;
                (row.page_creator_id, row.origin)
            }
        };

        let Ok(creator) = page_creator_id.parse::<Ulid>() else {
            tracing::warn!(
                %id,
                ?kind,
                page_creator_id,
                "Creator is not an account, so there is nobody to tell about its review"
            );
            return Ok(None);
        };

        if kind != SubmissionKind::Project || !IMPORT_ORIGINS.contains(&origin.as_str()) {
            return Ok(Some(Submitter::Creator(creator)));
        }

        let address = contact_person_email(id).await?;

        if address.is_empty() {
            tracing::warn!(
                %id,
                origin,
                "Imported project has no contact address, so there is nobody to tell about its review"
            );
            return Ok(None);
        }

        Ok(Some(Submitter::Contact { creator, address }))
    }

    /// The account the submission is managed by, whom its notices name.
    pub fn creator(&self) -> Ulid {
        match self {
            Submitter::Creator(creator) | Submitter::Contact { creator, .. } => *creator,
        }
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

/// The merge fields every notice about the project or event `id` carries:
/// what it is and where it lives, and, given the `manager` the notice
/// concerns, who that is. Orrery's `Entity.notice_context` gives the same
/// set, except that `project_id` is hydra's id rather than orrery's.
pub async fn notice_fields(
    kind: SubmissionKind,
    id: Ulid,
    manager: Option<Ulid>,
) -> Result<Vec<(String, String)>, Error> {
    let id_str = id.to_string();
    let entity_kind = kind.entity_kind();

    let (name, slug) = match kind {
        SubmissionKind::Project => {
            let row = sqlx::query!(
                r#"SELECT p.name,
                          (SELECT n.slug FROM entities n
                            WHERE n.kind = ? AND n.target = p.project_id) AS "slug?"
                     FROM projects p WHERE p.project_id = ?"#,
                entity_kind,
                id_str
            )
            .fetch_one(db())
            .await?;
            (row.name, row.slug)
        }
        SubmissionKind::Event => {
            let row = sqlx::query!(
                r#"SELECT e.name,
                          (SELECT n.slug FROM entities n
                            WHERE n.kind = ? AND n.target = e.event_id) AS "slug?"
                     FROM events e WHERE e.event_id = ?"#,
                entity_kind,
                id_str
            )
            .fetch_one(db())
            .await?;
            (row.name, row.slug)
        }
    };

    let slug = slug.ok_or_else(|| anyhow::anyhow!("{kind:?} {id} has no page to link to"))?;

    let mut fields = vec![
        ("project_type".to_string(), kind.type_name().to_string()),
        ("project_name".to_string(), name),
        ("project_id".to_string(), id_str),
        (
            "project_link".to_string(),
            format!("https://{}{}", env!("DOMAIN"), entity_kind.path(&slug)),
        ),
        ("project_slug".to_string(), slug),
    ];

    if let Some(manager) = manager {
        let info: UserEmailInfo = request(
            "user.info.email_info",
            UserEmailInfoRequest { user_id: manager },
        )
        .await?;

        fields.push(("user_full_name".to_string(), private_name(&info)));
        fields.push(("user_email".to_string(), info.primary_email));
    }

    Ok(fields)
}

/// What a notice calls someone, as orrery's `User.private_name` does: their
/// full name, else their handle, else their id.
fn private_name(info: &UserEmailInfo) -> String {
    let full_name = format!("{} {}", info.first_name, info.last_name)
        .trim()
        .to_string();

    if !full_name.is_empty() {
        full_name
    } else if !info.handle.is_empty() {
        info.handle.clone()
    } else {
        info.user_id.to_string()
    }
}

/// A project or event just created, as [`announce`] needs to know it.
#[derive(Debug)]
pub struct Arrival<'a> {
    pub kind: SubmissionKind,
    pub id: Ulid,
    /// The account that created it.
    pub creator: Ulid,
    /// The exchange it was submitted through, which reviews it.
    pub exchange: Option<&'a SubmissionExchange>,
    /// Whether it was published as it arrived, by staff or by an exchange
    /// that publishes what its own people add.
    pub published: bool,
    pub submission: &'a Submission,
}

/// Tell everyone who should hear of a project or event just created, as
/// orrery's `entity_post_save` does on an entity's first save.
///
/// - Without an exchange, SciStarter's reviewers hear of it.
/// - Through an exchange, unless the exchange published it as it arrived,
///   each of the exchange's people hears of it.
/// - Its submitter hears that it was published, if it was, and otherwise --
///   when it came through a form -- that it was received.
///
/// Every notice is attempted whether or not another failed; the first
/// failure is returned once all have been tried.
pub async fn announce(arrival: Arrival<'_>) -> Result<(), Error> {
    let Arrival {
        kind,
        id,
        creator,
        exchange,
        published,
        submission,
    } = arrival;

    let reviewers = match exchange {
        None => tell_reviewers(kind, id, creator, submission).await,
        Some(exchange) if !published => tell_exchange(kind, id, exchange).await,
        Some(_) => Ok(()),
    };

    let submitter = if published {
        mark_published(kind, id).await
    } else if *submission == Submission::Submitted {
        tell_submitter(kind.submitted_trigger(), kind, id).await
    } else {
        Ok(())
    };

    reviewers.and(submitter)
}

/// Record that the project or event `id` is public, and if it never was
/// before, tell its submitter so, as orrery does when it stamps
/// `published_at`. Unpublishing and publishing it again tells nobody.
pub async fn mark_published(kind: SubmissionKind, id: Ulid) -> Result<(), Error> {
    let id_str = id.to_string();

    let published_at = match kind {
        SubmissionKind::Project => {
            sqlx::query_scalar!(
                "SELECT published_at FROM projects WHERE project_id = ?",
                id_str
            )
            .fetch_one(db())
            .await?
        }
        SubmissionKind::Event => {
            sqlx::query_scalar!(
                "SELECT published_at FROM events WHERE event_id = ?",
                id_str
            )
            .fetch_one(db())
            .await?
        }
    };

    if published_at.is_some() {
        return Ok(());
    }

    let now = Some(TimeString::now());
    match kind {
        SubmissionKind::Project => {
            ProjectField::new(id, ProjectFieldValue::PublishedAt(now))
                .store()
                .await?
        }
        SubmissionKind::Event => {
            EventField::new(id, EventFieldValue::PublishedAt(now))
                .store()
                .await?
        }
    }

    tell_submitter(kind.published_trigger(), kind, id).await
}

/// [`mark_published`] for `entity`, when it has just been stored public and
/// is the page of a project or event. Any other entity has no submitter.
pub async fn entity_published(entity: &Entity) -> Result<(), Error> {
    if !entity.publish() {
        return Ok(());
    }

    let (Some(kind), EntityTarget::Ulid(id)) = (SubmissionKind::of(*entity.kind()), entity.target())
    else {
        return Ok(());
    };

    mark_published(kind, *id).await
}

/// When something orrery holds was first made public, from what orrery says
/// of it.
///
/// Orrery's own `published_at` when it has one. Something public without
/// one was public before orrery kept it, so no earlier than its creation,
/// which stands in; this is the same rule the migration that added
/// `published_at` here applied to what was already public.
pub fn first_public(
    published_at: Option<Timestamp>,
    publish: bool,
    created: Timestamp,
) -> Option<TimeString> {
    published_at
        .or(publish.then_some(created))
        .map(TimeString::from)
}

async fn tell_submitter(trigger: TriggerEvent, kind: SubmissionKind, id: Ulid) -> Result<(), Error> {
    let Some(submitter) = Submitter::of(kind, id).await? else {
        return Ok(());
    };

    let fields = notice_fields(kind, id, Some(submitter.creator())).await?;
    submitter.notify(trigger, &fields).await
}

/// SciStarter's reviewers hear of something that is not an exchange's to
/// review: in the general notice, or in the partner API's own.
async fn tell_reviewers(
    kind: SubmissionKind,
    id: Ulid,
    creator: Ulid,
    submission: &Submission,
) -> Result<(), Error> {
    let mut fields = notice_fields(kind, id, Some(creator)).await?;
    fields.push(("admin_link".to_string(), kind.admin_link(id)));

    match submission {
        Submission::Submitted | Submission::Imported => {
            trigger_email(TriggerEvent::ProjectCreatedStaff, creator, Some(&fields)).await
        }
        Submission::Api {
            key_email,
            notify_reviewers,
        } => {
            if !notify_reviewers {
                return Ok(());
            }

            let name = fields
                .iter()
                .find(|(field, _)| field == "project_name")
                .map(|(_, name)| name.clone())
                .unwrap_or_default();
            fields.push(("name".to_string(), name));
            fields.push(("key_email".to_string(), key_email.clone()));

            trigger_email(TriggerEvent::ProjectCreatedApi, creator, Some(&fields)).await
        }
    }
}

/// Each of an exchange's owner and staff hears of something waiting in its
/// approval queue. As in orrery, the notice names no submitter.
async fn tell_exchange(
    kind: SubmissionKind,
    id: Ulid,
    exchange: &SubmissionExchange,
) -> Result<(), Error> {
    let mut fields = notice_fields(kind, id, None).await?;
    fields.push(("exchange_name".to_string(), exchange.name.clone()));
    fields.push((
        "exchange_link".to_string(),
        format!("https://{}/exchange/{}", env!("DOMAIN"), exchange.path_id),
    ));

    let mut result = Ok(());

    for (person, _role) in access::people(Ulid::from_string(&exchange.exchange_id)?).await? {
        let told = trigger_email(TriggerEvent::ProjectCreatedExchange, person, Some(&fields)).await;
        result = result.and(told);
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert2::check;

    fn info(handle: &str, first_name: &str, last_name: &str) -> UserEmailInfo {
        UserEmailInfo {
            user_id: Ulid::from_parts(0, 7),
            handle: handle.to_string(),
            first_name: first_name.to_string(),
            last_name: last_name.to_string(),
            primary_email: "ada@example.invalid".to_string(),
            verified: true,
            badge_allowed: false,
            accept_emails: true,
        }
    }

    fn at(time: &str) -> Timestamp {
        time.parse().unwrap()
    }

    #[test]
    fn orrerys_first_public_time_is_kept() {
        let published = at("2025-03-02T10:00:00Z");
        let created = at("2025-03-01T09:00:00Z");

        check!(first_public(Some(published), true, created) == Some(published.into()));
        check!(first_public(Some(published), false, created) == Some(published.into()));
    }

    #[test]
    fn something_public_without_a_first_public_time_was_public_since_it_was_created() {
        let created = at("2025-03-01T09:00:00Z");

        check!(first_public(None, true, created) == Some(created.into()));
        check!(first_public(None, false, created) == None);
    }

    #[test]
    fn a_notice_calls_someone_by_their_full_name_first() {
        check!(private_name(&info("ada", "Ada", "Organizer")) == "Ada Organizer");
        check!(private_name(&info("ada", "Ada", "")) == "Ada");
        check!(private_name(&info("ada", "", "Organizer")) == "Organizer");
    }

    #[test]
    fn without_a_name_a_notice_falls_back_to_the_handle_then_the_id() {
        check!(private_name(&info("ada", "", "")) == "ada");
        check!(private_name(&info("", " ", "")) == Ulid::from_parts(0, 7).to_string());
    }
}
