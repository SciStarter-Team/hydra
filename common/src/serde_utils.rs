//! Serde utility functions

/// Helper function to deserialize empty string as None for any type T
///
/// Use this with `#[serde(deserialize_with = "common::serde_utils::deserialize_empty_string_as_none")]`
/// to handle query parameters like `?topic=` (empty string) as `None` instead of a parse error.
///
/// # Example
/// ```
/// use serde::Deserialize;
///
/// #[derive(Deserialize)]
/// struct Query {
///     #[serde(default, deserialize_with = "common::serde_utils::deserialize_empty_string_as_none")]
///     topic: Option<i32>,
/// }
/// ```
pub fn deserialize_empty_string_as_none<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    use serde::Deserialize;
    let s: Option<String> = Option::deserialize(deserializer)?;
    match s {
        Some(s) if s.is_empty() => Ok(None),
        Some(s) => s.parse().map(Some).map_err(serde::de::Error::custom),
        None => Ok(None),
    }
}
