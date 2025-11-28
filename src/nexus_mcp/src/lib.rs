//! nexus_mcp - MCP (Model Context Protocol) library for Nexus agents.
//!
//! This crate provides:
//! - MCP server implementation with tools, prompts, and resources
//! - MCP client for fetching tool definitions
//! - Code generation for Python MCP client wrappers
//! - Multi-server configuration and management

pub mod cli;
pub mod client;
pub mod codegen;
pub mod config;
pub mod error;
pub mod server;
pub mod services;
pub mod types;
pub mod utils;

// Re-export commonly used items for convenience
pub use cli::{run_generate_code, run_generate_external, run_shell, run_start_servers, Cli, Commands};
pub use codegen::CodeGenerator;
pub use config::{MultiServerConfig, ServerConfig};
pub use server::NexusMcpServer;
pub use services::ServerManager;

/// Generate Python tool files for all external MCP servers from configuration.
pub async fn generate_external_server_tools(
    config_path: impl AsRef<std::path::Path>,
    output_dir: impl AsRef<std::path::Path>,
) -> Result<(), error::NexusError> {
    let config = MultiServerConfig::from_file(config_path)?;
    let output_path = output_dir.as_ref();

    for server in config.servers {
        if server.is_external() {
            let server_url = server.url.as_ref().unwrap();
            let server_name = server.name.clone();
            let headers = server.headers.clone();

            eprintln!(
                "Generating tools for external server: {} ({})",
                server_name, server_url
            );

            let generator = CodeGenerator::with_config(server_url, &server_name, headers);
            generator
                .generate_code_files(output_path)
                .await
                .map_err(|e| {
                    error::NexusError::Server(format!(
                        "Failed to generate code for {}: {}",
                        server_name, e
                    ))
                })?;

            eprintln!("Successfully generated tools for {}", server_name);
        }
    }

    Ok(())
}
