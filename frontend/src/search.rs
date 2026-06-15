use crate::{
    component::{Component, Head, StandardLayout},
    form::QsQuery,
    local_time::{TimeFormat, local_time},
    middleware::{
        analytics::{Device, Interaction},
        authorization::Authenticated,
    },
};
use anyhow::Error;
use common::shared::db;
use poem::{Endpoint, Response, Result, Route, get, handler, session::Session, web::Query};
use sailfish::TemplateOnce;
use serde::Deserialize;

/// A search result for public entity search
#[derive(Debug, Clone)]
pub struct SearchResult {
    pub slug: String,
    pub entity_type: String,
    pub title: String,
}

impl SearchResult {
    /// Canonical link for this result based on its entity type.
    pub fn link(&self) -> String {
        format!("{}{}", path_prefix_for(&self.entity_type), self.slug)
    }
}

/// Map an entity-type label to its URL path prefix. Accepts both the
/// stored kind name (`Project`/`Event`) and the autocomplete display
/// label, so it works for every search result type.
fn path_prefix_for(entity_type: &str) -> &'static str {
    match entity_type {
        "Project" => "/projects/",
        "Event" => "/events/",
        _ => "/",
    }
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
    pub page: Option<u32>,
    pub location_type: Option<String>,
    #[serde(default)]
    pub topic: Vec<String>,
    #[serde(default)]
    pub audience: Vec<String>,
    #[serde(default)]
    pub activity: Vec<String>,
    pub show_completed: Option<String>,
    #[serde(default)]
    pub misc_filter: Vec<String>,
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
    pub goal: String,
    pub task: String,
    pub event_begin: Option<String>,
    pub is_completed: bool,
    pub geo_json: Option<String>,
}

impl FinderResult {
    /// Canonical link for this result based on its entity type.
    pub fn link(&self) -> String {
        format!("{}{}", path_prefix_for(&self.entity_type), self.slug)
    }

    pub fn distance_mi(&self) -> Option<f64> {
        self.distance_km.map(|km| km * 0.621371)
    }

    pub fn event_date_html(&self, format: TimeFormat) -> String {
        local_time(self.event_begin.as_deref().unwrap_or(""), format)
    }
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
    // Pagination
    current_page: u32,
    has_previous: bool,
    has_next: bool,
    total_pages: u32,
    // Filter options
    available_topics: Vec<String>,
    available_audiences: Vec<String>,
    available_activities: Vec<String>,
    // Current filter state
    location_type: String,
    selected_topics: Vec<String>,
    selected_audiences: Vec<String>,
    selected_activities: Vec<String>,
    show_completed: bool,
    has_classroom_materials: bool,
    has_app: bool,
    is_affiliate: bool,
    usa_only: bool,
    outside_usa: bool,
    exclusively_outdoors: bool,
}

impl FinderContent {
    fn pagination_params(&self, page: u32) -> String {
        let mut params = Vec::new();
        params.push(format!("page={}", page));
        if !self.query.is_empty() {
            params.push(format!("q={}", urlencoding::encode(&self.query)));
        }
        if !self.lat.is_empty() {
            params.push(format!("lat={}", urlencoding::encode(&self.lat)));
        }
        if !self.lng.is_empty() {
            params.push(format!("lng={}", urlencoding::encode(&self.lng)));
        }
        if !self.location_name.is_empty() {
            params.push(format!(
                "location_name={}",
                urlencoding::encode(&self.location_name)
            ));
        }
        if self.location_type != "either" {
            params.push(format!("location_type={}", &self.location_type));
        }
        for t in &self.selected_topics {
            params.push(format!("topic[]={}", urlencoding::encode(t)));
        }
        for a in &self.selected_audiences {
            params.push(format!("audience[]={}", urlencoding::encode(a)));
        }
        for a in &self.selected_activities {
            params.push(format!("activity[]={}", urlencoding::encode(a)));
        }
        if self.show_completed {
            params.push("show_completed=on".to_string());
        }
        for (flag, key) in [
            (self.has_classroom_materials, "has_classroom_materials"),
            (self.has_app, "has_app"),
            (self.is_affiliate, "is_affiliate"),
            (self.usa_only, "usa_only"),
            (self.outside_usa, "outside_usa"),
            (self.exclusively_outdoors, "exclusively_outdoors"),
        ] {
            if flag {
                params.push(format!("misc_filter[]={}", key));
            }
        }
        params.join("&")
    }
}

const FINDER_PER_PAGE: u32 = 24;

/// Load available filter options from the database
async fn load_filter_options() -> Result<(Vec<String>, Vec<String>, Vec<String>), Error> {
    let topics = sqlx::query_scalar!("SELECT DISTINCT topic FROM project_topics ORDER BY topic")
        .fetch_all(db())
        .await?;
    let audiences =
        sqlx::query_scalar!("SELECT DISTINCT audience FROM project_audiences ORDER BY audience")
            .fetch_all(db())
            .await?;
    let activities =
        sqlx::query_scalar!("SELECT DISTINCT activity FROM project_activities ORDER BY activity")
            .fetch_all(db())
            .await?;
    Ok((topics, audiences, activities))
}

/// Apply project-specific filters to a QueryBuilder
fn apply_project_filters<'a>(
    qb: &mut sqlx::QueryBuilder<'a, sqlx::Sqlite>,
    query: &'a FinderQuery,
) {
    // Filter out bad geographic data: (0,0) points and swapped lat/lng
    qb.push(" AND NOT (p.point IS NOT NULL AND ST_X(p.point) = 0 AND ST_Y(p.point) = 0)");
    qb.push(" AND NOT (p.point IS NOT NULL AND ABS(ST_Y(p.point)) > 90)");

    let location_type = query.location_type.as_deref().unwrap_or("either");
    match location_type {
        "remote" => {
            qb.push(" AND p.entirely_online = 1");
        }
        "onsite" => {
            qb.push(" AND p.in_person = 1");
        }
        _ => {}
    }

    if query.show_completed.is_none() {
        qb.push(" AND p.status != 'Complete'");
        qb.push(" AND (p.end IS NULL OR p.end = '' OR p.end >= datetime('now'))");
    }

    if !query.topic.is_empty() {
        qb.push(" AND p.project_id IN (SELECT project_id FROM project_topics WHERE topic IN (");
        let mut sep = qb.separated(", ");
        for t in &query.topic {
            sep.push_bind(t);
        }
        qb.push("))");
    }

    if !query.audience.is_empty() {
        qb.push(
            " AND p.project_id IN (SELECT project_id FROM project_audiences WHERE audience IN (",
        );
        let mut sep = qb.separated(", ");
        for a in &query.audience {
            sep.push_bind(a);
        }
        qb.push("))");
    }

    if !query.activity.is_empty() {
        qb.push(
            " AND p.project_id IN (SELECT project_id FROM project_activities WHERE activity IN (",
        );
        let mut sep = qb.separated(", ");
        for a in &query.activity {
            sep.push_bind(a);
        }
        qb.push("))");
    }

    let mf = &query.misc_filter;
    if mf.iter().any(|v| v == "has_classroom_materials") {
        qb.push(" AND p.classroom_materials != ''");
    }
    if mf.iter().any(|v| v == "has_app") {
        qb.push(
            " AND (p.mobile_app_url != '' OR p.mobile_app_url_android != '' OR p.mobile_app_url_ios != '')",
        );
    }
    if mf.iter().any(|v| v == "is_affiliate") {
        qb.push(" AND p.is_affiliate = 1");
    }
    if mf.iter().any(|v| v == "usa_only") {
        qb.push(" AND p.country = 'United States'");
    }
    if mf.iter().any(|v| v == "outside_usa") {
        qb.push(" AND p.country != 'United States' AND p.country != ''");
    }
    if mf.iter().any(|v| v == "exclusively_outdoors") {
        qb.push(" AND p.outdoors = 1 AND p.indoors = 0");
    }
}

/// Apply event-specific filters to a QueryBuilder
fn apply_event_filters<'a>(qb: &mut sqlx::QueryBuilder<'a, sqlx::Sqlite>, query: &'a FinderQuery) {
    // Filter out bad geographic data: (0,0) points and swapped lat/lng
    qb.push(" AND NOT (ev.point IS NOT NULL AND ST_X(ev.point) = 0 AND ST_Y(ev.point) = 0)");
    qb.push(" AND NOT (ev.point IS NOT NULL AND ABS(ST_Y(ev.point)) > 90)");

    let location_type = query.location_type.as_deref().unwrap_or("either");
    match location_type {
        "remote" => {
            qb.push(" AND ev.entirely_online = 1");
        }
        "onsite" => {
            qb.push(" AND ev.entirely_online = 0");
        }
        _ => {}
    }

    if query.show_completed.is_none() {
        qb.push(" AND ev.event_id IN (SELECT ei.event_id FROM event_instances ei WHERE ei.begin >= datetime('now'))");
    }

    if !query.topic.is_empty() {
        qb.push(" AND ev.event_id IN (SELECT event_id FROM event_tags WHERE tag IN (");
        let mut sep = qb.separated(", ");
        for t in &query.topic {
            sep.push_bind(t);
        }
        qb.push("))");
    }
}

fn make_snippet(desc: &str) -> String {
    if desc.len() > 200 {
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
        desc.to_string()
    }
}

struct FinderSearchResult {
    results: Vec<FinderResult>,
    total: u32,
}

/// Count projects matching text search + filters
fn build_project_count_text<'a>(
    search_term: &'a str,
    fq: &'a FinderQuery,
) -> sqlx::QueryBuilder<'a, sqlx::Sqlite> {
    let mut qb = sqlx::QueryBuilder::new(
        "SELECT COUNT(*) FROM entities_fts f JOIN entities e ON f.slug = e.slug JOIN projects p ON e.target = p.project_id WHERE e.kind = 'Project' AND entities_fts MATCH ",
    );
    qb.push_bind(search_term);
    qb.push(" AND e.publish = 1");
    apply_project_filters(&mut qb, fq);
    qb
}

/// Count events matching text search + filters
fn build_event_count_text<'a>(
    search_term: &'a str,
    fq: &'a FinderQuery,
) -> sqlx::QueryBuilder<'a, sqlx::Sqlite> {
    let mut qb = sqlx::QueryBuilder::new(
        "SELECT COUNT(*) FROM entities_fts f JOIN entities e ON f.slug = e.slug JOIN events ev ON e.target = ev.event_id WHERE e.kind = 'Event' AND entities_fts MATCH ",
    );
    qb.push_bind(search_term);
    qb.push(" AND e.publish = 1");
    apply_event_filters(&mut qb, fq);
    qb
}

/// Search all published entities with no text or location filter, just pagination and sidebar filters
async fn search_all(fq: &FinderQuery, page: u32) -> Result<FinderSearchResult, Error> {
    let limit = FINDER_PER_PAGE as i64;
    let offset = ((page - 1) * FINDER_PER_PAGE) as i64;

    // Count
    let mut proj_qb: sqlx::QueryBuilder<sqlx::Sqlite> = sqlx::QueryBuilder::new(
        "SELECT COUNT(*) FROM entities e JOIN projects p ON e.target = p.project_id WHERE e.kind = 'Project' AND e.publish = 1",
    );
    apply_project_filters(&mut proj_qb, fq);
    let proj_count: i64 = proj_qb.build_query_scalar().fetch_one(db()).await?;

    let mut evt_qb: sqlx::QueryBuilder<sqlx::Sqlite> = sqlx::QueryBuilder::new(
        "SELECT COUNT(*) FROM entities e JOIN events ev ON e.target = ev.event_id WHERE e.kind = 'Event' AND e.publish = 1",
    );
    apply_event_filters(&mut evt_qb, fq);
    let evt_count: i64 = evt_qb.build_query_scalar().fetch_one(db()).await?;
    let total = (proj_count + evt_count) as u32;

    // Build UNION ALL query
    let mut qb: sqlx::QueryBuilder<sqlx::Sqlite> = sqlx::QueryBuilder::new("SELECT * FROM (");

    // Project subquery
    qb.push(
        "SELECT e.slug, 'Project' AS entity_type, p.name AS title, p.description, p.image, \
         p.goal, p.task, p.status, \
         CASE WHEN p.entirely_online = 1 THEN 'Online' \
              WHEN p.city != '' AND p.state != '' THEN p.city || ', ' || p.state \
              WHEN p.location_name != '' THEN p.location_name \
              ELSE '' END AS location_display, \
         NULL AS event_begin, \
         CASE WHEN p.regions IS NOT NULL THEN AsGeoJSON(p.regions) \
              WHEN p.point IS NOT NULL THEN AsGeoJSON(p.point) \
              ELSE NULL END AS geo_json, \
         0.0 AS comparison_value \
         FROM entities e JOIN projects p ON e.target = p.project_id \
         WHERE e.kind = 'Project' AND e.publish = 1",
    );
    apply_project_filters(&mut qb, fq);

    qb.push(" UNION ALL ");

    // Event subquery
    qb.push(
        "SELECT e.slug, 'Event' AS entity_type, ev.name AS title, ev.description, ev.image, \
         '' AS goal, '' AS task, '' AS status, \
         CASE WHEN ev.entirely_online = 1 THEN 'Online' \
              WHEN ev.city != '' AND ev.state != '' THEN ev.city || ', ' || ev.state \
              WHEN ev.location_name != '' THEN ev.location_name \
              ELSE '' END AS location_display, \
         (SELECT ei.begin FROM event_instances ei WHERE ei.event_id = ev.event_id \
          AND ei.begin >= datetime('now') ORDER BY ei.begin LIMIT 1) AS event_begin, \
         CASE WHEN ev.regions IS NOT NULL THEN AsGeoJSON(ev.regions) \
              WHEN ev.point IS NOT NULL THEN AsGeoJSON(ev.point) \
              ELSE NULL END AS geo_json, \
         COALESCE(MAX(0.0, julianday((SELECT ei2.begin FROM event_instances ei2 \
          WHERE ei2.event_id = ev.event_id AND ei2.begin >= datetime('now') \
          ORDER BY ei2.begin LIMIT 1)) - julianday('now')), 0.0) * 10.0 * 1.0 AS comparison_value \
         FROM entities e JOIN events ev ON e.target = ev.event_id \
         WHERE e.kind = 'Event' AND e.publish = 1",
    );
    apply_event_filters(&mut qb, fq);

    qb.push(") ORDER BY comparison_value LIMIT ");
    qb.push_bind(limit);
    qb.push(" OFFSET ");
    qb.push_bind(offset);

    let rows = qb
        .build_query_as::<(
            String,         // slug
            String,         // entity_type
            String,         // title
            String,         // description
            String,         // image
            String,         // goal
            String,         // task
            String,         // status
            String,         // location_display
            Option<String>, // event_begin
            Option<String>, // geo_json
            f64,            // comparison_value
        )>()
        .fetch_all(db())
        .await?;

    let results = rows
        .into_iter()
        .map(|row| FinderResult {
            slug: row.0,
            entity_type: row.1,
            title: row.2,
            description_snippet: make_snippet(&row.3),
            image: row.4,
            goal: row.5,
            task: row.6,
            is_completed: row.7 == "Complete",
            location_display: row.8,
            event_begin: row.9,
            distance_km: None,
            geo_json: row.10,
        })
        .collect();

    Ok(FinderSearchResult { results, total })
}

async fn search_text_only(fq: &FinderQuery, page: u32) -> Result<FinderSearchResult, Error> {
    let q = fq.q.as_deref().unwrap_or("");
    let search_term = format!("{}*", q);
    let limit = FINDER_PER_PAGE as i64;
    let offset = ((page - 1) * FINDER_PER_PAGE) as i64;

    // Count
    let proj_count: i64 = build_project_count_text(&search_term, fq)
        .build_query_scalar()
        .fetch_one(db())
        .await?;
    let evt_count: i64 = build_event_count_text(&search_term, fq)
        .build_query_scalar()
        .fetch_one(db())
        .await?;
    let total = (proj_count + evt_count) as u32;

    // Build UNION ALL query
    let mut qb: sqlx::QueryBuilder<sqlx::Sqlite> = sqlx::QueryBuilder::new("SELECT * FROM (");

    // Project subquery
    qb.push(
        "SELECT f.slug, 'Project' AS entity_type, f.title, p.description, p.image, \
         p.goal, p.task, p.status, \
         CASE WHEN p.entirely_online = 1 THEN 'Online' \
              WHEN p.city != '' AND p.state != '' THEN p.city || ', ' || p.state \
              WHEN p.location_name != '' THEN p.location_name \
              ELSE '' END AS location_display, \
         NULL AS event_begin, \
         CASE WHEN p.regions IS NOT NULL THEN AsGeoJSON(p.regions) \
              WHEN p.point IS NOT NULL THEN AsGeoJSON(p.point) \
              ELSE NULL END AS geo_json, \
         0.0 AS comparison_value \
         FROM entities_fts f \
         JOIN entities e ON f.slug = e.slug \
         JOIN projects p ON e.target = p.project_id \
         WHERE e.kind = 'Project' AND entities_fts MATCH",
    );
    qb.push_bind(search_term.clone());
    qb.push(" AND e.publish = 1");
    apply_project_filters(&mut qb, fq);

    qb.push(" UNION ALL ");

    // Event subquery
    qb.push(
        "SELECT f.slug, 'Event' AS entity_type, f.title, ev.description, ev.image, \
         '' AS goal, '' AS task, '' AS status, \
         CASE WHEN ev.entirely_online = 1 THEN 'Online' \
              WHEN ev.city != '' AND ev.state != '' THEN ev.city || ', ' || ev.state \
              WHEN ev.location_name != '' THEN ev.location_name \
              ELSE '' END AS location_display, \
         (SELECT ei.begin FROM event_instances ei WHERE ei.event_id = ev.event_id \
          AND ei.begin >= datetime('now') ORDER BY ei.begin LIMIT 1) AS event_begin, \
         CASE WHEN ev.regions IS NOT NULL THEN AsGeoJSON(ev.regions) \
              WHEN ev.point IS NOT NULL THEN AsGeoJSON(ev.point) \
              ELSE NULL END AS geo_json, \
         COALESCE(MAX(0.0, julianday((SELECT ei2.begin FROM event_instances ei2 \
          WHERE ei2.event_id = ev.event_id AND ei2.begin >= datetime('now') \
          ORDER BY ei2.begin LIMIT 1)) - julianday('now')), 0.0) * 10.0 * 1.0 AS comparison_value \
         FROM entities_fts f \
         JOIN entities e ON f.slug = e.slug \
         JOIN events ev ON e.target = ev.event_id \
         WHERE e.kind = 'Event' AND entities_fts MATCH ",
    );
    qb.push_bind(search_term.clone());
    qb.push(" AND e.publish = 1");
    apply_event_filters(&mut qb, fq);

    qb.push(") ORDER BY comparison_value LIMIT ");
    qb.push_bind(limit);
    qb.push(" OFFSET ");
    qb.push_bind(offset);

    let rows = qb
        .build_query_as::<(
            String,         // slug
            String,         // entity_type
            Option<String>, // title
            String,         // description
            String,         // image
            String,         // goal
            String,         // task
            String,         // status
            String,         // location_display
            Option<String>, // event_begin
            Option<String>, // geo_json
            f64,            // comparison_value
        )>()
        .fetch_all(db())
        .await?;

    let results = rows
        .into_iter()
        .map(|row| FinderResult {
            slug: row.0,
            entity_type: row.1,
            title: row.2.unwrap_or_default(),
            description_snippet: make_snippet(&row.3),
            image: row.4,
            goal: row.5,
            task: row.6,
            is_completed: row.7 == "Complete",
            location_display: row.8,
            event_begin: row.9,
            distance_km: None,
            geo_json: row.10,
        })
        .collect();

    Ok(FinderSearchResult { results, total })
}

async fn search_with_location(
    fq: &FinderQuery,
    lat: f64,
    lng: f64,
    page: u32,
) -> Result<FinderSearchResult, Error> {
    let q = fq.q.as_deref().unwrap_or("");
    let search_term = format!("{}*", q);

    // Count
    let proj_count: i64 = build_project_count_text(&search_term, fq)
        .build_query_scalar()
        .fetch_one(db())
        .await?;
    let evt_count: i64 = build_event_count_text(&search_term, fq)
        .build_query_scalar()
        .fetch_one(db())
        .await?;
    let total = (proj_count + evt_count) as u32;

    let limit = FINDER_PER_PAGE as i64;
    let offset = ((page - 1) * FINDER_PER_PAGE) as i64;

    // Build UNION ALL with spatial columns
    let mut qb: sqlx::QueryBuilder<sqlx::Sqlite> = sqlx::QueryBuilder::new("SELECT * FROM (");

    // Project subquery
    qb.push(
        "SELECT f.slug, 'Project' AS entity_type, f.title, p.description, p.image, \
         p.goal, p.task, p.status, \
         CASE WHEN p.entirely_online = 1 THEN 'Online' \
              WHEN p.city != '' AND p.state != '' THEN p.city || ', ' || p.state \
              WHEN p.location_name != '' THEN p.location_name \
              ELSE '' END AS location_display, \
         NULL AS event_begin, \
         CASE WHEN p.entirely_online = 1 THEN NULL \
              WHEN p.regions IS NOT NULL THEN ST_Distance(ST_MakeValid(p.regions), MakePoint(",
    );
    qb.push_bind(lng);
    qb.push(", ");
    qb.push_bind(lat);
    qb.push(", 4326), 1) / 1000.0 WHEN p.point IS NOT NULL THEN ST_Distance(p.point, MakePoint(");
    qb.push_bind(lng);
    qb.push(", ");
    qb.push_bind(lat);
    qb.push(
        ", 4326), 1) / 1000.0 \
         ELSE NULL END AS distance_km, \
         CASE WHEN p.regions IS NOT NULL THEN AsGeoJSON(p.regions) \
              WHEN p.point IS NOT NULL THEN AsGeoJSON(p.point) \
              ELSE NULL END AS geo_json, \
         CASE WHEN p.entirely_online = 1 THEN 200.0 \
              WHEN p.regions IS NOT NULL THEN COALESCE(ST_Distance(ST_MakeValid(p.regions), MakePoint(",
    );
    qb.push_bind(lng);
    qb.push(", ");
    qb.push_bind(lat);
    qb.push(
        ", 4326), 1) / 1000.0, 0) + COALESCE(NULLIF(SQRT(ST_Area(ST_MakeValid(p.regions), 1) / 1000000.0), 0.0), 22583.0) \
              WHEN p.point IS NOT NULL THEN COALESCE(ST_Distance(p.point, MakePoint(",
    );
    qb.push_bind(lng);
    qb.push(", ");
    qb.push_bind(lat);
    qb.push(
        ", 4326), 1) / 1000.0, 22583.0) \
              ELSE 0.0 END * 1.0 AS comparison_value \
         FROM entities_fts f \
         JOIN entities e ON f.slug = e.slug \
         JOIN projects p ON e.target = p.project_id \
         WHERE e.kind = 'Project' AND entities_fts MATCH ",
    );
    qb.push_bind(search_term.clone());
    qb.push(" AND e.publish = 1");
    apply_project_filters(&mut qb, fq);
    qb.push(" GROUP BY f.slug");

    qb.push(" UNION ALL ");

    // Event subquery
    qb.push(
        "SELECT f.slug, 'Event' AS entity_type, f.title, ev.description, ev.image, \
         '' AS goal, '' AS task, '' AS status, \
         CASE WHEN ev.entirely_online = 1 THEN 'Online' \
              WHEN ev.city != '' AND ev.state != '' THEN ev.city || ', ' || ev.state \
              WHEN ev.location_name != '' THEN ev.location_name \
              ELSE '' END AS location_display, \
         (SELECT ei.begin FROM event_instances ei WHERE ei.event_id = ev.event_id \
          AND ei.begin >= datetime('now') ORDER BY ei.begin LIMIT 1) AS event_begin, \
         CASE WHEN ev.entirely_online = 1 THEN NULL \
              WHEN ev.regions IS NOT NULL THEN ST_Distance(ST_MakeValid(ev.regions), MakePoint(",
    );
    qb.push_bind(lng);
    qb.push(", ");
    qb.push_bind(lat);
    qb.push(", 4326), 1) / 1000.0 WHEN ev.point IS NOT NULL THEN ST_Distance(ev.point, MakePoint(");
    qb.push_bind(lng);
    qb.push(", ");
    qb.push_bind(lat);
    qb.push(
        ", 4326), 1) / 1000.0 \
         ELSE NULL END AS distance_km, \
         CASE WHEN ev.regions IS NOT NULL THEN AsGeoJSON(ev.regions) \
              WHEN ev.point IS NOT NULL THEN AsGeoJSON(ev.point) \
              ELSE NULL END AS geo_json, \
         COALESCE(MAX(0.0, julianday((SELECT ei2.begin FROM event_instances ei2 \
          WHERE ei2.event_id = ev.event_id AND ei2.begin >= datetime('now') \
          ORDER BY ei2.begin LIMIT 1)) - julianday('now')), 0.0) * 10.0 \
         + CASE WHEN ev.entirely_online = 1 THEN 100.0 \
                WHEN ev.regions IS NOT NULL THEN COALESCE(ST_Distance(ST_MakeValid(ev.regions), MakePoint(",
    );
    qb.push_bind(lng);
    qb.push(", ");
    qb.push_bind(lat);
    qb.push(
        ", 4326), 1) / 1000.0, 0) \
                WHEN ev.point IS NOT NULL THEN COALESCE(ST_Distance(ev.point, MakePoint(",
    );
    qb.push_bind(lng);
    qb.push(", ");
    qb.push_bind(lat);
    qb.push(
        ", 4326), 1) / 1000.0, 22583.0) \
                ELSE 0.0 END * 1.0 AS comparison_value \
         FROM entities_fts f \
         JOIN entities e ON f.slug = e.slug \
         JOIN events ev ON e.target = ev.event_id \
         WHERE e.kind = 'Event' AND entities_fts MATCH",
    );
    qb.push_bind(search_term.clone());
    qb.push(" AND e.publish = 1");
    apply_event_filters(&mut qb, fq);
    qb.push(" GROUP BY f.slug");

    qb.push(") ORDER BY comparison_value LIMIT ");
    qb.push_bind(limit);
    qb.push(" OFFSET ");
    qb.push_bind(offset);

    let rows = qb
        .build_query_as::<(
            String,         // slug
            String,         // entity_type
            Option<String>, // title
            String,         // description
            String,         // image
            String,         // goal
            String,         // task
            String,         // status
            String,         // location_display
            Option<String>, // event_begin
            Option<f64>,    // distance_km
            Option<String>, // geo_json
            f64,            // comparison_value
        )>()
        .fetch_all(db())
        .await?;

    let results = rows
        .into_iter()
        .map(|row| FinderResult {
            slug: row.0,
            entity_type: row.1,
            title: row.2.unwrap_or_default(),
            description_snippet: make_snippet(&row.3),
            image: row.4,
            goal: row.5,
            task: row.6,
            is_completed: row.7 == "Complete",
            location_display: row.8,
            event_begin: row.9,
            distance_km: row.10,
            geo_json: row.11,
        })
        .collect();

    Ok(FinderSearchResult { results, total })
}

/// Count projects matching location-only + filters
fn build_project_count_location<'a>(fq: &'a FinderQuery) -> sqlx::QueryBuilder<'a, sqlx::Sqlite> {
    let mut qb = sqlx::QueryBuilder::new(
        "SELECT COUNT(*) FROM entities e JOIN projects p ON e.target = p.project_id WHERE e.kind = 'Project' AND e.publish = 1 AND (p.point IS NOT NULL OR p.regions IS NOT NULL OR p.entirely_online = 1)",
    );
    apply_project_filters(&mut qb, fq);
    qb
}

/// Count events matching location-only + filters
fn build_event_count_location<'a>(fq: &'a FinderQuery) -> sqlx::QueryBuilder<'a, sqlx::Sqlite> {
    let mut qb = sqlx::QueryBuilder::new(
        "SELECT COUNT(*) FROM entities e JOIN events ev ON e.target = ev.event_id WHERE e.kind = 'Event' AND e.publish = 1 AND (ev.point IS NOT NULL OR ev.regions IS NOT NULL OR ev.entirely_online = 1)",
    );
    apply_event_filters(&mut qb, fq);
    qb
}

async fn search_location_only(
    fq: &FinderQuery,
    lat: f64,
    lng: f64,
    page: u32,
) -> Result<FinderSearchResult, Error> {
    // Count
    let proj_count: i64 = build_project_count_location(fq)
        .build_query_scalar()
        .fetch_one(db())
        .await?;
    let evt_count: i64 = build_event_count_location(fq)
        .build_query_scalar()
        .fetch_one(db())
        .await?;
    let total = (proj_count + evt_count) as u32;

    let limit = FINDER_PER_PAGE as i64;
    let offset = ((page - 1) * FINDER_PER_PAGE) as i64;

    let mut qb: sqlx::QueryBuilder<sqlx::Sqlite> = sqlx::QueryBuilder::new("SELECT * FROM (");

    // Project subquery
    qb.push(
        "SELECT e.slug, 'Project' AS entity_type, p.name AS title, p.description, p.image, \
         p.goal, p.task, p.status, \
         CASE WHEN p.entirely_online = 1 THEN 'Online' \
              WHEN p.city != '' AND p.state != '' THEN p.city || ', ' || p.state \
              WHEN p.location_name != '' THEN p.location_name \
              ELSE '' END AS location_display, \
         NULL AS event_begin, \
         CASE WHEN p.entirely_online = 1 THEN NULL \
              WHEN p.regions IS NOT NULL THEN ST_Distance(ST_MakeValid(p.regions), MakePoint(",
    );
    qb.push_bind(lng);
    qb.push(", ");
    qb.push_bind(lat);
    qb.push(", 4326), 1) / 1000.0 WHEN p.point IS NOT NULL THEN ST_Distance(p.point, MakePoint(");
    qb.push_bind(lng);
    qb.push(", ");
    qb.push_bind(lat);
    qb.push(
        ", 4326), 1) / 1000.0 \
         ELSE NULL END AS distance_km, \
         CASE WHEN p.regions IS NOT NULL THEN AsGeoJSON(p.regions) \
              WHEN p.point IS NOT NULL THEN AsGeoJSON(p.point) \
              ELSE NULL END AS geo_json, \
         CASE WHEN p.entirely_online = 1 THEN 200.0 \
              WHEN p.regions IS NOT NULL THEN COALESCE(ST_Distance(ST_MakeValid(p.regions), MakePoint(",
    );
    qb.push_bind(lng);
    qb.push(", ");
    qb.push_bind(lat);
    qb.push(
        ", 4326), 1) / 1000.0, 0) + COALESCE(NULLIF(SQRT(ST_Area(ST_MakeValid(p.regions), 1) / 1000000.0), 0.0), 22583.0) \
              WHEN p.point IS NOT NULL THEN COALESCE(ST_Distance(p.point, MakePoint(",
    );
    qb.push_bind(lng);
    qb.push(", ");
    qb.push_bind(lat);
    qb.push(
        ", 4326), 1) / 1000.0, 22583.0) \
              ELSE 0.0 END * 1.0 AS comparison_value \
         FROM entities e JOIN projects p ON e.target = p.project_id \
         WHERE e.kind = 'Project' AND e.publish = 1 \
         AND (p.point IS NOT NULL OR p.regions IS NOT NULL OR p.entirely_online = 1)",
    );
    apply_project_filters(&mut qb, fq);

    qb.push(" UNION ALL ");

    // Event subquery
    qb.push(
        "SELECT e.slug, 'Event' AS entity_type, ev.name AS title, ev.description, ev.image, \
         '' AS goal, '' AS task, '' AS status, \
         CASE WHEN ev.entirely_online = 1 THEN 'Online' \
              WHEN ev.city != '' AND ev.state != '' THEN ev.city || ', ' || ev.state \
              WHEN ev.location_name != '' THEN ev.location_name \
              ELSE '' END AS location_display, \
         (SELECT ei.begin FROM event_instances ei WHERE ei.event_id = ev.event_id \
          AND ei.begin >= datetime('now') ORDER BY ei.begin LIMIT 1) AS event_begin, \
         CASE WHEN ev.entirely_online = 1 THEN NULL \
              WHEN ev.regions IS NOT NULL THEN ST_Distance(ST_MakeValid(ev.regions), MakePoint(",
    );
    qb.push_bind(lng);
    qb.push(", ");
    qb.push_bind(lat);
    qb.push(", 4326), 1) / 1000.0 WHEN ev.point IS NOT NULL THEN ST_Distance(ev.point, MakePoint(");
    qb.push_bind(lng);
    qb.push(", ");
    qb.push_bind(lat);
    qb.push(
        ", 4326), 1) / 1000.0 \
         ELSE NULL END AS distance_km, \
         CASE WHEN ev.regions IS NOT NULL THEN AsGeoJSON(ev.regions) \
              WHEN ev.point IS NOT NULL THEN AsGeoJSON(ev.point) \
              ELSE NULL END AS geo_json, \
         COALESCE(MAX(0.0, julianday((SELECT ei2.begin FROM event_instances ei2 \
          WHERE ei2.event_id = ev.event_id AND ei2.begin >= datetime('now') \
          ORDER BY ei2.begin LIMIT 1)) - julianday('now')), 0.0) * 10.0 \
         + CASE WHEN ev.entirely_online = 1 THEN 100.0 \
                WHEN ev.regions IS NOT NULL THEN COALESCE(ST_Distance(ST_MakeValid(ev.regions), MakePoint(",
    );
    qb.push_bind(lng);
    qb.push(", ");
    qb.push_bind(lat);
    qb.push(
        ", 4326), 1) / 1000.0, 0) \
                WHEN ev.point IS NOT NULL THEN COALESCE(ST_Distance(ev.point, MakePoint(",
    );
    qb.push_bind(lng);
    qb.push(", ");
    qb.push_bind(lat);
    qb.push(
        ", 4326), 1) / 1000.0, 22583.0) \
                ELSE 0.0 END * 1.0 AS comparison_value \
         FROM entities e JOIN events ev ON e.target = ev.event_id \
         WHERE e.kind = 'Event' AND e.publish = 1 \
         AND (ev.point IS NOT NULL OR ev.regions IS NOT NULL OR ev.entirely_online = 1)",
    );
    apply_event_filters(&mut qb, fq);

    qb.push(") ORDER BY comparison_value LIMIT ");
    qb.push_bind(limit);
    qb.push(" OFFSET ");
    qb.push_bind(offset);

    let rows = qb
        .build_query_as::<(
            String,         // slug
            String,         // entity_type
            String,         // title
            String,         // description
            String,         // image
            String,         // goal
            String,         // task
            String,         // status
            String,         // location_display
            Option<String>, // event_begin
            Option<f64>,    // distance_km
            Option<String>, // geo_json
            f64,            // comparison_value
        )>()
        .fetch_all(db())
        .await?;

    let results = rows
        .into_iter()
        .map(|row| FinderResult {
            slug: row.0,
            entity_type: row.1,
            title: row.2,
            description_snippet: make_snippet(&row.3),
            image: row.4,
            goal: row.5,
            task: row.6,
            is_completed: row.7 == "Complete",
            location_display: row.8,
            event_begin: row.9,
            distance_km: row.10,
            geo_json: row.11,
        })
        .collect();

    Ok(FinderSearchResult { results, total })
}

#[handler]
pub async fn handle_finder(
    _session: &Session,
    QsQuery(query): QsQuery<FinderQuery>,
    Authenticated(user_info): Authenticated,
    device: Device,
    interaction: Interaction,
) -> Result<Response, poem::Error> {
    let q = query.q.as_deref().unwrap_or("").trim().to_string();
    let lat = query.lat;
    let lng = query.lng;
    let location_name = query.location_name.clone().unwrap_or_default();
    let page = query.page.unwrap_or(1).max(1);

    let has_location = lat.is_some() && lng.is_some();
    let has_query = !q.is_empty();

    let (available_topics, available_audiences, available_activities) =
        load_filter_options().await.map_err(|e| {
            poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
        })?;

    let search_result = if has_location {
        let lat_val = lat.unwrap();
        let lng_val = lng.unwrap();
        if has_query {
            search_with_location(&query, lat_val, lng_val, page)
                .await
                .unwrap_or_else(|e| {
                    tracing::error!("Finder search_with_location error: {}", e);
                    FinderSearchResult {
                        results: Vec::new(),
                        total: 0,
                    }
                })
        } else {
            search_location_only(&query, lat_val, lng_val, page)
                .await
                .unwrap_or_else(|e| {
                    tracing::error!("Finder search_location_only error: {}", e);
                    FinderSearchResult {
                        results: Vec::new(),
                        total: 0,
                    }
                })
        }
    } else if has_query {
        search_text_only(&query, page).await.unwrap_or_else(|e| {
            tracing::error!("Finder search_text_only error: {}", e);
            FinderSearchResult {
                results: Vec::new(),
                total: 0,
            }
        })
    } else {
        search_all(&query, page).await.unwrap_or_else(|e| {
            tracing::error!("Finder search_all error: {}", e);
            FinderSearchResult {
                results: Vec::new(),
                total: 0,
            }
        })
    };

    // Log search event
    {
        use common::message::log::SearchEvent;
        use common::service::send;
        use common::timestring::TimeString;

        let search_type = match (has_query, has_location) {
            (true, true) => "text_location",
            (false, true) => "location_only",
            (true, false) => "text_only",
            (false, false) => "browse_all",
        };

        let event = SearchEvent {
            when: TimeString::now().to_string(),
            device_id: device.as_ref().to_string(),
            interaction_id: interaction.as_ref().to_string(),
            user_id: user_info.as_ref().map(|u| u.user_id.to_string()),
            query_text: if q.is_empty() { None } else { Some(q.clone()) },
            lat: query.lat,
            lng: query.lng,
            location_name: if query.location_name.as_deref().unwrap_or("").is_empty() {
                None
            } else {
                query.location_name.clone()
            },
            location_type: query.location_type.clone(),
            topics: if query.topic.is_empty() {
                None
            } else {
                serde_json::to_string(&query.topic).ok()
            },
            audiences: if query.audience.is_empty() {
                None
            } else {
                serde_json::to_string(&query.audience).ok()
            },
            activities: if query.activity.is_empty() {
                None
            } else {
                serde_json::to_string(&query.activity).ok()
            },
            misc_filters: if query.misc_filter.is_empty() {
                None
            } else {
                serde_json::to_string(&query.misc_filter).ok()
            },
            show_completed: query.show_completed.is_some(),
            page: page as i32,
            result_count: search_result.total as i32,
            search_type: search_type.to_string(),
        };

        tokio::spawn(async move {
            if let Err(e) = send("log.search_event", event).await {
                tracing::warn!("Failed to log search event: {}", e);
            }
        });
    }

    let total_pages = if search_result.total == 0 {
        0
    } else {
        (search_result.total + FINDER_PER_PAGE - 1) / FINDER_PER_PAGE
    };

    let location_type = query
        .location_type
        .as_deref()
        .unwrap_or("either")
        .to_string();

    let lat_str = lat.map(|v| v.to_string()).unwrap_or_default();
    let lng_str = lng.map(|v| v.to_string()).unwrap_or_default();

    let layout = StandardLayout::new(
        "/finder",
        FinderContent {
            results: search_result.results,
            query: q,
            location_name,
            lat: lat_str,
            lng: lng_str,
            geosearch_key: env!("GEOSEARCH_KEY").to_string(),
            current_page: page,
            has_previous: page > 1,
            has_next: page < total_pages,
            total_pages,
            available_topics,
            available_audiences,
            available_activities,
            location_type,
            selected_topics: query.topic.clone(),
            selected_audiences: query.audience.clone(),
            selected_activities: query.activity.clone(),
            show_completed: query.show_completed.is_some(),
            has_classroom_materials: query
                .misc_filter
                .iter()
                .any(|v| v == "has_classroom_materials"),
            has_app: query.misc_filter.iter().any(|v| v == "has_app"),
            is_affiliate: query.misc_filter.iter().any(|v| v == "is_affiliate"),
            usa_only: query.misc_filter.iter().any(|v| v == "usa_only"),
            outside_usa: query.misc_filter.iter().any(|v| v == "outside_usa"),
            exclusively_outdoors: query
                .misc_filter
                .iter()
                .any(|v| v == "exclusively_outdoors"),
        },
    )
    .prepare(user_info.clone())
    .await
    .map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    FinderPage {
        head: Head::default()
            .with_geosearch()
            .with_maplibre()
            .prepare(None)
            .await
            .map_err(|e| {
                poem::Error::from_string(
                    e.to_string(),
                    poem::http::StatusCode::INTERNAL_SERVER_ERROR,
                )
            })?,
        layout,
    }
    .into_response(None)
    .await
    .map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })
}
