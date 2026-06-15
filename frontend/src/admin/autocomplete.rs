use crate::pagecontent::PageContentKind;
use anyhow::Error;
use poem::{
    Endpoint, Response, Result, Route, get, handler,
    web::{Path, Query},
};
use sailfish::TemplateOnce;
use serde::Deserialize;
use ulid::Ulid;

#[derive(Debug, Clone)]
pub enum AutocompleteEntityType {
    Projects,
    Events,
    Users,
    PageContent,
    PageBuilderPages,
    EntitySlugs,
}

impl std::fmt::Display for AutocompleteEntityType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AutocompleteEntityType::Projects => write!(f, "projects"),
            AutocompleteEntityType::Events => write!(f, "events"),
            AutocompleteEntityType::Users => write!(f, "users"),
            AutocompleteEntityType::PageContent => write!(f, "pagecontent"),
            AutocompleteEntityType::PageBuilderPages => write!(f, "pagebuilder"),
            AutocompleteEntityType::EntitySlugs => write!(f, "entityslugs"),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub enum PublishFilter {
    All,
    Published,
    Unpublished,
}

impl std::fmt::Display for PublishFilter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PublishFilter::All => write!(f, "All"),
            PublishFilter::Published => write!(f, "Published"),
            PublishFilter::Unpublished => write!(f, "Unpublished"),
        }
    }
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/partials/entity_autocomplete.stpl.html")]
pub struct EntityAutocomplete {
    pub entity_type: AutocompleteEntityType,
    pub publish_filter: PublishFilter,
    pub field_name: String,
    pub field_label: String,
    pub placeholder: String,
    pub selected_id: Option<Ulid>,
    pub selected_name: Option<String>,
    pub read_only: bool,
    pub kind_filter: Option<String>,
}

impl EntityAutocomplete {
    pub fn new_project_selector(
        field_name: impl Into<String>,
        field_label: impl Into<String>,
        placeholder: impl Into<String>,
    ) -> Self {
        Self {
            entity_type: AutocompleteEntityType::Projects,
            publish_filter: PublishFilter::All,
            field_name: field_name.into(),
            field_label: field_label.into(),
            placeholder: placeholder.into(),
            selected_id: None,
            selected_name: None,
            read_only: false,
            kind_filter: None,
        }
    }

    pub fn new_event_selector(
        field_name: impl Into<String>,
        field_label: impl Into<String>,
        placeholder: impl Into<String>,
    ) -> Self {
        Self {
            entity_type: AutocompleteEntityType::Events,
            publish_filter: PublishFilter::All,
            field_name: field_name.into(),
            field_label: field_label.into(),
            placeholder: placeholder.into(),
            selected_id: None,
            selected_name: None,
            read_only: false,
            kind_filter: None,
        }
    }

    pub fn new_user_selector(
        field_name: impl Into<String>,
        field_label: impl Into<String>,
        placeholder: impl Into<String>,
    ) -> Self {
        Self {
            entity_type: AutocompleteEntityType::Users,
            publish_filter: PublishFilter::All,
            field_name: field_name.into(),
            field_label: field_label.into(),
            placeholder: placeholder.into(),
            selected_id: None,
            selected_name: None,
            read_only: false,
            kind_filter: None,
        }
    }

    pub fn new_pagecontent_selector(
        field_name: impl Into<String>,
        field_label: impl Into<String>,
        placeholder: impl Into<String>,
    ) -> Self {
        Self {
            entity_type: AutocompleteEntityType::PageContent,
            publish_filter: PublishFilter::All,
            field_name: field_name.into(),
            field_label: field_label.into(),
            placeholder: placeholder.into(),
            selected_id: None,
            selected_name: None,
            read_only: false,
            kind_filter: None,
        }
    }

    pub fn new_pagebuilder_selector(
        field_name: impl Into<String>,
        field_label: impl Into<String>,
        placeholder: impl Into<String>,
    ) -> Self {
        Self {
            entity_type: AutocompleteEntityType::PageBuilderPages,
            publish_filter: PublishFilter::All,
            field_name: field_name.into(),
            field_label: field_label.into(),
            placeholder: placeholder.into(),
            selected_id: None,
            selected_name: None,
            read_only: false,
            kind_filter: None,
        }
    }

    pub fn with_selected(mut self, id: Ulid, name: impl Into<String>) -> Self {
        self.selected_id = Some(id);
        self.selected_name = Some(name.into());
        self
    }

    pub fn with_selected_id(mut self, id: Option<Ulid>) -> Self {
        if let Some(id) = id {
            self.selected_id = Some(id);
            self.selected_name = None; // Will be resolved via endpoint
        }
        self
    }

    pub fn with_publish_filter(mut self, filter: PublishFilter) -> Self {
        self.publish_filter = filter;
        self
    }

    pub fn with_read_only(mut self, read_only: bool) -> Self {
        self.read_only = read_only;
        self
    }

    pub fn with_kind_filter(mut self, kind: PageContentKind) -> Self {
        self.kind_filter = Some(kind.as_ref().to_string());
        self
    }
}

#[derive(Debug, Deserialize)]
pub struct AutocompleteQuery {
    pub search: Option<String>,
    pub publish: Option<PublishFilter>,
    pub field_name: Option<String>,
    pub kind: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ResolveQuery {
    pub field_name: String,
    pub field_label: String,
    pub placeholder: String,
    pub publish: Option<PublishFilter>,
    pub read_only: Option<bool>,
    pub kind: Option<String>,
}

#[derive(Debug)]
pub struct AutocompleteResult {
    pub id: Ulid,
    pub name: String,
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/partials/autocomplete_results.stpl.html")]
pub struct AutocompleteResults {
    pub results: Vec<AutocompleteResult>,
    pub field_name: String,
}

impl AutocompleteResults {
    pub fn new(results: Vec<AutocompleteResult>, field_name: String) -> Self {
        Self {
            results,
            field_name,
        }
    }
}

pub async fn search_projects(query: &AutocompleteQuery) -> Result<Vec<AutocompleteResult>, Error> {
    use common::shared::db;

    let search_term = query.search.as_deref().unwrap_or("").trim();
    if search_term.is_empty() {
        return Ok(Vec::new());
    }

    let publish_filter = query.publish.as_ref().unwrap_or(&PublishFilter::All);

    let mut query_builder = sqlx::QueryBuilder::new(
        "SELECT p.project_id, p.name FROM projects_fts f JOIN projects p ON f.project_id = p.project_id LEFT JOIN entities ent ON ent.target = p.project_id AND ent.kind = 'Project' WHERE f.name MATCH ",
    );
    query_builder.push_bind(format!("{}*", search_term));

    // Add publish filter - checking entities.publish (NULL means no entity = unpublished)
    match publish_filter {
        PublishFilter::Published => {
            query_builder.push(" AND ent.publish = ");
            query_builder.push_bind(1);
        }
        PublishFilter::Unpublished => {
            query_builder.push(" AND (ent.publish = ");
            query_builder.push_bind(0);
            query_builder.push(" OR ent.publish IS NULL)");
        }
        PublishFilter::All => {} // no additional filter
    }

    query_builder.push(" ORDER BY rank LIMIT 10");

    let rows = query_builder
        .build_query_as::<(String, String)>()
        .fetch_all(db())
        .await?;

    let results = rows
        .into_iter()
        .map(|(id_str, name)| -> Result<AutocompleteResult, Error> {
            let id = id_str
                .parse::<Ulid>()
                .map_err(|e| anyhow::anyhow!("Invalid ULID in database: {} - {}", id_str, e))?;
            Ok(AutocompleteResult { id, name })
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(results)
}

pub async fn search_events(query: &AutocompleteQuery) -> Result<Vec<AutocompleteResult>, Error> {
    use common::shared::db;

    let search_term = query.search.as_deref().unwrap_or("").trim();
    if search_term.is_empty() {
        return Ok(Vec::new());
    }

    let publish_filter = query.publish.as_ref().unwrap_or(&PublishFilter::All);

    let mut query_builder = sqlx::QueryBuilder::new(
        "SELECT e.event_id, e.name FROM events_fts f JOIN events e ON f.event_id = e.event_id LEFT JOIN entities ent ON ent.target = e.event_id AND ent.kind = 'Event' WHERE f.name MATCH ",
    );
    query_builder.push_bind(format!("{}*", search_term));

    // Add publish filter - checking entities.publish (NULL means no entity = unpublished)
    match publish_filter {
        PublishFilter::Published => {
            query_builder.push(" AND ent.publish = ");
            query_builder.push_bind(1);
        }
        PublishFilter::Unpublished => {
            query_builder.push(" AND (ent.publish = ");
            query_builder.push_bind(0);
            query_builder.push(" OR ent.publish IS NULL)");
        }
        PublishFilter::All => {} // no additional filter
    }

    query_builder.push(" ORDER BY rank LIMIT 10");

    let rows = query_builder
        .build_query_as::<(String, String)>()
        .fetch_all(db())
        .await?;

    let results = rows
        .into_iter()
        .map(|(id_str, name)| -> Result<AutocompleteResult, Error> {
            let id = id_str
                .parse::<Ulid>()
                .map_err(|e| anyhow::anyhow!("Invalid ULID in database: {} - {}", id_str, e))?;
            Ok(AutocompleteResult { id, name })
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(results)
}

pub async fn resolve_project_name(id: Ulid) -> Result<Option<String>, Error> {
    use common::shared::db;

    let row = sqlx::query_as::<_, (String,)>("SELECT name FROM projects WHERE project_id = ?")
        .bind(id.to_string())
        .fetch_optional(db())
        .await?;

    Ok(row.map(|(name,)| name))
}

pub async fn resolve_project_names(ids: Vec<Ulid>) -> Result<Vec<(Ulid, String)>, Error> {
    use common::shared::db;

    if ids.is_empty() {
        return Ok(Vec::new());
    }

    let mut query_builder =
        sqlx::QueryBuilder::new("SELECT project_id, name FROM projects WHERE project_id IN (");
    let mut separated = query_builder.separated(", ");
    for id in &ids {
        separated.push_bind(id.to_string());
    }
    separated.push_unseparated(")");

    let rows = query_builder
        .build_query_as::<(String, String)>()
        .fetch_all(db())
        .await?;

    let results = rows
        .into_iter()
        .map(|(id_str, name)| -> Result<(Ulid, String), Error> {
            let id = id_str
                .parse::<Ulid>()
                .map_err(|e| anyhow::anyhow!("Invalid ULID in database: {} - {}", id_str, e))?;
            Ok((id, name))
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(results)
}

pub async fn resolve_event_name(id: Ulid) -> Result<Option<String>, Error> {
    use common::shared::db;

    let row = sqlx::query_as::<_, (String,)>("SELECT name FROM events WHERE event_id = ?")
        .bind(id.to_string())
        .fetch_optional(db())
        .await?;

    Ok(row.map(|(name,)| name))
}

pub async fn search_users(query: &AutocompleteQuery) -> Result<Vec<AutocompleteResult>, Error> {
    use common::{
        message::user::{UserDetailsListResponse, UserDetailsSearchRequest},
        shared::nats,
    };

    let search_term = query.search.as_deref().unwrap_or("").trim();
    if search_term.is_empty() {
        return Ok(Vec::new());
    }

    let search_request = UserDetailsSearchRequest {
        search: Some(search_term.to_string()),
        role: None,
        email_status: None,
        page: Some(1),
        limit: Some(10),
    };

    let request_payload = flexbuffers::to_vec(&search_request)
        .map_err(|e| anyhow::anyhow!("Failed to serialize search request: {}", e))?;

    let response = nats()
        .request("user.admin.user_search", request_payload.into())
        .await
        .map_err(|e| anyhow::anyhow!("NATS request failed: {}", e))?;

    let search_response: UserDetailsListResponse = flexbuffers::from_slice(&response.payload)
        .map_err(|e| anyhow::anyhow!("Failed to deserialize search response: {}", e))?;

    let results = search_response
        .users
        .into_iter()
        .map(|user| AutocompleteResult {
            id: user.user_id,
            name: user.primary_email,
        })
        .collect();

    Ok(results)
}

pub async fn resolve_user_name(id: Ulid) -> Result<Option<String>, Error> {
    use common::{
        message::user::{UserDetailsGetRequest, UserDetailsResponse},
        shared::nats,
    };

    let get_request = UserDetailsGetRequest { user_id: id };

    let request_payload = flexbuffers::to_vec(&get_request)
        .map_err(|e| anyhow::anyhow!("Failed to serialize get request: {}", e))?;

    let response = nats()
        .request("user.admin.user_get", request_payload.into())
        .await
        .map_err(|e| anyhow::anyhow!("NATS request failed: {}", e))?;

    // If the user doesn't exist, the user service returns an error
    match flexbuffers::from_slice::<UserDetailsResponse>(&response.payload) {
        Ok(user_response) => Ok(Some(user_response.user.primary_email)),
        Err(_) => Ok(None), // User not found
    }
}

pub async fn resolve_user_names(ids: Vec<Ulid>) -> Result<Vec<(Ulid, String)>, Error> {
    use common::{
        message::user::{UserDetailsBatchGetRequest, UserDetailsBatchGetResponse},
        shared::nats,
    };

    if ids.is_empty() {
        return Ok(Vec::new());
    }

    let batch_request = UserDetailsBatchGetRequest { user_ids: ids };

    let request_payload = flexbuffers::to_vec(&batch_request)
        .map_err(|e| anyhow::anyhow!("Failed to serialize batch request: {}", e))?;

    let response = nats()
        .request("user.admin.user_batch_get", request_payload.into())
        .await
        .map_err(|e| anyhow::anyhow!("NATS request failed: {}", e))?;

    let batch_response: UserDetailsBatchGetResponse = flexbuffers::from_slice(&response.payload)
        .map_err(|e| anyhow::anyhow!("Failed to deserialize batch response: {}", e))?;

    let results = batch_response
        .users
        .into_iter()
        .map(|mapping| (mapping.user_id, mapping.primary_email))
        .collect();

    Ok(results)
}

pub async fn search_pagecontent(
    query: &AutocompleteQuery,
) -> Result<Vec<AutocompleteResult>, Error> {
    use common::shared::db;

    let search_term = query.search.as_deref().unwrap_or("").trim();
    if search_term.is_empty() {
        return Ok(Vec::new());
    }

    let search_pattern = format!("%{}%", search_term);

    let mut query_builder =
        sqlx::QueryBuilder::new("SELECT pagecontent_id, name FROM pagecontent WHERE name LIKE ");
    query_builder.push_bind(&search_pattern);

    // Add kind filter if specified
    if let Some(kind) = &query.kind {
        query_builder.push(" AND kind = ");
        query_builder.push_bind(kind);
    }

    query_builder.push(" ORDER BY name LIMIT 10");

    let rows = query_builder
        .build_query_as::<(String, String)>()
        .fetch_all(db())
        .await?;

    let results = rows
        .into_iter()
        .map(|(id_str, name)| -> Result<AutocompleteResult, Error> {
            let id = id_str
                .parse::<Ulid>()
                .map_err(|e| anyhow::anyhow!("Invalid ULID in database: {} - {}", id_str, e))?;
            Ok(AutocompleteResult { id, name })
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(results)
}

pub async fn resolve_pagecontent_name(id: Ulid) -> Result<Option<String>, Error> {
    use common::shared::db;

    let row =
        sqlx::query_as::<_, (String,)>("SELECT name FROM pagecontent WHERE pagecontent_id = ?")
            .bind(id.to_string())
            .fetch_optional(db())
            .await?;

    Ok(row.map(|(name,)| name))
}

pub async fn search_pagebuilder_pages(
    query: &AutocompleteQuery,
) -> Result<Vec<AutocompleteResult>, Error> {
    use common::shared::db;

    let search_term = query.search.as_deref().unwrap_or("").trim();
    if search_term.is_empty() {
        return Ok(Vec::new());
    }

    let search_pattern = format!("%{}%", search_term);

    let rows = sqlx::query_as::<_, (String, String)>(
        "SELECT page_id, name FROM pagebuilder_pages WHERE name LIKE ? ORDER BY name LIMIT 10",
    )
    .bind(&search_pattern)
    .fetch_all(db())
    .await?;

    let results = rows
        .into_iter()
        .map(|(id_str, name)| -> Result<AutocompleteResult, Error> {
            let id = id_str
                .parse::<Ulid>()
                .map_err(|e| anyhow::anyhow!("Invalid ULID in database: {} - {}", id_str, e))?;
            Ok(AutocompleteResult { id, name })
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(results)
}

pub async fn resolve_pagebuilder_page_name(id: Ulid) -> Result<Option<String>, Error> {
    use common::shared::db;

    let row =
        sqlx::query_as::<_, (String,)>("SELECT name FROM pagebuilder_pages WHERE page_id = ?")
            .bind(id.to_string())
            .fetch_optional(db())
            .await?;

    Ok(row.map(|(name,)| name))
}

pub async fn resolve_pagecontent_names(ids: Vec<Ulid>) -> Result<Vec<(Ulid, String)>, Error> {
    use common::shared::db;

    if ids.is_empty() {
        return Ok(Vec::new());
    }

    let mut query_builder = sqlx::QueryBuilder::new(
        "SELECT pagecontent_id, name FROM pagecontent WHERE pagecontent_id IN (",
    );
    let mut separated = query_builder.separated(", ");
    for id in &ids {
        separated.push_bind(id.to_string());
    }
    separated.push_unseparated(")");

    let rows = query_builder
        .build_query_as::<(String, String)>()
        .fetch_all(db())
        .await?;

    let results = rows
        .into_iter()
        .map(|(id_str, name)| -> Result<(Ulid, String), Error> {
            let id = id_str
                .parse::<Ulid>()
                .map_err(|e| anyhow::anyhow!("Invalid ULID in database: {} - {}", id_str, e))?;
            Ok((id, name))
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(results)
}

// Entity slug autocomplete (returns slug strings, not ULIDs)
#[derive(Debug)]
pub struct SlugAutocompleteResult {
    pub slug: String,
    pub name: String,
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/partials/slug_autocomplete_results.stpl.html")]
pub struct SlugAutocompleteResults {
    pub results: Vec<SlugAutocompleteResult>,
    pub field_name: String,
}

impl SlugAutocompleteResults {
    pub fn new(results: Vec<SlugAutocompleteResult>, field_name: String) -> Self {
        Self {
            results,
            field_name,
        }
    }
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/partials/slug_autocomplete.stpl.html")]
pub struct SlugAutocomplete {
    pub field_name: String,
    pub field_label: String,
    pub placeholder: String,
    pub selected_slug: Option<String>,
    pub selected_name: Option<String>,
    pub read_only: bool,
}

impl SlugAutocomplete {
    pub fn new(
        field_name: impl Into<String>,
        field_label: impl Into<String>,
        placeholder: impl Into<String>,
    ) -> Self {
        Self {
            field_name: field_name.into(),
            field_label: field_label.into(),
            placeholder: placeholder.into(),
            selected_slug: None,
            selected_name: None,
            read_only: false,
        }
    }

    pub fn with_selected(mut self, slug: impl Into<String>, name: impl Into<String>) -> Self {
        self.selected_slug = Some(slug.into());
        self.selected_name = Some(name.into());
        self
    }

    pub fn with_selected_slug(mut self, slug: Option<String>) -> Self {
        self.selected_slug = slug;
        self.selected_name = None; // Will be resolved via endpoint
        self
    }

    pub fn with_read_only(mut self, read_only: bool) -> Self {
        self.read_only = read_only;
        self
    }
}

pub async fn search_entity_slugs(
    query: &AutocompleteQuery,
) -> Result<Vec<SlugAutocompleteResult>, Error> {
    use common::shared::db;

    let search_term = query.search.as_deref().unwrap_or("").trim();
    if search_term.is_empty() {
        return Ok(Vec::new());
    }

    let search_pattern = format!("%{}%", search_term);

    let rows = sqlx::query!(
        r#"SELECT e.slug,
                  COALESCE(p.name, ev.name, '') as "entity_name: String"
           FROM entities e
           LEFT JOIN projects p ON e.kind = 'Project' AND e.target = p.project_id
           LEFT JOIN events ev ON e.kind = 'Event' AND e.target = ev.event_id
           WHERE (e.slug LIKE ?1 OR COALESCE(p.name, ev.name, '') LIKE ?1)
           ORDER BY e.slug LIMIT 10"#,
        search_pattern
    )
    .fetch_all(db())
    .await?;

    let results = rows
        .into_iter()
        .filter_map(|row| {
            if row.entity_name.is_empty() {
                return None;
            }
            Some(SlugAutocompleteResult {
                slug: row.slug,
                name: row.entity_name,
            })
        })
        .collect();

    Ok(results)
}

pub async fn resolve_entity_slug_name(slug: &str) -> Result<Option<String>, Error> {
    use common::shared::db;

    let row = sqlx::query!(
        r#"SELECT COALESCE(p.name, ev.name, '') as "entity_name: String"
           FROM entities e
           LEFT JOIN projects p ON e.kind = 'Project' AND e.target = p.project_id
           LEFT JOIN events ev ON e.kind = 'Event' AND e.target = ev.event_id
           WHERE e.slug = ?1"#,
        slug
    )
    .fetch_optional(db())
    .await?;

    Ok(row.and_then(|r| {
        let name = r.entity_name;
        if name.is_empty() { None } else { Some(name) }
    }))
}

pub fn routes() -> impl Endpoint {
    Route::new()
        .at("/projects", get(handle_projects_autocomplete))
        .at("/events", get(handle_events_autocomplete))
        .at("/users", get(handle_users_autocomplete))
        .at("/pagecontent", get(handle_pagecontent_autocomplete))
        .at("/pagebuilder", get(handle_pagebuilder_autocomplete))
        .at("/entityslugs", get(handle_entityslugs_autocomplete))
        .at("/resolve/projects/:id", get(handle_resolve_project_widget))
        .at("/resolve/events/:id", get(handle_resolve_event_widget))
        .at("/resolve/users/:id", get(handle_resolve_user_widget))
        .at(
            "/resolve/pagecontent/:id",
            get(handle_resolve_pagecontent_widget),
        )
        .at(
            "/resolve/pagebuilder/:id",
            get(handle_resolve_pagebuilder_widget),
        )
        .at(
            "/resolve/entityslugs/:slug",
            get(handle_resolve_entityslug_widget),
        )
}

#[handler]
async fn handle_projects_autocomplete(Query(query): Query<AutocompleteQuery>) -> Result<Response> {
    let results = search_projects(&query).await.map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    let template = AutocompleteResults::new(results, query.field_name.unwrap_or_default());
    let html = template.render_once().map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    Ok(Response::builder().content_type("text/html").body(html))
}

#[handler]
async fn handle_events_autocomplete(Query(query): Query<AutocompleteQuery>) -> Result<Response> {
    let results = search_events(&query).await.map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    let template = AutocompleteResults::new(results, query.field_name.unwrap_or_default());
    let html = template.render_once().map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    Ok(Response::builder().content_type("text/html").body(html))
}

#[handler]
async fn handle_resolve_project_widget(
    Path(id): Path<Ulid>,
    Query(query): Query<ResolveQuery>,
) -> Result<Response> {
    // Try to resolve the project name, but fall back gracefully if it fails
    let name = resolve_project_name(id).await.ok().flatten();

    let mut widget = if let Some(project_name) = name {
        // ID exists as a project, use it with the resolved name
        EntityAutocomplete::new_project_selector(
            query.field_name,
            query.field_label,
            query.placeholder,
        )
        .with_publish_filter(query.publish.unwrap_or(PublishFilter::All))
        .with_selected(id, project_name)
    } else {
        // ID doesn't exist as a project (might be wrong type), create empty widget
        EntityAutocomplete::new_project_selector(
            query.field_name,
            query.field_label,
            query.placeholder,
        )
        .with_publish_filter(query.publish.unwrap_or(PublishFilter::All))
    };

    if let Some(read_only) = query.read_only {
        widget = widget.with_read_only(read_only);
    }

    let html = widget.render_once().map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    Ok(Response::builder().content_type("text/html").body(html))
}

#[handler]
async fn handle_resolve_event_widget(
    Path(id): Path<Ulid>,
    Query(query): Query<ResolveQuery>,
) -> Result<Response> {
    // Try to resolve the event name, but fall back gracefully if it fails
    let name = resolve_event_name(id).await.ok().flatten();

    let mut widget = if let Some(event_name) = name {
        // ID exists as an event, use it with the resolved name
        EntityAutocomplete::new_event_selector(
            query.field_name,
            query.field_label,
            query.placeholder,
        )
        .with_publish_filter(query.publish.unwrap_or(PublishFilter::All))
        .with_selected(id, event_name)
    } else {
        // ID doesn't exist as an event (might be wrong type), create empty widget
        EntityAutocomplete::new_event_selector(
            query.field_name,
            query.field_label,
            query.placeholder,
        )
        .with_publish_filter(query.publish.unwrap_or(PublishFilter::All))
    };

    if let Some(read_only) = query.read_only {
        widget = widget.with_read_only(read_only);
    }

    let html = widget.render_once().map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    Ok(Response::builder().content_type("text/html").body(html))
}

#[handler]
async fn handle_users_autocomplete(Query(query): Query<AutocompleteQuery>) -> Result<Response> {
    let results = search_users(&query).await.map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    let template = AutocompleteResults::new(results, query.field_name.unwrap_or_default());
    let html = template.render_once().map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    Ok(Response::builder().content_type("text/html").body(html))
}

#[handler]
async fn handle_resolve_user_widget(
    Path(id): Path<Ulid>,
    Query(query): Query<ResolveQuery>,
) -> Result<Response> {
    let email = resolve_user_name(id).await.map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    let mut widget = EntityAutocomplete::new_user_selector(
        query.field_name,
        query.field_label,
        query.placeholder,
    )
    .with_selected(id, email.unwrap_or_else(|| "Unknown User".to_string()));

    if let Some(read_only) = query.read_only {
        widget = widget.with_read_only(read_only);
    }

    let html = widget.render_once().map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    Ok(Response::builder().content_type("text/html").body(html))
}

#[handler]
async fn handle_pagecontent_autocomplete(
    Query(query): Query<AutocompleteQuery>,
) -> Result<Response> {
    let results = search_pagecontent(&query).await.map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    let template = AutocompleteResults::new(results, query.field_name.unwrap_or_default());
    let html = template.render_once().map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    Ok(Response::builder().content_type("text/html").body(html))
}

#[handler]
async fn handle_resolve_pagecontent_widget(
    Path(id): Path<Ulid>,
    Query(query): Query<ResolveQuery>,
) -> Result<Response> {
    let name = resolve_pagecontent_name(id).await.ok().flatten();

    let mut widget = if let Some(pagecontent_name) = name {
        EntityAutocomplete::new_pagecontent_selector(
            query.field_name,
            query.field_label,
            query.placeholder,
        )
        .with_selected(id, pagecontent_name)
    } else {
        EntityAutocomplete::new_pagecontent_selector(
            query.field_name,
            query.field_label,
            query.placeholder,
        )
    };

    if let Some(read_only) = query.read_only {
        widget = widget.with_read_only(read_only);
    }

    let html = widget.render_once().map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    Ok(Response::builder().content_type("text/html").body(html))
}

#[handler]
async fn handle_pagebuilder_autocomplete(
    Query(query): Query<AutocompleteQuery>,
) -> Result<Response> {
    let results = search_pagebuilder_pages(&query).await.map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    let template = AutocompleteResults::new(results, query.field_name.unwrap_or_default());
    let html = template.render_once().map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    Ok(Response::builder().content_type("text/html").body(html))
}

#[handler]
async fn handle_resolve_pagebuilder_widget(
    Path(id): Path<Ulid>,
    Query(query): Query<ResolveQuery>,
) -> Result<Response> {
    let name = resolve_pagebuilder_page_name(id).await.ok().flatten();

    let mut widget = if let Some(page_name) = name {
        EntityAutocomplete::new_pagebuilder_selector(
            query.field_name,
            query.field_label,
            query.placeholder,
        )
        .with_selected(id, page_name)
    } else {
        EntityAutocomplete::new_pagebuilder_selector(
            query.field_name,
            query.field_label,
            query.placeholder,
        )
    };

    if let Some(read_only) = query.read_only {
        widget = widget.with_read_only(read_only);
    }

    let html = widget.render_once().map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    Ok(Response::builder().content_type("text/html").body(html))
}

#[handler]
async fn handle_entityslugs_autocomplete(
    Query(query): Query<AutocompleteQuery>,
) -> Result<Response> {
    let results = search_entity_slugs(&query).await.map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    let template = SlugAutocompleteResults::new(results, query.field_name.unwrap_or_default());
    let html = template.render_once().map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    Ok(Response::builder().content_type("text/html").body(html))
}

#[derive(Debug, Deserialize)]
pub struct SlugResolveQuery {
    pub field_name: String,
    pub field_label: String,
    pub placeholder: String,
    pub read_only: Option<bool>,
}

#[handler]
async fn handle_resolve_entityslug_widget(
    Path(slug): Path<String>,
    Query(query): Query<SlugResolveQuery>,
) -> Result<Response> {
    let name = resolve_entity_slug_name(&slug).await.ok().flatten();

    let mut widget = if let Some(entity_name) = name {
        SlugAutocomplete::new(query.field_name, query.field_label, query.placeholder)
            .with_selected(slug, entity_name)
    } else {
        SlugAutocomplete::new(query.field_name, query.field_label, query.placeholder)
    };

    if let Some(read_only) = query.read_only {
        widget = widget.with_read_only(read_only);
    }

    let html = widget.render_once().map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    Ok(Response::builder().content_type("text/html").body(html))
}
