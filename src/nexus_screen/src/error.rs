use thiserror::Error;

pub type Result<T> = std::result::Result<T, ScreenError>;

#[derive(Debug, Error)]
#[allow(dead_code)]
pub enum ScreenError {
    #[error("Screen error: {0}")]
    Screen(String),

    #[error("Screen capture error: {0}")]
    ScreenCapture(String),

    #[error("Video encoding error: {0}")]
    VideoEncoding(String),

    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("Configuration error: {0}")]
    Configuration(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("TUI error: {0}")]
    Tui(String),

    #[error("API error: {0}")]
    Api(String),

    #[error("{0}")]
    Other(String),
}

impl From<anyhow::Error> for ScreenError {
    fn from(err: anyhow::Error) -> Self {
        ScreenError::Other(err.to_string())
    }
}

