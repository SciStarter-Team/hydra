use thiserror::Error;

/// Custom error type for the log-service library
#[derive(Error, Debug)]
pub enum LogServiceError {
    /// Database-related errors (SQLx, SQLite, etc.)
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    /// DataFusion query engine errors
    #[error("DataFusion error: {0}")]
    DataFusion(#[from] datafusion::error::DataFusionError),

    /// Arrow format errors
    #[error("Arrow error: {0}")]
    Arrow(#[from] datafusion::arrow::error::ArrowError),

    /// Parquet file format errors
    #[error("Parquet error: {0}")]
    Parquet(#[from] parquet::errors::ParquetError),

    /// Object store errors (for file access)
    #[error("Object store error: {0}")]
    ObjectStore(#[from] object_store::Error),

    /// Serialization/deserialization errors
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// Flexbuffers serialization errors
    #[error("Flexbuffers error: {0}")]
    Flexbuffers(#[from] flexbuffers::SerializationError),

    /// URL parsing errors
    #[error("URL error: {0}")]
    Url(#[from] url::ParseError),

    /// ULID parsing errors
    #[error("ULID error: {0}")]
    Ulid(#[from] ulid::DecodeError),

    /// Date/time parsing errors
    #[error("Time error: {0}")]
    Time(#[from] jiff::Error),

    /// Channel communication errors
    #[error("Channel send error")]
    ChannelSend,

    /// Task join errors
    #[error("Task join error: {0}")]
    TaskJoin(#[from] tokio::task::JoinError),

    /// I/O errors
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Analytics-specific errors
    #[error("Analytics error: {0}")]
    Analytics(String),

    /// Cache operation errors
    #[error("Cache error: {0}")]
    Cache(String),

    /// Configuration errors
    #[error("Configuration error: {0}")]
    Config(String),

    /// Validation errors
    #[error("Validation error: {0}")]
    Validation(String),

    /// Invalid file path errors
    #[error("Invalid file path: {0}")]
    InvalidFilePath(std::path::PathBuf),

    /// Missing required field or builder
    #[error("Missing required field: {0}")]
    MissingField(String),

    /// Unsupported data type
    #[error("Unsupported data type: {0}")]
    UnsupportedDataType(String),
}

/// Result type alias for log-service operations
pub type Result<T> = std::result::Result<T, LogServiceError>;

impl<T> From<tokio::sync::mpsc::error::SendError<T>> for LogServiceError {
    fn from(_: tokio::sync::mpsc::error::SendError<T>) -> Self {
        LogServiceError::ChannelSend
    }
}
