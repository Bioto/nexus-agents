use thiserror::Error;

/// Common result type for storage operations.
pub type Result<T> = std::result::Result<T, StorageError>;

/// Storage-level errors that can be shared across crates.
#[derive(Debug, Error)]
pub enum StorageError {
    #[error("Configuration error: {0}")]
    Configuration(String),
    #[error("Database error: {0}")]
    Database(String),
    #[error("Network error: {0}")]
    Network(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Serialization error: {0}")]
    Serialization(String),
    #[error("{0}")]
    Other(String),
}

#[cfg(feature = "clickhouse")]
impl From<reqwest::Error> for StorageError {
    fn from(err: reqwest::Error) -> Self {
        StorageError::Network(err.to_string())
    }
}

impl From<serde_json::Error> for StorageError {
    fn from(err: serde_json::Error) -> Self {
        StorageError::Serialization(err.to_string())
    }
}

#[cfg(feature = "postgres")]
impl From<sqlx::Error> for StorageError {
    fn from(err: sqlx::Error) -> Self {
        StorageError::Database(err.to_string())
    }
}

#[cfg(feature = "clickhouse")]
impl From<clickhouse_rs::errors::Error> for StorageError {
    fn from(err: clickhouse_rs::errors::Error) -> Self {
        StorageError::Database(err.to_string())
    }
}
