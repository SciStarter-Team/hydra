use crate::error::Result;
use datafusion::arrow::datatypes::Schema;
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use std::io::BufWriter;
use std::path::Path;
use tokio::sync::mpsc;

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
    sender: mpsc::Sender<T>,
    writer_handle: tokio::task::JoinHandle<Result<()>>,
}

impl<T: TableRow> Table<T> {
    /// Set up a new table with its own dedicated writer
    pub async fn new(base_path: impl AsRef<Path>) -> Result<Self> {
        let (sender, receiver) = mpsc::channel::<T>(1000);
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

    /// Insert a row into this table
    pub async fn insert(&self, row: T) -> Result<()> {
        self.sender.send(row).await?;
        Ok(())
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
    current_date: String,
}

impl TableWriter {
    fn new(table_name: &str, base_path: PathBuf) -> Self {
        Self {
            table_name: table_name.to_string(),
            base_path,
            current_date: String::new(),
        }
    }

    /// Update current date tracking - no longer maintains persistent file handles
    fn update_current_date(&mut self) {
        let today = Timestamp::now().strftime("%Y-%m-%d").to_string();
        if today != self.current_date {
            self.current_date = today;
            // Date update completed successfully
        }
    }

    fn get_wal_file_path(&self, date: &str) -> String {
        self.base_path
            .join("wal")
            .join(&self.table_name)
            .join(format!("{}_{}.jsonl", self.table_name, date))
            .to_string_lossy()
            .to_string()
    }

    /// Write a row directly to this table's WAL with per-write locking
    async fn write_row<T: Serialize>(&mut self, row: &T) -> Result<()> {
        self.update_current_date();

        let wal_path = self.get_wal_file_path(&self.current_date);
        if let Some(parent) = Path::new(&wal_path).parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let line = format!("{}\n", serde_json::to_string(row)?);

        // Move entire atomic write operation (including lock acquisition) to spawn_blocking
        // to avoid blocking the async executor during disk I/O operations
        tokio::task::spawn_blocking(move || -> Result<()> {
            // Retry with exponential backoff for lock acquisition (infinite retries for data integrity)
            let mut retry_delay = std::time::Duration::from_millis(1);

            loop {
                // Open file fresh each time to ensure we see latest data from other processes
                let file = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&wal_path)?;

                match file.try_lock() {
                    Ok(_) => {
                        // Successfully acquired exclusive lock - write immediately
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
    mut rx: mpsc::Receiver<T>,
    base_path: PathBuf,
) -> Result<()> {
    let mut writer = TableWriter::new(T::TABLE_NAME, base_path);

    // Writer task started successfully

    // Process rows until channel is closed
    while let Some(row) = rx.recv().await {
        if let Err(e) = writer.write_row(&row).await {
            eprintln!("Failed to write row to {}: {}", T::TABLE_NAME, e);
        }
    }

    // Writer task shutting down normally
    Ok(())
}
