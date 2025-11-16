use bollard::container::{Config, CreateContainerOptions, LogOutput, StartContainerOptions};
use bollard::exec::StartExecResults;
use bollard::Docker;
use futures_util::stream::StreamExt;
use std::collections::HashMap;
use std::env;
use std::path::Path;
use tokio::time;

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
            image_name: "nexus_py".to_string(),
            image_tag: "latest".to_string(),
            dockerfile_path: "src/nexus_py/.docker/Dockerfile".to_string(),
            build_context: ".".to_string(),
        }
    }
}

#[derive(Debug)]
pub enum DockerError {
    ConnectionFailed(String),
    BuildFailed(String),
    ImageNotFound(String),
    ContainerCreationFailed(String),
    ContainerStartFailed(String),
    ExecutionFailed(String),
    IoError(String),
}

impl std::fmt::Display for DockerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DockerError::ConnectionFailed(msg) => write!(f, "Docker connection failed: {}", msg),
            DockerError::BuildFailed(msg) => write!(f, "Docker build failed: {}", msg),
            DockerError::ImageNotFound(msg) => write!(f, "Docker image not found: {}", msg),
            DockerError::ContainerCreationFailed(msg) => {
                write!(f, "Container creation failed: {}", msg)
            }
            DockerError::ContainerStartFailed(msg) => write!(f, "Container start failed: {}", msg),
            DockerError::ExecutionFailed(msg) => write!(f, "Container execution failed: {}", msg),
            DockerError::IoError(msg) => write!(f, "IO error: {}", msg),
        }
    }
}

impl std::error::Error for DockerError {}

pub struct DockerService {
    docker: Docker,
    config: DockerConfig,
}

impl DockerService {
    /// Create a new Docker service instance
    /// Connects to the system Docker daemon via the default socket
    /// Supports standard Docker, Docker Desktop, and Colima
    pub async fn new(config: DockerConfig) -> Result<Self, DockerError> {
        // Try to connect to Docker daemon
        // First check DOCKER_HOST, then try Colima sockets, then default
        let docker = if let Ok(docker_host) = env::var("DOCKER_HOST") {
            // Use DOCKER_HOST if set
            Docker::connect_with_socket(&docker_host, 120, bollard::API_DEFAULT_VERSION).map_err(
                |e| {
                    DockerError::ConnectionFailed(format!(
                        "Failed to connect to Docker daemon at {}: {}",
                        docker_host, e
                    ))
                },
            )?
        } else if let Ok(home) = env::var("HOME") {
            // Try Colima sockets (new location first, then old)
            let colima_socket_new = format!("{}/.config/colima/default/docker.sock", home);
            let colima_socket_old = format!("{}/.colima/default/docker.sock", home);

            if Path::new(&colima_socket_new).exists() {
                Docker::connect_with_socket(
                    &format!("unix://{}", colima_socket_new),
                    120,
                    bollard::API_DEFAULT_VERSION,
                )
                .map_err(|e| {
                    DockerError::ConnectionFailed(format!(
                        "Failed to connect to Colima Docker daemon: {}",
                        e
                    ))
                })?
            } else if Path::new(&colima_socket_old).exists() {
                Docker::connect_with_socket(
                    &format!("unix://{}", colima_socket_old),
                    120,
                    bollard::API_DEFAULT_VERSION,
                )
                .map_err(|e| {
                    DockerError::ConnectionFailed(format!(
                        "Failed to connect to Colima Docker daemon: {}",
                        e
                    ))
                })?
            } else {
                // Fall back to default connection
                Docker::connect_with_local_defaults().map_err(|e| {
                    DockerError::ConnectionFailed(format!(
                        "Failed to connect to Docker daemon: {}. \
                        Make sure Docker is running and accessible.",
                        e
                    ))
                })?
            }
        } else {
            // No HOME, use default connection
            Docker::connect_with_local_defaults().map_err(|e| {
                DockerError::ConnectionFailed(format!(
                    "Failed to connect to Docker daemon: {}. \
                    Make sure Docker is running and accessible.",
                    e
                ))
            })?
        };

        Ok(Self { docker, config })
    }

    /// Create a Docker service with default configuration
    pub async fn with_defaults() -> Result<Self, DockerError> {
        Self::new(DockerConfig::default()).await
    }

    /// Get the full image name (name:tag)
    fn image_full_name(&self) -> String {
        format!("{}:{}", self.config.image_name, self.config.image_tag)
    }

    /// Check if the Docker image exists
    pub async fn image_exists(&self) -> Result<bool, DockerError> {
        let image_name = self.image_full_name();

        // Try inspect_image first (most reliable)
        match self.docker.inspect_image(&image_name).await {
            Ok(_) => {
                return Ok(true);
            }
            Err(e) => {
                let error_msg = e.to_string().to_lowercase();

                // If it's a "not found" error, try fallback method
                if error_msg.contains("not found")
                    || error_msg.contains("no such image")
                    || error_msg.contains("404")
                {
                    // Fallback: list all images and check repo_tags
                    let mut list_options = bollard::image::ListImagesOptions::<String>::default();
                    list_options.all = true;

                    let images =
                        self.docker
                            .list_images(Some(list_options))
                            .await
                            .map_err(|e| {
                                DockerError::ImageNotFound(format!("Failed to list images: {}", e))
                            })?;

                    // Debug: print what we're looking for and what we found
                    eprintln!("Looking for image: '{}'", image_name);
                    eprintln!("Checking {} images...", images.len());

                    // Check if any image has the tag we're looking for
                    let exists = images.iter().any(|img| {
                        // Check repo_tags (may be empty for some images)
                        if !img.repo_tags.is_empty() {
                            img.repo_tags.iter().any(|tag| {
                                let matches = tag == &image_name
                                    || tag == &self.config.image_name
                                    || tag.starts_with(&format!("{}:", self.config.image_name))
                                    || (tag.split(':').next().unwrap_or("")
                                        == self.config.image_name);
                                if matches {
                                    eprintln!("Found matching image: '{}'", tag);
                                }
                                matches
                            })
                        } else {
                            false
                        }
                    });

                    if !exists {
                        eprintln!(
                            "Image '{}' not found in {} listed images",
                            image_name,
                            images.len()
                        );
                        // Show all images with nexus_py in the name for debugging
                        let nexus_images: Vec<_> = images
                            .iter()
                            .filter(|img| img.repo_tags.iter().any(|tag| tag.contains("nexus")))
                            .flat_map(|img| img.repo_tags.iter())
                            .collect();
                        if !nexus_images.is_empty() {
                            eprintln!("Found nexus-related images: {:?}", nexus_images);
                        }
                        // Show first few images for debugging
                        let sample: Vec<_> = images
                            .iter()
                            .filter(|img| !img.repo_tags.is_empty())
                            .take(10)
                            .flat_map(|img| img.repo_tags.iter())
                            .collect();
                        if !sample.is_empty() {
                            eprintln!("Sample of available images: {:?}", sample);
                        }

                        // Last resort: try to inspect by ID or try creating a container
                        // Sometimes images exist but aren't in list_images
                        eprintln!("Attempting direct container creation test...");
                        let test_config = Config {
                            image: Some(image_name.clone()),
                            cmd: Some(vec!["echo".to_string(), "test".to_string()]),
                            ..Default::default()
                        };
                        match self
                            .docker
                            .create_container(None::<CreateContainerOptions<String>>, test_config)
                            .await
                        {
                            Ok(container) => {
                                // Image exists! Clean up the test container
                                let _ = self
                                    .docker
                                    .remove_container(
                                        &container.id,
                                        Some(bollard::container::RemoveContainerOptions {
                                            force: true,
                                            ..Default::default()
                                        }),
                                    )
                                    .await;
                                eprintln!("Image exists (verified by test container creation)");
                                return Ok(true);
                            }
                            Err(e) => {
                                let err_msg = e.to_string().to_lowercase();
                                if err_msg.contains("no such image")
                                    || err_msg.contains("not found")
                                {
                                    eprintln!(
                                        "Image confirmed not found via container creation test"
                                    );
                                } else {
                                    // Other error might mean image exists but can't create container
                                    eprintln!("Container creation test error (might indicate image exists): {}", e);
                                }
                            }
                        }
                    }

                    Ok(exists)
                } else {
                    // For other errors (connection issues, etc.), return an error
                    Err(DockerError::ImageNotFound(format!(
                        "Failed to check if image exists: {}",
                        e
                    )))
                }
            }
        }
    }

    /// Create a container with the specified command
    pub async fn create_container(
        &self,
        command: Vec<String>,
        env: Option<HashMap<String, String>>,
    ) -> Result<String, DockerError> {
        self.create_container_with_mounts(command, env, None).await
    }

    /// Create a container with the specified command and volume mounts
    pub async fn create_container_with_mounts(
        &self,
        command: Vec<String>,
        env: Option<HashMap<String, String>>,
        mounts: Option<Vec<(String, String)>>, // Vec of (host_path, container_path) tuples
    ) -> Result<String, DockerError> {
        use bollard::models::{HostConfig, Mount, MountTypeEnum};

        let image_name = self.image_full_name();
        let container_name = format!("{}_{}", self.config.image_name, uuid::Uuid::new_v4());

        // Build mounts if provided
        let host_config = if let Some(mounts) = mounts {
            let docker_mounts: Vec<Mount> = mounts
                .into_iter()
                .map(|(host_path, container_path)| Mount {
                    target: Some(container_path),
                    source: Some(host_path),
                    typ: Some(MountTypeEnum::BIND),
                    read_only: Some(false),
                    ..Default::default()
                })
                .collect();

            Some(HostConfig {
                mounts: Some(docker_mounts),
                ..Default::default()
            })
        } else {
            None
        };

        let container_config = Config {
            image: Some(image_name),
            cmd: Some(command),
            env: env.map(|e| e.into_iter().map(|(k, v)| format!("{}={}", k, v)).collect()),
            attach_stdout: Some(true),
            attach_stderr: Some(true),
            host_config: host_config,
            ..Default::default()
        };

        let options = CreateContainerOptions {
            name: container_name.clone(),
            platform: None,
        };

        let result = self
            .docker
            .create_container(Some(options), container_config)
            .await
            .map_err(|e| {
                DockerError::ContainerCreationFailed(format!("Failed to create container: {}", e))
            })?;

        Ok(result.id)
    }

    /// Start a container by ID
    pub async fn start_container(&self, container_id: &str) -> Result<(), DockerError> {
        self.docker
            .start_container(container_id, None::<StartContainerOptions<String>>)
            .await
            .map_err(|e| {
                DockerError::ContainerStartFailed(format!("Failed to start container: {}", e))
            })?;

        Ok(())
    }

    /// Execute a command in a container and wait for completion
    pub async fn exec_in_container(
        &self,
        container_id: &str,
        command: Vec<String>,
    ) -> Result<(String, String, i32), DockerError> {
        use bollard::exec::CreateExecOptions;

        let exec_config = CreateExecOptions {
            cmd: Some(command),
            attach_stdout: Some(true),
            attach_stderr: Some(true),
            ..Default::default()
        };

        let exec_result = self
            .docker
            .create_exec(container_id, exec_config)
            .await
            .map_err(|e| DockerError::ExecutionFailed(format!("Failed to create exec: {}", e)))?;

        let exec_response = self
            .docker
            .start_exec(&exec_result.id, None)
            .await
            .map_err(|e| DockerError::ExecutionFailed(format!("Failed to start exec: {}", e)))?;

        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        match exec_response {
            StartExecResults::Attached {
                mut output,
                input: _,
            } => {
                while let Some(result) = output.next().await {
                    match result {
                        Ok(LogOutput::StdOut { message }) => {
                            stdout.extend_from_slice(&message);
                        }
                        Ok(LogOutput::StdErr { message }) => {
                            stderr.extend_from_slice(&message);
                        }
                        Ok(LogOutput::Console { message }) => {
                            stdout.extend_from_slice(&message);
                        }
                        Ok(LogOutput::StdIn { .. }) => {
                            // Ignore stdin
                        }
                        Err(e) => {
                            return Err(DockerError::ExecutionFailed(format!(
                                "Exec stream error: {}",
                                e
                            )));
                        }
                    }
                }
            }
            StartExecResults::Detached => {
                // Detached execution - wait a bit and then inspect
                time::sleep(time::Duration::from_millis(100)).await;
            }
        }

        // Get exit code
        let inspect_result = self
            .docker
            .inspect_exec(&exec_result.id)
            .await
            .map_err(|e| DockerError::ExecutionFailed(format!("Failed to inspect exec: {}", e)))?;

        let exit_code = inspect_result
            .exit_code
            .unwrap_or(-1)
            .try_into()
            .unwrap_or(-1);

        Ok((
            String::from_utf8_lossy(&stdout).to_string(),
            String::from_utf8_lossy(&stderr).to_string(),
            exit_code,
        ))
    }

    /// Run a command in a new container and return the output
    pub async fn run_command(
        &self,
        command: Vec<String>,
        env: Option<HashMap<String, String>>,
    ) -> Result<(String, String, i32), DockerError> {
        self.run_command_with_mounts(command, env, None).await
    }

    /// Run a command in a new container with volume mounts and return the output
    pub async fn run_command_with_mounts(
        &self,
        command: Vec<String>,
        env: Option<HashMap<String, String>>,
        mounts: Option<Vec<(String, String)>>,
    ) -> Result<(String, String, i32), DockerError> {
        let container_id = self
            .create_container_with_mounts(command.clone(), env, mounts)
            .await?;
        self.start_container(&container_id).await?;

        // Wait for container to finish
        // Try waiting, but if it fails, we'll poll the container state instead
        let wait_success = tokio::time::timeout(
            time::Duration::from_secs(300), // 5 minute timeout
            async {
                let mut wait_stream = self.docker.wait_container::<String>(&container_id, None);
                while let Some(result) = wait_stream.next().await {
                    match result {
                        Ok(_status) => {
                            // Container finished
                            return Ok::<bool, DockerError>(true);
                        }
                        Err(e) => {
                            // Wait failed - container might have already finished or there's an issue
                            // We'll check the state via inspect instead
                            eprintln!("Warning: Container wait stream error: {}, will check container state directly", e);
                            return Ok::<bool, DockerError>(false);
                        }
                    }
                }
                // Stream ended - container might have already finished
                Ok::<bool, DockerError>(false)
            }
        ).await;

        // If wait failed or timed out, poll the container state
        if wait_success.is_err() || matches!(wait_success, Ok(Ok(false))) {
            // Poll container state until it's stopped
            let mut attempts = 0;
            let max_attempts = 60; // 30 seconds max (0.5s * 60)
            loop {
                let inspect_result = self
                    .docker
                    .inspect_container(&container_id, None)
                    .await
                    .map_err(|e| {
                        DockerError::ExecutionFailed(format!("Failed to inspect container: {}", e))
                    })?;

                if let Some(state) = inspect_result.state {
                    if let Some(status) = &state.status {
                        match status {
                            bollard::models::ContainerStateStatusEnum::EXITED
                            | bollard::models::ContainerStateStatusEnum::DEAD => {
                                break; // Container has finished
                            }
                            _ => {
                                // Container still running, continue polling
                            }
                        }
                    }
                }

                attempts += 1;
                if attempts >= max_attempts {
                    return Err(DockerError::ExecutionFailed(
                        "Container did not finish within timeout".to_string(),
                    ));
                }

                time::sleep(time::Duration::from_millis(500)).await;
            }
        }

        // Get logs
        let logs_options = bollard::container::LogsOptions::<String> {
            stdout: true,
            stderr: true,
            ..Default::default()
        };

        let mut logs = self.docker.logs(&container_id, Some(logs_options));

        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        while let Some(chunk) = logs.next().await {
            match chunk {
                Ok(LogOutput::StdOut { message }) => {
                    stdout.extend_from_slice(&message);
                }
                Ok(LogOutput::StdErr { message }) => {
                    stderr.extend_from_slice(&message);
                }
                Ok(LogOutput::Console { message }) => {
                    stdout.extend_from_slice(&message);
                }
                Ok(LogOutput::StdIn { .. }) => {
                    // Ignore stdin
                }
                Err(e) => {
                    return Err(DockerError::ExecutionFailed(format!(
                        "Log stream error: {}",
                        e
                    )));
                }
            }
        }

        // Get exit code
        let inspect_result = self
            .docker
            .inspect_container(&container_id, None)
            .await
            .map_err(|e| {
                DockerError::ExecutionFailed(format!("Failed to inspect container: {}", e))
            })?;

        let exit_code = inspect_result
            .state
            .and_then(|s| s.exit_code)
            .unwrap_or(-1)
            .try_into()
            .unwrap_or(-1);

        // Clean up container
        let _ = self.remove_container(&container_id, true).await;

        Ok((
            String::from_utf8_lossy(&stdout).to_string(),
            String::from_utf8_lossy(&stderr).to_string(),
            exit_code,
        ))
    }

    /// Remove a container
    pub async fn remove_container(
        &self,
        container_id: &str,
        force: bool,
    ) -> Result<(), DockerError> {
        let options = bollard::container::RemoveContainerOptions {
            force,
            ..Default::default()
        };

        self.docker
            .remove_container(container_id, Some(options))
            .await
            .map_err(|e| {
                DockerError::ExecutionFailed(format!("Failed to remove container: {}", e))
            })?;

        Ok(())
    }

    /// Execute Python code in a container
    pub async fn execute_python_code(
        &self,
        code: &str,
    ) -> Result<(String, String, i32), DockerError> {
        self.execute_python_code_with_mounts(code, None).await
    }

    /// Execute Python code in a container with volume mounts
    pub async fn execute_python_code_with_mounts(
        &self,
        code: &str,
        mounts: Option<Vec<(String, String)>>,
    ) -> Result<(String, String, i32), DockerError> {
        // Create a temporary file in the container with the code
        // We'll use the exec-code command that the Dockerfile supports
        let command = vec![
            "exec-code".to_string(),
            "--code".to_string(),
            code.to_string(),
        ];

        self.run_command_with_mounts(command, None, mounts).await
    }

    /// Execute a Python script file in a container
    pub async fn execute_python_script(
        &self,
        script_path: &str,
    ) -> Result<(String, String, i32), DockerError> {
        // Mount the script as a volume or copy it in
        // For now, we'll assume the script is available in the container
        // In a production setup, you'd want to handle volume mounting properly
        let command = vec![
            "exec-code".to_string(),
            "--file".to_string(),
            script_path.to_string(),
        ];

        self.run_command(command, None).await
    }
}
