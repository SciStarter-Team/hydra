use ulid::Ulid;

#[derive(sqlx::Type, serde::Serialize, serde::Deserialize)]
pub enum ContactRole {
    Manager,
    ContactPerson,
    Technical,
    Notification,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct Contact {
    contact_id: Ulid,
    role: ContactRole,
    name: Option<String>,
    affiliation: Option<String>,
    email: String,
    phone: Option<String>,
    address: Option<String>,
}
