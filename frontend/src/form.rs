use poem::{FromRequest, Request, RequestBody, Result as PoemResult};
use serde::Deserialize;

/// Helper to deserialize either a single string or array of strings
/// Useful for cases where a field might be sent as either format
pub fn deserialize_string_or_vec<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StringOrVec {
        String(String),
        Vec(Vec<String>),
    }

    match StringOrVec::deserialize(deserializer)? {
        StringOrVec::String(s) => {
            if s.is_empty() {
                Ok(vec![])
            } else {
                Ok(vec![s])
            }
        }
        StringOrVec::Vec(v) => Ok(v),
    }
}

/// Custom form extractor using serde_qs to support array notation (field[])
/// For POST request bodies
pub struct QsForm<T>(pub T);

impl<'a, T: serde::de::DeserializeOwned> FromRequest<'a> for QsForm<T> {
    async fn from_request(_req: &'a Request, body: &mut RequestBody) -> PoemResult<Self> {
        let data = body.take()?.into_vec().await?;
        let config = serde_qs::Config::new(10, false); // max_depth=10, strict=false
        let form = config.deserialize_bytes(&data).map_err(|e| {
            poem::Error::from_string(
                format!("Failed to parse form data: {}", e),
                poem::http::StatusCode::BAD_REQUEST,
            )
        })?;
        Ok(QsForm(form))
    }
}

/// Custom query extractor using serde_qs to support array notation (field[])
/// For GET query parameters
pub struct QsQuery<T>(pub T);

impl<'a, T: serde::de::DeserializeOwned> FromRequest<'a> for QsQuery<T> {
    async fn from_request(req: &'a Request, _body: &mut RequestBody) -> PoemResult<Self> {
        let query_string = req.uri().query().unwrap_or("");
        let config = serde_qs::Config::new(10, false); // max_depth=10, strict=false
        let parsed = config.deserialize_str(query_string).map_err(|e| {
            poem::Error::from_string(
                format!("Failed to parse query string: {}", e),
                poem::http::StatusCode::BAD_REQUEST,
            )
        })?;
        Ok(QsQuery(parsed))
    }
}
