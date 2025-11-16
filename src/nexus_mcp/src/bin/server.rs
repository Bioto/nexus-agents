use clap::Parser;
use nexus_mcp::server::NexusMcpServer;
use rmcp::serve_server;
use rmcp::transport::{
    stdio,
    streamable_http_server::{
        session::local::LocalSessionManager, tower::StreamableHttpService,
        StreamableHttpServerConfig,
    },
};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

#[derive(Parser, Debug)]
#[command(name = "nexus-mcp-server")]
#[command(about = "Nexus MCP Server - supports both stdio and streamable HTTP transports")]
struct Args {
    /// Transport type to use: "stdio" or "http"
    #[arg(short, long, default_value = "stdio")]
    transport: String,

    /// Bind address for HTTP transport (e.g., "127.0.0.1:8000")
    /// Only used when transport is "http"
    #[arg(short, long, default_value = "127.0.0.1:8000")]
    bind: String,

    /// HTTP path endpoint (default: "/mcp")
    /// Only used when transport is "http"
    #[arg(long, default_value = "/mcp")]
    path: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    // Use stderr for logging/debugging - stdout is reserved for MCP protocol when using stdio
    eprintln!("Starting Nexus MCP Server...");
    eprintln!("Transport: {}", args.transport);

    let server = NexusMcpServer::new();

    match args.transport.as_str() {
        "stdio" => {
            eprintln!("Using stdio transport");
            let transport = stdio();
            eprintln!("Server waiting for connections on stdin/stdout...");
            if let Err(e) = serve_server(server, transport).await {
                eprintln!("[ERROR] serve_server error: {}", e);
                return Err(e.into());
            }
        }
        "http" => {
            eprintln!("Using streamable HTTP transport");
            let bind_addr: SocketAddr = args
                .bind
                .parse()
                .map_err(|e| format!("Invalid bind address '{}': {}", args.bind, e))?;

            eprintln!("Binding HTTP server to {} (path: {})", bind_addr, args.path);

            let service: StreamableHttpService<NexusMcpServer, LocalSessionManager> =
                StreamableHttpService::new(
                    || Ok(NexusMcpServer::new()),
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
