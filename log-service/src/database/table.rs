use crate::error::{LogServiceError, Result};
use datafusion::arrow::datatypes::Schema;
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use std::io::BufWriter;
use std::path::Path;
use tokio::sync::{mpsc, oneshot};

use std::path::PathBuf;

/// Registration for automatic table discovery
pub struct TableRegistration {
    pub name: &'static str,
    pub schema: fn() -> Schema,
}

inventory::collect!(TableRegistration);

/// Get all registered tables
pub fn get_registered_tables() -> Vec<&'static TableRegistration> {
    inventory::iter::<TableRegistration>().collect()
}

/// Trait for table row types - each table has its own implementing type
pub trait TableRow: Clone + Serialize + for<'de> Deserialize<'de> + Send + Sync + 'static {
    /// The name of the table this row belongs to
    const TABLE_NAME: &'static str;

    /// Get the Arrow schema for this table
    fn schema() -> Schema;
}

/// Self-contained table with embedded writer
pub struct Table<T: TableRow> {
    sender: mpsc::Sender<Write<T>>,
    writer_handle: tokio::task::JoinHandle<Result<()>>,
}

/// Rows for the writer, and where to report whether they reached the WAL.
struct Write<T> {
    rows: Vec<T>,
    written: oneshot::Sender<Result<()>>,
}

impl<T: TableRow> Table<T> {
    /// Set up a new table with its own dedicated writer
    pub async fn new(base_path: impl AsRef<Path>) -> Result<Self> {
        let (sender, receiver) = mpsc::channel::<Write<T>>(1000);
        let base_path = base_path.as_ref().to_path_buf();

        // Spawn the writer task for this specific table
        let writer_handle =
            tokio::spawn(async move { table_writer_task::<T>(receiver, base_path).await });

        // Table setup completed successfully

        Ok(Self {
            sender,
            writer_handle,
        })
    }

    /// Insert a row into this table, returning once it is in the WAL.
    pub async fn insert(&self, row: T) -> Result<()> {
        self.insert_all(vec![row]).await
    }

    /// Insert rows into this table together, returning once they are in the
    /// WAL.
    ///
    /// They go in as one append, so an importer that logs an event as several
    /// rows learns of a failure before it records the event as logged, rather
    /// than recording it and losing the rows in the writer.
    pub async fn insert_all(&self, rows: Vec<T>) -> Result<()> {
        if rows.is_empty() {
            return Ok(());
        }

        let (written, outcome) = oneshot::channel();

        self.sender.send(Write { rows, written }).await?;

        // Dropped unanswered only if the writer has stopped.
        outcome.await.map_err(|_| LogServiceError::ChannelSend)?
    }

    /// Get the table name
    pub fn name(&self) -> &'static str {
        T::TABLE_NAME
    }

    /// Get the schema
    pub fn schema(&self) -> Schema {
        T::schema()
    }

    /// Gracefully shutdown this table's writer
    pub async fn shutdown(self) -> Result<()> {
        drop(self.sender); // Close the channel to signal shutdown
        self.writer_handle.await??;
        Ok(())
    }
}

/// Individual table writer with per-write locking and daily rotation
struct TableWriter {
    table_name: String,
    base_path: std::path::PathBuf,
}

impl TableWriter {
    fn new(table_name: &str, base_path: PathBuf) -> Self {
        Self {
            table_name: table_name.to_string(),
            base_path,
        }
    }

    /// Append rows to this table's WAL as one locked write
    async fn write_rows<T: Serialize>(&mut self, rows: &[T]) -> Result<()> {
        let wal_dir = self.base_path.join("wal").join(&self.table_name);
        tokio::fs::create_dir_all(&wal_dir).await?;

        let mut line = String::new();
        for row in rows {
            line.push_str(&serde_json::to_string(row)?);
            line.push('\n');
        }

        let table_name = self.table_name.clone();

        // Move entire atomic write operation (including lock acquisition) to spawn_blocking
        // to avoid blocking the async executor during disk I/O operations
        tokio::task::spawn_blocking(move || -> Result<()> {
            use std::os::unix::fs::MetadataExt;

            // Retry with exponential backoff for lock acquisition (infinite retries for data integrity)
            let mut retry_delay = std::time::Duration::from_millis(1);

            loop {
                // The day is read on every attempt, not once per write: a
                // write still waiting at midnight belongs in the new day's
                // file, since the compactor is about to take the old one.
                let date = Timestamp::now().strftime("%Y-%m-%d").to_string();
                let wal_path = wal_dir.join(format!("{table_name}_{date}.jsonl"));

                // Open file fresh each time to ensure we see latest data from other processes
                let file = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&wal_path)?;

                match file.try_lock() {
                    Ok(_) => {
                        // The compactor takes this lock to move a finished
                        // day's file out of the WAL. A file opened before it
                        // moved is no longer at its path once the lock is
                        // ours, and a row appended to it would never be read.
                        let current = std::fs::metadata(&wal_path).ok();
                        let opened = file.metadata()?;

                        if current.is_none_or(|current| current.ino() != opened.ino()) {
                            continue;
                        }

                        use std::io::Write;

                        // Create a new writer for this specific write operation
                        let mut writer = BufWriter::new(&file);
                        writer.write_all(line.as_bytes())?;
                        writer.flush()?;

                        // Lock automatically released when file goes out of scope
                        return Ok(());
                    }
                    Err(_) => {
                        // Retry after delay - lock contention is expected
                        std::thread::sleep(retry_delay);
                        retry_delay =
                            std::cmp::min(retry_delay * 2, std::time::Duration::from_millis(500));
                    }
                }
            }
        })
        .await?
    }
}

/// Generic writer task for any table type
async fn table_writer_task<T: TableRow>(
    mut rx: mpsc::Receiver<Write<T>>,
    base_path: PathBuf,
) -> Result<()> {
    let mut writer = TableWriter::new(T::TABLE_NAME, base_path);

    // Process rows until channel is closed
    while let Some(write) = rx.recv().await {
        let outcome = writer.write_rows(&write.rows).await;

        if let Err(e) = &outcome {
            eprintln!("Failed to write rows to {}: {}", T::TABLE_NAME, e);
        }

        // A caller that stopped waiting has nobody to tell.
        let _ = write.written.send(outcome);
    }

    // Writer task shutting down normally
    Ok(())
}
