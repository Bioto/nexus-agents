use thiserror::Error;

pub type Result<T> = std::result::Result<T, VoiceError>;

#[derive(Debug, Error)]
#[allow(dead_code)]
pub enum VoiceError {
    #[error("Audio error: {0}")]
    Audio(String),

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

impl From<anyhow::Error> for VoiceError {
    fn from(err: anyhow::Error) -> Self {
        VoiceError::Other(err.to_string())
    }
}

impl From<cpal::DevicesError> for VoiceError {
    fn from(err: cpal::DevicesError) -> Self {
        VoiceError::Audio(format!("Device error: {}", err))
    }
}

impl From<cpal::StreamError> for VoiceError {
    fn from(err: cpal::StreamError) -> Self {
        VoiceError::Audio(format!("Stream error: {}", err))
    }
}

impl From<cpal::DefaultStreamConfigError> for VoiceError {
    fn from(err: cpal::DefaultStreamConfigError) -> Self {
        VoiceError::Audio(format!("Stream config error: {}", err))
    }
}

impl From<cpal::SupportedStreamConfigsError> for VoiceError {
    fn from(err: cpal::SupportedStreamConfigsError) -> Self {
        VoiceError::Audio(format!("Supported config error: {}", err))
    }
}

impl From<cpal::BuildStreamError> for VoiceError {
    fn from(err: cpal::BuildStreamError) -> Self {
        VoiceError::Audio(format!("Build stream error: {}", err))
    }
}

impl From<cpal::PlayStreamError> for VoiceError {
    fn from(err: cpal::PlayStreamError) -> Self {
        VoiceError::Audio(format!("Play stream error: {}", err))
    }
}

impl From<cpal::PauseStreamError> for VoiceError {
    fn from(err: cpal::PauseStreamError) -> Self {
        VoiceError::Audio(format!("Pause stream error: {}", err))
    }
}

