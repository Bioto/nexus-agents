use crate::config::{MultiServerConfig, ServerConfig};
use crate::error::NexusError;
use crate::server::NexusMcpServer;
use clap::Args;
use rmcp::transport::streamable_http_server::{
    session::local::LocalSessionManager, tower::StreamableHttpService, StreamableHttpServerConfig,
};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

fn load_dotenv() {
    dotenv::dotenv().ok();
}

#[derive(Args, Debug)]
#[command(about = "Start multiple MCP servers from a configuration file")]
pub struct StartServersArgs {
    /// Path to the configuration file (TOML format)
    #[arg(short, long, default_value = "src/nexus_mcp/mcp-servers.toml")]
    pub config: PathBuf,
}

/// Handle for a running server instance
struct ServerHandle {
    name: String,
    #[allow(dead_code)] // Reserved for future manual shutdown functionality
    shutdown_tx: oneshot::Sender<()>,
    join_handle: tokio::task::JoinHandle<Result<(), NexusError>>,
}

/// Start a single HTTP server instance
async fn start_http_server(
    config: ServerConfig,
    shutdown_token: CancellationToken,
) -> Result<ServerHandle, NexusError> {
    let bind_addr: SocketAddr = config.parse_bind_addr()?;
    let path = config.path.clone();
    let name = config.name.clone();
    let name_for_logging = name.clone();

    eprintln!(
        "[{}] Starting HTTP server on {} (path: {})",
        name, bind_addr, path
    );

    let service: StreamableHttpService<NexusMcpServer, LocalSessionManager> =
        StreamableHttpService::new(
            || Ok(NexusMcpServer::new()),
            Arc::new(LocalSessionManager::default()),
            StreamableHttpServerConfig {
                stateful_mode: true,
                sse_keep_alive: None,
            },
        );

    let router = axum::Router::new().nest_service(&path, service);
    let tcp_listener = tokio::net::TcpListener::bind(bind_addr)
        .await
        .map_err(|e| NexusError::Io(e))?;

    eprintln!(
        "[{}] HTTP server started. Endpoint: http://{}{}",
        name, bind_addr, path
    );

    let (shutdown_tx, shutdown_rx) = oneshot::channel();

    let join_handle = tokio::spawn(async move {
        let shutdown_future = async move {
            shutdown_token.cancelled().await;
            shutdown_rx.await.ok();
        };

        axum::serve(tcp_listener, router)
            .with_graceful_shutdown(shutdown_future)
            .await
            .map_err(|e| NexusError::Io(e))?;

        eprintln!("[{}] Server shutdown complete", name_for_logging);
        Ok(())
    });

    Ok(ServerHandle {
        name,
        shutdown_tx,
        join_handle,
    })
}

/// Start a single stdio server instance
async fn start_stdio_server(
    config: ServerConfig,
    _shutdown_token: CancellationToken,
) -> Result<ServerHandle, NexusError> {
    Err(NexusError::Config(format!(
        "stdio transport is not supported in multi-server mode. \
            Server '{}' cannot use stdio transport as it requires exclusive access to stdin/stdout. \
            Please use HTTP transport for multi-server setups or run stdio servers separately.",
        config.name
    )))
}

/// Start all servers from configuration
pub async fn run_start_servers(args: StartServersArgs) -> Result<(), NexusError> {
    load_dotenv();
    eprintln!("Loading configuration from: {}", args.config.display());
    let config = MultiServerConfig::from_file(&args.config)?;

    eprintln!("Found {} server(s) in configuration", config.servers.len());

    // Create a cancellation token for graceful shutdown
    let shutdown_token = CancellationToken::new();

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
            "http" => start_http_server(server_config, shutdown_token.clone()).await?,
            "stdio" => start_stdio_server(server_config, shutdown_token.clone()).await?,
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
    let mut results = Vec::new();
    for handle in handles {
        let name = handle.name.clone();
        match handle.join_handle.await {
            Ok(Ok(())) => {
                eprintln!("[{}] Server exited successfully", name);
            }
            Ok(Err(e)) => {
                eprintln!("[{}] Server error: {}", name, e);
                results.push(Err(e));
            }
            Err(e) => {
                eprintln!("[{}] Server task error: {}", name, e);
                results.push(Err(NexusError::Server(format!("Task join error: {}", e))));
            }
        }
    }

    // Return error if any server failed
    if let Some(err) = results.into_iter().find(|r| r.is_err()) {
        err
    } else {
        eprintln!("All servers shutdown successfully");
        Ok(())
    }
}
