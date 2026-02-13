use anyhow::Error;
use common::shared::db;
use poem::{
    Endpoint, Response, Result, Route, get, handler,
    session::Session,
    web::Query,
};
use sailfish::TemplateOnce;
use serde::Deserialize;
use crate::{
    component::{Component, Head, StandardLayout},
    middleware::authorization::Authenticated,
};

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

fn deserialize_optional_f64<'de, D>(deserializer: D) -> Result<Option<f64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s: Option<String> = Option::deserialize(deserializer)?;
    match s {
        None => Ok(None),
        Some(s) if s.is_empty() => Ok(None),
        Some(s) => s.parse().map(Some).map_err(serde::de::Error::custom),
    }
}

#[derive(Debug, Deserialize)]
pub struct FinderQuery {
    pub q: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_f64")]
    pub lat: Option<f64>,
    #[serde(default, deserialize_with = "deserialize_optional_f64")]
    pub lng: Option<f64>,
    pub location_name: Option<String>,
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
    Route::new()
        .at("/autocomplete", get(handle_search_autocomplete))
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

// --- Finder page ---

#[derive(Debug, Clone)]
pub struct FinderResult {
    pub slug: String,
    pub entity_type: String,
    pub title: String,
    pub location_display: String,
    pub image: String,
    pub description_snippet: String,
    pub distance_km: Option<f64>,
}

#[derive(TemplateOnce)]
#[template(path = "pages/finder.stpl.html")]
struct FinderPage {
    head: <Head as Component>::Prepared,
    layout: <StandardLayout<FinderContent> as Component>::Prepared,
}

#[derive(TemplateOnce)]
#[template(path = "pages/finder_content.stpl.html")]
struct FinderContent {
    results: Vec<FinderResult>,
    query: String,
    location_name: String,
    lat: String,
    lng: String,
    geosearch_key: String,
}

async fn search_text_only(query: &str) -> Result<Vec<FinderResult>, Error> {
    let search_term = format!("{}*", query);

    let rows = sqlx::query!(
        r#"SELECT f.slug as "slug: String",
                  f.entity_type as "entity_type: String",
                  f.title as "title: String",
                  COALESCE(p.description, ev.description, '') as "description: String",
                  COALESCE(p.image, ev.image, '') as "image: String",
                  CASE
                    WHEN COALESCE(p.entirely_online, ev.entirely_online, 0) = 1 THEN 'Online'
                    WHEN COALESCE(p.city, ev.city, '') != '' AND COALESCE(p.state, ev.state, '') != '' THEN COALESCE(p.city, ev.city, '') || ', ' || COALESCE(p.state, ev.state, '')
                    WHEN COALESCE(p.location_name, ev.location_name, '') != '' THEN COALESCE(p.location_name, ev.location_name, '')
                    ELSE ''
                  END as "location_display: String"
           FROM entities_fts f
           JOIN entities e ON f.slug = e.slug
           LEFT JOIN projects p ON e.kind = 'Project' AND e.target = p.project_id
           LEFT JOIN events ev ON e.kind = 'Event' AND e.target = ev.event_id
           WHERE entities_fts MATCH ?1
             AND e.publish = 1
             AND e.kind IN ('Project', 'Event')
           GROUP BY f.slug
           ORDER BY rank
           LIMIT 50"#,
        search_term
    )
    .fetch_all(db())
    .await?;

    Ok(rows
        .into_iter()
        .filter_map(|row| {
            let entity_type = row.entity_type?;
            let display_type = match entity_type.to_lowercase().as_str() {
                "project" => "Project",
                "event" => "Event",
                _ => return None,
            };
            let desc = row.description.unwrap_or_default();
            let snippet = if desc.len() > 200 {
                format!(
                    "{}...",
                    &desc[..desc
                        .char_indices()
                        .take(200)
                        .last()
                        .map(|(i, _)| i)
                        .unwrap_or(200)]
                )
            } else {
                desc
            };
            Some(FinderResult {
                slug: row.slug?,
                entity_type: display_type.to_string(),
                title: row.title.unwrap_or_default(),
                location_display: row.location_display.unwrap_or_default(),
                image: row.image.unwrap_or_default(),
                description_snippet: snippet,
                distance_km: None,
            })
        })
        .collect())
}

struct SpatialRow {
    slug: String,
    entity_type: String,
    title: String,
    description: String,
    image: String,
    location_display: String,
    distance: Option<f64>,
    region_area: Option<f64>,
}

async fn search_with_location(
    query: &str,
    lat: f64,
    lng: f64,
) -> Result<Vec<FinderResult>, Error> {
    let search_term = format!("{}*", query);

    // Query projects
    let project_rows = sqlx::query!(
        r#"SELECT f.slug as "slug: String",
                  f.title as "title: String",
                  p.description as "description: String",
                  p.image as "image: String",
                  CASE
                    WHEN p.entirely_online = 1 THEN 'Online'
                    WHEN p.city != '' AND p.state != '' THEN p.city || ', ' || p.state
                    WHEN p.location_name != '' THEN p.location_name
                    ELSE ''
                  END as "location_display: String",
                  ST_Distance(p.point, MakePoint(?2, ?3, 4326), 1) as "distance: Option<f64>",
                  CASE WHEN p.regions IS NOT NULL AND ST_Contains(p.regions, MakePoint(?2, ?3, 4326))
                       THEN ST_Area(p.regions, 1)
                       ELSE NULL
                  END as "region_area: Option<f64>"
           FROM entities_fts f
           JOIN entities e ON f.slug = e.slug
           JOIN projects p ON e.target = p.project_id
           WHERE e.kind = 'Project'
             AND entities_fts MATCH ?1
             AND e.publish = 1
           GROUP BY f.slug
           LIMIT 50"#,
        search_term,
        lng,
        lat
    )
    .fetch_all(db())
    .await?;

    // Query events
    let event_rows = sqlx::query!(
        r#"SELECT f.slug as "slug: String",
                  f.title as "title: String",
                  ev.description as "description: String",
                  ev.image as "image: String",
                  CASE
                    WHEN ev.entirely_online = 1 THEN 'Online'
                    WHEN ev.city != '' AND ev.state != '' THEN ev.city || ', ' || ev.state
                    WHEN ev.location_name != '' THEN ev.location_name
                    ELSE ''
                  END as "location_display: String",
                  ST_Distance(ev.point, MakePoint(?2, ?3, 4326), 1) as "distance: Option<f64>",
                  CASE WHEN ev.regions IS NOT NULL AND ST_Contains(ev.regions, MakePoint(?2, ?3, 4326))
                       THEN ST_Area(ev.regions, 1)
                       ELSE NULL
                  END as "region_area: Option<f64>"
           FROM entities_fts f
           JOIN entities e ON f.slug = e.slug
           JOIN events ev ON e.target = ev.event_id
           WHERE e.kind = 'Event'
             AND entities_fts MATCH ?1
             AND e.publish = 1
           GROUP BY f.slug
           LIMIT 50"#,
        search_term,
        lng,
        lat
    )
    .fetch_all(db())
    .await?;

    let mut spatial: Vec<SpatialRow> = Vec::new();

    for row in project_rows {
        spatial.push(SpatialRow {
            slug: row.slug.unwrap_or_default(),
            entity_type: "Project".to_string(),
            title: row.title.unwrap_or_default(),
            description: row.description,
            image: row.image,
            location_display: row.location_display.unwrap_or_default(),
            distance: row.distance.flatten(),
            region_area: row.region_area.flatten(),
        });
    }

    for row in event_rows {
        spatial.push(SpatialRow {
            slug: row.slug.unwrap_or_default(),
            entity_type: "Event".to_string(),
            title: row.title.unwrap_or_default(),
            description: row.description,
            image: row.image,
            location_display: row.location_display.unwrap_or_default(),
            distance: row.distance.flatten(),
            region_area: row.region_area.flatten(),
        });
    }

    // Sort: region containment first (smaller area wins), then by distance
    spatial.sort_by(|a, b| match (&a.region_area, &b.region_area) {
        (Some(area_a), Some(area_b)) => area_a
            .partial_cmp(area_b)
            .unwrap_or(std::cmp::Ordering::Equal),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => {
            let dist_a = a.distance.unwrap_or(f64::MAX);
            let dist_b = b.distance.unwrap_or(f64::MAX);
            dist_a
                .partial_cmp(&dist_b)
                .unwrap_or(std::cmp::Ordering::Equal)
        }
    });

    spatial.truncate(50);

    Ok(spatial
        .into_iter()
        .map(|row| {
            let desc = row.description;
            let snippet = if desc.len() > 200 {
                format!(
                    "{}...",
                    &desc[..desc
                        .char_indices()
                        .take(200)
                        .last()
                        .map(|(i, _)| i)
                        .unwrap_or(200)]
                )
            } else {
                desc
            };
            let distance_km = row.distance.map(|d| d / 1000.0);
            FinderResult {
                slug: row.slug,
                entity_type: row.entity_type,
                title: row.title,
                location_display: row.location_display,
                image: row.image,
                description_snippet: snippet,
                distance_km,
            }
        })
        .collect())
}

async fn search_location_only(lat: f64, lng: f64) -> Result<Vec<FinderResult>, Error> {
    // Query nearby projects
    let project_rows = sqlx::query!(
        r#"SELECT e.slug as "slug: String",
                  p.name as "title: String",
                  p.description as "description: String",
                  p.image as "image: String",
                  CASE
                    WHEN p.entirely_online = 1 THEN 'Online'
                    WHEN p.city != '' AND p.state != '' THEN p.city || ', ' || p.state
                    WHEN p.location_name != '' THEN p.location_name
                    ELSE ''
                  END as "location_display: String",
                  ST_Distance(p.point, MakePoint(?1, ?2, 4326), 1) as "distance: Option<f64>",
                  CASE WHEN p.regions IS NOT NULL AND ST_Contains(p.regions, MakePoint(?1, ?2, 4326))
                       THEN ST_Area(p.regions, 1)
                       ELSE NULL
                  END as "region_area: Option<f64>"
           FROM entities e
           JOIN projects p ON e.target = p.project_id
           WHERE e.kind = 'Project'
             AND e.publish = 1
             AND (p.point IS NOT NULL OR p.regions IS NOT NULL OR p.entirely_online = 1)
           ORDER BY
             CASE WHEN p.regions IS NOT NULL AND ST_Contains(p.regions, MakePoint(?1, ?2, 4326))
                  THEN 0 ELSE 1 END,
             ST_Distance(p.point, MakePoint(?1, ?2, 4326), 1)
           LIMIT 50"#,
        lng,
        lat
    )
    .fetch_all(db())
    .await?;

    // Query nearby events
    let event_rows = sqlx::query!(
        r#"SELECT e.slug as "slug: String",
                  ev.name as "title: String",
                  ev.description as "description: String",
                  ev.image as "image: String",
                  CASE
                    WHEN ev.entirely_online = 1 THEN 'Online'
                    WHEN ev.city != '' AND ev.state != '' THEN ev.city || ', ' || ev.state
                    WHEN ev.location_name != '' THEN ev.location_name
                    ELSE ''
                  END as "location_display: String",
                  ST_Distance(ev.point, MakePoint(?1, ?2, 4326), 1) as "distance: Option<f64>",
                  CASE WHEN ev.regions IS NOT NULL AND ST_Contains(ev.regions, MakePoint(?1, ?2, 4326))
                       THEN ST_Area(ev.regions, 1)
                       ELSE NULL
                  END as "region_area: Option<f64>"
           FROM entities e
           JOIN events ev ON e.target = ev.event_id
           WHERE e.kind = 'Event'
             AND e.publish = 1
             AND (ev.point IS NOT NULL OR ev.regions IS NOT NULL OR ev.entirely_online = 1)
           ORDER BY
             CASE WHEN ev.regions IS NOT NULL AND ST_Contains(ev.regions, MakePoint(?1, ?2, 4326))
                  THEN 0 ELSE 1 END,
             ST_Distance(ev.point, MakePoint(?1, ?2, 4326), 1)
           LIMIT 50"#,
        lng,
        lat
    )
    .fetch_all(db())
    .await?;

    let mut spatial: Vec<SpatialRow> = Vec::new();

    for row in project_rows {
        spatial.push(SpatialRow {
            slug: row.slug,
            entity_type: "Project".to_string(),
            title: row.title,
            description: row.description,
            image: row.image,
            location_display: row.location_display,
            distance: row.distance.flatten(),
            region_area: row.region_area.flatten(),
        });
    }

    for row in event_rows {
        spatial.push(SpatialRow {
            slug: row.slug,
            entity_type: "Event".to_string(),
            title: row.title,
            description: row.description,
            image: row.image,
            location_display: row.location_display,
            distance: row.distance.flatten(),
            region_area: row.region_area.flatten(),
        });
    }

    // Sort: region containment first (smaller area wins), then by distance
    spatial.sort_by(|a, b| match (&a.region_area, &b.region_area) {
        (Some(area_a), Some(area_b)) => area_a
            .partial_cmp(area_b)
            .unwrap_or(std::cmp::Ordering::Equal),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => {
            let dist_a = a.distance.unwrap_or(f64::MAX);
            let dist_b = b.distance.unwrap_or(f64::MAX);
            dist_a
                .partial_cmp(&dist_b)
                .unwrap_or(std::cmp::Ordering::Equal)
        }
    });

    spatial.truncate(50);

    Ok(spatial
        .into_iter()
        .map(|row| {
            let desc = row.description;
            let snippet = if desc.len() > 200 {
                format!(
                    "{}...",
                    &desc[..desc
                        .char_indices()
                        .take(200)
                        .last()
                        .map(|(i, _)| i)
                        .unwrap_or(200)]
                )
            } else {
                desc
            };
            let distance_km = row.distance.map(|d| d / 1000.0);
            FinderResult {
                slug: row.slug,
                entity_type: row.entity_type,
                title: row.title,
                location_display: row.location_display,
                image: row.image,
                description_snippet: snippet,
                distance_km,
            }
        })
        .collect())
}

#[handler]
pub async fn handle_finder(
    _session: &Session,
    Query(query): Query<FinderQuery>,
    Authenticated(user_info): Authenticated,
) -> Result<Response, poem::Error> {
    let q = query.q.as_deref().unwrap_or("").trim().to_string();
    let lat = query.lat;
    let lng = query.lng;
    let location_name = query.location_name.unwrap_or_default();

    let has_location = lat.is_some() && lng.is_some();
    let has_query = !q.is_empty();

    let results = if !has_query && !has_location {
        Vec::new()
    } else if has_location {
        let lat_val = lat.unwrap();
        let lng_val = lng.unwrap();
        if has_query {
            search_with_location(&q, lat_val, lng_val)
                .await
                .unwrap_or_default()
        } else {
            search_location_only(lat_val, lng_val)
                .await
                .unwrap_or_default()
        }
    } else {
        search_text_only(&q).await.unwrap_or_default()
    };

    let lat_str = lat.map(|v| v.to_string()).unwrap_or_default();
    let lng_str = lng.map(|v| v.to_string()).unwrap_or_default();

    let layout = StandardLayout::new(
        "/finder",
        FinderContent {
            results,
            query: q,
            location_name,
            lat: lat_str,
            lng: lng_str,
            geosearch_key: env!("GEOSEARCH_KEY").to_string(),
        },
    )
    .prepare(user_info.clone())
    .await
    .map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    FinderPage {
        head: Head::default().with_geosearch().prepare(None).await.map_err(|e| {
            poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
        })?,
        layout,
    }
    .into_response(None)
    .await
    .map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })
}
