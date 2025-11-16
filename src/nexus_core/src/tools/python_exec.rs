use crate::models::tool::{Tool, ToolParameter};
use crate::models::Result;
use crate::tools::ExecutableTool;
use nexus_py::{DockerService, DockerConfig};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use log::{debug, info, error};
use std::sync::{Arc, Mutex};

/// Python execution tool that can execute Python code with access to MCP server files
/// Uses Docker service to run code in a container with the servers directory mounted
pub struct PythonExec {
    servers_dir: PathBuf,
    docker_service: Arc<Mutex<Option<Arc<DockerService>>>>,
}

impl PythonExec {
    pub fn new(servers_dir: impl Into<PathBuf>) -> Self {
        Self {
            servers_dir: servers_dir.into(),
            docker_service: Arc::new(Mutex::new(None)),
        }
    }
    
    /// Get or create the Docker service, using the current async runtime
    fn get_docker_service(&self) -> Result<Arc<DockerService>> {
        let mut service_guard = self.docker_service.lock()
            .map_err(|e| {
                crate::models::Error::Other(format!("Failed to lock Docker service mutex: {}", e))
            })?;
        
        if let Some(service) = service_guard.as_ref() {
            return Ok(service.clone());
        }
        
        // Try to use the current runtime handle
        let handle = tokio::runtime::Handle::try_current()
            .map_err(|_| {
                crate::models::Error::Other(
                    "Cannot create Docker service: not in an async runtime context".to_string()
                )
            })?;
        
        // Create Docker service using the current runtime
        let config = DockerConfig::default();
        let docker_service = handle.block_on(async {
            DockerService::new(config).await
                .map_err(|e| {
                    crate::models::Error::Other(format!(
                        "Failed to create Docker service: {}. Make sure Docker is running.",
                        e
                    ))
                })
        })?;
        
        let service = Arc::new(docker_service);
        *service_guard = Some(service.clone());
        Ok(service)
    }
}

impl ExecutableTool for PythonExec {
    fn name(&self) -> &str {
        "execute_python"
    }

    fn definition(&self) -> Tool {
        let mut parameters = HashMap::new();
        parameters.insert(
            "code".to_string(),
            ToolParameter::new("string", "Python code to execute. Can import from servers/ directory using relative imports like 'from servers.nexus_mcp_server.echo import echo'")
                .with_required(true),
        );

        Tool::new(
            "execute_python",
            "Execute Python code with access to MCP server tools. The code can import and use tools from the servers/ directory. Each tool file is self-contained and can be used independently.",
            parameters,
        )
    }

    fn execute(&self, args: Value) -> Result<String> {
        println!("[execute_python] Starting Python code execution");
        info!("[execute_python] Starting Python code execution");
        
        let code = args
            .get("code")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                println!("[execute_python] ERROR: Missing required parameter: code");
                error!("[execute_python] Missing required parameter: code");
                crate::models::Error::Configuration(
                    "Missing required parameter: code".to_string(),
                )
            })?;

        println!("[execute_python] Code length: {} characters", code.len());
        println!("[execute_python] Code preview: {}", code.chars().take(200).collect::<String>());
        debug!("[execute_python] Code length: {} characters", code.len());
        debug!("[execute_python] Code preview: {}", code.chars().take(200).collect::<String>());

        // Add servers directory to Python path and execute
        println!("[execute_python] Canonicalizing servers directory: {}", self.servers_dir.display());
        let servers_path = self.servers_dir.canonicalize()
            .map_err(|e| {
                println!("[execute_python] ERROR: Failed to canonicalize servers path: {}", e);
                error!("[execute_python] Failed to canonicalize servers path: {}", e);
                crate::models::Error::Other(format!("Failed to canonicalize servers path: {}", e))
            })?;
        
        println!("[execute_python] Servers directory: {}", servers_path.display());
        debug!("[execute_python] Servers directory: {}", servers_path.display());
        
        // Get parent directory (workspace root) to add to Python path
        let workspace_root = servers_path.parent()
            .ok_or_else(|| {
                println!("[execute_python] ERROR: Servers directory has no parent");
                error!("[execute_python] Servers directory has no parent");
                crate::models::Error::Other("Servers directory has no parent".to_string())
            })?;
        
        println!("[execute_python] Workspace root: {}", workspace_root.display());
        debug!("[execute_python] Workspace root: {}", workspace_root.display());

        // Mount the workspace root into the container at /workspace
        let workspace_root_str = workspace_root.to_string_lossy().to_string();
        let mounts = vec![
            (workspace_root_str.clone(), "/workspace".to_string())
        ];
        
        println!("[execute_python] Mounting workspace root: {} -> /workspace", workspace_root_str);
        
        // Update the code to use /workspace instead of the host path
        // Note: Using raw string with proper indentation - the indentation after \n\ is preserved
        let container_code = format!(
            "import sys
import importlib.util
from pathlib import Path

# Workspace root is mounted at /workspace
workspace_root = Path(\"/workspace\")
sys.path.insert(0, str(workspace_root))

# Helper to import from servers directory (handles hyphens in directory names)
def import_tool(server_name, tool_name):
    server_path = workspace_root / 'servers' / server_name / f'{{tool_name}}.py'
    spec = importlib.util.spec_from_file_location(tool_name, server_path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module

{}",
            code
        );

        println!("[execute_python] Executing Python code via Docker (code length: {} chars)", container_code.len());
        info!("[execute_python] Executing Python code via Docker");
        debug!("[execute_python] Code length: {} characters", container_code.len());
        
        // Debug: print first 500 chars of generated code to check indentation
        let preview = container_code.chars().take(500).collect::<String>();
        println!("[execute_python] Generated code preview (first 500 chars):\n{}", preview);

        // Get Docker service (lazy initialization)
        let docker_service = self.get_docker_service()
            .map_err(|e| {
                println!("[execute_python] ERROR: Failed to get Docker service: {}", e);
                error!("[execute_python] Failed to get Docker service: {}", e);
                e
            })?;
        
        // Execute the code in Docker
        println!("[execute_python] Calling docker_service.execute_python_code_with_mounts()...");
        
        // Use the current runtime handle to execute async code
        let handle = tokio::runtime::Handle::try_current()
            .map_err(|_| {
                crate::models::Error::Other(
                    "Cannot execute Docker command: not in an async runtime context".to_string()
                )
            })?;
        
        let (stdout, stderr, exit_code) = handle.block_on(async {
            docker_service.execute_python_code_with_mounts(&container_code, Some(mounts)).await
        })
        .map_err(|e| {
            println!("[execute_python] ERROR: Docker execution failed: {}", e);
            error!("[execute_python] Docker execution failed: {}", e);
            crate::models::Error::Other(format!("Docker execution failed: {}", e))
        })?;
        
        // Convert to ExecutionResult format
        use nexus_py::ExecutionResult;
        let result = ExecutionResult {
            stdout,
            stderr,
            exit_code,
        };

        println!("[execute_python] Python execution completed with exit code: {}", result.exit_code);
        println!("[execute_python] STDOUT length: {} characters", result.stdout.len());
        println!("[execute_python] STDERR length: {} characters", result.stderr.len());
        info!("[execute_python] Python execution completed with exit code: {}", result.exit_code);
        debug!("[execute_python] STDOUT length: {} characters", result.stdout.len());
        debug!("[execute_python] STDERR length: {} characters", result.stderr.len());

        if result.exit_code != 0 {
            println!("[execute_python] ERROR: Python code exited with non-zero code: {}", result.exit_code);
            println!("[execute_python] STDOUT: {}", result.stdout);
            println!("[execute_python] STDERR: {}", result.stderr);
            error!("[execute_python] Python code exited with non-zero code: {}", result.exit_code);
            error!("[execute_python] STDOUT: {}", result.stdout);
            error!("[execute_python] STDERR: {}", result.stderr);
            return Err(crate::models::Error::Other(format!(
                "Python code exited with code {}:\nSTDOUT:\n{}\nSTDERR:\n{}",
                result.exit_code, result.stdout, result.stderr
            )));
        }

        // Return stdout, or stderr if stdout is empty
        let output = if result.stdout.trim().is_empty() {
            println!("[execute_python] Using STDERR as output (STDOUT is empty)");
            debug!("[execute_python] Using STDERR as output (STDOUT is empty)");
            result.stderr
        } else {
            println!("[execute_python] Using STDOUT as output");
            debug!("[execute_python] Using STDOUT as output");
            result.stdout
        };

        println!("[execute_python] Execution successful, output length: {} characters", output.len());
        info!("[execute_python] Execution successful, output length: {} characters", output.len());
        Ok(format!("```<result>{}</result>```", output))
    }
}

