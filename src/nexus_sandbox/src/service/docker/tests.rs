#[cfg(test)]
mod tests {
    use crate::service::docker::{DockerConfig, DockerError};

    #[test]
    fn test_docker_config_defaults() {
        let config = DockerConfig::default();
        assert_eq!(config.image_name, "nexus_sandbox");
        assert_eq!(config.image_tag, "latest");
        assert_eq!(config.dockerfile_path, "src/nexus_sandbox/.docker/Dockerfile");
        assert_eq!(config.build_context, ".");
    }

    #[test]
    fn test_docker_config_validation_valid() {
        let config = DockerConfig {
            image_name: "my-image".to_string(),
            image_tag: "v1.0.0".to_string(),
            ..Default::default()
        };
        assert!(config.validate().is_ok());

        let config = DockerConfig {
            image_name: "my_image".to_string(),
            image_tag: "latest".to_string(),
            ..Default::default()
        };
        assert!(config.validate().is_ok());

        let config = DockerConfig {
            image_name: "my/image".to_string(),
            image_tag: "1.0".to_string(),
            ..Default::default()
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_docker_config_validation_invalid_name() {
        let config = DockerConfig {
            image_name: "invalid name".to_string(),
            ..Default::default()
        };
        assert!(matches!(config.validate(), Err(DockerError::InvalidConfig(_))));

        let config = DockerConfig {
            image_name: "!invalid".to_string(),
            ..Default::default()
        };
        assert!(matches!(config.validate(), Err(DockerError::InvalidConfig(_))));
    }

    #[test]
    fn test_docker_config_validation_invalid_tag() {
        let config = DockerConfig {
            image_tag: "invalid tag".to_string(),
            ..Default::default()
        };
        assert!(matches!(config.validate(), Err(DockerError::InvalidConfig(_))));

        let config = DockerConfig {
            image_tag: "/invalid".to_string(),
            ..Default::default()
        };
        assert!(matches!(config.validate(), Err(DockerError::InvalidConfig(_))));
    }
}

