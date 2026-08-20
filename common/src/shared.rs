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
    let nats = async_nats::connect_with_options(
        NATS_HOST,
        async_nats::ConnectOptions::default().request_timeout(None), // Disable default request timeout
    )
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
    use anyhow::Error;
    use sqlx::{
        SqlitePool,
        sqlite::{SqliteConnectOptions, SqlitePoolOptions},
    };
    use std::{path::PathBuf, sync::OnceLock, time::Duration};
    use tokio::sync::OnceCell;

    #[cfg(debug_assertions)]
    const STORAGE_PATH: &'static str = env!("LOCAL_STORAGE_PATH");

    #[cfg(not(debug_assertions))]
    const STORAGE_PATH: &'static str = env!("STORAGE_PATH");

    const MAX_CONNECTION_LIFETIME: u64 = 60 * 60 * 4;
    const MAX_CONNECTION_IDLE_TIME: u64 = 60;

    /// Connections held open for read-only queries. Small: these serve
    /// occasional staff queries, not production traffic.
    const READ_ONLY_MAX_CONNECTIONS: u32 = 2;

    static DB: OnceLock<SqlitePool> = OnceLock::new();

    /// The file [`init`] opened, remembered so that [`read_only_db`] can
    /// open a second pool over the same database.
    static DB_PATH: OnceLock<PathBuf> = OnceLock::new();

    static READ_ONLY_DB: OnceCell<SqlitePool> = OnceCell::const_new();

    pub fn db() -> &'static SqlitePool {
        DB.get().expect("Shared connection pool is not initialized")
    }

    /// A second pool over this service's own database, opened on first
    /// use, on which writing is impossible.
    ///
    /// The connections are opened with `SQLITE_OPEN_READONLY` and put
    /// into `query_only` mode, so SQLite itself refuses every attempt to
    /// change the database, whatever statement asks for it. The pool is
    /// separate from [`db`] so that those settings can never leak onto a
    /// connection the rest of the service uses.
    ///
    /// This backs the ad-hoc query endpoint in [`crate::query`].
    pub async fn read_only_db() -> Result<&'static SqlitePool, Error> {
        READ_ONLY_DB
            .get_or_try_init(|| async {
                let path = DB_PATH
                    .get()
                    .ok_or_else(|| anyhow::anyhow!("Shared connection pool is not initialized"))?;

                read_only_pool(path).await
            })
            .await
    }

    /// Open a pool over `path` on which writing is impossible. This is
    /// how [`read_only_db`] opens the service's own database, and how a
    /// test gets the same guarantee over a database of its own.
    pub async fn read_only_pool(path: impl AsRef<std::path::Path>) -> Result<SqlitePool, Error> {
        Ok(SqlitePoolOptions::new()
            .max_connections(READ_ONLY_MAX_CONNECTIONS)
            .idle_timeout(Duration::from_secs(MAX_CONNECTION_IDLE_TIME))
            .connect_with(read_only_options(path))
            .await?)
    }

    /// Connection settings that make writing impossible.
    ///
    /// `read_only` opens the file with `SQLITE_OPEN_READONLY`, and
    /// `query_only` makes SQLite reject any statement that would change
    /// the database even if the file could be written. Neither depends
    /// on the shape of the statement being run.
    fn read_only_options(path: impl AsRef<std::path::Path>) -> SqliteConnectOptions {
        // SAFETY: mod_spatialite is a trusted extension shipped with the
        // deployment; loading it is required for spatial query support.
        unsafe {
            SqliteConnectOptions::new()
                .filename(path)
                .read_only(true)
                .pragma("query_only", "ON")
                .busy_timeout(Duration::from_secs(30))
                .extension("mod_spatialite")
        }
    }

    pub async fn init(db_name: impl AsRef<str>) {
        println!("Storage directory: {}", STORAGE_PATH);

        let path = std::path::Path::join(
            STORAGE_PATH.as_ref(),
            format!("{}.sqlite", db_name.as_ref()),
        );

        DB_PATH
            .set(path.clone())
            .expect("Unable to set shared database path");

        let conx = SqlitePoolOptions::new()
            .max_connections(100)
            .max_lifetime(Duration::from_secs(MAX_CONNECTION_LIFETIME))
            .idle_timeout(Duration::from_secs(MAX_CONNECTION_IDLE_TIME))
            .connect_with(
                // SAFETY: mod_spatialite is a trusted extension shipped with the
                // deployment; loading it is required for spatial query support.
                unsafe {
                    SqliteConnectOptions::new()
                        .filename(path)
                        .create_if_missing(true)
                        .busy_timeout(std::time::Duration::from_secs(300))
                        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
                        .synchronous(sqlx::sqlite::SqliteSynchronous::Normal)
                        .foreign_keys(true)
                        .optimize_on_close(true, Some(1000))
                        .extension("mod_spatialite")
                },
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

        let nats = async_nats::connect_with_options(
            super::NATS_HOST,
            async_nats::ConnectOptions::default().request_timeout(None), // Disable default request timeout
        )
        .await
        .expect("Unable to connect to NATS");

        super::NATS
            .set(nats)
            .expect("Unable to set shared NATS connection");
    }

    pub async fn extra_db(
        db_name: impl AsRef<str>,
        create: bool,
    ) -> Result<SqlitePool, sqlx::Error> {
        Ok(SqlitePool::connect_with(
            // SAFETY: mod_spatialite is a trusted extension shipped with the
            // deployment; loading it is required for spatial query support.
            unsafe {
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
                    .extension("mod_spatialite")
            },
        )
        .await?)
    }
}

#[cfg(feature = "database")]
pub use database::{db, extra_db, init, read_only_db, read_only_pool};

#[cfg(not(feature = "database"))]
pub async fn init() {
    let nats = async_nats::connect_with_options(
        NATS_HOST,
        async_nats::ConnectOptions::default().request_timeout(None), // Disable default request timeout
    )
    .await
    .expect("Unable to connect to NATS");

    NATS.set(nats)
        .expect("Unable to set shared NATS connection");
}
