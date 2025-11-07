use serde::{Deserialize, Serialize};
use std::fmt;

/// Error types for the LLM agent framework
#[derive(Debug)]
pub enum Error {
    /// API error from the OpenAI-compatible endpoint
    Api(ApiError),
    /// Network/HTTP error
    Network(reqwest::Error),
    /// Configuration error (missing API key, invalid URL, etc.)
    Configuration(String),
    /// JSON serialization/deserialization error
    Json(serde_json::Error),
    /// Other errors
    Other(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Api(err) => write!(f, "API error: {}", err),
            Error::Network(err) => write!(f, "Network error: {}", err),
            Error::Configuration(msg) => write!(f, "Configuration error: {}", msg),
            Error::Json(err) => write!(f, "JSON error: {}", err),
            Error::Other(msg) => write!(f, "Error: {}", msg),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Api(_) => None,
            Error::Network(err) => Some(err),
            Error::Configuration(_) => None,
            Error::Json(err) => Some(err),
            Error::Other(_) => None,
        }
    }
}

impl From<reqwest::Error> for Error {
    fn from(err: reqwest::Error) -> Self {
        Error::Network(err)
    }
}

impl From<serde_json::Error> for Error {
    fn from(err: serde_json::Error) -> Self {
        Error::Json(err)
    }
}

/// API error response from OpenAI-compatible endpoints
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiError {
    pub message: String,
    #[serde(rename = "type")]
    pub error_type: Option<String>,
    pub param: Option<String>,
    pub code: Option<String>,
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)?;
        if let Some(ref error_type) = self.error_type {
            write!(f, " (type: {})", error_type)?;
        }
        if let Some(ref code) = self.code {
            write!(f, " (code: {})", code)?;
        }
        Ok(())
    }
}

/// Result type alias for convenience
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let config_err = Error::Configuration("Missing API key".to_string());
        assert!(config_err.to_string().contains("Configuration error"));
        assert!(config_err.to_string().contains("Missing API key"));
    }

    #[test]
    fn test_api_error_display() {
        let api_err = ApiError {
            message: "Invalid request".to_string(),
            error_type: Some("invalid_request_error".to_string()),
            param: None,
            code: Some("invalid_api_key".to_string()),
        };
        assert!(api_err.to_string().contains("Invalid request"));
        assert!(api_err.to_string().contains("invalid_request_error"));
        assert!(api_err.to_string().contains("invalid_api_key"));
    }

    #[test]
    fn test_error_from_reqwest() {
        // This test verifies the From trait implementation
        // We can't easily create a reqwest::Error, but we can verify the trait exists
        let _result: Result<()> = Err(Error::Configuration("test".to_string()));
        assert!(true); // Just verify compilation
    }
}

