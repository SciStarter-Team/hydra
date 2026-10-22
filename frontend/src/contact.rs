use ulid::Ulid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, sqlx::Type, serde::Serialize, serde::Deserialize)]
pub enum ContactRole {
    Manager,
    ContactPerson,
    Technical,
    Notification,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Contact {
    pub contact_id: Ulid,
    pub role: ContactRole,
    pub name: Option<String>,
    pub affiliation: Option<String>,
    pub email: String,
    pub phone: Option<String>,
    pub address: Option<String>,
}

impl Contact {
    fn new(role: ContactRole) -> Contact {
        Contact {
            contact_id: Ulid::new(),
            role,
            name: None,
            affiliation: None,
            email: String::new(),
            phone: None,
            address: None,
        }
    }
}

// orrery describes a project's contacts with flat fields: the contact person
// (`contact_*`), the manager (`manager_*`), and `contact_is_manager`, under
// which the contact person is the manager and the `manager_*` fields go
// unused. Here they are a list. The first ContactPerson is orrery's contact
// person, and `contact_is_manager` is a Manager carrying that person's
// address. The functions below edit the list as orrery's importers and API
// edit the flat fields.

/// Whether the project's manager is its contact person (orrery's
/// `contact_is_manager`).
pub fn manager_is_contact_person(contacts: &[Contact]) -> bool {
    let Some(contact) = contacts
        .iter()
        .find(|c| c.role == ContactRole::ContactPerson)
        .filter(|c| !c.email.is_empty())
    else {
        return false;
    };

    contacts
        .iter()
        .any(|c| c.role == ContactRole::Manager && c.email == contact.email)
}

/// Edit the project's contact person, adding one if there is none. A manager
/// who is the contact person is edited alike, so stays the contact person.
pub fn edit_contact_person(contacts: &mut Box<[Contact]>, edit: impl Fn(&mut Contact)) {
    let mut held = contacts.to_vec();
    let shared = manager_is_contact_person(&held);

    match held
        .iter()
        .position(|c| c.role == ContactRole::ContactPerson)
    {
        Some(at) => edit(&mut held[at]),
        None => {
            let mut contact = Contact::new(ContactRole::ContactPerson);
            edit(&mut contact);
            held.insert(0, contact);
        }
    }

    if shared {
        let contact = held
            .iter()
            .find(|c| c.role == ContactRole::ContactPerson)
            .cloned();
        if let (Some(contact), Some(manager)) = (
            contact,
            held.iter_mut().find(|c| c.role == ContactRole::Manager),
        ) {
            *manager = Contact {
                contact_id: manager.contact_id,
                role: ContactRole::Manager,
                ..contact
            };
        }
    }

    *contacts = held.into_boxed_slice();
}

/// Edit the project's own manager, adding one if there is none. A manager who
/// is the contact person is left alone, as orrery leaves `manager_*` unused
/// while `contact_is_manager` holds.
pub fn edit_manager(contacts: &mut Box<[Contact]>, edit: impl Fn(&mut Contact)) {
    if manager_is_contact_person(contacts) {
        return;
    }

    let mut held = contacts.to_vec();

    match held.iter_mut().find(|c| c.role == ContactRole::Manager) {
        Some(manager) => edit(manager),
        None => {
            let mut manager = Contact::new(ContactRole::Manager);
            edit(&mut manager);
            held.push(manager);
        }
    }

    *contacts = held.into_boxed_slice();
}

/// Leave the project with no manager of its own. A manager who is the contact
/// person stays, as for [`edit_manager`].
pub fn remove_manager(contacts: &mut Box<[Contact]>) {
    if manager_is_contact_person(contacts) {
        return;
    }

    let held: Vec<Contact> = contacts
        .iter()
        .filter(|c| c.role != ContactRole::Manager)
        .cloned()
        .collect();

    *contacts = held.into_boxed_slice();
}

/// The address of the project's contact person (orrery's `contact_email`),
/// read without loading the whole project. Empty when it has none. The rows
/// are stored in list order, so the first is the contact person.
pub async fn contact_person_email(project_id: Ulid) -> Result<String, anyhow::Error> {
    let project_id = project_id.to_string();

    let email = sqlx::query_scalar!(
        r#"SELECT email FROM project_contacts
           WHERE project_id = ? AND role = ?
           ORDER BY rowid LIMIT 1"#,
        project_id,
        ContactRole::ContactPerson,
    )
    .fetch_optional(common::shared::db())
    .await?;

    Ok(email.unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert2::check;

    fn contact(role: ContactRole, email: &str) -> Contact {
        Contact {
            email: email.to_string(),
            ..Contact::new(role)
        }
    }

    fn shared() -> Box<[Contact]> {
        Box::new([
            contact(ContactRole::ContactPerson, "contact@example.org"),
            contact(ContactRole::Manager, "contact@example.org"),
        ])
    }

    #[test]
    fn a_manager_with_the_contacts_address_is_the_contact() {
        check!(manager_is_contact_person(&shared()));
        check!(!manager_is_contact_person(&[
            contact(ContactRole::ContactPerson, "contact@example.org"),
            contact(ContactRole::Manager, "manager@example.org"),
        ]));
        check!(!manager_is_contact_person(&[
            contact(ContactRole::ContactPerson, ""),
            contact(ContactRole::Manager, ""),
        ]));
    }

    #[test]
    fn editing_the_contact_edits_a_manager_who_is_the_contact() {
        let mut contacts = shared();

        edit_contact_person(&mut contacts, |c| c.email = "new@example.org".to_string());

        check!(contacts[0].email == "new@example.org");
        check!(contacts[1].email == "new@example.org");
        check!(contacts[1].role == ContactRole::Manager);
    }

    #[test]
    fn editing_the_contact_adds_one_when_there_is_none() {
        let mut contacts: Box<[Contact]> =
            Box::new([contact(ContactRole::Manager, "manager@example.org")]);

        edit_contact_person(&mut contacts, |c| c.name = Some("Ada".to_string()));

        check!(contacts.len() == 2);
        check!(contacts[0].role == ContactRole::ContactPerson);
        check!(contacts[1].email == "manager@example.org");
    }

    #[test]
    fn a_manager_is_edited_or_added() {
        let mut contacts: Box<[Contact]> =
            Box::new([contact(ContactRole::Manager, "old@example.org")]);
        edit_manager(&mut contacts, |m| m.email = "new@example.org".to_string());
        check!(contacts.len() == 1);
        check!(contacts[0].email == "new@example.org");

        let mut contacts: Box<[Contact]> =
            Box::new([contact(ContactRole::ContactPerson, "contact@example.org")]);
        edit_manager(&mut contacts, |m| m.email = "new@example.org".to_string());
        check!(contacts.len() == 2);
        check!(contacts[1].role == ContactRole::Manager);
    }

    #[test]
    fn a_manager_who_is_the_contact_is_left_alone() {
        let mut edited = shared();
        edit_manager(&mut edited, |m| m.email = "new@example.org".to_string());
        check!(edited[1].email == "contact@example.org");

        let mut removed = shared();
        remove_manager(&mut removed);
        check!(removed.len() == 2);
    }

    #[test]
    fn removing_the_manager_keeps_the_contact() {
        let mut contacts: Box<[Contact]> = Box::new([
            contact(ContactRole::ContactPerson, "contact@example.org"),
            contact(ContactRole::Manager, "manager@example.org"),
        ]);

        remove_manager(&mut contacts);

        check!(contacts.len() == 1);
        check!(contacts[0].role == ContactRole::ContactPerson);
    }
}
