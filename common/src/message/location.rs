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
