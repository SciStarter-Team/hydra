//! What the unit tests share.

/// A fresh in-memory database with every migration applied.
pub async fn migrated() -> sqlx::SqlitePool {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("pool");

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("migrations");

    pool
}
