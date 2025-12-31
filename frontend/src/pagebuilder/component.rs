//! PageBuilderComponent - Reusable template components with slots

use anyhow::Error;
use common::{shared::db, timestring::TimeString};
use ulid::Ulid;

use crate::peering::LoadStorePeered;

/// A reusable page component with template source containing slots
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PageBuilderComponent {
    pub component_id: Ulid,
    pub name: String,
    pub description: String,
    pub source: String,
    pub category: String,
    pub created: TimeString,
    pub updated: TimeString,
}

impl PageBuilderComponent {
    /// Create a new component with the given name and source
    pub fn new(name: impl Into<String>, source: impl Into<String>) -> Self {
        let now = TimeString::now();
        Self {
            component_id: Ulid::new(),
            name: name.into(),
            description: String::new(),
            source: source.into(),
            category: "general".to_string(),
            created: now.clone(),
            updated: now,
        }
    }

    /// Create a new component with a specific ID
    pub fn new_with_id(
        component_id: Ulid,
        name: impl Into<String>,
        source: impl Into<String>,
    ) -> Self {
        let now = TimeString::now();
        Self {
            component_id,
            name: name.into(),
            description: String::new(),
            source: source.into(),
            category: "general".to_string(),
            created: now.clone(),
            updated: now,
        }
    }

    /// Set the component description
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    /// Set the component category
    pub fn with_category(mut self, category: impl Into<String>) -> Self {
        self.category = category.into();
        self
    }

    /// Update the source and refresh the updated timestamp
    pub fn set_source(&mut self, source: impl Into<String>) {
        self.source = source.into();
        self.updated = TimeString::now();
    }

    /// List all components, optionally filtered by category
    pub async fn list(category: Option<&str>) -> Result<Vec<Self>, Error> {
        struct Row {
            component_id: String,
            name: String,
            description: String,
            source: String,
            category: String,
            created: String,
            updated: String,
        }

        let rows = if let Some(cat) = category {
            sqlx::query_as!(
                Row,
                r#"SELECT component_id, name, description, source, category, created, updated
                   FROM pagebuilder_components
                   WHERE category = ?
                   ORDER BY name"#,
                cat
            )
            .fetch_all(db())
            .await?
        } else {
            sqlx::query_as!(
                Row,
                r#"SELECT component_id, name, description, source, category, created, updated
                   FROM pagebuilder_components
                   ORDER BY name"#
            )
            .fetch_all(db())
            .await?
        };

        rows.into_iter()
            .map(|row| {
                Ok(Self {
                    component_id: row.component_id.parse()?,
                    name: row.name,
                    description: row.description,
                    source: row.source,
                    category: row.category,
                    created: TimeString::try_from(row.created)?,
                    updated: TimeString::try_from(row.updated)?,
                })
            })
            .collect()
    }

    /// Get distinct categories
    pub async fn categories() -> Result<Vec<String>, Error> {
        struct Row {
            category: String,
        }

        let rows = sqlx::query_as!(
            Row,
            r#"SELECT DISTINCT category FROM pagebuilder_components ORDER BY category"#
        )
        .fetch_all(db())
        .await?;

        Ok(rows.into_iter().map(|r| r.category).collect())
    }
}

impl LoadStorePeered for PageBuilderComponent {
    type DatabaseKey = Ulid;

    const GROUP: &str = "pagebuilder_component";

    async fn save_to_database(&self) -> Result<(), Error> {
        let component_id_str = self.component_id.to_string();
        let created_str = self.created.to_string();
        let updated_str = self.updated.to_string();

        sqlx::query!(
            r#"INSERT OR REPLACE INTO pagebuilder_components
               (component_id, name, description, source, category, created, updated)
               VALUES (?, ?, ?, ?, ?, ?, ?)"#,
            component_id_str,
            self.name,
            self.description,
            self.source,
            self.category,
            created_str,
            updated_str,
        )
        .execute(db())
        .await?;

        Ok(())
    }

    async fn read_from_database(key: Self::DatabaseKey) -> Result<Self, Error> {
        struct Row {
            component_id: String,
            name: String,
            description: String,
            source: String,
            category: String,
            created: String,
            updated: String,
        }

        let key_str = key.to_string();

        let row = sqlx::query_as!(
            Row,
            r#"SELECT component_id, name, description, source, category, created, updated
               FROM pagebuilder_components
               WHERE component_id = ?"#,
            key_str
        )
        .fetch_one(db())
        .await?;

        Ok(Self {
            component_id: row.component_id.parse()?,
            name: row.name,
            description: row.description,
            source: row.source,
            category: row.category,
            created: TimeString::try_from(row.created)?,
            updated: TimeString::try_from(row.updated)?,
        })
    }

    async fn delete_from_database(key: Self::DatabaseKey) -> Result<(), Error> {
        let key_str = key.to_string();

        sqlx::query!(
            r#"DELETE FROM pagebuilder_components WHERE component_id = ?"#,
            key_str
        )
        .execute(db())
        .await?;

        Ok(())
    }
}
