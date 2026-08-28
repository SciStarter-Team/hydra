//! GET `/staff/usermap/map.html` and `/staff/usermap/users.geojson` — where
//! SciStarter's accounts are. Ported from orrery's `src/staff/usermap.rs`.
//!
//! The features carry geometry and nothing else: the map draws clustered
//! anonymous dots, so the response holds only what is drawn.

use common::{
    message::user::{UserPointsRequest, UserPointsResponse},
    service::request,
};
use poem::{Response, handler};
use sailfish::TemplateOnce;

/// How many locations one page of the streamed response carries. Bounded by
/// what fits comfortably in one NATS message, not by the page size the map
/// wants — the map wants all of them.
const PAGE: i64 = 1_000;

#[derive(TemplateOnce)]
#[template(path = "staff/usermap.stpl.html")]
struct UserMap;

/// GET `/staff/usermap/map.html` — the map page.
#[handler]
pub async fn map_html() -> Result<Response, anyhow::Error> {
    Ok(Response::builder()
        .content_type("text/html; charset=utf-8")
        .body(UserMap.render_once()?))
}

/// GET `/staff/usermap/users.geojson` — the locations.
///
/// Streamed a page at a time, because the collection covers every account with
/// a location and no single NATS message would hold it.
#[handler]
#[tracing::instrument]
pub async fn users_geojson() -> Response {
    // `None` ends the stream; `Some(0)` sends the header and `Some(n)` fetches
    // the nth page.
    let stream = futures::stream::unfold(Some(0i64), move |page| async move {
        let page = page?;

        if page == 0 {
            return Some((
                Ok::<_, std::io::Error>(String::from(
                    "{\"type\": \"FeatureCollection\", \
                     \"crs\": {\"type\": \"name\", \"properties\": {\"name\": \"urn:ogc:def:crs:OGC:1.3:CRS84\"}}, \
                     \"features\": [",
                )),
                Some(1i64),
            ));
        }

        let response: Result<UserPointsResponse, _> = request(
            "user.points.page",
            UserPointsRequest {
                offset: (page - 1) * PAGE,
                limit: PAGE,
            },
        )
        .await;

        let points = match response {
            Ok(response) => response.points,
            Err(error) => {
                tracing::error!(%error, "usermap page request failed");
                // Close the collection rather than truncating it mid-array, so
                // the map gets valid — if short — GeoJSON.
                return Some((Ok(String::from("]}")), None));
            }
        };

        let short = points.len() < PAGE as usize;

        let mut chunk = String::new();
        for (index, geometry) in points.iter().enumerate() {
            if page > 1 || index > 0 {
                chunk.push(',');
            }
            chunk.push('\n');
            chunk.push_str(&format!(
                r#"{{"type": "Feature", "properties": {{}}, "geometry": {geometry}}}"#
            ));
        }

        if short {
            chunk.push_str("\n]}");
        }

        Some((Ok(chunk), if short { None } else { Some(page + 1) }))
    });

    Response::builder()
        .content_type("application/geo+json")
        .body(poem::Body::from_bytes_stream(stream))
}
