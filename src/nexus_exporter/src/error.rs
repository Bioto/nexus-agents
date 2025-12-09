use thiserror::Error;

/// Custom error type for the nexus-exporter crate.
#[derive(Error, Debug)]
pub enum ExporterError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("PDF generation error: {0}")]
    Pdf(String),

    #[error("Invalid configuration: {0}")]
    Config(String),

    #[error("Other error: {0}")]
    Other(String),
}

/// Result type alias for the nexus-exporter crate.
pub type Result<T> = std::result::Result<T, ExporterError>;
