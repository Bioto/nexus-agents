//! Server manager for starting and managing multiple MCP servers.

use crate::config::ServerConfig;
use crate::error::NexusError;
use crate::server::NexusMcpServer;
use rmcp::transport::streamable_http_server::{
    session::local::LocalSessionManager, tower::StreamableHttpService, StreamableHttpServerConfig,
};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;
use x_toolbox::nutrition::mcp_server::NutritionMcpServer;

/// Handle for a running server instance.
pub struct ServerHandle {
    /// Name of the server.
    pub name: String,
    /// Sender to trigger shutdown.
    #[allow(dead_code)] // Reserved for future manual shutdown functionality
    pub shutdown_tx: oneshot::Sender<()>,
    /// Join handle for the server task.
    pub join_handle: tokio::task::JoinHandle<Result<(), NexusError>>,
}

/// Manager for starting and managing MCP servers.
pub struct ServerManager {
    shutdown_token: CancellationToken,
}

impl ServerManager {
    /// Create a new server manager.
    pub fn new() -> Self {
        Self {
            shutdown_token: CancellationToken::new(),
        }
    }

    /// Get a clone of the shutdown token for graceful shutdown handling.
    pub fn shutdown_token(&self) -> CancellationToken {
        self.shutdown_token.clone()
    }

    /// Start an HTTP server instance.
    pub async fn start_http_server(&self, config: ServerConfig) -> Result<ServerHandle, NexusError> {
        let bind_addr: SocketAddr = config.parse_bind_addr()?;
        let path = config.path.clone();
        let name = config.name.clone();
        let server_type = config.server_type.clone();
        let name_for_logging = name.clone();
        let shutdown_token = self.shutdown_token.clone();

        eprintln!(
            "[{}] Starting {} HTTP server on {} (path: {})",
            name, server_type, bind_addr, path
        );

        let router = match server_type.as_str() {
            "nexus" => self.create_nexus_router(&path),
            "nutrition" => self.create_nutrition_router(&path, &name).await?,
            _ => {
                return Err(NexusError::Config(format!(
                    "Unknown server type '{}' for server '{}'. Supported types: nexus, nutrition",
                    server_type, name
                )));
            }
        };

        let tcp_listener = tokio::net::TcpListener::bind(bind_addr).await?;

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
                .await?;

            eprintln!("[{}] Server shutdown complete", name_for_logging);
            Ok(())
        });

        Ok(ServerHandle {
            name,
            shutdown_tx,
            join_handle,
        })
    }

    /// Create a router for the Nexus MCP server.
    fn create_nexus_router(&self, path: &str) -> axum::Router {
        let service: StreamableHttpService<NexusMcpServer, LocalSessionManager> =
            StreamableHttpService::new(
                || Ok(NexusMcpServer::new()),
                Arc::new(LocalSessionManager::default()),
                StreamableHttpServerConfig {
                    stateful_mode: true,
                    sse_keep_alive: None,
                },
            );
        axum::Router::new().nest_service(path, service)
    }

    /// Create a router for the Nutrition MCP server.
    async fn create_nutrition_router(
        &self,
        path: &str,
        name: &str,
    ) -> Result<axum::Router, NexusError> {
        let db = x_toolbox::nutrition::Database::new().await.map_err(|e| {
            NexusError::Config(format!("Failed to initialize nutrition database: {}", e))
        })?;
        let nutrition_server = NutritionMcpServer::with_database(db).await.map_err(|e| {
            NexusError::Config(format!(
                "Failed to create nutrition server for '{}': {}",
                name, e
            ))
        })?;

        let service: StreamableHttpService<NutritionMcpServer, LocalSessionManager> =
            StreamableHttpService::new(
                move || Ok(nutrition_server.clone()),
                Arc::new(LocalSessionManager::default()),
                StreamableHttpServerConfig {
                    stateful_mode: true,
                    sse_keep_alive: None,
                },
            );
        Ok(axum::Router::new().nest_service(path, service))
    }

    /// Start a stdio server (not supported in multi-server mode).
    pub async fn start_stdio_server(&self, config: ServerConfig) -> Result<ServerHandle, NexusError> {
        Err(NexusError::Config(format!(
            "stdio transport is not supported in multi-server mode. \
            Server '{}' cannot use stdio transport as it requires exclusive access to stdin/stdout. \
            Please use HTTP transport for multi-server setups or run stdio servers separately.",
            config.name
        )))
    }

    /// Wait for all server handles to complete and return results.
    pub async fn wait_for_servers(
        &self,
        handles: Vec<ServerHandle>,
    ) -> Result<(), NexusError> {
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
}

impl Default for ServerManager {
    fn default() -> Self {
        Self::new()
    }
}

