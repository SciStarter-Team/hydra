use anyhow::Error;
use common::shared::db;
use poem::{
    Endpoint, Response, Result, Route, get, handler,
    web::Query,
};
use sailfish::TemplateOnce;
use serde::Deserialize;

/// A search result for public entity search
#[derive(Debug, Clone)]
pub struct SearchResult {
    pub slug: String,
    pub entity_type: String,
    pub title: String,
}

#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    pub q: Option<String>,
}

#[derive(Debug, TemplateOnce)]
#[template(path = "search/autocomplete_results.stpl.html")]
pub struct SearchAutocompleteResults {
    pub results: Vec<SearchResult>,
}

/// Search published entities using FTS
pub async fn search_entities(query: &str) -> Result<Vec<SearchResult>, Error> {
    let query = query.trim();
    if query.is_empty() {
        return Ok(Vec::new());
    }

    // Use FTS5 MATCH with prefix search
    let search_term = format!("{}*", query);

    let rows = sqlx::query!(
        r#"SELECT f.slug as "slug: String",
                  f.entity_type as "entity_type: String",
                  f.title as "title: String"
           FROM entities_fts f
           JOIN entities e ON f.slug = e.slug
           WHERE entities_fts MATCH ?1
             AND e.publish = 1
           GROUP BY f.slug
           ORDER BY rank
           LIMIT 20"#,
        search_term
    )
    .fetch_all(db())
    .await?;

    let results = rows
        .into_iter()
        .filter_map(|row| {
            let entity_type = row.entity_type?;
            let display_type = match entity_type.to_lowercase().as_str() {
                "project" => "Project",
                "event" => "Event",
                _ => "Page",
            };
            Some(SearchResult {
                slug: row.slug?,
                entity_type: display_type.to_string(),
                title: row.title?,
            })
        })
        .collect();

    Ok(results)
}

pub fn routes() -> impl Endpoint {
    Route::new().at("/autocomplete", get(handle_search_autocomplete))
}

#[handler]
async fn handle_search_autocomplete(Query(query): Query<SearchQuery>) -> Result<Response> {
    let search_term = query.q.as_deref().unwrap_or("");

    let results = search_entities(search_term).await.map_err(|e| {
        tracing::error!("Search error: {}", e);
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    let template = SearchAutocompleteResults { results };

    let html = template.render_once().map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    Ok(Response::builder().content_type("text/html").body(html))
}
