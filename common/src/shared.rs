use std::sync::OnceLock;

#[cfg(debug_assertions)]
const NATS_HOST: &'static str = "127.0.0.1";

#[cfg(not(debug_assertions))]
const NATS_HOST: &'static str = env!("NATS_HOST");

static NATS: OnceLock<async_nats::Client> = OnceLock::new();

pub fn nats() -> &'static async_nats::Client {
    NATS.get()
        .expect("Shared NATS connection is not initialized")
}

pub async fn extra_nats() -> async_nats::Client {
    let nats = async_nats::connect_with_options(NATS_HOST, async_nats::ConnectOptions::default())
        .await
        .expect("Unable to connect to NATS");
    nats
}

pub fn hash_identifier(ident: impl AsRef<str>) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(ident.as_ref().as_bytes());
    hex::encode(hasher.finalize())
}

pub fn hash_identifier_lower(ident: impl AsRef<str>) -> String {
    hash_identifier(ident.as_ref().to_lowercase())
}

pub fn hash_identifier_capfirst(ident: impl AsRef<str>) -> String {
    let ident = ident.as_ref().to_lowercase();
    let mut letters = ident.chars();
    let transformed: String = if let Some(first) = letters.next() {
        first.to_uppercase().chain(letters).collect()
    } else {
        String::new()
    };
    hash_identifier(transformed)
}

#[cfg(feature = "database")]
mod database {
    use std::{sync::OnceLock, time::Duration};
    use sqlx::{
        SqlitePool,
        sqlite::{SqliteConnectOptions, SqlitePoolOptions},
    };

    #[cfg(debug_assertions)]
    const STORAGE_PATH: &'static str = env!("LOCAL_STORAGE_PATH");

    #[cfg(not(debug_assertions))]
    const STORAGE_PATH: &'static str = env!("STORAGE_PATH");

    const MAX_CONNECTION_LIFETIME: u64 = 60 * 60 * 4;
    const MAX_CONNECTION_IDLE_TIME: u64 = 60;

    static DB: OnceLock<SqlitePool> = OnceLock::new();

    pub fn db() -> &'static SqlitePool {
        DB.get().expect("Shared connection pool is not initialized")
    }

    pub async fn init(db_name: impl AsRef<str>) {
        let conx = SqlitePoolOptions::new()
            .max_connections(100)
            .max_lifetime(Duration::from_secs(MAX_CONNECTION_LIFETIME))
            .idle_timeout(Duration::from_secs(MAX_CONNECTION_IDLE_TIME))
            .connect_with(
                SqliteConnectOptions::new()
                    .filename(std::path::Path::join(
                        STORAGE_PATH.as_ref(),
                        format!("{}.sqlite", db_name.as_ref()),
                    ))
                    .create_if_missing(true)
                    .busy_timeout(std::time::Duration::from_secs(300))
                    .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
                    .synchronous(sqlx::sqlite::SqliteSynchronous::Normal)
                    .foreign_keys(true)
                    .optimize_on_close(true, Some(1000))
                    .extension("mod_spatialite"),
            )
            .await
            .expect("Unable to connect to SQLite");

        sqlx::query!(
            r#"create table if not exists "misc" (
              "key" text primary key not null,
              "value" any not null
            ) strict;"#
        )
        .execute(&conx)
        .await
        .expect("Error creating misc KV table");

        DB.set(conx)
            .expect("Unable to set shared database connection pool");

        let nats = async_nats::connect_with_options(super::NATS_HOST, async_nats::ConnectOptions::default())
            .await
            .expect("Unable to connect to NATS");

        super::NATS.set(nats)
            .expect("Unable to set shared NATS connection");
    }

    pub async fn extra_db(db_name: impl AsRef<str>, create: bool) -> Result<SqlitePool, sqlx::Error> {
        Ok(SqlitePool::connect_with(
            SqliteConnectOptions::new()
                .filename(std::path::Path::join(
                    STORAGE_PATH.as_ref(),
                    format!("{}.sqlite", db_name.as_ref()),
                ))
                .create_if_missing(create)
                .busy_timeout(std::time::Duration::from_secs(300))
                .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
                .synchronous(sqlx::sqlite::SqliteSynchronous::Normal)
                .foreign_keys(true)
                .optimize_on_close(true, Some(1000))
                .extension("mod_spatialite"),
        )
        .await?)
    }
}

#[cfg(feature = "database")]
pub use database::{db, extra_db, init};

#[cfg(not(feature = "database"))]
pub async fn init() {
    let nats = async_nats::connect_with_options(NATS_HOST, async_nats::ConnectOptions::default())
        .await
        .expect("Unable to connect to NATS");

    NATS.set(nats)
        .expect("Unable to set shared NATS connection");
}
