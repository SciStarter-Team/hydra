use anyhow::Error;
use async_nats::service::Request;
use common::{
    message::location::{
        GeocodeForwardRequest, GeocodeForwardResponse, GeocodeRequest, GeocodeResolveRequest,
        GeocodeResolveResponse, GeocodeResponse, GeocodeResult,
    },
    service::decode_structured_message,
    shared::db,
    timestring::TimeString,
};

#[derive(serde::Deserialize)]
struct OpenCageResult {
    formatted: String,
    geometry: OpenCageGeometry,
}

#[derive(serde::Deserialize)]
struct OpenCageGeometry {
    lat: f64,
    lng: f64,
}

#[derive(serde::Deserialize)]
struct OpenCageResponse {
    results: Vec<OpenCageResult>,
}

pub async fn geocode(req: &Request) -> Result<GeocodeResponse, Error> {
    let message: GeocodeRequest = decode_structured_message(req.message.clone())?;

    let normalized = message.query.trim().to_lowercase();
    if normalized.len() < 3 {
        return Ok(GeocodeResponse {
            results: Vec::new(),
        });
    }

    // Check cache first
    let cached = sqlx::query!(
        r#"SELECT id, formatted
           FROM geocode_cache
           WHERE query = ?
           LIMIT 5"#,
        normalized
    )
    .fetch_all(db())
    .await?;

    if !cached.is_empty() {
        let results = cached
            .into_iter()
            .map(|row| GeocodeResult {
                id: row.id,
                formatted: row.formatted,
            })
            .collect();
        return Ok(GeocodeResponse { results });
    }

    // Call OpenCageData API
    let api_key = env!("OPENCAGEDATA_KEY");
    let client = reqwest::Client::new();
    let response = client
        .get("https://api.opencagedata.com/geocode/v1/json")
        .query(&[
            ("q", normalized.as_str()),
            ("key", api_key),
            ("limit", "5"),
            ("no_annotations", "1"),
        ])
        .send()
        .await?
        .error_for_status()?
        .json::<OpenCageResponse>()
        .await?;

    let now = TimeString::now().to_string();
    let mut results = Vec::new();

    for ocr in response.results {
        let id = ulid::Ulid::new().to_string();

        sqlx::query!(
            r#"INSERT INTO geocode_cache (id, query, formatted, latitude, longitude, created_at)
               VALUES (?, ?, ?, ?, ?, ?)
               ON CONFLICT (query, formatted) DO NOTHING"#,
            id,
            normalized,
            ocr.formatted,
            ocr.geometry.lat,
            ocr.geometry.lng,
            now
        )
        .execute(db())
        .await?;

        // Read the actual id (handles both insert and conflict cases)
        let actual_id = sqlx::query_scalar!(
            r#"SELECT id FROM geocode_cache WHERE query = ? AND formatted = ?"#,
            normalized,
            ocr.formatted
        )
        .fetch_one(db())
        .await?;

        results.push(GeocodeResult {
            id: actual_id,
            formatted: ocr.formatted,
        });
    }

    Ok(GeocodeResponse { results })
}

pub async fn resolve(req: &Request) -> Result<GeocodeResolveResponse, Error> {
    let message: GeocodeResolveRequest = decode_structured_message(req.message.clone())?;

    let row = sqlx::query!(
        r#"SELECT formatted, latitude, longitude
           FROM geocode_cache
           WHERE id = ?"#,
        message.id
    )
    .fetch_one(db())
    .await?;

    Ok(GeocodeResolveResponse {
        formatted: row.formatted,
        latitude: row.latitude,
        longitude: row.longitude,
    })
}

pub async fn forward(req: &Request) -> Result<GeocodeForwardResponse, Error> {
    let message: GeocodeForwardRequest = decode_structured_message(req.message.clone())?;

    let normalized = message.query.trim().to_lowercase();
    if normalized.len() < 3 {
        anyhow::bail!("Query too short");
    }

    // Check cache first
    let cached = sqlx::query!(
        r#"SELECT latitude, longitude
           FROM geocode_cache
           WHERE query = ?
           LIMIT 1"#,
        normalized
    )
    .fetch_optional(db())
    .await?;

    if let Some(row) = cached {
        return Ok(GeocodeForwardResponse {
            latitude: row.latitude,
            longitude: row.longitude,
        });
    }

    // Call OpenCageData API
    let api_key = env!("OPENCAGEDATA_KEY");
    let client = reqwest::Client::new();
    let response = client
        .get("https://api.opencagedata.com/geocode/v1/json")
        .query(&[
            ("q", normalized.as_str()),
            ("key", api_key),
            ("limit", "1"),
            ("no_annotations", "1"),
        ])
        .send()
        .await?
        .error_for_status()?
        .json::<OpenCageResponse>()
        .await?;

    let result = response
        .results
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("No geocode results"))?;

    let now = TimeString::now().to_string();
    let id = ulid::Ulid::new().to_string();

    sqlx::query!(
        r#"INSERT INTO geocode_cache (id, query, formatted, latitude, longitude, created_at)
           VALUES (?, ?, ?, ?, ?, ?)
           ON CONFLICT (query, formatted) DO NOTHING"#,
        id,
        normalized,
        result.formatted,
        result.geometry.lat,
        result.geometry.lng,
        now
    )
    .execute(db())
    .await?;

    Ok(GeocodeForwardResponse {
        latitude: result.geometry.lat,
        longitude: result.geometry.lng,
    })
}
