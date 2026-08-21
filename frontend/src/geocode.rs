use anyhow::Error;
use common::{
    message::location::{GeocodeForwardRequest, GeocodeForwardResponse, Point},
    shared::db,
};
use ulid::Ulid;

/// Geocode address components to a GeoJSON point string.
pub async fn geocode_address_to_point(city: &str, state: &str, country: &str) -> Option<String> {
    let parts: Vec<&str> = [city, state, country]
        .into_iter()
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

    let point = Point {
        latitude: response.latitude,
        longitude: response.longitude,
    };

    Some(point.to_string())
}

/// Read current address fields from DB and geocode to update the project's point column.
pub async fn geocode_and_update_project_point(project_id: Ulid) -> Result<(), Error> {
    let project_id_str = project_id.to_string();

    let row = sqlx::query!(
        r#"SELECT city, state, country FROM projects WHERE project_id = ?"#,
        project_id_str
    )
    .fetch_one(db())
    .await?;

    let city = row.city;
    let state = row.state;
    let country = row.country;

    if let Some(geojson) = geocode_address_to_point(&city, &state, &country).await {
        sqlx::query!(
            r#"UPDATE projects SET point = GeomFromGeoJSON(?1) WHERE project_id = ?2"#,
            geojson,
            project_id_str
        )
        .execute(db())
        .await?;

        // A derived point still moves the footprint, and this writes it
        // straight to the column rather than through the project save.
        crate::region::refresh_project(&project_id_str).await?;
    }

    Ok(())
}

/// Read current address fields from DB and geocode to update the event's point column.
pub async fn geocode_and_update_event_point(event_id: Ulid) -> Result<(), Error> {
    let event_id_str = event_id.to_string();

    let row = sqlx::query!(
        r#"SELECT city, state, country FROM events WHERE event_id = ?"#,
        event_id_str
    )
    .fetch_one(db())
    .await?;

    let city = row.city;
    let state = row.state;
    let country = row.country;

    if let Some(geojson) = geocode_address_to_point(&city, &state, &country).await {
        sqlx::query!(
            r#"UPDATE events SET point = GeomFromGeoJSON(?1) WHERE event_id = ?2"#,
            geojson,
            event_id_str
        )
        .execute(db())
        .await?;
    }

    Ok(())
}
