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
}

impl std::fmt::Display for AutocompleteEntityType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AutocompleteEntityType::Projects => write!(f, "projects"),
            AutocompleteEntityType::Events => write!(f, "events"),
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
}

#[derive(Debug, Deserialize)]
pub struct AutocompleteQuery {
    pub search: Option<String>,
    pub publish: Option<PublishFilter>,
    pub field_name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ResolveQuery {
    pub field_name: String,
    pub field_label: String,
    pub placeholder: String,
    pub publish: Option<PublishFilter>,
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
        "SELECT p.project_id, p.name FROM projects_fts f JOIN projects p ON f.project_id = p.project_id WHERE f.name MATCH ",
    );
    query_builder.push_bind(format!("{}*", search_term));

    // Add publish filter
    match publish_filter {
        PublishFilter::Published => {
            query_builder.push(" AND p.publish = ");
            query_builder.push_bind(1);
        }
        PublishFilter::Unpublished => {
            query_builder.push(" AND p.publish = ");
            query_builder.push_bind(0);
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
        "SELECT e.event_id, e.name FROM events_fts f JOIN events e ON f.event_id = e.event_id WHERE f.name MATCH ",
    );
    query_builder.push_bind(format!("{}*", search_term));

    // Add publish filter
    match publish_filter {
        PublishFilter::Published => {
            query_builder.push(" AND e.publish = ");
            query_builder.push_bind(1);
        }
        PublishFilter::Unpublished => {
            query_builder.push(" AND e.publish = ");
            query_builder.push_bind(0);
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

    let mut query_builder = sqlx::QueryBuilder::new("SELECT project_id, name FROM projects WHERE project_id IN (");
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

pub fn routes() -> impl Endpoint {
    Route::new()
        .at("/projects", get(handle_projects_autocomplete))
        .at("/events", get(handle_events_autocomplete))
        .at("/resolve/projects/:id", get(handle_resolve_project_widget))
        .at("/resolve/events/:id", get(handle_resolve_event_widget))
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
    let name = resolve_project_name(id).await.map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    let widget = EntityAutocomplete::new_project_selector(
        query.field_name,
        query.field_label,
        query.placeholder,
    )
    .with_publish_filter(query.publish.unwrap_or(PublishFilter::All))
    .with_selected(id, name.unwrap_or_else(|| "Unknown Project".to_string()));

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
    let name = resolve_event_name(id).await.map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    let widget = EntityAutocomplete::new_event_selector(
        query.field_name,
        query.field_label,
        query.placeholder,
    )
    .with_publish_filter(query.publish.unwrap_or(PublishFilter::All))
    .with_selected(id, name.unwrap_or_else(|| "Unknown Event".to_string()));

    let html = widget.render_once().map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    Ok(Response::builder().content_type("text/html").body(html))
}
