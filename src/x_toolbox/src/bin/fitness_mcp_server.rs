//! Fitness MCP Server binary.
//!
//! Provides a standalone MCP server for the fitness/personal trainer module,
//! supporting both stdio and streamable HTTP transports.

use clap::Parser;
use rmcp::serve_server;
use rmcp::transport::stdio;
use rmcp::transport::streamable_http_server::{
    session::local::LocalSessionManager, tower::StreamableHttpService, StreamableHttpServerConfig,
};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use x_toolbox::fitness::FitnessMcpServer;
use x_toolbox::nutrition::Database;

#[derive(Parser, Debug)]
#[command(name = "fitness-mcp-server")]
#[command(
    about = "Fitness/Personal Trainer MCP Server - supports both stdio and streamable HTTP transports"
)]
struct Args {
    /// Transport type to use: "stdio" or "http"
    #[arg(short, long, default_value = "stdio")]
    transport: String,

    /// Bind address for HTTP transport (e.g., "127.0.0.1:8003")
    /// Only used when transport is "http"
    #[arg(short, long, default_value = "0.0.0.0:8003")]
    bind: String,

    /// HTTP path endpoint (default: "/mcp")
    /// Only used when transport is "http"
    #[arg(long, default_value = "/mcp")]
    path: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    let _ = dotenvy::dotenv();

    // Use stderr for logging/debugging - stdout is reserved for MCP protocol when using stdio
    eprintln!("Starting Fitness MCP Server...");
    eprintln!("Transport: {}", args.transport);

    // Log database config (for debugging)
    if let Ok(host) = dotenvy::var("POSTGRES_HOST") {
        eprintln!("POSTGRES_HOST: {}", host);
    }

    match args.transport.as_str() {
        "stdio" => {
            eprintln!("Using stdio transport");
            let server = FitnessMcpServer::new().await?;
            serve_server(server, stdio()).await?;
        }
        "http" => {
            eprintln!("Using streamable HTTP transport");
            let bind_addr: SocketAddr = args
                .bind
                .parse()
                .map_err(|e| format!("Invalid bind address '{}': {}", args.bind, e))?;

            eprintln!("Binding HTTP server to {} (path: {})", bind_addr, args.path);

            // Create database connection and server
            let db = Database::new().await?;
            let server = FitnessMcpServer::with_database(db).await?;

            let service: StreamableHttpService<FitnessMcpServer, LocalSessionManager> =
                StreamableHttpService::new(
                    move || Ok(server.clone()),
                    Arc::new(LocalSessionManager::default()),
                    StreamableHttpServerConfig {
                        stateful_mode: true,
                        sse_keep_alive: None,
                    },
                );

            let router = axum::Router::new().nest_service(&args.path, service);
            let tcp_listener = tokio::net::TcpListener::bind(bind_addr).await?;
            let ct = CancellationToken::new();

            eprintln!("HTTP server started. Waiting for connections...");
            eprintln!("Endpoint: http://{}{}", bind_addr, args.path);

            // Handle graceful shutdown
            let ct_clone = ct.clone();
            tokio::spawn(async move {
                tokio::signal::ctrl_c().await.ok();
                eprintln!("Received shutdown signal, shutting down gracefully...");
                ct_clone.cancel();
            });

            axum::serve(tcp_listener, router)
                .with_graceful_shutdown(async move {
                    ct.cancelled().await;
                })
                .await?;
        }
        _ => {
            eprintln!(
                "[ERROR] Invalid transport type: {}. Use 'stdio' or 'http'",
                args.transport
            );
            return Err(format!("Invalid transport type: {}", args.transport).into());
        }
    }

    eprintln!("[DEBUG] Server ended");
    Ok(())
}
