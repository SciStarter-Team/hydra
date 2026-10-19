//! Helpers shared by the integration tests. Built as a test target of its
//! own as well, which holds no tests.

#![allow(dead_code)]

use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use std::path::Path;

/// A log cache database in `dir`, migrated as the service migrates its own:
/// where participation retractions are queued.
pub async fn log_cache(dir: &Path) -> anyhow::Result<SqlitePool> {
    let pool = SqlitePoolOptions::new()
        .connect_with(
            SqliteConnectOptions::new()
                .filename(dir.join("log_cache.sqlite"))
                .create_if_missing(true),
        )
        .await?;

    sqlx::migrate!("./migrations").run(&pool).await?;
    Ok(pool)
}
