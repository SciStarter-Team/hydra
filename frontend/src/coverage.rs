//! Coverage geometry that arrives from outside: partner API requests and
//! imported catalogs.
//!
//! A project's `regions` column holds a MULTIPOLYGON in SRID 4326. What
//! arrives from outside is looser: GeoJSON (a bare Point, Polygon or
//! MultiPolygon, a Feature, or a FeatureCollection, whose features are
//! unioned), either as an object or as text holding one, or else text holding
//! WKT or EWKT, or a bare list of "lon lat" pairs separated by commas. This is
//! what orrery's `get_geometry` accepts. The functions here turn it into the
//! EWKT a project stores, leaving the geometry work to SpatiaLite.

use serde_json::Value;
use sqlx::SqlitePool;

/// Why a coverage could not become a project's regions.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CoverageError {
    #[error("Unrecognized geometry format")]
    Unrecognized,

    #[error("Unrecognized geometry type")]
    UnrecognizedType,

    #[error("Boundary can not be a single point. It must be a polygon or multi-polygon.")]
    Point,

    #[error("Boundary must be a polygon or multi-polygon.")]
    NotPolygonal,

    #[error("geometry could not be examined: {0}")]
    Database(String),
}

impl From<sqlx::Error> for CoverageError {
    fn from(err: sqlx::Error) -> Self {
        CoverageError::Database(err.to_string())
    }
}

/// Whether GeoJSON (an object, or text holding one) covers the whole globe:
/// its coordinates are exactly the corners at ±180 longitude and ±90
/// latitude, as orrery's `geojson_is_global` decides.
pub fn is_global(value: &Value) -> bool {
    let parsed;
    let value = match value {
        Value::String(text) => match serde_json::from_str::<Value>(text) {
            Ok(inner) => {
                parsed = inner;
                &parsed
            }
            Err(_) => return false,
        },
        other => other,
    };

    let Some(object) = value.as_object().filter(|object| !object.is_empty()) else {
        return false;
    };

    let mut seen = Vec::new();
    if let Some(coordinates) = object.get("coordinates") {
        collect_numbers(coordinates, &mut seen);
    }

    seen.sort_by(f64::total_cmp);
    seen.dedup();

    seen == [-180.0, -90.0, 90.0, 180.0]
}

fn collect_numbers(value: &Value, into: &mut Vec<f64>) {
    match value {
        Value::Number(number) => into.extend(number.as_f64()),
        Value::Array(items) => {
            for item in items {
                collect_numbers(item, into);
            }
        }
        _ => {}
    }
}

/// The MultiPolygon, as EWKT in SRID 4326, that a coverage describes. A
/// Polygon becomes a MultiPolygon of one.
pub async fn multipolygon(pool: &SqlitePool, coverage: &Value) -> Result<String, CoverageError> {
    let geometry = geometry(pool, coverage).await?;

    let kind: Option<String> = sqlx::query_scalar("SELECT GeometryType(GeomFromEWKT(?))")
        .bind(&geometry)
        .fetch_one(pool)
        .await?;

    match kind
        .as_deref()
        .and_then(|kind| kind.split_whitespace().next())
    {
        Some("POINT") => Err(CoverageError::Point),
        Some("POLYGON" | "MULTIPOLYGON") => {
            let cast: Option<String> =
                sqlx::query_scalar("SELECT AsEWKT(CastToMultiPolygon(GeomFromEWKT(?)))")
                    .bind(&geometry)
                    .fetch_one(pool)
                    .await?;

            cast.ok_or(CoverageError::NotPolygonal)
        }
        _ => Err(CoverageError::NotPolygonal),
    }
}

/// The geometry a coverage describes, as EWKT in SRID 4326, whatever its type.
async fn geometry(pool: &SqlitePool, coverage: &Value) -> Result<String, CoverageError> {
    match coverage {
        Value::String(text) => from_text(pool, text).await,
        Value::Object(object) => match object.get("type").and_then(Value::as_str) {
            Some("FeatureCollection") => {
                let features = object
                    .get("features")
                    .and_then(Value::as_array)
                    .map(Vec::as_slice)
                    .unwrap_or_default();

                let mut union: Option<String> = None;

                for feature in features {
                    let part = Box::pin(geometry(pool, feature)).await?;

                    union = Some(match union {
                        None => part,
                        Some(sofar) => {
                            let joined: Option<String> = sqlx::query_scalar(
                                "SELECT AsEWKT(GUnion(GeomFromEWKT(?), GeomFromEWKT(?)))",
                            )
                            .bind(&sofar)
                            .bind(&part)
                            .fetch_one(pool)
                            .await?;

                            joined.ok_or(CoverageError::Unrecognized)?
                        }
                    });
                }

                union.ok_or(CoverageError::Unrecognized)
            }
            Some("Feature") => match object.get("geometry") {
                Some(inner) => Box::pin(geometry(pool, inner)).await,
                None => Err(CoverageError::UnrecognizedType),
            },
            Some("Point" | "Polygon" | "MultiPolygon") => {
                from_geojson(pool, &coverage.to_string()).await
            }
            _ => Err(CoverageError::UnrecognizedType),
        },
        _ => Err(CoverageError::Unrecognized),
    }
}

/// The geometry text describes: GeoJSON, read just as the same object would
/// be, EWKT or WKT, or failing those a ring of "lon lat" pairs separated by
/// commas.
async fn from_text(pool: &SqlitePool, text: &str) -> Result<String, CoverageError> {
    let text = text.trim();

    let parsed = if text.starts_with('{') {
        return match serde_json::from_str::<Value>(text) {
            Ok(object @ Value::Object(_)) => Box::pin(geometry(pool, &object)).await,
            _ => Err(CoverageError::Unrecognized),
        };
    } else if text
        .get(..5)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("SRID="))
    {
        sqlx::query_scalar("SELECT AsEWKT(GeomFromEWKT(?))")
            .bind(text)
            .fetch_one(pool)
            .await?
    } else {
        sqlx::query_scalar("SELECT AsEWKT(GeomFromText(?, 4326))")
            .bind(text)
            .fetch_one(pool)
            .await?
    };

    if let Some(parsed) = parsed {
        return Ok(parsed);
    }

    let ring = pairs_as_ring(text).ok_or(CoverageError::Unrecognized)?;

    let parsed: Option<String> = sqlx::query_scalar("SELECT AsEWKT(GeomFromText(?, 4326))")
        .bind(format!("POLYGON(({ring}))"))
        .fetch_one(pool)
        .await?;

    parsed.ok_or(CoverageError::Unrecognized)
}

async fn from_geojson(pool: &SqlitePool, text: &str) -> Result<String, CoverageError> {
    let parsed: Option<String> =
        sqlx::query_scalar("SELECT AsEWKT(SetSRID(GeomFromGeoJSON(?), 4326))")
            .bind(text)
            .fetch_one(pool)
            .await?;

    parsed.ok_or(CoverageError::Unrecognized)
}

/// "lon lat, lon lat, ..." as the body of a WKT ring, or None if the text is
/// not a list of numeric pairs.
fn pairs_as_ring(text: &str) -> Option<String> {
    let pairs = text
        .split(',')
        .map(|pair| {
            let numbers = pair
                .split_whitespace()
                .map(str::parse::<f64>)
                .collect::<Result<Vec<_>, _>>()
                .ok()?;

            match numbers.as_slice() {
                [lon, lat] => Some(format!("{lon} {lat}")),
                _ => None,
            }
        })
        .collect::<Option<Vec<_>>>()?;

    Some(pairs.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert2::check;
    use serde_json::json;
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

    async fn database() -> SqlitePool {
        // SAFETY: mod_spatialite is a trusted extension, the same one the
        // application loads.
        let options = unsafe { SqliteConnectOptions::new().extension("mod_spatialite") };

        SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options.filename(":memory:"))
            .await
            .unwrap()
    }

    const SQUARE: &str = "POLYGON((0 0, 1 0, 1 1, 0 1, 0 0))";

    #[test]
    fn the_whole_globe_is_global() {
        let globe = json!({
            "type": "Polygon",
            "coordinates": [[[-180, -90], [180, -90], [180, 90], [-180, 90], [-180, -90]]],
        });

        check!(is_global(&globe));
        check!(is_global(&Value::String(globe.to_string())));
    }

    #[test]
    fn a_smaller_area_is_not_global() {
        let australia = json!({
            "type": "Polygon",
            "coordinates": [[[113, -44], [154, -44], [154, -10], [113, -10], [113, -44]]],
        });

        check!(!is_global(&australia));
        check!(!is_global(&json!({})));
        check!(!is_global(&json!("not json")));
    }

    #[tokio::test]
    async fn a_polygon_becomes_a_multipolygon() {
        let pool = database().await;

        let polygon = json!({
            "type": "Polygon",
            "coordinates": [[[0, 0], [1, 0], [1, 1], [0, 1], [0, 0]]],
        });

        check!(
            multipolygon(&pool, &polygon).await
                == Ok("SRID=4326;MULTIPOLYGON(((0 0,1 0,1 1,0 1,0 0)))".to_string())
        );
    }

    #[tokio::test]
    async fn text_may_be_wkt_ewkt_geojson_or_pairs() {
        let pool = database().await;
        let expected = Ok("SRID=4326;MULTIPOLYGON(((0 0,1 0,1 1,0 1,0 0)))".to_string());

        check!(multipolygon(&pool, &json!(SQUARE)).await == expected);
        check!(multipolygon(&pool, &json!(format!("SRID=4326;{SQUARE}"))).await == expected);
        check!(
            multipolygon(
                &pool,
                &json!(r#"{"type":"Polygon","coordinates":[[[0,0],[1,0],[1,1],[0,1],[0,0]]]}"#)
            )
            .await
                == expected
        );
        check!(multipolygon(&pool, &json!("0 0, 1 0, 1 1, 0 1, 0 0")).await == expected);
    }

    #[tokio::test]
    async fn a_feature_collection_is_unioned() {
        let pool = database().await;

        let collection = json!({
            "type": "FeatureCollection",
            "features": [
                {"type": "Feature", "geometry": {"type": "Polygon", "coordinates": [[[0, 0], [1, 0], [1, 1], [0, 1], [0, 0]]]}},
                {"type": "Feature", "geometry": {"type": "Polygon", "coordinates": [[[5, 5], [6, 5], [6, 6], [5, 6], [5, 5]]]}},
            ],
        });

        let ewkt = multipolygon(&pool, &collection).await.unwrap();

        let parts: i64 = sqlx::query_scalar("SELECT NumGeometries(GeomFromEWKT(?))")
            .bind(&ewkt)
            .fetch_one(&pool)
            .await
            .unwrap();
        check!(parts == 2);
    }

    #[tokio::test]
    async fn text_holding_geojson_is_read_as_the_object() {
        let pool = database().await;
        let square =
            json!({"type": "Polygon", "coordinates": [[[0, 0], [1, 0], [1, 1], [0, 1], [0, 0]]]});
        let feature = json!({"type": "Feature", "properties": {}, "geometry": square});
        let collection = json!({"type": "FeatureCollection", "features": [feature]});

        for object in [square, feature, collection] {
            let text = Value::String(object.to_string());
            check!(
                multipolygon(&pool, &text).await == multipolygon(&pool, &object).await,
                "{object}"
            );
            check!(multipolygon(&pool, &text).await.is_ok(), "{object}");
        }

        check!(
            multipolygon(&pool, &json!(r#"{"type": "Polygon""#)).await
                == Err(CoverageError::Unrecognized)
        );
    }

    #[tokio::test]
    async fn a_point_is_refused() {
        let pool = database().await;

        check!(
            multipolygon(&pool, &json!({"type": "Point", "coordinates": [1, 2]})).await
                == Err(CoverageError::Point)
        );
    }

    #[tokio::test]
    async fn anything_else_is_refused() {
        let pool = database().await;

        check!(
            multipolygon(
                &pool,
                &json!({"type": "LineString", "coordinates": [[0, 0], [1, 1]]})
            )
            .await
                == Err(CoverageError::UnrecognizedType)
        );
        check!(
            multipolygon(&pool, &json!("LINESTRING(0 0, 1 1)")).await
                == Err(CoverageError::NotPolygonal)
        );
        check!(
            multipolygon(&pool, &json!("somewhere nice")).await == Err(CoverageError::Unrecognized)
        );
        check!(multipolygon(&pool, &json!(42)).await == Err(CoverageError::Unrecognized));
    }
}
