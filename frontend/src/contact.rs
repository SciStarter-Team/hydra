use ulid::Ulid;

#[derive(Debug, Clone, Copy, sqlx::Type, serde::Serialize, serde::Deserialize)]
pub enum ContactRole {
    Manager,
    ContactPerson,
    Technical,
    Notification,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Contact {
    pub contact_id: Ulid,
    pub role: ContactRole,
    pub name: Option<String>,
    pub affiliation: Option<String>,
    pub email: String,
    pub phone: Option<String>,
    pub address: Option<String>,
}
