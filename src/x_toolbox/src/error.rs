use thiserror::Error;

pub type Result<T> = std::result::Result<T, ToolboxError>;

#[derive(Debug, Error)]
#[allow(dead_code)]
pub enum ToolboxError {
    #[error("Toolbox error: {0}")]
    Toolbox(String),

    #[error("Configuration error: {0}")]
    Configuration(String),

    #[error("Database error: {0}")]
    Database(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("{0}")]
    Other(String),
}

impl From<anyhow::Error> for ToolboxError {
    fn from(err: anyhow::Error) -> Self {
        ToolboxError::Other(err.to_string())
    }
}

impl From<sqlx::Error> for ToolboxError {
    fn from(err: sqlx::Error) -> Self {
        ToolboxError::Database(err.to_string())
    }
}

impl From<uuid::Error> for ToolboxError {
    fn from(err: uuid::Error) -> Self {
        ToolboxError::Validation(format!("Invalid UUID: {}", err))
    }
}

impl From<rust_decimal::Error> for ToolboxError {
    fn from(err: rust_decimal::Error) -> Self {
        ToolboxError::Validation(format!("Invalid decimal: {}", err))
    }
}

impl From<serde_json::Error> for ToolboxError {
    fn from(err: serde_json::Error) -> Self {
        ToolboxError::Validation(format!("JSON serialization error: {}", err))
    }
}


