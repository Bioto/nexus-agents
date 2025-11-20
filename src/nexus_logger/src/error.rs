use thiserror::Error;

pub type Result<T> = std::result::Result<T, LoggerError>;

#[derive(Debug, Error)]
#[allow(dead_code)]
pub enum LoggerError {
    #[error("Logger error: {0}")]
    Logger(String),

    #[error("Configuration error: {0}")]
    Configuration(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("{0}")]
    Other(String),
}

impl From<anyhow::Error> for LoggerError {
    fn from(err: anyhow::Error) -> Self {
        LoggerError::Other(err.to_string())
    }
}

impl From<nexus_core::models::Error> for LoggerError {
    fn from(err: nexus_core::models::Error) -> Self {
        LoggerError::Other(err.to_string())
    }
}

impl From<nexus_screen::ScreenError> for LoggerError {
    fn from(err: nexus_screen::ScreenError) -> Self {
        LoggerError::Other(err.to_string())
    }
}
