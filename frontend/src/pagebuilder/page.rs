//! PageBuilderPage - Pages built from components with slot-based composition

use anyhow::Error;
use common::{shared::db, timestring::TimeString};
use ulid::Ulid;

use crate::peering::LoadStorePeered;

/// A page built from components using slot-based composition
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PageBuilderPage {
    pub page_id: Ulid,
    pub name: String,
    pub description: String,
    pub source: String,
    pub created: TimeString,
    pub updated: TimeString,
}

impl PageBuilderPage {
    /// Create a new page with the given name and source
    pub fn new(name: impl Into<String>, source: impl Into<String>) -> Self {
        let now = TimeString::now();
        Self {
            page_id: Ulid::new(),
            name: name.into(),
            description: String::new(),
            source: source.into(),
            created: now.clone(),
            updated: now,
        }
    }

    /// Create a new page with a specific ID
    pub fn new_with_id(page_id: Ulid, name: impl Into<String>, source: impl Into<String>) -> Self {
        let now = TimeString::now();
        Self {
            page_id,
            name: name.into(),
            description: String::new(),
            source: source.into(),
            created: now.clone(),
            updated: now,
        }
    }

    /// Set the page description
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    /// Update the source and refresh the updated timestamp
    pub fn set_source(&mut self, source: impl Into<String>) {
        self.source = source.into();
        self.updated = TimeString::now();
    }

    /// List all pages
    pub async fn list() -> Result<Vec<Self>, Error> {
        struct Row {
            page_id: String,
            name: String,
            description: String,
            source: String,
            created: String,
            updated: String,
        }

        let rows = sqlx::query_as!(
            Row,
            r#"SELECT page_id, name, description, source, created, updated
               FROM pagebuilder_pages
               ORDER BY name"#
        )
        .fetch_all(db())
        .await?;

        rows.into_iter()
            .map(|row| {
                Ok(Self {
                    page_id: row.page_id.parse()?,
                    name: row.name,
                    description: row.description,
                    source: row.source,
                    created: TimeString::try_from(row.created)?,
                    updated: TimeString::try_from(row.updated)?,
                })
            })
            .collect()
    }
}

impl LoadStorePeered for PageBuilderPage {
    type DatabaseKey = Ulid;

    const GROUP: &str = "pagebuilder_page";

    async fn save_to_database(&self) -> Result<(), Error> {
        let page_id_str = self.page_id.to_string();
        let created_str = self.created.to_string();
        let updated_str = self.updated.to_string();

        sqlx::query!(
            r#"INSERT OR REPLACE INTO pagebuilder_pages
               (page_id, name, description, source, created, updated)
               VALUES (?, ?, ?, ?, ?, ?)"#,
            page_id_str,
            self.name,
            self.description,
            self.source,
            created_str,
            updated_str,
        )
        .execute(db())
        .await?;

        Ok(())
    }

    async fn read_from_database(key: Self::DatabaseKey) -> Result<Self, Error> {
        struct Row {
            page_id: String,
            name: String,
            description: String,
            source: String,
            created: String,
            updated: String,
        }

        let key_str = key.to_string();

        let row = sqlx::query_as!(
            Row,
            r#"SELECT page_id, name, description, source, created, updated
               FROM pagebuilder_pages
               WHERE page_id = ?"#,
            key_str
        )
        .fetch_one(db())
        .await?;

        Ok(Self {
            page_id: row.page_id.parse()?,
            name: row.name,
            description: row.description,
            source: row.source,
            created: TimeString::try_from(row.created)?,
            updated: TimeString::try_from(row.updated)?,
        })
    }

    async fn delete_from_database(key: Self::DatabaseKey) -> Result<(), Error> {
        let key_str = key.to_string();

        sqlx::query!(
            r#"DELETE FROM pagebuilder_pages WHERE page_id = ?"#,
            key_str
        )
        .execute(db())
        .await?;

        Ok(())
    }
}
