use crate::lru_cache::LruCache;
use poem::web::{Json, Path, Query};
use poem::{handler, IntoResponse, Response};
use serde::{Deserialize, Serialize};
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

const CACHE_SIZE: usize = 500;
const CACHE_DURATION: Duration = Duration::from_secs(86400); // 24 hours

static ZIPCODE_CACHE: LazyLock<Mutex<LruCache<Vec<ZipcodeResult>>>> =
    LazyLock::new(|| Mutex::new(LruCache::new(CACHE_SIZE, CACHE_DURATION)));

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZipcodeResult {
    pub postal_code: String,
    pub latitude: f64,
    pub longitude: f64,
    pub formatted: String,
}

#[derive(Debug, Deserialize)]
struct ZipcodeStackResult {
    postal_code: String,
    latitude: f64,
    longitude: f64,
    city: Option<String>,
    state: Option<String>,
    country_code: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ZipcodeStackResponse {
    results: serde_json::Value,
}

#[derive(Debug, Deserialize)]
pub struct ZipcodeQuery {
    pub country: Option<String>,
}

impl ZipcodeStackResult {
    fn to_result(&self) -> ZipcodeResult {
        let mut parts = Vec::new();
        if let Some(ref city) = self.city {
            if !city.is_empty() {
                parts.push(city.as_str());
            }
        }
        if let Some(ref state) = self.state {
            if !state.is_empty() {
                parts.push(state.as_str());
            }
        }
        let postal = &self.postal_code;
        let location = parts.join(", ");
        let formatted = if let Some(ref cc) = self.country_code {
            if location.is_empty() {
                format!("{postal} ({cc})")
            } else {
                format!("{location} {postal}, {cc}")
            }
        } else if location.is_empty() {
            postal.to_string()
        } else {
            format!("{location} {postal}")
        };

        ZipcodeResult {
            postal_code: self.postal_code.clone(),
            latitude: self.latitude,
            longitude: self.longitude,
            formatted,
        }
    }
}

fn cache_key(code: &str, country: Option<&str>) -> String {
    match country {
        Some(c) => format!("{code}:{c}"),
        None => code.to_string(),
    }
}

async fn fetch_from_api(
    code: &str,
    country: Option<&str>,
) -> Result<Vec<ZipcodeResult>, anyhow::Error> {
    let mut url = format!(
        "https://api.zipcodestack.com/v1/search?codes={code}&apikey={}",
        env!("ZIPCODESTACK_KEY"),
    );
    if let Some(country) = country {
        url.push_str("&country=");
        url.push_str(country);
    }

    let response: ZipcodeStackResponse = reqwest::get(&url).await?.json().await?;

    // The results field is an object keyed by postal code, each containing an array
    let mut results: Vec<ZipcodeResult> = if let Some(obj) = response.results.as_object() {
        obj.values()
            .flat_map(|v| {
                serde_json::from_value::<Vec<ZipcodeStackResult>>(v.clone()).unwrap_or_default()
            })
            .map(|r| r.to_result())
            .collect()
    } else {
        Vec::new()
    };

    // Sort US results first since the primary audience is US-based
    results.sort_by_key(|r| if r.formatted.ends_with(", US") { 0 } else { 1 });

    Ok(results)
}

#[handler]
pub async fn zipcode_lookup(
    Path(code): Path<String>,
    Query(query): Query<ZipcodeQuery>,
) -> Response {
    let code = code.trim();
    let country = query.country.as_deref();
    let key = cache_key(code, country);

    // Check cache
    if let Some(cached) = ZIPCODE_CACHE.lock().unwrap().get(&key) {
        return Json(serde_json::json!({ "results": cached })).into_response();
    }

    // Fetch from API
    match fetch_from_api(code, country).await {
        Ok(results) => {
            ZIPCODE_CACHE
                .lock()
                .unwrap()
                .insert(key, results.clone());
            Json(serde_json::json!({ "results": results })).into_response()
        }
        Err(e) => {
            tracing::error!("Zipcodestack API error: {e}");
            Json(serde_json::json!({ "results": [], "error": "Lookup failed" })).into_response()
        }
    }
}
