use bollard::container::{Config, CreateContainerOptions, LogOutput, StartContainerOptions};
use bollard::exec::StartExecResults;
use bollard::Docker;
use futures_util::stream::StreamExt;
use std::collections::HashMap;
use std::env;
use std::path::Path;
use tokio::time;
use tracing::{debug, info, warn};

use crate::service::docker::config::DockerConfig;
use crate::service::docker::error::DockerError;

// Constants for timeouts and limits
const CONTAINER_TIMEOUT_SECS: u64 = 300; // 5 minutes
const MAX_POLL_ATTEMPTS: usize = 60;
const POLL_INTERVAL_MS: u64 = 500;
const DOCKER_CONNECT_TIMEOUT_SECS: u64 = 120;

/// Service for managing Docker containers and executing code within them.
pub struct DockerService {
    docker: Docker,
    config: DockerConfig,
}

impl DockerService {
    /// Create a new Docker service instance
    /// Connects to the system Docker daemon via the default socket
    /// Supports standard Docker, Docker Desktop, and Colima
    #[must_use]
    pub async fn new(config: DockerConfig) -> Result<Self, DockerError> {
        // Validate configuration
        config.validate()?;

        // Try to connect to Docker daemon
        // First check DOCKER_HOST, then try Colima sockets, then default
        let docker = if let Ok(docker_host) = env::var("DOCKER_HOST") {
            // Use DOCKER_HOST if set
            debug!("Connecting to Docker via DOCKER_HOST: {}", docker_host);
            Docker::connect_with_socket(&docker_host, DOCKER_CONNECT_TIMEOUT_SECS, bollard::API_DEFAULT_VERSION).map_err(
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
                debug!("Connecting to Docker via Colima socket (new): {}", colima_socket_new);
                Docker::connect_with_socket(
                    &format!("unix://{}", colima_socket_new),
                    DOCKER_CONNECT_TIMEOUT_SECS,
                    bollard::API_DEFAULT_VERSION,
                )
                .map_err(|e| {
                    DockerError::ConnectionFailed(format!(
                        "Failed to connect to Colima Docker daemon: {}",
                        e
                    ))
                })?
            } else if Path::new(&colima_socket_old).exists() {
                debug!("Connecting to Docker via Colima socket (old): {}", colima_socket_old);
                Docker::connect_with_socket(
                    &format!("unix://{}", colima_socket_old),
                    DOCKER_CONNECT_TIMEOUT_SECS,
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
                debug!("Connecting to Docker via local defaults");
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
            debug!("Connecting to Docker via local defaults (no HOME)");
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
    #[must_use]
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

                    debug!("Looking for image: '{}'", image_name);
                    debug!("Checking {} images...", images.len());

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
                                    debug!("Found matching image: '{}'", tag);
                                }
                                matches
                            })
                        } else {
                            false
                        }
                    });

                    if !exists {
                        warn!("Image '{}' not found in {} listed images", image_name, images.len());
                        
                        // Last resort: try to inspect by ID or try creating a container
                        // Sometimes images exist but aren't in list_images
                        debug!("Attempting direct container creation test...");
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
                                debug!("Image exists (verified by test container creation)");
                                return Ok(true);
                            }
                            Err(e) => {
                                let err_msg = e.to_string().to_lowercase();
                                if err_msg.contains("no such image")
                                    || err_msg.contains("not found")
                                {
                                    debug!("Image confirmed not found via container creation test");
                                } else {
                                    // Other error might mean image exists but can't create container
                                    warn!("Container creation test error (might indicate image exists): {}", e);
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

    /// Check if a container is running
    pub async fn is_container_running(&self, container_id: &str) -> Result<bool, DockerError> {
        let inspect_result = self
            .docker
            .inspect_container(container_id, None)
            .await
            .map_err(|e| {
                DockerError::ExecutionFailed(format!("Failed to inspect container: {}", e))
            })?;

        if let Some(state) = inspect_result.state {
            if let Some(status) = &state.status {
                return Ok(matches!(
                    status,
                    bollard::models::ContainerStateStatusEnum::RUNNING
                ));
            }
        }

        Ok(false)
    }

    /// Ensure a container is running, restarting it if necessary
    pub async fn ensure_container_running(&self, container_id: &str) -> Result<(), DockerError> {
        let is_running = self.is_container_running(container_id).await?;

        if !is_running {
            // Container is not running - try to start it
            // This works for both stopped and exited containers
            debug!(
                "Container {} is not running, attempting to start it...",
                container_id
            );
            match self.start_container(container_id).await {
                Ok(_) => {
                    // Give it a moment to start
                    time::sleep(time::Duration::from_millis(POLL_INTERVAL_MS)).await;
                    // Verify it's actually running now
                    let is_running_after = self.is_container_running(container_id).await?;
                    if !is_running_after {
                        return Err(DockerError::ContainerStartFailed(format!(
                            "Container {} started but is not running",
                            container_id
                        )));
                    }
                    debug!("Container {} successfully started", container_id);
                }
                Err(e) => {
                    // Check if container exists at all
                    let inspect_result = self.docker.inspect_container(container_id, None).await;
                    match inspect_result {
                        Ok(_) => {
                            // Container exists but failed to start
                            return Err(DockerError::ContainerStartFailed(format!(
                                "Failed to restart container {}: {}",
                                container_id, e
                            )));
                        }
                        Err(_) => {
                            // Container doesn't exist
                            return Err(DockerError::ContainerStartFailed(format!(
                                "Container {} does not exist. It may have been removed.",
                                container_id
                            )));
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Execute a command in a container and wait for completion
    pub async fn exec_in_container(
        &self,
        container_id: &str,
        command: Vec<String>,
    ) -> Result<(String, String, i32), DockerError> {
        self.exec_in_container_with_env(container_id, command, None).await
    }

    pub async fn exec_in_container_with_env(
        &self,
        container_id: &str,
        command: Vec<String>,
        env: Option<Vec<String>>,
    ) -> Result<(String, String, i32), DockerError> {
        use bollard::exec::CreateExecOptions;

        let exec_config = CreateExecOptions {
            cmd: Some(command),
            attach_stdout: Some(true),
            attach_stderr: Some(true),
            env: env,
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
        self.run_command_with_mounts_and_cleanup(command, env, mounts, true)
            .await
    }

    /// Run a command in a new container with volume mounts and return the output
    /// If `remove_after` is false, the container will be kept running and the container ID
    /// will be included in the error message (as a workaround since we can't change the return type)
    pub async fn run_command_with_mounts_and_cleanup(
        &self,
        command: Vec<String>,
        env: Option<HashMap<String, String>>,
        mounts: Option<Vec<(String, String)>>,
        remove_after: bool,
    ) -> Result<(String, String, i32), DockerError> {
        let container_id = self
            .create_container_with_mounts(command.clone(), env, mounts)
            .await?;
        self.start_container(&container_id).await?;

        // Wait for container to finish
        // Try waiting, but if it fails, we'll poll the container state instead
        let wait_success = tokio::time::timeout(
            time::Duration::from_secs(CONTAINER_TIMEOUT_SECS),
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
                            warn!("Container wait stream error: {}, will check container state directly", e);
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
                if attempts >= MAX_POLL_ATTEMPTS {
                    return Err(DockerError::ExecutionFailed(
                        "Container did not finish within timeout".to_string(),
                    ));
                }

                time::sleep(time::Duration::from_millis(POLL_INTERVAL_MS)).await;
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

        // Clean up container if requested
        if remove_after {
            let _ = self.remove_container(&container_id, true).await;
        } else {
            info!("Container {} kept alive (not removed)", container_id);
        }

        Ok((
            String::from_utf8_lossy(&stdout).to_string(),
            String::from_utf8_lossy(&stderr).to_string(),
            exit_code,
        ))
    }

    /// Run a command in a new container with volume mounts and return the output and container ID
    /// The container is NOT removed, allowing it to be reused
    pub async fn run_command_with_mounts_keep_alive(
        &self,
        command: Vec<String>,
        env: Option<HashMap<String, String>>,
        mounts: Option<Vec<(String, String)>>,
    ) -> Result<(String, String, i32, String), DockerError> {
        let container_id = self
            .create_container_with_mounts(command.clone(), env, mounts)
            .await?;
        self.start_container(&container_id).await?;

        // Wait for container to finish
        let wait_success = tokio::time::timeout(
            time::Duration::from_secs(CONTAINER_TIMEOUT_SECS),
            async {
                let mut wait_stream = self.docker.wait_container::<String>(&container_id, None);
                while let Some(result) = wait_stream.next().await {
                    match result {
                        Ok(_status) => {
                            return Ok::<bool, DockerError>(true);
                        }
                        Err(e) => {
                            warn!("Container wait stream error: {}, will check container state directly", e);
                            return Ok::<bool, DockerError>(false);
                        }
                    }
                }
                Ok::<bool, DockerError>(false)
            }
        ).await;

        // If wait failed or timed out, poll the container state
        if wait_success.is_err() || matches!(wait_success, Ok(Ok(false))) {
            let mut attempts = 0;
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
                                break;
                            }
                            _ => {}
                        }
                    }
                }

                attempts += 1;
                if attempts >= MAX_POLL_ATTEMPTS {
                    return Err(DockerError::ExecutionFailed(
                        "Container did not finish within timeout".to_string(),
                    ));
                }

                time::sleep(time::Duration::from_millis(POLL_INTERVAL_MS)).await;
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
                Ok(LogOutput::StdIn { .. }) => {}
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

        // Container is NOT removed - caller is responsible for cleanup
        Ok((
            String::from_utf8_lossy(&stdout).to_string(),
            String::from_utf8_lossy(&stderr).to_string(),
            exit_code,
            container_id,
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
        self.execute_python_code_with_mounts_and_cleanup(code, mounts, true)
            .await
    }

    /// Execute Python code in a container with volume mounts
    /// If `remove_after` is false, the container will be kept running
    pub async fn execute_python_code_with_mounts_and_cleanup(
        &self,
        code: &str,
        mounts: Option<Vec<(String, String)>>,
        remove_after: bool,
    ) -> Result<(String, String, i32), DockerError> {
        // Create a temporary file in the container with the code
        // We'll use the exec-code command that the Dockerfile supports
        let command = vec![
            "exec-code".to_string(),
            "--code".to_string(),
            code.to_string(),
        ];

        self.run_command_with_mounts_and_cleanup(command, None, mounts, remove_after)
            .await
    }

    /// Execute Python code in an existing container using exec
    /// This allows reusing a running container for multiple executions
    /// Automatically ensures the container is running before executing
    pub async fn execute_python_code_in_container(
        &self,
        container_id: &str,
        code: &str,
    ) -> Result<(String, String, i32), DockerError> {
        self.execute_python_code_in_container_with_env(container_id, code, None).await
    }

    /// Execute Python code in an existing container using exec with environment variables
    pub async fn execute_python_code_in_container_with_env(
        &self,
        container_id: &str,
        code: &str,
        env: Option<Vec<String>>,
    ) -> Result<(String, String, i32), DockerError> {
        // Ensure the container is running before trying to exec
        self.ensure_container_running(container_id).await?;

        // Use nexus_sandbox exec-code since the persistent container has entrypoint overridden to /bin/sh
        let command = vec![
            "nexus_sandbox".to_string(),
            "exec-code".to_string(),
            "--code".to_string(),
            code.to_string(),
        ];

        self.exec_in_container_with_env(container_id, command, env).await
    }

    /// Create a long-running container that can be reused for multiple Python executions
    /// The container runs a sleep command to keep it alive
    /// Overrides the Dockerfile entrypoint to use /bin/sh since the image has ENTRYPOINT ["nexus_sandbox"]
    pub async fn create_persistent_container(
        &self,
        mounts: Option<Vec<(String, String)>>,
    ) -> Result<String, DockerError> {
        use bollard::models::{HostConfig, Mount, MountTypeEnum};

        let image_name = self.image_full_name();
        let container_name = format!("{}_{}", self.config.image_name, uuid::Uuid::new_v4());

        // Build mounts if provided
        let mut host_config = if let Some(mounts) = mounts {
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
            Some(HostConfig::default())
        };

        // Add host.docker.internal to extra_hosts for Linux compatibility
        // This allows containers to reach the host machine
        // Format: "hostname:ip" where "host-gateway" is a special Docker keyword
        if let Some(ref mut config) = host_config {
            config.extra_hosts = Some(vec!["host.docker.internal:host-gateway".to_string()]);
        }

        // Override entrypoint to /bin/sh and use sleep infinity to keep container running
        // The Dockerfile has ENTRYPOINT ["nexus_sandbox"], so we need to override it
        let container_config = Config {
            image: Some(image_name),
            entrypoint: Some(vec!["/bin/sh".to_string()]), // Override the nexus_sandbox entrypoint
            cmd: Some(vec!["-c".to_string(), "sleep infinity".to_string()]), // Run sleep infinity via sh
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

        let container_id = result.id;
        self.start_container(&container_id).await?;
        Ok(container_id)
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
