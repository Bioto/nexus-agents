use thiserror::Error;

#[derive(Error, Debug)]
pub enum DockerError {
    #[error("Docker connection failed: {0}")]
    ConnectionFailed(String),
    
    #[error("Docker build failed: {0}")]
    BuildFailed(String),
    
    #[error("Docker image not found: {0}")]
    ImageNotFound(String),
    
    #[error("Container creation failed: {0}")]
    ContainerCreationFailed(String),
    
    #[error("Container start failed: {0}")]
    ContainerStartFailed(String),
    
    #[error("Container execution failed: {0}")]
    ExecutionFailed(String),
    
    #[error("IO error: {0}")]
    IoError(String),

    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),
}

