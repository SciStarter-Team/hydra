//! Taking participation back out of the log, as the compactor does it.

use anyhow::Result;
use assert2::check;
use log_service::database::compactor::Compactor;
use log_service::database::retraction;
use log_service::database::rewrite;
use log_service::database::storage::Database;
use log_service::database::tables::ParticipationRow;
use sqlx::SqlitePool;
use std::path::Path;
use std::sync::Arc;
use tempfile::TempDir;
use tokio::time::{Duration, sleep};
use tokio_util::sync::CancellationToken;

mod support;

fn row(user: &str, when: &str) -> ParticipationRow {
    ParticipationRow {
        origin: 3,
        when: when.to_string(),
        duration: 1.0,
        where_point: None,
        magnitude: 1,
        authenticated: true,
        extra: String::new(),
        fidelity: 0.5,
        entity_id: "01JKXMGGMD577ZA8Q57S2GY1V8".to_string(),
        user_id: Some(user.to_string()),
        task_name: Some("Data collection".to_string()),
    }
}

/// A finished day of the participation WAL, holding `rows`.
async fn finished_day(database: &Database, rows: &[ParticipationRow]) -> Result<()> {
    let dir = database.path().join("wal").join("participation");
    tokio::fs::create_dir_all(&dir).await?;

    let lines: Vec<String> = rows
        .iter()
        .map(serde_json::to_string)
        .collect::<std::result::Result<_, _>>()?;

    tokio::fs::write(
        dir.join("participation_2024-01-14.jsonl"),
        lines.join("\n") + "\n",
    )
    .await?;

    Ok(())
}

/// Queue a retraction as the importer does, dated far enough back that the
/// compactor does not wait for more to arrive with it.
async fn retract(pool: &SqlitePool, row: &ParticipationRow, count: u32) -> Result<()> {
    let mut tx = common::transaction::Immediate::begin(pool).await?;
    retraction::queue(&mut tx, row, count, "test").await?;
    tx.commit().await?;

    sqlx::query("UPDATE participation_retractions SET queued_at = '2024-01-01T00:00:00Z'")
        .execute(pool)
        .await?;

    Ok(())
}

fn logged(base: &Path) -> Result<Vec<ParticipationRow>> {
    let mut rows = Vec::new();

    for path in rewrite::parquet_files(base, "participation")? {
        for batch in rewrite::read_parquet(&path, None)? {
            rows.extend(rewrite::rows_from_batch::<ParticipationRow>(&batch?)?);
        }
    }

    Ok(rows)
}

async fn remaining(pool: &SqlitePool) -> Result<Vec<i64>> {
    Ok(
        sqlx::query_scalar(
            "SELECT remaining FROM participation_retractions ORDER BY retraction_id",
        )
        .fetch_all(pool)
        .await?,
    )
}

async fn run_compactor(database: &Arc<Database>, pool: &SqlitePool) -> Result<()> {
    let compactor = Compactor::with_config(database.clone(), Duration::from_millis(50), 30, 10_000)
        .with_log_cache(pool.clone());
    let cancel = CancellationToken::new();

    let task = {
        let cancel = cancel.clone();
        tokio::spawn(async move { compactor.run(cancel).await })
    };

    sleep(Duration::from_millis(500)).await;
    cancel.cancel();
    task.await??;
    Ok(())
}

#[tokio::test]
async fn the_compactor_takes_back_exactly_what_was_retracted() -> Result<()> {
    let dir = TempDir::new()?;
    let database = Arc::new(Database::new(dir.path().join("log")).await?);
    let pool = support::log_cache(dir.path()).await?;

    let kept = row("01JKXMFYFZ00000000000KEPT0", "2024-01-14T10:00:00.250Z");
    let taken = row("01JKXMFYFZ0000000000TAKEN0", "2024-01-14T11:00:00Z");
    let missing = row("01JKXMFYFZ000000000MISSING", "2024-01-14T12:00:00Z");

    finished_day(
        &database,
        &[
            kept.clone(),
            taken.clone(),
            kept.clone(),
            taken.clone(),
            taken.clone(),
        ],
    )
    .await?;

    retract(&pool, &taken, 2).await?;
    retract(&pool, &missing, 1).await?;

    run_compactor(&database, &pool).await?;

    let rows = logged(database.path())?;
    check!(rows.iter().filter(|row| **row == kept).count() == 2);
    check!(rows.iter().filter(|row| **row == taken).count() == 1);

    // Carried out in full, and the one naming rows the log does not hold
    // still waiting, with nothing taken in its place.
    check!(remaining(&pool).await? == [0, 1]);

    let moves: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM log_file_moves")
        .fetch_one(&pool)
        .await?;
    check!(moves == 0);

    Ok(())
}

#[tokio::test]
async fn a_recorded_replacement_is_put_in_place_before_anything_else() -> Result<()> {
    let dir = TempDir::new()?;
    let target = dir.path().join("participation_2024-01-14.jsonl");
    std::fs::write(&target, "old\n")?;

    // Written and recorded, then interrupted before the rename.
    let replacement = rewrite::replace_wal(&target, ["new".to_string()])?;
    let pool = support::log_cache(dir.path()).await?;

    sqlx::query(r#"INSERT INTO log_file_moves ("from", "to") VALUES (?, ?)"#)
        .bind(replacement.temporary().to_string_lossy())
        .bind(replacement.target().to_string_lossy())
        .execute(&pool)
        .await?;

    retraction::finish_moves(&pool).await?;

    check!(std::fs::read_to_string(&target)? == "new\n");
    check!(!replacement.temporary().exists());

    // Finishing again, with the rename already made, changes nothing.
    retraction::finish_moves(&pool).await?;
    check!(std::fs::read_to_string(&target)? == "new\n");

    Ok(())
}
