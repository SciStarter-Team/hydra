use futures::stream::BoxStream;
use sqlx::{Database, Error as SqlxError, Executor, Sqlite, SqlitePool, pool::PoolConnection};
use std::future::Future;
use std::pin::Pin;

/// A SQLite transaction that uses `BEGIN IMMEDIATE` and automatically rolls back if dropped.
///
/// This transaction type provides immediate write lock acquisition, which means it will
/// fail fast if another writer is active, rather than waiting. This makes it ideal for
/// handling write conflicts in concurrent applications.
///
/// # Features
///
/// - **Immediate locking**: Uses `BEGIN IMMEDIATE` to acquire write locks immediately
/// - **Automatic rollback**: Rolls back automatically if dropped without explicit commit
/// - **Built-in retry logic**: Optional retry mechanism with exponential backoff
/// - **Executor implementation**: Can be used anywhere an `Executor` is expected
///
/// # Example
///
/// ```rust
/// # use sqlx::SqlitePool;
/// # use common::transaction::Immediate;
/// # async fn example(pool: &SqlitePool) -> Result<(), sqlx::Error> {
/// // Simple transaction with automatic rollback
/// {
///     let mut tx = Immediate::begin(pool).await?;
///     sqlx::query("INSERT INTO users (name) VALUES (?)").bind("Alice").execute(&mut tx).await?;
///     tx.commit().await?; // Must explicitly commit
/// } // Would rollback here if commit() wasn't called
///
/// // Transaction with retry logic
/// let mut tx = Immediate::begin_with_retry(pool, None, None).await?;
/// tx.execute(sqlx::query("UPDATE users SET active = true")).await?;
/// tx.commit().await?;
///
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct Immediate {
    conn: Option<PoolConnection<Sqlite>>,
}

impl Immediate {
    /// Begin an immediate transaction.
    ///
    /// This will execute `BEGIN IMMEDIATE` which attempts to acquire a write lock
    /// immediately. If another writer is active, this will fail fast with a
    /// `SQLITE_BUSY` error rather than waiting.
    ///
    /// # Errors
    ///
    /// - `SQLITE_BUSY` (code 5) if another writer is active
    /// - `SQLITE_LOCKED` (code 6) if the database is locked
    /// - `PoolTimedOut` if no connections are available
    /// - Other database errors for connection or SQL issues
    ///
    /// # Example
    ///
    /// ```rust
    /// # use sqlx::SqlitePool;
    /// # use common::transaction::Immediate;
    /// # async fn example(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    /// let mut tx = Immediate::begin(pool).await?;
    /// sqlx::query("INSERT INTO users (name) VALUES (?)").bind("Alice").execute(&mut tx).await?;
    /// tx.commit().await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn begin(pool: &SqlitePool) -> Result<Self, SqlxError> {
        let mut conn = pool.acquire().await?;

        // Start immediate transaction - fails fast if write lock unavailable
        sqlx::query("BEGIN IMMEDIATE").execute(&mut *conn).await?;

        Ok(Self { conn: Some(conn) })
    }

    /// Begin an immediate transaction with retry logic.
    ///
    /// This will attempt to begin an immediate transaction, retrying on transient
    /// errors with exponential backoff. Only retries on `SQLITE_BUSY`, `SQLITE_LOCKED`,
    /// and `PoolTimedOut` errors.
    ///
    /// # Parameters
    ///
    /// - `max_attempts`: Maximum number of attempts (default: 5)
    /// - `base_delay_ms`: Base delay in milliseconds between retries (default: 10)
    ///
    /// The delay doubles after each attempt, capped at 1000ms.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use sqlx::SqlitePool;
    /// # use common::transaction::Immediate;
    /// # async fn example(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    /// // Use defaults (5 attempts, 10ms base delay)
    /// let tx = Immediate::begin_with_retry(pool, None, None).await?;
    ///
    /// // Custom retry settings
    /// let tx = Immediate::begin_with_retry(pool, 3, 50).await?;
    ///
    /// // Mix of default and custom
    /// let tx = Immediate::begin_with_retry(pool, 10, None).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn begin_with_retry(
        pool: &SqlitePool,
        max_attempts: impl Into<Option<u32>>,
        base_delay_ms: impl Into<Option<u64>>,
    ) -> Result<Self, SqlxError> {
        use tokio::time::{Duration, sleep};

        let max_attempts = max_attempts.into().unwrap_or(5);
        let base_delay_ms = base_delay_ms.into().unwrap_or(10);

        let mut delay = Duration::from_millis(base_delay_ms);
        let mut last_error = None;

        for attempt in 1..=max_attempts {
            match Self::begin(pool).await {
                Ok(tx) => return Ok(tx),
                Err(err) => {
                    last_error = Some(err);

                    if attempt < max_attempts
                        && Self::is_retryable_error(last_error.as_ref().unwrap())
                    {
                        eprintln!(
                            "BEGIN IMMEDIATE failed on attempt {}/{}, retrying in {:?}",
                            attempt, max_attempts, delay
                        );

                        sleep(delay).await;
                        delay = std::cmp::min(delay * 2, Duration::from_millis(1000));
                    }
                }
            }
        }

        Err(last_error.unwrap())
    }

    /// Commit the transaction.
    ///
    /// This consumes the transaction and makes all changes permanent.
    /// After calling this method, the transaction cannot be used again.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use sqlx::SqlitePool;
    /// # use common::transaction::Immediate;
    /// # async fn example(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    /// let mut tx = Immediate::begin(pool).await?;
    /// sqlx::query("INSERT INTO users (name) VALUES (?)").bind("Alice").execute(&mut tx).await?;
    /// tx.commit().await?; // Changes are now permanent
    /// # Ok(())
    /// # }
    /// ```
    pub async fn commit(mut self) -> Result<(), SqlxError> {
        let mut conn = self.conn.take().expect("Transaction has been consumed");

        sqlx::query("COMMIT").execute(&mut *conn).await?;

        Ok(())
    }

    /// Explicitly rollback the transaction.
    ///
    /// This consumes the transaction and discards all changes.
    /// Note that transactions will automatically rollback when dropped,
    /// so this method is only needed when you want explicit control.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use sqlx::SqlitePool;
    /// # use common::transaction::Immediate;
    /// # async fn example(pool: &SqlitePool, some_error_condition: bool) -> Result<(), sqlx::Error> {
    /// let mut tx = Immediate::begin(pool).await?;
    /// sqlx::query("INSERT INTO users (name) VALUES (?)").bind("Alice").execute(&mut tx).await?;
    ///
    /// if some_error_condition {
    ///     tx.rollback().await?; // Explicitly discard changes
    /// } else {
    ///     tx.commit().await?;
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn rollback(mut self) -> Result<(), SqlxError> {
        let mut conn = self.conn.take().expect("Transaction has been consumed");

        sqlx::query("ROLLBACK").execute(&mut *conn).await?;

        Ok(())
    }

    /// Check if an error is retryable for SQLite operations.
    ///
    /// Returns `true` for:
    /// - `SQLITE_BUSY` (code 5) - database is locked by another process
    /// - `SQLITE_LOCKED` (code 6) - database table is locked  
    /// - `PoolTimedOut` - connection pool is exhausted
    fn is_retryable_error(err: &SqlxError) -> bool {
        match err {
            SqlxError::Database(db_err) => {
                // SQLite error codes can be either string or numeric representations
                if let Some(code) = db_err.code() {
                    matches!(code.as_ref(), "5" | "6" | "SQLITE_BUSY" | "SQLITE_LOCKED")
                } else {
                    false
                }
            }
            SqlxError::PoolTimedOut => true,
            _ => false,
        }
    }
}

impl<'c> Executor<'c> for &'c mut Immediate {
    type Database = Sqlite;

    fn fetch_many<'e, 'q: 'e, E: 'q>(
        self,
        query: E,
    ) -> BoxStream<
        'e,
        Result<sqlx::Either<sqlx::sqlite::SqliteQueryResult, sqlx::sqlite::SqliteRow>, SqlxError>,
    >
    where
        'c: 'e,
        E: sqlx::Execute<'q, Self::Database>,
    {
        let conn = self.conn.as_mut().expect("Transaction has been consumed");
        conn.fetch_many(query)
    }

    fn fetch_optional<'e, 'q: 'e, E: 'q>(
        self,
        query: E,
    ) -> Pin<Box<dyn Future<Output = Result<Option<sqlx::sqlite::SqliteRow>, SqlxError>> + Send + 'e>>
    where
        'c: 'e,
        E: sqlx::Execute<'q, Self::Database>,
    {
        let conn = self.conn.as_mut().expect("Transaction has been consumed");
        conn.fetch_optional(query)
    }

    fn prepare_with<'e, 'q: 'e>(
        self,
        sql: &'q str,
        parameters: &'e [<Self::Database as Database>::TypeInfo],
    ) -> Pin<
        Box<
            dyn Future<Output = Result<<Self::Database as Database>::Statement<'q>, SqlxError>>
                + Send
                + 'e,
        >,
    >
    where
        'c: 'e,
    {
        let conn = self.conn.as_mut().expect("Transaction has been consumed");
        conn.prepare_with(sql, parameters)
    }

    fn describe<'e, 'q: 'e>(
        self,
        sql: &'q str,
    ) -> Pin<Box<dyn Future<Output = Result<sqlx::Describe<Self::Database>, SqlxError>> + Send + 'e>>
    where
        'c: 'e,
    {
        let conn = self.conn.as_mut().expect("Transaction has been consumed");
        conn.describe(sql)
    }
}

/// Automatic rollback when dropped without explicit commit.
///
/// This ensures that if a transaction is dropped (due to an error, early return, etc.)
/// without being explicitly committed, it will be rolled back to prevent partial state.
///
/// Note: The rollback is performed in a spawned task since Drop cannot be async.
impl Drop for Immediate {
    fn drop(&mut self) {
        if let Some(mut conn) = self.conn.take() {
            tokio::spawn(async move {
                if let Err(e) = sqlx::query("ROLLBACK").execute(&mut *conn).await {
                    eprintln!("Failed to rollback transaction in Drop: {}", e);
                }
            });
        }
    }
}
