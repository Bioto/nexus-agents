//! Nutrition OpenAPI Server
//!
//! Exposes Nutrition MCP tools as a REST API with OpenAPI documentation.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{Html, IntoResponse, Json},
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::RwLock;
use clap::Parser;
use x_toolbox::nutrition::mcp_server::NutritionMcpServer;
use rmcp::model::Tool;

#[derive(Parser, Debug)]
#[command(name = "nutrition-openapi-server")]
#[command(about = "Nutrition MCP Server exposed as REST API with OpenAPI documentation")]
struct Args {
    /// Bind address (e.g., "127.0.0.1:8082" or "0.0.0.0:8082")
    #[arg(short, long, default_value = "0.0.0.0:8082")]
    bind: String,

    /// API key for authentication (optional)
    #[arg(long, env = "MCP_API_KEY")]
    api_key: Option<String>,
}

/// OpenAPI-wrapped Nutrition MCP server state
pub struct OpenApiNutritionServer {
    pub server: NutritionMcpServer,
}

/// Tool call request body
#[derive(Debug, Deserialize)]
pub struct ToolCallRequest {
    pub name: String,
    #[serde(default)]
    pub arguments: Value,
}

/// Tool call response
#[derive(Debug, Serialize)]
pub struct ToolCallResponse {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Tool info for listing
#[derive(Debug, Serialize)]
pub struct ToolInfo {
    pub name: String,
    pub description: Option<String>,
    pub input_schema: Value,
}

/// Server info response
#[derive(Debug, Serialize)]
pub struct ServerInfo {
    pub name: String,
    pub version: String,
    pub description: String,
    pub tools_count: usize,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    
    // Load environment variables
    let _ = dotenvy::dotenv();
    
    eprintln!("Starting Nutrition OpenAPI Server...");
    eprintln!("Connecting to database...");
    
    let server = NutritionMcpServer::new().await?;
    let tools = server.tool_router.list_all();
    eprintln!("Loaded {} tools", tools.len());
    
    let state = Arc::new(RwLock::new(OpenApiNutritionServer { server }));

    let bind_addr: std::net::SocketAddr = args
        .bind
        .parse()
        .map_err(|e| format!("Invalid bind address '{}': {}", args.bind, e))?;

    let mut app = Router::new()
        .route("/", get(root_handler))
        .route("/docs", get(swagger_ui))
        .route("/openapi.json", get(openapi_spec))
        .route("/info", get(server_info))
        .route("/tools", get(list_tools))
        .route("/tools/list", get(list_tools))
        .route("/tools/call", post(call_tool))
        .route("/tools/{name}", post(call_tool_by_name))
        .with_state(state);

    // Add optional API key authentication
    if let Some(api_key) = args.api_key {
        eprintln!("API key authentication enabled");
        app = app.layer(axum::middleware::from_fn(move |req: axum::extract::Request, next: axum::middleware::Next| {
            let key = api_key.clone();
            async move {
                let path = req.uri().path();
                if path == "/docs" || path == "/openapi.json" || path == "/" {
                    return next.run(req).await;
                }
                if let Some(auth_header) = req.headers().get("authorization") {
                    if let Ok(auth_str) = auth_header.to_str() {
                        if auth_str == format!("Bearer {}", key) || auth_str == key {
                            return next.run(req).await;
                        }
                    }
                }
                (StatusCode::UNAUTHORIZED, "Invalid or missing API key").into_response()
            }
        }));
    }

    // Add CORS
    let cors = tower_http::cors::CorsLayer::permissive();
    let app = app.layer(cors);

    let tcp_listener = tokio::net::TcpListener::bind(bind_addr).await?;

    eprintln!("========================================");
    eprintln!("  Nutrition OpenAPI Server Started");
    eprintln!("========================================");
    eprintln!();
    eprintln!("OpenAPI Docs: http://{}/docs", bind_addr);
    eprintln!("API Endpoint: http://{}/", bind_addr);
    eprintln!();

    // Handle graceful shutdown
    let ct = tokio_util::sync::CancellationToken::new();
    let ct_clone = ct.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        eprintln!("Shutting down...");
        ct_clone.cancel();
    });

    axum::serve(tcp_listener, app)
        .with_graceful_shutdown(async move { ct.cancelled().await })
        .await?;

    Ok(())
}

async fn root_handler() -> impl IntoResponse {
    Html(r#"<!DOCTYPE html><html><head><meta http-equiv="refresh" content="0; url=/docs"></head></html>"#)
}

async fn swagger_ui() -> impl IntoResponse {
    Html(r#"<!DOCTYPE html>
<html>
<head>
    <title>Nutrition MCP Server - API Documentation</title>
    <link rel="stylesheet" type="text/css" href="https://cdn.jsdelivr.net/npm/swagger-ui-dist@5/swagger-ui.css">
    <style>body { margin: 0; } .swagger-ui .topbar { display: none; }</style>
</head>
<body>
    <div id="swagger-ui"></div>
    <script src="https://cdn.jsdelivr.net/npm/swagger-ui-dist@5/swagger-ui-bundle.js"></script>
    <script>
        window.onload = function() {
            SwaggerUIBundle({
                url: '/openapi.json',
                dom_id: '#swagger-ui',
                deepLinking: true,
                presets: [SwaggerUIBundle.presets.apis],
                layout: "BaseLayout"
            });
        };
    </script>
</body>
</html>"#)
}

async fn openapi_spec(
    State(state): State<Arc<RwLock<OpenApiNutritionServer>>>,
) -> impl IntoResponse {
    let state = state.read().await;
    let tools = state.server.tool_router.list_all();

    let mut paths = serde_json::Map::new();
    
    paths.insert("/info".to_string(), json!({
        "get": { "summary": "Get server information", "tags": ["Server"], "responses": { "200": { "description": "OK" } } }
    }));
    
    paths.insert("/tools".to_string(), json!({
        "get": { "summary": "List all available tools", "tags": ["Tools"], "responses": { "200": { "description": "OK" } } }
    }));
    
    paths.insert("/tools/call".to_string(), json!({
        "post": {
            "summary": "Call a tool by name",
            "tags": ["Tools"],
            "requestBody": { "required": true, "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ToolCallRequest" } } } },
            "responses": { "200": { "description": "OK" } }
        }
    }));

    // Add per-tool endpoints
    for tool in &tools {
        paths.insert(format!("/tools/{}", tool.name), json!({
            "post": {
                "summary": format!("{}", tool.description.as_ref().map(|s| s.as_ref()).unwrap_or(&tool.name)),
                "description": tool.description.as_ref().map(|s| s.as_ref()).unwrap_or(""),
                "operationId": tool.name.replace("-", "_"),
                "tags": ["Tools"],
                "requestBody": { "required": true, "content": { "application/json": { "schema": tool.input_schema } } },
                "responses": { "200": { "description": "Tool executed successfully" } }
            }
        }));
    }

    Json(json!({
        "openapi": "3.1.0",
        "info": {
            "title": "Nutrition MCP Server",
            "description": "Nutrition database management - ingredients, recipes, meal plans, and nutritional information.",
            "version": "0.1.0"
        },
        "servers": [{ "url": "/", "description": "Current server" }],
        "tags": [
            { "name": "Tools", "description": "Nutrition MCP tools" },
            { "name": "Server", "description": "Server information" }
        ],
        "paths": paths,
        "components": {
            "schemas": {
                "ToolCallRequest": {
                    "type": "object",
                    "required": ["name"],
                    "properties": {
                        "name": { "type": "string" },
                        "arguments": { "type": "object" }
                    }
                }
            }
        }
    }))
}

async fn server_info(
    State(state): State<Arc<RwLock<OpenApiNutritionServer>>>,
) -> impl IntoResponse {
    let state = state.read().await;
    let tools = state.server.tool_router.list_all();

    Json(ServerInfo {
        name: "nutrition-mcp-server".to_string(),
        version: "0.1.0".to_string(),
        description: "Nutrition MCP Server - Manage ingredients, recipes, meal plans, and nutritional information".to_string(),
        tools_count: tools.len(),
    })
}

async fn list_tools(
    State(state): State<Arc<RwLock<OpenApiNutritionServer>>>,
) -> impl IntoResponse {
    let state = state.read().await;
    let tools = state.server.tool_router.list_all();

    let tool_infos: Vec<ToolInfo> = tools
        .into_iter()
        .map(|t| ToolInfo {
            name: t.name.to_string(),
            description: t.description.map(|d| d.to_string()),
            input_schema: Value::Object((*t.input_schema).clone()),
        })
        .collect();

    Json(tool_infos)
}

async fn call_tool(
    State(state): State<Arc<RwLock<OpenApiNutritionServer>>>,
    Json(request): Json<ToolCallRequest>,
) -> impl IntoResponse {
    call_tool_internal(state, request.name, request.arguments).await
}

async fn call_tool_by_name(
    State(state): State<Arc<RwLock<OpenApiNutritionServer>>>,
    Path(name): Path<String>,
    Json(arguments): Json<Value>,
) -> impl IntoResponse {
    call_tool_internal(state, name, arguments).await
}

async fn call_tool_internal(
    state: Arc<RwLock<OpenApiNutritionServer>>,
    name: String,
    arguments: Value,
) -> impl IntoResponse {
    let state_guard = state.read().await;
    let tools = state_guard.server.tool_router.list_all();
    
    if !tools.iter().any(|t| t.name == name) {
        return (
            StatusCode::NOT_FOUND,
            Json(ToolCallResponse {
                success: false,
                result: None,
                error: Some(format!("Tool '{}' not found", name)),
            }),
        );
    }

    let args_map = if arguments.is_null() { None } else { arguments.as_object().cloned() };
    let server = state_guard.server.clone();
    drop(state_guard);

    match server.call_tool_http(&name, args_map).await {
        Ok(result) => {
            let content: Vec<Value> = result
                .content
                .iter()
                .map(|c| {
                    if let Some(raw) = c.raw.as_text() {
                        json!({ "type": "text", "text": raw.text })
                    } else {
                        json!({ "type": "unknown" })
                    }
                })
                .collect();

            (StatusCode::OK, Json(ToolCallResponse {
                success: true,
                result: Some(json!({ "content": content })),
                error: None,
            }))
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(ToolCallResponse {
            success: false,
            result: None,
            error: Some(format!("{:?}", e)),
        })),
    }
}

