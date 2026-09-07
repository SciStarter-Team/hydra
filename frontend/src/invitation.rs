//! One-time invitation URLs: `/invitations/<id>` performs an action on behalf
//! of whoever opens it, once they are signed in.
//!
//! orrery mints links of the same shape from the same staff page. The two
//! systems keep their own rows rather than sharing a table, so an invitation
//! is stored as data — a kind and its arguments — instead of as the
//! serialized callable orrery's older invitations use, which only Python
//! could ever run. That is what lets both sides describe the same invitation.
//!
//! A link is spent by the first person to accept it: the row is deleted in
//! the same statement that reads it, so two people racing to open one link
//! cannot both win.

use anyhow::Error;
use common::{
    message::user::{UserDetails, UserDetailsGetRequest, UserDetailsResponse},
    service::request,
    shared::db,
    timestring::TimeString,
};
use poem::{IntoResponse, Response, handler, web::Path, web::Redirect};
use ulid::Ulid;

use crate::{
    contact::{Contact, ContactRole},
    event::field::{EventField, EventFieldType, EventFieldValue},
    middleware::authorization::Authenticated,
    peering::LoadStorePeered,
    project::field::{ProjectField, ProjectFieldType, ProjectFieldValue},
};

/// A page whose creator is one of these is unclaimed in practice: it was
/// imported or created on someone's behalf and is waiting for its real
/// steward. Handing one of these over needs no further authorization.
const PLACEHOLDER_OWNER_EMAILS: [&str; 1] = ["info@scistarter.org"];

/// The two kinds of page whose stewardship can be handed over. Both carry a
/// page creator, a set of page editors, and contact records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PageKind {
    Project,
    Event,
}

impl PageKind {
    /// The `entities.kind` value naming this kind of page.
    pub fn entity_kind(self) -> &'static str {
        match self {
            PageKind::Project => "Project",
            PageKind::Event => "Event",
        }
    }

    pub fn from_entity_kind(kind: &str) -> Option<Self> {
        match kind {
            "Project" => Some(PageKind::Project),
            "Event" => Some(PageKind::Event),
            _ => None,
        }
    }
}

impl std::fmt::Display for PageKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.entity_kind())
    }
}

/// Hand one page to whoever accepts the invitation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ClaimPage {
    pub page_id: Ulid,
    pub page_kind: PageKind,
    /// Whether the page may be taken from an owner who is a real person.
    /// Minting one of these is a deliberate act at the staff page, because
    /// accepting it takes someone's page away.
    #[serde(default)]
    pub allow_transfer: bool,
}

/// What an invitation does when accepted.
///
/// The variant name is stored in the `kind` column and its contents in
/// `payload`, so a row stays legible to a reader — and to orrery — without
/// knowing this enum's memory layout.
#[derive(Debug, Clone)]
pub enum Invitation {
    ClaimPage(ClaimPage),
}

impl Invitation {
    const CLAIM_PAGE: &'static str = "claim-page";

    fn kind(&self) -> &'static str {
        match self {
            Invitation::ClaimPage(_) => Self::CLAIM_PAGE,
        }
    }

    fn payload(&self) -> Result<String, Error> {
        Ok(match self {
            Invitation::ClaimPage(claim) => serde_json::to_string(claim)?,
        })
    }

    fn from_stored(kind: &str, payload: &str) -> Result<Self, Error> {
        match kind {
            Self::CLAIM_PAGE => Ok(Invitation::ClaimPage(serde_json::from_str(payload)?)),
            other => Err(anyhow::anyhow!("Unknown invitation kind `{other}`")),
        }
    }

    /// Store this invitation and return the URL that accepts it.
    pub async fn mint(&self) -> Result<String, Error> {
        let invitation_id = Ulid::new().to_string();
        let kind = self.kind();
        let payload = self.payload()?;
        let created = TimeString::now().to_string();

        sqlx::query!(
            "INSERT INTO invitations (invitation_id, kind, payload, created)
             VALUES (?, ?, ?, ?)",
            invitation_id,
            kind,
            payload,
            created,
        )
        .execute(db())
        .await?;

        Ok(format!("/invitations/{invitation_id}"))
    }

    /// Read and spend the invitation with this id.
    ///
    /// One statement, so exactly one caller can take a given invitation
    /// however many arrive at once.
    async fn take(invitation_id: Ulid) -> Result<Option<Self>, Error> {
        let invitation_id = invitation_id.to_string();

        let row = sqlx::query!(
            "DELETE FROM invitations WHERE invitation_id = ?
             RETURNING kind, payload",
            invitation_id
        )
        .fetch_optional(db())
        .await?;

        row.map(|row| Self::from_stored(&row.kind, &row.payload))
            .transpose()
    }
}

/// The name and address to list on a page whose stewardship has just changed
/// hands. Nothing about the page describes its new steward, so this is read
/// from the account that accepted the invitation.
async fn steward_details(user_id: Ulid) -> Result<UserDetails, Error> {
    let response: UserDetailsResponse =
        request("user.admin.user_get", UserDetailsGetRequest { user_id }).await?;

    Ok(response.user)
}

/// Who owns this page now, if anybody.
///
/// Read directly rather than through the field readers, which insist on
/// parsing the column as a ULID and so cannot describe a page that has no
/// creator at all.
pub async fn page_owner(page_id: Ulid, page_kind: PageKind) -> Result<Option<Ulid>, Error> {
    let page_id = page_id.to_string();

    let stored = match page_kind {
        PageKind::Project => {
            sqlx::query_scalar!(
                "SELECT page_creator_id FROM projects WHERE project_id = ?",
                page_id
            )
            .fetch_optional(db())
            .await?
        }
        PageKind::Event => {
            sqlx::query_scalar!(
                "SELECT page_creator_id FROM events WHERE event_id = ?",
                page_id
            )
            .fetch_optional(db())
            .await?
        }
    };

    Ok(stored.and_then(|stored| Ulid::from_string(&stored).ok()))
}

/// Whether the page named by an invitation is still here. A page can be
/// deleted between minting a link and someone opening it, and the field
/// writers would otherwise happily record a steward for a page that is gone.
pub async fn page_exists(page_id: Ulid, page_kind: PageKind) -> Result<bool, Error> {
    let page_id = page_id.to_string();

    Ok(match page_kind {
        PageKind::Project => sqlx::query_scalar!(
            "SELECT 1 AS found FROM projects WHERE project_id = ?",
            page_id
        )
        .fetch_optional(db())
        .await?
        .is_some(),
        PageKind::Event => {
            sqlx::query_scalar!("SELECT 1 AS found FROM events WHERE event_id = ?", page_id)
                .fetch_optional(db())
                .await?
                .is_some()
        }
    })
}

/// Whether this page is waiting for a steward, so that handing it over takes
/// nothing away from anybody.
pub async fn is_unclaimed(page_id: Ulid, page_kind: PageKind) -> Result<bool, Error> {
    let Some(owner) = page_owner(page_id, page_kind).await? else {
        return Ok(true);
    };

    let Ok(owner) = steward_details(owner).await else {
        // An owner id naming no account is not a steward either.
        return Ok(true);
    };

    Ok(PLACEHOLDER_OWNER_EMAILS.contains(&owner.primary_email.as_str()))
}

/// The contacts a page should list once `steward` holds it.
///
/// The manager and contact-person records are replaced outright rather than
/// merged. They describe a person, and the page is changing hands, so leaving
/// the previous steward's affiliation, phone or postal address beside the new
/// steward's name would publish a record that was never true of anybody.
/// Technical and notification addresses are left alone: they name a system or
/// a mailbox rather than the person responsible for the page.
fn steward_contacts(existing: &[Contact], steward: &UserDetails) -> Vec<Contact> {
    let mut contacts: Vec<Contact> = existing
        .iter()
        .filter(|contact| {
            matches!(
                contact.role,
                ContactRole::Technical | ContactRole::Notification
            )
        })
        .cloned()
        .collect();

    for role in [ContactRole::Manager, ContactRole::ContactPerson] {
        contacts.push(Contact {
            contact_id: Ulid::new(),
            role,
            name: Some(steward.display_name()),
            affiliation: None,
            email: steward.primary_email.clone(),
            phone: None,
            address: None,
        });
    }

    contacts
}

/// `editors` with `steward` added, if they were not already an editor.
fn with_editor(editors: &[Ulid], steward: Ulid) -> Vec<Ulid> {
    let mut editors = editors.to_vec();

    if !editors.contains(&steward) {
        editors.push(steward);
    }

    editors
}

/// Hand `page` to `steward`: page creator, page editor, and the manager and
/// contact person the page lists.
pub async fn grant_page_stewardship(
    page_id: Ulid,
    page_kind: PageKind,
    steward_id: Ulid,
) -> Result<(), Error> {
    let steward = steward_details(steward_id).await?;

    match page_kind {
        PageKind::Project => {
            let editors = match ProjectField::load((page_id, ProjectFieldType::PageEditorIds))
                .await?
                .field
            {
                ProjectFieldValue::PageEditorIds(editors) => editors,
                other => return Err(anyhow::anyhow!("Expected page editor ids, got {other:?}")),
            };

            let contacts = match ProjectField::load((page_id, ProjectFieldType::Contacts))
                .await?
                .field
            {
                ProjectFieldValue::Contacts(contacts) => contacts,
                other => return Err(anyhow::anyhow!("Expected contacts, got {other:?}")),
            };

            for field in [
                ProjectFieldValue::PageCreatorId(steward_id),
                ProjectFieldValue::PageEditorIds(
                    with_editor(&editors, steward_id).into_boxed_slice(),
                ),
                ProjectFieldValue::Contacts(
                    steward_contacts(&contacts, &steward).into_boxed_slice(),
                ),
            ] {
                ProjectField {
                    project_id: page_id,
                    field,
                    source: None,
                }
                .store()
                .await?;
            }
        }
        PageKind::Event => {
            let editors = match EventField::load((page_id, EventFieldType::PageEditorIds))
                .await?
                .field
            {
                EventFieldValue::PageEditorIds(editors) => editors,
                other => return Err(anyhow::anyhow!("Expected page editor ids, got {other:?}")),
            };

            let contacts = match EventField::load((page_id, EventFieldType::Contacts))
                .await?
                .field
            {
                EventFieldValue::Contacts(contacts) => contacts,
                other => return Err(anyhow::anyhow!("Expected contacts, got {other:?}")),
            };

            for field in [
                EventFieldValue::PageCreatorId(steward_id),
                EventFieldValue::PageEditorIds(
                    with_editor(&editors, steward_id).into_boxed_slice(),
                ),
                EventFieldValue::Contacts(steward_contacts(&contacts, &steward).into_boxed_slice()),
            ] {
                EventField {
                    event_id: page_id,
                    field,
                }
                .store()
                .await?;
            }
        }
    }

    Ok(())
}

/// The path of the page itself, to send a new steward to once they have it.
pub async fn page_path(page_id: Ulid) -> Result<String, Error> {
    let page_id = page_id.to_string();

    let slug = sqlx::query_scalar!("SELECT slug FROM entities WHERE target = ?", page_id)
        .fetch_optional(db())
        .await?;

    Ok(slug
        .map(|slug| format!("/{slug}"))
        .unwrap_or_else(|| "/".to_string()))
}

async fn accept_claim(claim: ClaimPage, steward_id: Ulid) -> Result<Response, Error> {
    if !page_exists(claim.page_id, claim.page_kind).await? {
        tracing::warn!(
            "claim-page invitation names missing {} {}",
            claim.page_kind,
            claim.page_id,
        );
        return Ok(Redirect::see_other("/?error=project-does-not-exist").into_response());
    }

    let owner = page_owner(claim.page_id, claim.page_kind).await?;

    if owner != Some(steward_id)
        && !claim.allow_transfer
        && !is_unclaimed(claim.page_id, claim.page_kind).await?
    {
        tracing::warn!(
            "claim-page invitation for {} {} refused: already owned",
            claim.page_kind,
            claim.page_id,
        );
        return Ok(Redirect::see_other("/?error=project-already-claimed").into_response());
    }

    tracing::info!(
        "claim-page: {} {} granted to {}",
        claim.page_kind,
        claim.page_id,
        steward_id,
    );

    grant_page_stewardship(claim.page_id, claim.page_kind, steward_id).await?;

    Ok(Redirect::see_other(page_path(claim.page_id).await?).into_response())
}

/// GET `/invitations/:invitation_id` — accept an invitation.
///
/// A signed-out visitor is sent to the login and signup page and returned
/// here afterwards, which is the whole point of the link: one URL that turns
/// a stranger into the steward of a page.
#[handler]
#[tracing::instrument(skip(user_info))]
pub async fn accept_invitation(
    Path(invitation_id): Path<String>,
    Authenticated(user_info): Authenticated,
) -> Result<Response, Error> {
    let Some(user_info) = user_info else {
        return Ok(
            Redirect::see_other(format!("/login?next=/invitations/{invitation_id}"))
                .into_response(),
        );
    };

    let Ok(invitation_id) = invitation_id.parse::<Ulid>() else {
        return Ok(Redirect::see_other("/?error=invalid-invitation").into_response());
    };

    let Some(invitation) = Invitation::take(invitation_id).await? else {
        return Ok(Redirect::see_other("/?error=invalid-invitation").into_response());
    };

    match invitation {
        Invitation::ClaimPage(claim) => accept_claim(claim, user_info.user_id).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(name: &str, email: &str) -> UserDetails {
        let (first_name, last_name) = name.split_once(' ').unwrap_or((name, ""));

        UserDetails {
            user_id: Ulid::new(),
            handle: String::new(),
            first_name: first_name.to_string(),
            last_name: last_name.to_string(),
            primary_email: email.to_string(),
            is_active: true,
            is_staff: false,
            is_superuser: false,
            is_page_manager: false,
            verified: true,
            date_joined: String::new(),
            emails: Vec::new(),
            orcid: None,
        }
    }

    fn contact(role: ContactRole, name: &str, email: &str) -> Contact {
        Contact {
            contact_id: Ulid::new(),
            role,
            name: Some(name.to_string()),
            affiliation: Some("Somewhere Else University".to_string()),
            email: email.to_string(),
            phone: Some("555-0100".to_string()),
            address: Some("12 Old Lab Road".to_string()),
        }
    }

    #[test]
    fn a_claim_survives_the_database() {
        let minted = Invitation::ClaimPage(ClaimPage {
            page_id: Ulid::new(),
            page_kind: PageKind::Event,
            allow_transfer: true,
        });

        let stored = Invitation::from_stored(minted.kind(), &minted.payload().unwrap()).unwrap();

        let (Invitation::ClaimPage(minted), Invitation::ClaimPage(stored)) = (minted, stored);

        assert_eq!(minted.page_id, stored.page_id);
        assert_eq!(minted.page_kind, stored.page_kind);
        assert_eq!(minted.allow_transfer, stored.allow_transfer);
    }

    #[test]
    fn an_unknown_kind_is_refused() {
        assert!(Invitation::from_stored("no-such-handler", "{}").is_err());
    }

    /// orrery mints links for the same two kinds of page and names them with
    /// these same strings in its `entities` rows.
    #[test]
    fn page_kinds_match_the_entity_rows() {
        for kind in [PageKind::Project, PageKind::Event] {
            assert_eq!(PageKind::from_entity_kind(kind.entity_kind()), Some(kind));
        }

        assert_eq!(PageKind::from_entity_kind("Page"), None);
    }

    #[test]
    fn the_new_steward_becomes_manager_and_contact() {
        let contacts =
            steward_contacts(&[], &user("Rosalind Franklin", "rosalind@example.invalid"));

        let roles: Vec<_> = contacts.iter().map(|c| c.role).collect();

        assert!(matches!(roles[0], ContactRole::Manager));
        assert!(matches!(roles[1], ContactRole::ContactPerson));

        for contact in &contacts {
            assert_eq!(contact.name.as_deref(), Some("Rosalind Franklin"));
            assert_eq!(contact.email, "rosalind@example.invalid");
        }
    }

    #[test]
    fn the_previous_steward_is_not_left_beside_the_new_one() {
        let existing = [
            contact(
                ContactRole::Manager,
                "Dorothy Hodgkin",
                "dorothy@example.invalid",
            ),
            contact(
                ContactRole::ContactPerson,
                "Dorothy Hodgkin",
                "dorothy@example.invalid",
            ),
        ];

        let contacts = steward_contacts(
            &existing,
            &user("Rosalind Franklin", "rosalind@example.invalid"),
        );

        assert_eq!(contacts.len(), 2);

        for contact in &contacts {
            assert_eq!(contact.email, "rosalind@example.invalid");
            assert_eq!(contact.affiliation, None);
            assert_eq!(contact.phone, None);
            assert_eq!(contact.address, None);
        }
    }

    /// A technical or notification address names a system or a mailbox, not
    /// the person responsible for the page, so changing hands leaves it be.
    #[test]
    fn technical_and_notification_addresses_are_kept() {
        let existing = [
            contact(
                ContactRole::Manager,
                "Dorothy Hodgkin",
                "dorothy@example.invalid",
            ),
            contact(ContactRole::Technical, "Ops", "ops@example.invalid"),
            contact(
                ContactRole::Notification,
                "Alerts",
                "alerts@example.invalid",
            ),
        ];

        let contacts = steward_contacts(
            &existing,
            &user("Rosalind Franklin", "rosalind@example.invalid"),
        );

        let kept: Vec<_> = contacts.iter().map(|c| c.email.as_str()).collect();

        assert!(kept.contains(&"ops@example.invalid"));
        assert!(kept.contains(&"alerts@example.invalid"));
        assert!(!kept.contains(&"dorothy@example.invalid"));
    }

    #[test]
    fn an_editor_is_added_once() {
        let existing = Ulid::new();
        let steward = Ulid::new();

        assert_eq!(with_editor(&[existing], steward), vec![existing, steward]);
        assert_eq!(
            with_editor(&[existing, steward], steward),
            vec![existing, steward]
        );
    }
}
