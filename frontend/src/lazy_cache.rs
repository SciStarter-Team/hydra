use std::time::Duration;

use anyhow::Error;
use common::service::decode_structured_message;
use common::shared::{db, nats};
use common::timestring::TimeString;
use futures::StreamExt;
use jiff::Timestamp;
use serde::de::DeserializeOwned;

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

/// A cached value whose source also announces it, to every instance at
/// once, whenever it may have changed. The announcement replaces what is
/// cached as it arrives, so a change is seen without waiting for the cached
/// value to go stale; `max_age` still bounds how long a missed announcement
/// is missed for.
pub trait Announced: LazyCached {
    /// The NATS subject the source announces on.
    const SUBJECT: &'static str;

    /// What the source announces, which is also what it answers when asked.
    type Value: DeserializeOwned;

    /// The cached form of `value`. `fetch_fresh` stores the source's answer
    /// in the same form.
    fn cache_value(value: Self::Value) -> Result<String, Error>;
}

/// Cache each value `source`'s source announces, for as long as the instance
/// runs. Every instance listens, since every instance has a cache of its own.
pub async fn follow<T: Announced>(source: T) {
    let key = source.cache_key();

    loop {
        match nats().subscribe(T::SUBJECT).await {
            Ok(mut announcements) => {
                while let Some(message) = announcements.next().await {
                    let stored = match decode_structured_message::<T::Value>(message) {
                        Ok(value) => match T::cache_value(value) {
                            Ok(value) => store_cached(&key, &value).await,
                            Err(e) => Err(e),
                        },
                        Err(e) => Err(e),
                    };

                    if let Err(e) = stored {
                        tracing::warn!(key, "lazy cache announcement not stored: {e}");
                    }
                }

                tracing::warn!(key, "lazy cache announcements ended; listening again");
            }
            Err(e) => tracing::warn!(key, "can't listen for lazy cache announcements: {e}"),
        }

        tokio::time::sleep(Duration::from_secs(5)).await;
    }
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
