#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub struct LocationInfo {
    pub country_code: String,
    pub country_name: String,
    pub region_name: String,
    pub city_name: String,
    pub latitude: String,
    pub longitude: String,
    pub zip_code: String,
    pub time_zone: String,
}

#[derive(Debug, Clone)]
pub struct Point {
    pub latitude: f64,
    pub longitude: f64,
}

impl std::fmt::Display for Point {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            r#"{{"type":"Point","coordinates":[{},{}]}}"#,
            self.longitude, self.latitude
        )
    }
}

impl Default for LocationInfo {
    fn default() -> Self {
        LocationInfo {
            country_code: String::new(),
            country_name: String::new(),
            region_name: String::new(),
            city_name: String::new(),
            latitude: String::from("0.0"),
            longitude: String::from("0.0"),
            zip_code: String::new(),
            time_zone: String::from("+00:00"),
        }
    }
}

impl LocationInfo {
    /// Convert to Point if latitude and longitude are valid numbers
    /// Returns None if either latitude or longitude is empty or invalid
    pub fn as_point(&self) -> Option<Point> {
        if self.latitude.is_empty() || self.longitude.is_empty() {
            return None;
        }

        let latitude = self.latitude.parse().ok()?;
        let longitude = self.longitude.parse().ok()?;

        Some(Point {
            latitude,
            longitude,
        })
    }
}

#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub struct GeocodeRequest {
    pub query: String,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct GeocodeResult {
    pub id: String,
    pub formatted: String,
}

#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub struct GeocodeResponse {
    pub results: Vec<GeocodeResult>,
}

#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub struct GeocodeResolveRequest {
    pub id: String,
}

#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub struct GeocodeResolveResponse {
    pub formatted: String,
    pub latitude: f64,
    pub longitude: f64,
}

#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub struct GeocodeForwardRequest {
    pub query: String,
}

#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub struct GeocodeForwardResponse {
    pub latitude: f64,
    pub longitude: f64,
}

#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub struct GeocodeReverseRequest {
    pub latitude: f64,
    pub longitude: f64,
}

/// Address components for a reverse-geocoded point. Components the geocoder
/// couldn't determine are empty strings.
#[derive(serde::Serialize, serde::Deserialize, Debug, Default)]
pub struct GeocodeReverseResponse {
    pub formatted: String,
    pub road: String,
    pub house_number: String,
    pub city: String,
    pub state: String,
    pub postcode: String,
    pub country: String,
    /// ISO 3166-1 alpha-2, lowercase.
    pub country_code: String,
}
