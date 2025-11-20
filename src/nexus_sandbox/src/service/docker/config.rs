use crate::service::docker::error::DockerError;
use regex::Regex;

#[derive(Debug, Clone)]
pub struct DockerConfig {
    pub image_name: String,
    pub image_tag: String,
    pub dockerfile_path: String,
    pub build_context: String,
}

impl Default for DockerConfig {
    fn default() -> Self {
        Self {
            image_name: "nexus_sandbox".to_string(),
            image_tag: "latest".to_string(),
            dockerfile_path: "src/nexus_sandbox/.docker/Dockerfile".to_string(),
            build_context: ".".to_string(),
        }
    }
}

impl DockerConfig {
    /// Validate the configuration
    pub fn validate(&self) -> Result<(), DockerError> {
        // Validate image name (alphanumeric, dashes, underscores, slashes)
        let name_regex = Regex::new(r"^[a-zA-Z0-9][a-zA-Z0-9._/-]*$").map_err(|e| 
            DockerError::InvalidConfig(format!("Failed to compile regex: {}", e)))?;
            
        if !name_regex.is_match(&self.image_name) {
            return Err(DockerError::InvalidConfig(format!(
                "Invalid image name: '{}'. Must match pattern: {}", 
                self.image_name, 
                name_regex.as_str()
            )));
        }

        // Validate image tag (alphanumeric, dashes, underscores, periods)
        let tag_regex = Regex::new(r"^[a-zA-Z0-9][a-zA-Z0-9._-]*$").map_err(|e| 
            DockerError::InvalidConfig(format!("Failed to compile regex: {}", e)))?;

        if !tag_regex.is_match(&self.image_tag) {
            return Err(DockerError::InvalidConfig(format!(
                "Invalid image tag: '{}'. Must match pattern: {}", 
                self.image_tag, 
                tag_regex.as_str()
            )));
        }

        Ok(())
    }
}

