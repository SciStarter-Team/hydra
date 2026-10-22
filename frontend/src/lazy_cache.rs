use std::time::Duration;

use anyhow::Error;
use common::shared::db;
use common::timestring::TimeString;
use jiff::Timestamp;

/// A trait for values that can be lazily cached in SQLite.
///
/// Implementors define how to fetch a fresh value and how long
/// a cached value remains acceptable before a background refresh
/// is triggered.
pub trait LazyCached: Clone + Send + Sync + 'static {
    /// The cache key for this particular lookup
    /// (e.g., "participant_count:{project_id}")
    fn cache_key(&self) -> String;

    /// How long before a cached value is considered stale.
    /// Stale values are still returned immediately, but a background
    /// refresh is spawned.
    fn max_age(&self) -> Duration;

    /// Fetch a fresh value from the authoritative source
    /// (e.g., via a NATS request).
    fn fetch_fresh(&self) -> impl std::future::Future<Output = Result<String, Error>> + Send;
}

/// Look up a cached value.
///
/// - If cached and fresh: returns the value immediately.
/// - If cached but stale: returns the stale value immediately and
///   spawns a background task to refresh it.
/// - If not cached at all: blocks on `fetch_fresh()`, caches the
///   result, and returns it.
pub async fn get_cached<T: LazyCached>(source: &T) -> Result<String, Error> {
    let key = source.cache_key();

    let row = sqlx::query!(
        r#"SELECT value, updated_at FROM lazy_cache WHERE cache_key = ?"#,
        key
    )
    .fetch_optional(db())
    .await?;

    match row {
        Some(row) => {
            let value = row.value;

            // Check staleness
            if let Ok(updated_at) = row.updated_at.parse::<Timestamp>() {
                let now = Timestamp::now();
                let age = now.duration_since(updated_at).unsigned_abs();
                if age > source.max_age() {
                    // Stale — spawn background refresh
                    let source = source.clone();
                    tokio::spawn(async move {
                        if let Err(e) = refresh_cache(&source).await {
                            tracing::warn!(
                                key = source.cache_key(),
                                "lazy cache background refresh failed: {e}"
                            );
                        }
                    });
                }
            }

            Ok(value)
        }
        None => {
            // No cached value — block on fetch
            let value = source.fetch_fresh().await?;
            store_cached(&key, &value).await?;
            Ok(value)
        }
    }
}

/// Push a scalar subquery reading `source`'s cached value, so that a query
/// built synchronously can use it.
///
/// It reads whatever is stored, stale or not, and NULL when nothing is: call
/// [`get_cached`] beforehand so there is a value and its refresh is
/// scheduled.
pub(crate) fn push_cached_value<T: LazyCached>(
    qb: &mut sqlx::QueryBuilder<sqlx::Sqlite>,
    source: &T,
) {
    qb.push("(SELECT value FROM lazy_cache WHERE cache_key = ");
    qb.push_bind(source.cache_key());
    qb.push(")");
}

async fn refresh_cache<T: LazyCached>(source: &T) -> Result<(), Error> {
    let value = source.fetch_fresh().await?;
    store_cached(&source.cache_key(), &value).await
}

async fn store_cached(key: &str, value: &str) -> Result<(), Error> {
    let now = TimeString::now().to_string();

    sqlx::query!(
        r#"
        INSERT INTO lazy_cache (cache_key, value, updated_at)
        VALUES (?, ?, ?)
        ON CONFLICT (cache_key) DO UPDATE SET
            value = excluded.value,
            updated_at = excluded.updated_at
        "#,
        key,
        value,
        now
    )
    .execute(db())
    .await?;

    Ok(())
}
