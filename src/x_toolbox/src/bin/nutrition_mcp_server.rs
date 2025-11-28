use clap::Parser;
use rmcp::serve_server;
use rmcp::transport::stdio;
use rmcp::transport::streamable_http_server::{
    session::local::LocalSessionManager, tower::StreamableHttpService, StreamableHttpServerConfig,
};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use x_toolbox::nutrition::mcp_server::NutritionMcpServer;

#[derive(Parser, Debug)]
#[command(name = "nutrition-mcp-server")]
#[command(about = "Nutrition MCP Server - supports both stdio and streamable HTTP transports")]
struct Args {
    /// Transport type to use: "stdio" or "http"
    #[arg(short, long, default_value = "stdio")]
    transport: String,

    /// Bind address for HTTP transport (e.g., "127.0.0.1:8002")
    /// Only used when transport is "http"
    #[arg(short, long, default_value = "0.0.0.0:8002")]
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
    eprintln!("Starting Nutrition MCP Server...");
    eprintln!("Transport: {}", args.transport);

    // Log OpenAI config (optional, for debugging)
    if let Ok(key) = dotenvy::var("OPENAI_API_KEY") {
        eprintln!("OPENAI_API_KEY: {}...", &key[..key.len().min(8)]);
    }
    if let Ok(url) = dotenvy::var("OPENAI_BASE_URL") {
        eprintln!("OPENAI_BASE_URL: {}", url);
    }

    match args.transport.as_str() {
        "stdio" => {
            eprintln!("Using stdio transport");
            let server = NutritionMcpServer::new().await?;
            serve_server(server, stdio()).await?;
        }
        "http" => {
            eprintln!("Using streamable HTTP transport");
            let bind_addr: SocketAddr = args
                .bind
                .parse()
                .map_err(|e| format!("Invalid bind address '{}': {}", args.bind, e))?;

            eprintln!("Binding HTTP server to {} (path: {})", bind_addr, args.path);

            // Note: Since NutritionMcpServer::new() is async, we need to block on it here
            // This is a limitation of the current rmcp StreamableHttpService design
            let db = x_toolbox::nutrition::Database::new().await?;
            let server = NutritionMcpServer::with_database(db).await?;
            
            let service: StreamableHttpService<NutritionMcpServer, LocalSessionManager> =
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

