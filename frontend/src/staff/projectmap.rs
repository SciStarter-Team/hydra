//! GET `/staff/projectmap/map.html` and `/staff/projectmap/projects.geojson` —
//! where in the world SciStarter's projects can be taken part in. Ported from
//! orrery's `src/staff/projectmap.rs`.
//!
//! The map itself is drawn in the browser: the page fetches every project's
//! coverage geometry plus a country outline set, intersects the two with turf,
//! and shades each country by how many projects reach it. All this endpoint has
//! to do is hand over the coverage geometry.
//!
//! Only projects whose team drew a coverage region appear. A project that
//! merely says it is global has no geometry and is not counted, which the page
//! says in as many words.

use common::shared::db;
use poem::{Response, handler, web::Query};
use sailfish::TemplateOnce;
use serde::Deserialize;

/// The topic that `mode=ipbes` narrows the map to.
///
/// The IPBES report SciStarter contributes to is about ecology and the
/// environment, so that view answers "where can you do ecological citizen
/// science". orrery spells this as the integer id of a choice value; hydra
/// stores the topic by name.
const IPBES_TOPIC: &str = "Ecology & Environment";

/// How many projects one page of the streamed response carries. The geometry
/// dominates the response size, so the page is small.
const PAGE: i64 = 100;

/// The map's appearance, and which projects it covers. Every field has a
/// default, so a bare `/staff/projectmap/map.html` is a working map.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct MapParams {
    /// `ipbes` narrows to [`IPBES_TOPIC`]; anything else covers every project.
    mode: Option<String>,
    opacity: Option<f32>,
    borders: Option<bool>,
    red: Option<u8>,
    green: Option<u8>,
    blue: Option<u8>,
}

impl MapParams {
    fn is_ipbes(&self) -> bool {
        self.mode.as_deref() == Some("ipbes")
    }
}

#[derive(TemplateOnce)]
#[template(path = "staff/projectmap.stpl.html")]
struct ProjectMap {
    mode: Option<String>,
    /// How the prose describes the projects being counted, so the narrowed map
    /// does not claim to describe every project on the site.
    subject: &'static str,
    opacity: f32,
    borders: bool,
    red: u8,
    green: u8,
    blue: u8,
}

/// GET `/staff/projectmap/map.html` — the map page.
#[handler]
pub async fn map_html(Query(params): Query<MapParams>) -> Result<Response, anyhow::Error> {
    let subject = if params.is_ipbes() {
        "environmental and ecological"
    } else {
        "citizen science"
    };

    let page = ProjectMap {
        mode: params.mode,
        subject,
        opacity: params.opacity.unwrap_or(0.08),
        borders: params.borders.unwrap_or(false),
        red: params.red.unwrap_or(64),
        green: params.green.unwrap_or(200),
        blue: params.blue.unwrap_or(239),
    };

    Ok(Response::builder()
        .content_type("text/html; charset=utf-8")
        .body(page.render_once()?))
}

/// One project's coverage, already rendered as a GeoJSON `Feature`.
struct Feature {
    name: String,
    geometry: String,
}

impl Feature {
    fn render(&self) -> String {
        // The name goes through `serde_json` rather than a hand-rolled escape:
        // project names carry quotes, backslashes and newlines, and a feature
        // that fails to parse takes the whole map down with it.
        let name = serde_json::to_string(&self.name).unwrap_or_else(|_| String::from("\"\""));
        format!(
            r#"{{"type": "Feature", "properties": {{"name": {name}}}, "geometry": {}}}"#,
            self.geometry
        )
    }
}

/// One page of published projects that have coverage geometry.
async fn page(offset: i64, ipbes: bool) -> Result<Vec<Feature>, anyhow::Error> {
    let rows = if ipbes {
        sqlx::query_as!(
            Feature,
            r#"
            SELECT p.name AS "name!", AsGeoJSON(p.regions) AS "geometry!: String"
            FROM projects p
            JOIN entities e ON e.target = p.project_id AND e.kind = 'Project'
            WHERE e.publish = 1
              AND p.regions IS NOT NULL
              AND EXISTS (
                  SELECT 1 FROM project_topics t
                  WHERE t.project_id = p.project_id AND t.topic = ?1
              )
            ORDER BY p.project_id
            LIMIT ?2 OFFSET ?3
            "#,
            IPBES_TOPIC,
            PAGE,
            offset,
        )
        .fetch_all(db())
        .await?
    } else {
        sqlx::query_as!(
            Feature,
            r#"
            SELECT p.name AS "name!", AsGeoJSON(p.regions) AS "geometry!: String"
            FROM projects p
            JOIN entities e ON e.target = p.project_id AND e.kind = 'Project'
            WHERE e.publish = 1
              AND p.regions IS NOT NULL
            ORDER BY p.project_id
            LIMIT ?1 OFFSET ?2
            "#,
            PAGE,
            offset,
        )
        .fetch_all(db())
        .await?
    };

    Ok(rows)
}

/// GET `/staff/projectmap/projects.geojson` — the coverage geometry.
///
/// Streamed a page at a time. The whole collection is tens of megabytes of
/// polygon, and buffering it would hold that much per concurrent request.
#[handler]
#[tracing::instrument]
pub async fn projects_geojson(Query(params): Query<MapParams>) -> Response {
    let ipbes = params.is_ipbes();

    // `None` offset means the header has yet to be sent; `Some(n)` is the next
    // page to fetch. The stream ends after the page that comes back short.
    let stream = futures::stream::unfold(Some(0i64), move |offset| async move {
        let offset = offset?;

        if offset == 0 {
            return Some((
                Ok::<_, std::io::Error>(String::from(
                    "{\"type\": \"FeatureCollection\", \
                     \"crs\": {\"type\": \"name\", \"properties\": {\"name\": \"urn:ogc:def:crs:OGC:1.3:CRS84\"}}, \
                     \"features\": [",
                )),
                Some(1i64),
            ));
        }

        // Offsets are one page ahead of themselves so that offset 0 can carry
        // the header; page N covers rows [(N-1) * PAGE, N * PAGE).
        let rows = match page((offset - 1) * PAGE, ipbes).await {
            Ok(rows) => rows,
            Err(error) => {
                tracing::error!(%error, "projectmap page query failed");
                // The collection is closed rather than truncated mid-array, so
                // the client gets valid — if short — GeoJSON instead of a parse
                // error.
                return Some((Ok(String::from("]}")), None));
            }
        };

        let short = rows.len() < PAGE as usize;

        let mut chunk = String::new();
        for (index, feature) in rows.iter().enumerate() {
            if offset > 1 || index > 0 {
                chunk.push(',');
            }
            chunk.push('\n');
            chunk.push_str(&feature.render());
        }

        if short {
            chunk.push_str("\n]}");
        }

        Some((Ok(chunk), if short { None } else { Some(offset + 1) }))
    });

    Response::builder()
        .content_type("application/geo+json")
        .body(poem::Body::from_bytes_stream(stream))
}
