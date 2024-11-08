use poem::endpoint::{EmbeddedFileEndpoint, EmbeddedFilesEndpoint};
use rust_embed::Embed;
use sqlx::{sqlite::SqliteConnectOptions, SqlitePool};

#[cfg(debug_assertions)]
const STORAGE_PATH: &'static str = ".";

#[cfg(not(debug_assertions))]
const STORAGE_PATH: &'static str = env!("STORAGE_PATH");

#[derive(Embed)]
#[folder = "dist/"]
struct Static;

type StaticFiles = EmbeddedFilesEndpoint<Static>;
type StaticFile = EmbeddedFileEndpoint<Static>;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let conx = SqlitePool::connect_with(
        SqliteConnectOptions::new()
            .filename(std::path::Path::join(
                STORAGE_PATH.as_ref(),
                "frontend.sqlite",
            ))
            .create_if_missing(true)
            .busy_timeout(std::time::Duration::from_secs(300))
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
            .synchronous(sqlx::sqlite::SqliteSynchronous::Normal)
            .foreign_keys(true)
            .optimize_on_close(true, Some(1000))
            .extension("mod_spatialite"),
    )
    .await?;

    sqlx::migrate!("./migrations").run(&conx).await?;

    Ok(())
}
