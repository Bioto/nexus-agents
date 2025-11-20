use thiserror::Error;

#[derive(Error, Debug)]
pub enum NexusError {
    #[error("Configuration error: {0}")]
    Config(String),

    #[error("HTTP error: {0}")]
    Http(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("MCP error: {0}")]
    Mcp(String),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Server error: {0}")]
    Server(String),

    #[error("Parsing error: {0}")]
    Parse(String),
    
    #[error("Unknown error: {0}")]
    Unknown(String),
}

impl From<serde_json::Error> for NexusError {
    fn from(err: serde_json::Error) -> Self {
        NexusError::Serialization(err.to_string())
    }
}

impl From<toml::de::Error> for NexusError {
    fn from(err: toml::de::Error) -> Self {
        NexusError::Config(format!("Failed to parse TOML: {}", err))
    }
}

impl From<reqwest::Error> for NexusError {
    fn from(err: reqwest::Error) -> Self {
        NexusError::Http(err.to_string())
    }
}

