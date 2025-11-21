use anyhow::Error;
use common::timestring::TimeString;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use ulid::Ulid;

use crate::peering::LoadStorePeered;
use common::{shared::db, transaction::Immediate};

pub mod handlers;
pub mod routes;

// Re-export routes function for convenience
pub use routes::routes;

/// Lightweight exchange info for page rendering (no nested collections)
#[derive(Debug, Clone)]
pub struct ExchangeInfo {
    pub exchange_id: Ulid,
    pub path_id: i64,
    pub slug: String,
    pub name: String,
    pub description: String,
    pub logo_url: Option<String>,
    pub settings: ExchangeSettings,
}

impl ExchangeInfo {
    /// Load exchange metadata by path_id (efficient for page views)
    pub async fn load_by_id(path_id: i64) -> Result<Self, Error> {
        let row = sqlx::query!(
            "SELECT exchange_id, path_id, slug, name, description, logo_url, settings FROM exchanges WHERE path_id = ?",
            path_id
        )
        .fetch_one(db())
        .await?;

        Ok(ExchangeInfo {
            exchange_id: Ulid::from_string(&row.exchange_id)?,
            path_id: row.path_id,
            slug: row.slug,
            name: row.name,
            description: row.description,
            logo_url: row.logo_url,
            settings: serde_json::from_str(&row.settings)?,
        })
    }

    /// Load exchange metadata by slug (efficient for page views)
    pub async fn load_by_slug(slug: &str) -> Result<Self, Error> {
        let row = sqlx::query!(
            "SELECT exchange_id, path_id, slug, name, description, logo_url, settings FROM exchanges WHERE slug = ?",
            slug
        )
        .fetch_one(db())
        .await?;

        Ok(ExchangeInfo {
            exchange_id: Ulid::from_string(&row.exchange_id)?,
            path_id: row.path_id,
            slug: row.slug,
            name: row.name,
            description: row.description,
            logo_url: row.logo_url,
            settings: serde_json::from_str(&row.settings)?,
        })
    }

    /// Get predefined searches for this exchange
    pub async fn get_predefined_searches(&self) -> Result<Vec<PredefinedSearch>, Error> {
        let exchange_id_str = self.exchange_id.to_string();
        let rows = sqlx::query!(
            r#"
            SELECT search_id as "search_id!: String", name, search_params, display_order
            FROM exchange_predefined_searches
            WHERE exchange_id = ?
            ORDER BY display_order
            "#,
            exchange_id_str
        )
        .fetch_all(db())
        .await?;

        rows.into_iter()
            .map(|r| {
                Ok(PredefinedSearch {
                    search_id: Ulid::from_string(&r.search_id)?,
                    name: r.name,
                    search_params: serde_json::from_str(&r.search_params)?,
                    display_order: r.display_order as i32,
                })
            })
            .collect::<Result<Vec<_>, Error>>()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExchangeSettings {
    pub allow_project_voting: bool,
    pub allow_project_reviews: bool,
    pub require_approval: bool,
    pub restricted_add: bool,
    pub custom_css: Option<String>,
}

impl Default for ExchangeSettings {
    fn default() -> Self {
        Self {
            allow_project_voting: false,
            allow_project_reviews: false,
            require_approval: false,
            restricted_add: false,
            custom_css: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PredefinedSearch {
    pub search_id: Ulid,
    pub name: String,
    pub search_params: SearchParams,
    pub display_order: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SearchParams {
    #[serde(default)]
    pub phrase: Vec<String>,
    #[serde(default)]
    pub activity: Option<String>,
    #[serde(default)]
    pub topic: Option<String>,
    #[serde(default)]
    pub audience: Vec<String>,
    #[serde(default)]
    pub inactive: bool,
    #[serde(default = "default_page")]
    pub page: usize,
    #[serde(default = "default_page_size")]
    pub page_size: usize,
}

fn default_page() -> usize {
    1
}

fn default_page_size() -> usize {
    20
}

/// Main Exchange type - implements LoadStorePeered for peer sync
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Exchange {
    pub exchange_id: Ulid,
    pub path_id: i64,
    pub slug: String,
    pub name: String,
    pub description: String,
    pub logo_url: Option<String>,
    pub settings: ExchangeSettings,
    pub predefined_searches: Vec<PredefinedSearch>,
    pub hidden_projects: HashSet<Ulid>,
}

impl LoadStorePeered for Exchange {
    type DatabaseKey = Ulid; // exchange_id
    const GROUP: &str = "exchange";
    const SPECIFIC: &str = "all";

    async fn save_to_database(&self) -> Result<(), Error> {
        let mut tx = Immediate::begin_with_retry(db(), None, None).await?;

        // Save main exchange
        let settings_json = serde_json::to_string(&self.settings)?;
        let exchange_id_str = self.exchange_id.to_string();

        sqlx::query!(
            r#"
            INSERT OR REPLACE INTO exchanges
            (exchange_id, path_id, slug, name, description, logo_url, settings)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            "#,
            exchange_id_str,
            self.path_id,
            self.slug,
            self.name,
            self.description,
            self.logo_url,
            settings_json
        )
        .execute(&mut tx)
        .await?;

        // Delete old nested data
        sqlx::query!(
            "DELETE FROM exchange_predefined_searches WHERE exchange_id = ?",
            exchange_id_str
        )
        .execute(&mut tx)
        .await?;

        sqlx::query!(
            "DELETE FROM exchange_hidden_projects WHERE exchange_id = ?",
            exchange_id_str
        )
        .execute(&mut tx)
        .await?;

        // Save predefined searches
        for search in &self.predefined_searches {
            let search_params_json = serde_json::to_string(&search.search_params)?;
            let search_id_str = search.search_id.to_string();

            sqlx::query!(
                r#"
                INSERT INTO exchange_predefined_searches
                (search_id, exchange_id, name, search_params, display_order)
                VALUES (?, ?, ?, ?, ?)
                "#,
                search_id_str,
                exchange_id_str,
                search.name,
                search_params_json,
                search.display_order
            )
            .execute(&mut tx)
            .await?;
        }

        // Save hidden projects
        for project_id in &self.hidden_projects {
            let project_id_str = project_id.to_string();
            let now = TimeString::now().to_string();

            sqlx::query!(
                r#"
                INSERT INTO exchange_hidden_projects
                (exchange_id, project_id, hidden_by, hidden_at)
                VALUES (?, ?, ?, ?)
                "#,
                exchange_id_str,
                project_id_str,
                "system",
                now
            )
            .execute(&mut tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    async fn read_from_database(exchange_id: Ulid) -> Result<Self, Error> {
        let exchange_id_str = exchange_id.to_string();

        // Read main exchange
        let row = sqlx::query!(
            "SELECT exchange_id, path_id, slug, name, description, logo_url, settings FROM exchanges WHERE exchange_id = ?",
            exchange_id_str
        )
        .fetch_one(db())
        .await?;

        // Read predefined searches
        let searches = sqlx::query!(
            r#"
            SELECT search_id as "search_id!: String", exchange_id, name, search_params, display_order
            FROM exchange_predefined_searches
            WHERE exchange_id = ?
            ORDER BY display_order
            "#,
            exchange_id_str
        )
        .fetch_all(db())
        .await?;

        let predefined_searches = searches
            .into_iter()
            .map(|s| {
                Ok(PredefinedSearch {
                    search_id: Ulid::from_string(&s.search_id)?,
                    name: s.name,
                    search_params: serde_json::from_str(&s.search_params)?,
                    display_order: s.display_order as i32,
                })
            })
            .collect::<Result<Vec<_>, Error>>()?;

        // Read hidden projects
        let hidden = sqlx::query!(
            "SELECT project_id FROM exchange_hidden_projects WHERE exchange_id = ?",
            exchange_id_str
        )
        .fetch_all(db())
        .await?;

        let hidden_projects = hidden
            .into_iter()
            .map(|h| Ulid::from_string(&h.project_id))
            .collect::<Result<HashSet<_>, _>>()?;

        Ok(Exchange {
            exchange_id: Ulid::from_string(&row.exchange_id)?,
            path_id: row.path_id,
            slug: row.slug,
            name: row.name,
            description: row.description,
            logo_url: row.logo_url,
            settings: serde_json::from_str(&row.settings)?,
            predefined_searches,
            hidden_projects,
        })
    }

    async fn delete_from_database(exchange_id: Ulid) -> Result<(), Error> {
        let exchange_id_str = exchange_id.to_string();
        // Delete exchange and all related data (CASCADE handles predefined_searches and hidden_projects)
        sqlx::query!("DELETE FROM exchanges WHERE exchange_id = ?", exchange_id_str)
            .execute(db())
            .await?;

        Ok(())
    }
}

// Template helper types

use std::sync::Arc;
use common::message::user::BasicInfo;
use crate::middleware::authorization::Authenticated;

/// Wrapper for user info that templates can use
#[derive(Debug, Clone)]
pub struct TemplateUser(pub Option<Arc<BasicInfo>>);

impl TemplateUser {
    pub fn is_authenticated(&self) -> bool {
        self.0.is_some()
    }

    pub fn id(&self) -> Option<Ulid> {
        self.0.as_ref().map(|info| info.user_id)
    }
}

impl From<Authenticated> for TemplateUser {
    fn from(auth: Authenticated) -> Self {
        TemplateUser(auth.0)
    }
}

/// Wrapper for exchange info with pre-computed authorization
#[derive(Debug, Clone)]
pub struct TemplateExchange {
    pub info: ExchangeInfo,
    pub user_is_authorized: bool,
}

impl TemplateExchange {
    /// Returns path_id for use in URLs (backward compatible)
    pub fn exchange_id(&self) -> i64 {
        self.info.path_id
    }

    pub fn restricted_add(&self) -> bool {
        self.info.settings.restricted_add
    }

    pub fn user_is_authorized(&self) -> bool {
        self.user_is_authorized
    }
}
