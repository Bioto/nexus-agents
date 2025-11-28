//! Standalone MCP server binary supporting stdio and HTTP transports.

use axum::{
    body::Body,
    extract::State,
    http::{header, Request, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
};
use clap::Parser;
use nexus_mcp::server::NexusMcpServer;
use rmcp::serve_server;
use rmcp::transport::stdio;
use rmcp::transport::streamable_http_server::{
    session::local::LocalSessionManager, tower::StreamableHttpService, StreamableHttpServerConfig,
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
    #[arg(short, long, default_value = "0.0.0.0:8000")]
    bind: String,

    /// HTTP path endpoint (default: "/mcp")
    /// Only used when transport is "http"
    #[arg(long, default_value = "/mcp")]
    path: String,

    /// Bearer token for HTTP authentication (optional)
    /// When set, clients must include "Authorization: Bearer <token>" header
    /// Can also be set via MCP_AUTH_TOKEN environment variable
    #[arg(long, env = "MCP_AUTH_TOKEN")]
    auth_token: Option<String>,
}

/// Bearer token authentication middleware.
/// Returns 401 Unauthorized with WWW-Authenticate header if token is missing or invalid.
async fn bearer_auth_middleware(
    State(expected_token): State<String>,
    req: Request<Body>,
    next: Next,
) -> Response {
    // Extract Authorization header
    let auth_header = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok());

    match auth_header {
        Some(auth) if auth.starts_with("Bearer ") => {
            let token = &auth[7..]; // Skip "Bearer " prefix
            if token == expected_token {
                next.run(req).await
            } else {
                unauthorized_response("Invalid token")
            }
        }
        Some(_) => unauthorized_response("Invalid authorization scheme, expected Bearer"),
        None => unauthorized_response("Missing Authorization header"),
    }
}

/// Build a 401 Unauthorized response with WWW-Authenticate header per OAuth 2.1 spec.
fn unauthorized_response(error_description: &str) -> Response {
    (
        StatusCode::UNAUTHORIZED,
        [(
            header::WWW_AUTHENTICATE,
            format!(
                "Bearer error=\"invalid_token\", error_description=\"{}\"",
                error_description
            ),
        )],
        error_description.to_string(),
    )
        .into_response()
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    // Use stderr for logging/debugging - stdout is reserved for MCP protocol when using stdio
    eprintln!("Starting Nexus MCP Server...");
    eprintln!("Transport: {}", args.transport);

    match args.transport.as_str() {
        "stdio" => {
            eprintln!("Using stdio transport");
            let server = NexusMcpServer::new();
            serve_server(server, stdio()).await?;
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

            // Build router with optional Bearer auth middleware
            let router = if let Some(ref token) = args.auth_token {
                eprintln!("Authentication enabled (Bearer token required)");
                axum::Router::new()
                    .nest_service(&args.path, service)
                    .layer(middleware::from_fn_with_state(
                        token.clone(),
                        bearer_auth_middleware,
                    ))
            } else {
                eprintln!("Authentication disabled (no --auth-token or MCP_AUTH_TOKEN set)");
                axum::Router::new().nest_service(&args.path, service)
            };

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
