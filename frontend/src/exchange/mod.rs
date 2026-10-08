use anyhow::Error;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use ulid::Ulid;

use crate::peering::LoadStorePeered;
use common::{shared::db, transaction::Immediate};

pub mod access;
pub mod handlers;
pub mod orrery;
pub mod routes;
pub mod tracking;

// Re-export routes function for convenience
pub use routes::routes;

/// The exchange a project or event is being submitted through, given the
/// `?exchange=` its add form was opened with: the exchange's ULID, as a
/// submission's `exchange` holds it.
///
/// Anything that names no exchange -- blank, not a number, or a number no
/// exchange has -- submits it through none, as orrery does.
pub async fn submitted_through(path_id: &str) -> Result<Option<String>, Error> {
    let Ok(path_id) = path_id.trim().parse::<i64>() else {
        return Ok(None);
    };

    let exchange_id = sqlx::query_scalar!(
        "SELECT exchange_id FROM exchanges WHERE path_id = ?",
        path_id
    )
    .fetch_optional(db())
    .await?;

    Ok(exchange_id)
}

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

    /// The search the exchange first shows: nothing asked for, and the
    /// "include global projects" box as the exchange starts it out.
    pub fn initial_search(&self) -> SearchParams {
        SearchParams {
            global: self.settings.global_by_default,
            page: default_page(),
            page_size: default_page_size(),
            ..SearchParams::default()
        }
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

    /// Limit the exchange to projects that reach this region, plus the ones
    /// that can be done anywhere. `None` means the whole catalogue.
    ///
    /// This is what makes SciStarter Australia a view of Australia rather
    /// than a copy of SciStarter: without it every exchange shows every
    /// project.
    #[serde(default)]
    pub region_id: Option<i64>,

    /// Origins whose projects this exchange does not want, whatever else
    /// would have let them in -- a bulk import that duplicates what the
    /// exchange already lists, most often.
    ///
    /// Every field from here on is serde-defaulted so that one can be added
    /// without touching stored settings.
    #[serde(default)]
    pub excluded_origins: Vec<String>,

    /// Turn the exchange from "the catalogue, narrowed" into "only its own
    /// projects, plus whatever `include_tags` and `include_origins` name".
    /// Those two do nothing while this is off.
    #[serde(default)]
    pub exclude_scistarter: bool,

    /// Tags whose projects an exchange that excludes SciStarter still lists.
    #[serde(default)]
    pub include_tags: Vec<String>,

    /// Origins whose projects an exchange that excludes SciStarter still
    /// lists.
    #[serde(default)]
    pub include_origins: Vec<String>,

    /// A regular expression a project's name or tags must match, matched
    /// without regard to case. Empty means no limit.
    #[serde(default)]
    pub search: String,

    /// Whether the finder's "include global projects" box is ticked when
    /// the page loads. Exchanges set up before it existed listed global
    /// projects unconditionally, so it starts out true.
    #[serde(default = "global_by_default")]
    pub global_by_default: bool,
}

fn global_by_default() -> bool {
    true
}

impl Default for ExchangeSettings {
    fn default() -> Self {
        Self {
            allow_project_voting: false,
            allow_project_reviews: false,
            require_approval: false,
            restricted_add: false,
            custom_css: None,
            region_id: None,
            excluded_origins: Vec::new(),
            exclude_scistarter: false,
            include_tags: Vec::new(),
            include_origins: Vec::new(),
            search: String::new(),
            global_by_default: global_by_default(),
        }
    }
}

/// A project an exchange has taken out of its listings.
///
/// Only the exchange's own people still see it there, and `note` is theirs:
/// why it was hidden, and with `elevate` set, a flag that the note wants
/// attention.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HiddenProject {
    pub hidden_by: String,
    pub hidden_at: String,
    pub note: String,
    pub elevate: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PredefinedSearch {
    pub search_id: Ulid,
    pub name: String,
    pub search_params: SearchParams,
    pub display_order: i32,
}

impl PredefinedSearch {
    /// The phrases a prompted search searches for, given as one typed in.
    ///
    /// A prompt is one search term, as it is in orrery, where ticking it
    /// searches for the whole phrase -- commas and all.
    pub fn phrases(phrase: &str) -> Vec<String> {
        let phrase = phrase.trim();
        if phrase.is_empty() {
            Vec::new()
        } else {
            vec![phrase.to_string()]
        }
    }
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
    /// The finder's "include global projects" box: whether a project that
    /// can be done anywhere is listed on a regional exchange for that alone.
    /// How the box starts out is the exchange's choice; see
    /// [`ExchangeInfo::initial_search`].
    #[serde(default)]
    pub global: bool,
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
    pub hidden_projects: BTreeMap<Ulid, HiddenProject>,
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

        // An upsert rather than INSERT OR REPLACE: a `path_id` or `slug`
        // another exchange holds must fail here, not quietly delete that
        // exchange along with its searches and hidden projects.
        sqlx::query!(
            r#"
            INSERT INTO exchanges
            (exchange_id, path_id, slug, name, description, logo_url, settings)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT (exchange_id) DO UPDATE SET
              path_id = excluded.path_id,
              slug = excluded.slug,
              name = excluded.name,
              description = excluded.description,
              logo_url = excluded.logo_url,
              settings = excluded.settings
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
        for (project_id, hidden) in &self.hidden_projects {
            let project_id_str = project_id.to_string();

            sqlx::query!(
                r#"
                INSERT INTO exchange_hidden_projects
                (exchange_id, project_id, hidden_by, hidden_at, note, elevate)
                VALUES (?, ?, ?, ?, ?, ?)
                "#,
                exchange_id_str,
                project_id_str,
                hidden.hidden_by,
                hidden.hidden_at,
                hidden.note,
                hidden.elevate
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
            r#"SELECT project_id, hidden_by, hidden_at, note, elevate as "elevate: bool"
               FROM exchange_hidden_projects WHERE exchange_id = ?"#,
            exchange_id_str
        )
        .fetch_all(db())
        .await?;

        let hidden_projects = hidden
            .into_iter()
            .map(|h| {
                Ok((
                    Ulid::from_string(&h.project_id)?,
                    HiddenProject {
                        hidden_by: h.hidden_by,
                        hidden_at: h.hidden_at,
                        note: h.note,
                        elevate: h.elevate,
                    },
                ))
            })
            .collect::<Result<BTreeMap<_, _>, Error>>()?;

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
        sqlx::query!(
            "DELETE FROM exchanges WHERE exchange_id = ?",
            exchange_id_str
        )
        .execute(db())
        .await?;

        Ok(())
    }
}

// Template helper types

use crate::middleware::authorization::Authenticated;
use common::message::user::BasicInfo;
use std::sync::Arc;

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
