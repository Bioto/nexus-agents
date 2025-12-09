use ffmpeg_next as ffmpeg;
use nexus_storage::StorageError;
use thiserror::Error;

pub type Result<T> = std::result::Result<T, RecorderError>;

#[derive(Debug, Error)]
#[allow(dead_code)]
pub enum RecorderError {
    // Audio errors
    #[error("Audio error: {0}")]
    Audio(String),

    // Screen/Video errors
    #[error("Screen capture error: {0}")]
    ScreenCapture(String),

    #[error("Video encoding error: {0}")]
    VideoEncoding(String),

    // Input capture errors
    #[error("Input capture error: {0}")]
    InputCapture(String),

    // Common errors
    #[error("Configuration error: {0}")]
    Configuration(String),

    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("TUI error: {0}")]
    Tui(String),

    #[error("API error: {0}")]
    Api(String),

    #[error("Database error: {0}")]
    Database(String),

    #[error("Window PID lookup failed for window {window_id}")]
    WindowPidLookup { window_id: String },

    #[error("Command '{command}' failed: {message}")]
    CommandFailed { command: String, message: String },

    #[error("Monitor {index} not found")]
    MonitorNotFound { index: usize },

    #[error("{0}")]
    Other(String),
}

// FFmpeg error conversion
impl From<ffmpeg::Error> for RecorderError {
    fn from(err: ffmpeg::Error) -> Self {
        RecorderError::VideoEncoding(err.to_string())
    }
}

// anyhow error conversion
impl From<anyhow::Error> for RecorderError {
    fn from(err: anyhow::Error) -> Self {
        if let Some(e) = err.downcast_ref::<ffmpeg::Error>() {
            return RecorderError::VideoEncoding(e.to_string());
        }
        RecorderError::Other(err.to_string())
    }
}

// nexus_core error conversion
impl From<nexus_core::models::Error> for RecorderError {
    fn from(err: nexus_core::models::Error) -> Self {
        RecorderError::Other(err.to_string())
    }
}

impl From<StorageError> for RecorderError {
    fn from(err: StorageError) -> Self {
        match err {
            StorageError::Configuration(msg) => RecorderError::Configuration(msg),
            StorageError::Database(msg) => RecorderError::Database(msg),
            StorageError::Network(msg) => RecorderError::Api(msg),
            StorageError::Io(io_err) => RecorderError::Io(io_err),
            StorageError::Serialization(msg) => RecorderError::Other(msg),
            StorageError::Other(msg) => RecorderError::Other(msg),
        }
    }
}

// CPAL audio error conversions
impl From<cpal::DevicesError> for RecorderError {
    fn from(err: cpal::DevicesError) -> Self {
        RecorderError::Audio(format!("Device error: {}", err))
    }
}

impl From<cpal::StreamError> for RecorderError {
    fn from(err: cpal::StreamError) -> Self {
        RecorderError::Audio(format!("Stream error: {}", err))
    }
}

impl From<cpal::DefaultStreamConfigError> for RecorderError {
    fn from(err: cpal::DefaultStreamConfigError) -> Self {
        RecorderError::Audio(format!("Stream config error: {}", err))
    }
}

impl From<cpal::SupportedStreamConfigsError> for RecorderError {
    fn from(err: cpal::SupportedStreamConfigsError) -> Self {
        RecorderError::Audio(format!("Supported config error: {}", err))
    }
}

impl From<cpal::BuildStreamError> for RecorderError {
    fn from(err: cpal::BuildStreamError) -> Self {
        RecorderError::Audio(format!("Build stream error: {}", err))
    }
}

impl From<cpal::PlayStreamError> for RecorderError {
    fn from(err: cpal::PlayStreamError) -> Self {
        RecorderError::Audio(format!("Play stream error: {}", err))
    }
}

impl From<cpal::PauseStreamError> for RecorderError {
    fn from(err: cpal::PauseStreamError) -> Self {
        RecorderError::Audio(format!("Pause stream error: {}", err))
    }
}
