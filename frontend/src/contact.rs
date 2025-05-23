use ulid::Ulid;

#[derive(Debug, sqlx::Type, serde::Serialize, serde::Deserialize)]
pub enum ContactRole {
    Manager,
    ContactPerson,
    Technical,
    Notification,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct Contact {
    pub contact_id: Ulid,
    pub role: ContactRole,
    pub name: Option<String>,
    pub affiliation: Option<String>,
    pub email: String,
    pub phone: Option<String>,
    pub address: Option<String>,
}
