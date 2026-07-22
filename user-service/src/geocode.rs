use anyhow::Error;
use async_nats::service::Request;
use common::{
    message::location::{
        GeocodeForwardRequest, GeocodeForwardResponse, GeocodeRequest, GeocodeResolveRequest,
        GeocodeResolveResponse, GeocodeResponse, GeocodeResult, GeocodeReverseRequest,
        GeocodeReverseResponse,
    },
    service::decode_structured_message,
    shared::db,
    timestring::TimeString,
};

#[derive(serde::Deserialize)]
struct OpenCageResult {
    formatted: String,
    geometry: OpenCageGeometry,
    #[serde(default)]
    components: OpenCageComponents,
}

#[derive(serde::Deserialize)]
struct OpenCageGeometry {
    lat: f64,
    lng: f64,
}

/// The address components OpenCage returns per result. The city-like level
/// arrives under whichever key fits the place (city / town / village /
/// hamlet); `city_level()` collapses them.
#[derive(serde::Deserialize, Default)]
struct OpenCageComponents {
    #[serde(default)]
    road: String,
    #[serde(default)]
    house_number: String,
    #[serde(default)]
    postcode: String,
    #[serde(default)]
    city: String,
    #[serde(default)]
    town: String,
    #[serde(default)]
    village: String,
    #[serde(default)]
    hamlet: String,
    #[serde(default)]
    state: String,
    #[serde(default)]
    country: String,
    #[serde(default)]
    country_code: String,
}

impl OpenCageComponents {
    fn city_level(&self) -> String {
        [&self.city, &self.town, &self.village, &self.hamlet]
            .into_iter()
            .find(|c| !c.is_empty())
            .cloned()
            .unwrap_or_default()
    }
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

/// `user.geocode.reverse` — resolve coordinates to address components via
/// OpenCage, cached in `geocode_reverse_cache`. A point with no result caches
/// (and returns) empty components — that's a real answer (open ocean), not an
/// error.
pub async fn reverse(req: &Request) -> Result<GeocodeReverseResponse, Error> {
    let message: GeocodeReverseRequest = decode_structured_message(req.message.clone())?;

    if !(-90.0..=90.0).contains(&message.latitude) || !(-180.0..=180.0).contains(&message.longitude)
    {
        anyhow::bail!("Coordinates out of range");
    }

    // Round to 5 decimal places (~1 m) so nearby lookups share a cache entry.
    let latitude = (message.latitude * 1e5).round() / 1e5;
    let longitude = (message.longitude * 1e5).round() / 1e5;

    let cached = sqlx::query!(
        r#"SELECT formatted, road, house_number, city, state, postcode, country, country_code
           FROM geocode_reverse_cache
           WHERE latitude = ? AND longitude = ?"#,
        latitude,
        longitude
    )
    .fetch_optional(db())
    .await?;

    if let Some(row) = cached {
        return Ok(GeocodeReverseResponse {
            formatted: row.formatted,
            road: row.road,
            house_number: row.house_number,
            city: row.city,
            state: row.state,
            postcode: row.postcode,
            country: row.country,
            country_code: row.country_code,
        });
    }

    let api_key = env!("OPENCAGEDATA_KEY");
    let query = format!("{latitude},{longitude}");
    let response = reqwest::Client::new()
        .get("https://api.opencagedata.com/geocode/v1/json")
        .query(&[
            ("q", query.as_str()),
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
        .map(|r| GeocodeReverseResponse {
            formatted: r.formatted,
            city: r.components.city_level(),
            road: r.components.road,
            house_number: r.components.house_number,
            state: r.components.state,
            postcode: r.components.postcode,
            country: r.components.country,
            country_code: r.components.country_code,
        })
        .unwrap_or_default();

    let id = ulid::Ulid::new().to_string();
    let now = TimeString::now().to_string();
    sqlx::query!(
        r#"INSERT INTO geocode_reverse_cache
               (id, latitude, longitude, formatted, road, house_number,
                city, state, postcode, country, country_code, created_at)
           VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
           ON CONFLICT (latitude, longitude) DO NOTHING"#,
        id,
        latitude,
        longitude,
        result.formatted,
        result.road,
        result.house_number,
        result.city,
        result.state,
        result.postcode,
        result.country,
        result.country_code,
        now
    )
    .execute(db())
    .await?;

    Ok(result)
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
