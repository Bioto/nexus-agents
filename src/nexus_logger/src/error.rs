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

impl From<nexus_audio::VoiceError> for LoggerError {
    fn from(err: nexus_audio::VoiceError) -> Self {
        match err {
            nexus_audio::VoiceError::Audio(msg) => LoggerError::Other(format!("Audio error: {}", msg)),
            nexus_audio::VoiceError::Configuration(msg) => LoggerError::Configuration(msg),
            nexus_audio::VoiceError::Io(e) => LoggerError::Io(e),
            nexus_audio::VoiceError::Tui(msg) => LoggerError::Other(format!("TUI error: {}", msg)),
            nexus_audio::VoiceError::Api(msg) => LoggerError::Other(format!("API error: {}", msg)),
            nexus_audio::VoiceError::Other(msg) => LoggerError::Other(msg),
        }
    }
}
