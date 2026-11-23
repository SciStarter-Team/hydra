use anyhow::Error;
use common::{
    message::location::{GeocodeForwardRequest, GeocodeForwardResponse, Point},
    shared::db,
};
use ulid::Ulid;

/// Geocode address components to a GeoJSON point string.
pub async fn geocode_address_to_point(city: &str, state: &str, country: &str) -> Option<String> {
    geocode_parts(&[city, state, country]).await
}

/// Geocode however many address components are to hand, in the order they
/// would be written on an envelope.
///
/// Empty components are dropped, so a caller can pass a fixed set of fields
/// without first working out which of them were filled in, and callers with
/// more to offer than a city and a state — a street address and a postal code,
/// say — can pass those too.
pub async fn geocode_parts(parts: &[&str]) -> Option<String> {
    Some(locate(parts).await?.to_string())
}

/// Where the address made of `parts` is, taking them as [`geocode_parts`]
/// does.
pub async fn locate(parts: &[&str]) -> Option<Point> {
    let parts: Vec<&str> = parts
        .iter()
        .copied()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();

    if parts.is_empty() {
        return None;
    }

    let query = parts.join(", ");

    let response: GeocodeForwardResponse =
        common::service::request("user.geocode.forward", GeocodeForwardRequest { query })
            .await
            .ok()?;

    Some(Point {
        latitude: response.latitude,
        longitude: response.longitude,
    })
}

/// Where a place is, as a GeoJSON point: from its whole address first, then,
/// when that finds nowhere (a street the geocoder doesn't know), from its zip
/// and locality alone.
pub async fn geocode_address(
    address: &str,
    city: &str,
    state: &str,
    zip: &str,
    country: &str,
) -> Option<String> {
    if let Some(point) = geocode_parts(&[address, city, state, zip, country]).await {
        return Some(point);
    }
    if address.trim().is_empty() {
        return None;
    }
    geocode_parts(&[city, state, zip, country]).await
}

/// Place the project at its address, read from the database, or leave it with
/// no point when the address finds nowhere, so an edited address never keeps
/// the point of the one it replaced.
pub async fn geocode_and_update_project_point(project_id: Ulid) -> Result<(), Error> {
    let project_id_str = project_id.to_string();

    let row = sqlx::query!(
        r#"SELECT address, city, state, zip, country FROM projects WHERE project_id = ?"#,
        project_id_str
    )
    .fetch_one(db())
    .await?;

    let point = geocode_address(&row.address, &row.city, &row.state, &row.zip, &row.country).await;
    sqlx::query!(
        r#"UPDATE projects SET point = SetSRID(GeomFromGeoJSON(?1), 4326) WHERE project_id = ?2"#,
        point,
        project_id_str
    )
    .execute(db())
    .await?;

    // A derived point still moves the footprint, and this writes it
    // straight to the column rather than through the project save.
    crate::region::refresh_project(&project_id_str).await?;

    Ok(())
}

/// Place the event at its address, as [`geocode_and_update_project_point`]
/// does a project.
pub async fn geocode_and_update_event_point(event_id: Ulid) -> Result<(), Error> {
    let event_id_str = event_id.to_string();

    let row = sqlx::query!(
        r#"SELECT address, city, state, zip, country FROM events WHERE event_id = ?"#,
        event_id_str
    )
    .fetch_one(db())
    .await?;

    let point = geocode_address(&row.address, &row.city, &row.state, &row.zip, &row.country).await;
    sqlx::query!(
        r#"UPDATE events SET point = SetSRID(GeomFromGeoJSON(?1), 4326) WHERE event_id = ?2"#,
        point,
        event_id_str
    )
    .execute(db())
    .await?;

    // As for a project's derived point.
    crate::region::refresh_event(&event_id_str).await?;

    Ok(())
}
