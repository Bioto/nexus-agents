pub mod docker;

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use tempfile::NamedTempFile;
use thiserror::Error;
use tracing::error;

#[derive(Debug, Clone)]
pub struct ExecutionResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

#[derive(Error, Debug)]
pub enum PythonExecutionError {
    #[error("uv is not installed or not in PATH. Please install uv first.")]
    UvNotFound,

    #[error("Script not found: {0}")]
    ScriptNotFound(String),

    #[error("Execution failed: {0}")]
    ExecutionFailed(String),

    #[error("IO error: {0}")]
    IoError(String),
}

pub struct PythonExecutionService;

impl PythonExecutionService {
    pub fn new() -> Self {
        Self
    }

    /// Execute a Python script using uv run
    /// The script can include inline dependencies like: # uv: dependencies = ["package1", "package2"]
    pub fn execute_script(
        &self,
        script_path: &Path,
    ) -> Result<ExecutionResult, PythonExecutionError> {
        // Check if script exists
        if !script_path.exists() {
            return Err(PythonExecutionError::ScriptNotFound(
                script_path.to_string_lossy().to_string(),
            ));
        }

        // Check if uv is available
        if Command::new("uv").arg("--version").output().is_err() {
            return Err(PythonExecutionError::UvNotFound);
        }

        // Execute uv run
        let mut cmd = Command::new("uv");
        cmd.arg("run");
        cmd.arg(script_path);
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let mut child = cmd.spawn().map_err(|e| {
            PythonExecutionError::ExecutionFailed(format!("Failed to spawn uv process: {}", e))
        })?;

        // Capture stdout and stderr
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| PythonExecutionError::IoError("Failed to capture stdout".to_string()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| PythonExecutionError::IoError("Failed to capture stderr".to_string()))?;

        // Read stdout
        let stdout_reader = BufReader::new(stdout);
        let mut stdout_lines = Vec::new();
        for line in stdout_reader.lines() {
            let line = line.map_err(|e| {
                PythonExecutionError::IoError(format!("Failed to read stdout: {}", e))
            })?;
            stdout_lines.push(line);
        }

        // Read stderr
        let stderr_reader = BufReader::new(stderr);
        let mut stderr_lines = Vec::new();
        for line in stderr_reader.lines() {
            let line = line.map_err(|e| {
                PythonExecutionError::IoError(format!("Failed to read stderr: {}", e))
            })?;
            stderr_lines.push(line);
        }

        // Wait for process to complete
        let status = child.wait().map_err(|e| {
            PythonExecutionError::ExecutionFailed(format!("Failed to wait for process: {}", e))
        })?;

        let exit_code = status.code().unwrap_or(-1);

        Ok(ExecutionResult {
            stdout: stdout_lines.join("\n"),
            stderr: stderr_lines.join("\n"),
            exit_code,
        })
    }

    /// Execute Python code from a string using uv run
    /// The code can include inline dependencies like: # uv: dependencies = ["package1", "package2"]
    /// A temporary file is created, executed, and automatically cleaned up
    pub fn execute_code(&self, code: &str) -> Result<ExecutionResult, PythonExecutionError> {
        // Check if uv is available
        if Command::new("uv").arg("--version").output().is_err() {
            return Err(PythonExecutionError::UvNotFound);
        }

        // Create a temporary file with .py extension so uv recognizes it as Python
        let mut temp_file = NamedTempFile::with_suffix(".py").map_err(|e| {
            PythonExecutionError::IoError(format!("Failed to create temporary file: {}", e))
        })?;

        temp_file.write_all(code.as_bytes()).map_err(|e| {
            PythonExecutionError::IoError(format!("Failed to write code to temporary file: {}", e))
        })?;

        temp_file.flush().map_err(|e| {
            PythonExecutionError::IoError(format!("Failed to flush temporary file: {}", e))
        })?;

        // Convert to TempPath to keep file alive during execution
        let temp_path = temp_file.into_temp_path();
        let script_path = temp_path.as_ref();

        // Execute using the existing execute_script method
        // temp_path is kept alive during execution
        // Note: uv run doesn't require execute permissions - it reads the file
        let result = self.execute_script(script_path)?;

        // File is automatically deleted when temp_path goes out of scope
        Ok(result)
    }

    /// Execute Python code from a string with streaming output
    /// Calls the callback for each line of output as it's produced
    /// A temporary file is created, executed, and automatically cleaned up
    pub fn execute_code_streaming(
        &self,
        code: &str,
        on_output: impl FnMut(&str) -> Result<(), Box<dyn std::error::Error>>,
    ) -> Result<ExecutionResult, PythonExecutionError> {
        // Check if uv is available
        if Command::new("uv").arg("--version").output().is_err() {
            return Err(PythonExecutionError::UvNotFound);
        }

        // Create a temporary file with .py extension so uv recognizes it as Python
        let mut temp_file = NamedTempFile::with_suffix(".py").map_err(|e| {
            PythonExecutionError::IoError(format!("Failed to create temporary file: {}", e))
        })?;

        temp_file.write_all(code.as_bytes()).map_err(|e| {
            PythonExecutionError::IoError(format!("Failed to write code to temporary file: {}", e))
        })?;

        temp_file.flush().map_err(|e| {
            PythonExecutionError::IoError(format!("Failed to flush temporary file: {}", e))
        })?;

        // Convert to TempPath to keep file alive during execution
        let temp_path = temp_file.into_temp_path();
        let script_path = temp_path.as_ref();

        // Set execute permissions on the temporary file
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(script_path)
                .map_err(|e| {
                    PythonExecutionError::IoError(format!("Failed to get file metadata: {}", e))
                })?
                .permissions();
            perms.set_mode(0o755);
            fs::set_permissions(script_path, perms).map_err(|e| {
                PythonExecutionError::IoError(format!("Failed to set file permissions: {}", e))
            })?;
        }

        // Execute using the existing execute_script_streaming method
        // temp_path is kept alive during execution
        let result = self.execute_script_streaming(script_path, on_output)?;

        // File is automatically deleted when temp_path goes out of scope
        Ok(result)
    }

    /// Execute a Python script with streaming output
    /// Calls the callback for each line of output as it's produced
    pub fn execute_script_streaming(
        &self,
        script_path: &Path,
        on_output: impl FnMut(&str) -> Result<(), Box<dyn std::error::Error>>,
    ) -> Result<ExecutionResult, PythonExecutionError> {
        // Check if script exists
        if !script_path.exists() {
            return Err(PythonExecutionError::ScriptNotFound(
                script_path.to_string_lossy().to_string(),
            ));
        }

        // Check if uv is available
        if Command::new("uv").arg("--version").output().is_err() {
            return Err(PythonExecutionError::UvNotFound);
        }

        // Execute uv run
        let mut cmd = Command::new("uv");
        cmd.arg("run");
        cmd.arg(script_path);
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let mut child = cmd.spawn().map_err(|e| {
            PythonExecutionError::ExecutionFailed(format!("Failed to spawn uv process: {}", e))
        })?;

        // Capture stdout and stderr
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| PythonExecutionError::IoError("Failed to capture stdout".to_string()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| PythonExecutionError::IoError("Failed to capture stderr".to_string()))?;

        // Read and stream stdout
        let stdout_reader = BufReader::new(stdout);
        let mut stdout_lines = Vec::new();
        let mut on_output = on_output;
        for line in stdout_reader.lines() {
            let line = line.map_err(|e| {
                PythonExecutionError::IoError(format!("Failed to read stdout: {}", e))
            })?;
            on_output(&line)
                .map_err(|e| PythonExecutionError::IoError(format!("Callback error: {}", e)))?;
            stdout_lines.push(line);
        }

        // Read and stream stderr
        let stderr_reader = BufReader::new(stderr);
        let mut stderr_lines = Vec::new();
        for line in stderr_reader.lines() {
            let line = line.map_err(|e| {
                PythonExecutionError::IoError(format!("Failed to read stderr: {}", e))
            })?;
            // Stream stderr to stderr output
            error!("{}", line);
            stderr_lines.push(line);
        }

        // Wait for process to complete
        let status = child.wait().map_err(|e| {
            PythonExecutionError::ExecutionFailed(format!("Failed to wait for process: {}", e))
        })?;

        let exit_code = status.code().unwrap_or(-1);

        Ok(ExecutionResult {
            stdout: stdout_lines.join("\n"),
            stderr: stderr_lines.join("\n"),
            exit_code,
        })
    }
}
