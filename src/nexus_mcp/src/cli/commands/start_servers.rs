//! Start multiple MCP servers from configuration.

use crate::config::MultiServerConfig;
use crate::error::NexusError;
use crate::services::ServerManager;
use clap::Args;
use std::path::PathBuf;

fn load_dotenv() {
    dotenv::dotenv().ok();
}

/// Arguments for the start-servers command.
#[derive(Args, Debug)]
#[command(about = "Start multiple MCP servers from a configuration file")]
pub struct StartServersArgs {
    /// Path to the configuration file (TOML format)
    #[arg(short, long, default_value = "src/nexus_mcp/mcp-servers.toml")]
    pub config: PathBuf,
}

/// Run the start-servers command.
pub async fn run_start_servers(args: StartServersArgs) -> Result<(), NexusError> {
    load_dotenv();
    eprintln!("Loading configuration from: {}", args.config.display());
    let config = MultiServerConfig::from_file(&args.config)?;

    eprintln!("Found {} server(s) in configuration", config.servers.len());

    let manager = ServerManager::new();
    let shutdown_token = manager.shutdown_token();

    // Spawn all servers
    let mut handles = Vec::new();
    for server_config in config.servers {
        // Skip external servers - they are configured but not started locally
        if server_config.is_external() {
            eprintln!(
                "[{}] External server configured: {} (not started - connect separately)",
                server_config.name,
                server_config.url.as_ref().unwrap()
            );
            if let Some(headers) = &server_config.headers {
                eprintln!("[{}] Custom headers: {:?}", server_config.name, headers);
            }
            continue;
        }

        let handle = match server_config.transport.as_str() {
            "http" => manager.start_http_server(server_config).await?,
            "stdio" => manager.start_stdio_server(server_config).await?,
            _ => {
                return Err(NexusError::Config(format!(
                    "Unsupported transport type: {}",
                    server_config.transport
                )));
            }
        };
        handles.push(handle);
    }

    eprintln!("All servers started. Press Ctrl+C to shutdown...");

    // Handle graceful shutdown
    let shutdown_token_clone = shutdown_token.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        eprintln!("\nReceived shutdown signal, shutting down all servers gracefully...");
        shutdown_token_clone.cancel();
    });

    // Wait for all servers to complete
    manager.wait_for_servers(handles).await
}
