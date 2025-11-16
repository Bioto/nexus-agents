use rmcp::{
    handler::server::{
        router::tool::ToolRouter,
        router::prompt::PromptRouter,
        ServerHandler,
        wrapper::{Json, Parameters},
    },
    model::*,
    ErrorData as McpError,
    tool, tool_router, prompt, prompt_router,
    schemars,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;
use crate::codegen::CodeGenerator;

/// MCP Server with tools, resources, and prompts for testing
#[derive(Clone)]
pub struct NexusMcpServer {
    counter: Arc<Mutex<i32>>,
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
    #[allow(dead_code)]
    prompt_router: PromptRouter<Self>,
}

// Tool definitions
#[tool_router]
impl NexusMcpServer {
    pub fn new() -> Self {
        Self {
            counter: Arc::new(Mutex::new(0)),
            tool_router: Self::tool_router(),
            prompt_router: Self::prompt_router(),
        }
    }

    /// Echo tool - echoes the input back
    #[tool(description = "Echoes the input string back to the user")]
    async fn echo(&self, params: Parameters<EchoParams>) -> Result<CallToolResult, McpError> {
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Echo: {}",
            params.0.message
        ))]))
    }

    /// Add tool - adds two numbers
    #[tool(description = "Adds two numbers together")]
    async fn add(&self, params: Parameters<AddParams>) -> Result<Json<AddResult>, McpError> {
        let result = params.0.a + params.0.b;
        Ok(Json(AddResult {
            result,
            operation: "add".to_string(),
        }))
    }

    /// Counter tool - increments and returns counter value
    #[tool(description = "Increments an internal counter and returns the new value")]
    async fn increment_counter(&self) -> Result<CallToolResult, McpError> {
        let mut counter = self.counter.lock().await;
        *counter += 1;
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Counter value: {}",
            *counter
        ))]))
    }

    /// Get counter tool - returns current counter value
    #[tool(description = "Returns the current value of the internal counter")]
    async fn get_counter(&self) -> Result<CallToolResult, McpError> {
        let counter = self.counter.lock().await;
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Current counter: {}",
            *counter
        ))]))
    }

    /// Generate code API tool - generates Python code API for all MCP tools
    #[tool(description = "Generates Python code API files in directory structure (servers/nexus-mcp-server/) following the Anthropic code execution pattern. Returns the path where files were generated.")]
    async fn generate_code_api(
        &self,
        params: Parameters<GenerateCodeApiParams>,
    ) -> Result<CallToolResult, McpError> {
        let server_url = params.0.server_url.unwrap_or_else(|| "http://127.0.0.1:8000".to_string());
        let output_dir = params.0.output_dir.unwrap_or_else(|| "servers".to_string());
        let generator = CodeGenerator::new(server_url);
        
        let output_path = std::path::Path::new(&output_dir);
        match generator.generate_code_files(output_path).await {
            Ok(_) => {
                let message = format!(
                    "Successfully generated code API in directory: {}\nStructure:\n  {}/_mcp_client.py\n  {}/nexus-mcp-server/",
                    output_dir, output_dir, output_dir
                );
                Ok(CallToolResult::success(vec![Content::text(message)]))
            }
            Err(e) => {
                // Convert error to string for error message
                let error_str = e.to_string();
                Err(McpError::internal_error(
                    "Failed to generate code API",
                    Some(serde_json::json!({ "details": error_str })),
                ))
            }
        }
    }
}

// Prompt definitions
#[prompt_router]
impl NexusMcpServer {
    /// Simple prompt template
    #[prompt(name = "greeting", description = "A simple greeting prompt")]
    async fn greeting_prompt(
        &self,
        params: Parameters<GreetingParams>,
    ) -> Result<GetPromptResult, McpError> {
        Ok(GetPromptResult {
            description: Some("A greeting prompt".into()),
            messages: vec![PromptMessage::new_text(
                PromptMessageRole::User,
                format!("Hello, {}! How can I help you today?", params.0.name),
            )],
        })
    }

    /// Code review prompt
    #[prompt(name = "code_review", description = "A prompt template for code review")]
    async fn code_review_prompt(
        &self,
        params: Parameters<CodeReviewParams>,
    ) -> Result<GetPromptResult, McpError> {
        Ok(GetPromptResult {
            description: Some("Code review prompt".into()),
            messages: vec![PromptMessage::new_text(
                PromptMessageRole::User,
                format!(
                    "Please review the following code:\n\n```{}\n{}\n```\n\nFocus on: {}",
                    params.0.language, params.0.code, params.0.focus
                ),
            )],
        })
    }

    /// Analysis prompt
    #[prompt(name = "analyze", description = "A prompt template for analysis tasks")]
    async fn analyze_prompt(
        &self,
        params: Parameters<AnalyzeParams>,
    ) -> Result<GetPromptResult, McpError> {
        Ok(GetPromptResult {
            description: Some("Analysis prompt".into()),
            messages: vec![PromptMessage::new_text(
                PromptMessageRole::User,
                format!(
                    "Please analyze the following:\n\nTopic: {}\n\nContext: {}\n\nProvide a detailed analysis.",
                    params.0.topic, params.0.context
                ),
            )],
        })
    }
}

// Parameter types for tools
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct EchoParams {
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AddParams {
    pub a: f64,
    pub b: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AddResult {
    pub result: f64,
    pub operation: String,
}

// Parameter types for prompts
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GreetingParams {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CodeReviewParams {
    pub code: String,
    pub language: String,
    pub focus: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AnalyzeParams {
    pub topic: String,
    pub context: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GenerateCodeApiParams {
    /// MCP server URL (default: http://127.0.0.1:8000)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_url: Option<String>,
    /// Output directory path (default: servers)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_dir: Option<String>,
}

// Implement ServerHandler
impl ServerHandler for NexusMcpServer {
    fn initialize(
        &self,
        _request: InitializeRequestParam,
        _context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> impl std::future::Future<Output = Result<InitializeResult, McpError>> + Send + '_ {
        eprintln!("[DEBUG] initialize() called");
        async move {
            eprintln!("[DEBUG] Returning initialize result with capabilities");
            Ok(InitializeResult {
                protocol_version: ProtocolVersion::V_2024_11_05,
                capabilities: ServerCapabilities {
                    tools: Some(ToolsCapability {
                        list_changed: None,
                    }),
                    resources: Some(ResourcesCapability {
                        subscribe: None,
                        list_changed: None,
                    }),
                    prompts: Some(PromptsCapability {
                        list_changed: None,
                    }),
                    ..Default::default()
                },
                server_info: Implementation {
                    name: "nexus-mcp-server".into(),
                    version: "0.1.0".into(),
                    icons: None,
                    title: None,
                    website_url: None,
                },
                instructions: None,
            })
        }
    }

    fn on_initialized(
        &self,
        context: rmcp::service::NotificationContext<rmcp::RoleServer>,
    ) -> impl std::future::Future<Output = ()> + Send + '_ {
        eprintln!("[DEBUG] on_initialized() called - client has completed initialization");
        let peer = context.peer.clone();
        async move {
            // Send initialization notification asynchronously to avoid blocking
            tokio::spawn(async move {
                let notification = LoggingMessageNotificationParam {
                    level: LoggingLevel::Info,
                    logger: Some("stdio".to_string()),
                    data: serde_json::json!({
                        "message": "Server initialized, waiting for connections..."
                    }),
                };

                if let Err(e) = peer.notify_logging_message(notification).await {
                    eprintln!("[WARN] Failed to send initialization notification: {}", e);
                }
            });
        }
    }

    fn call_tool(
        &self,
        request: CallToolRequestParam,
        context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> impl std::future::Future<Output = Result<CallToolResult, McpError>> + Send + '_ {
        let tool_router = self.tool_router.clone();
        let server = self.clone();
        async move {
            let tool_call_context = rmcp::handler::server::tool::ToolCallContext::new(
                &server,
                request,
                context,
            );
            tool_router.call(tool_call_context).await
        }
    }

    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParam>,
        _context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> impl std::future::Future<Output = Result<ListToolsResult, McpError>> + Send + '_ {
        let tools = self.tool_router.list_all();
        let tool_count = tools.len();
        eprintln!("[DEBUG] list_tools called, returning {} tools", tool_count);
        async move {
            Ok(ListToolsResult {
                tools,
                next_cursor: None,
            })
        }
    }

    fn get_prompt(
        &self,
        request: GetPromptRequestParam,
        context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> impl std::future::Future<Output = Result<GetPromptResult, McpError>> + Send + '_ {
        let prompt_router = self.prompt_router.clone();
        let server = self.clone();
        async move {
            let prompt_context = rmcp::handler::server::prompt::PromptContext::new(
                &server,
                request.name,
                request.arguments,
                context,
            );
            prompt_router.get_prompt(prompt_context).await
        }
    }

    fn list_prompts(
        &self,
        _request: Option<PaginatedRequestParam>,
        _context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> impl std::future::Future<Output = Result<ListPromptsResult, McpError>> + Send + '_ {
        let prompts = self.prompt_router.list_all();
        let prompt_count = prompts.len();
        eprintln!("[DEBUG] list_prompts called, returning {} prompts", prompt_count);
        async move {
            Ok(ListPromptsResult {
                prompts,
                next_cursor: None,
            })
        }
    }

    fn list_resources(
        &self,
        _request: Option<PaginatedRequestParam>,
        _context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> impl std::future::Future<Output = Result<ListResourcesResult, McpError>> + Send + '_ {
        eprintln!("[DEBUG] list_resources called");
        async move {
            Ok(ListResourcesResult {
                resources: vec![
                    RawResource {
                        uri: "test://example/text".into(),
                        name: "Example Text Resource".into(),
                        title: None,
                        description: Some("A simple text resource for testing".into()),
                        mime_type: Some("text/plain".into()),
                        size: None,
                        icons: None,
                    }
                    .no_annotation(),
                    RawResource {
                        uri: "test://example/json".into(),
                        name: "Example JSON Resource".into(),
                        title: None,
                        description: Some("A JSON resource for testing".into()),
                        mime_type: Some("application/json".into()),
                        size: None,
                        icons: None,
                    }
                    .no_annotation(),
                    RawResource {
                        uri: "test://counter/state".into(),
                        name: "Counter State".into(),
                        title: None,
                        description: Some("Returns the current counter state".into()),
                        mime_type: Some("text/plain".into()),
                        size: None,
                        icons: None,
                    }
                    .no_annotation(),
                ],
                next_cursor: None,
            })
        }
    }

    fn read_resource(
        &self,
        request: ReadResourceRequestParam,
        _context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> impl std::future::Future<Output = Result<ReadResourceResult, McpError>> + Send + '_ {
        let counter = self.counter.clone();
        async move {
            match request.uri.as_str() {
                "test://example/text" => Ok(ReadResourceResult {
                    contents: vec![ResourceContents::text(
                        "This is a test text resource.",
                        "test://example/text",
                    )],
                }),
                "test://example/json" => Ok(ReadResourceResult {
                    contents: vec![ResourceContents::text(
                        serde_json::json!({"message": "This is a test JSON resource", "status": "ok"}).to_string(),
                        "test://example/json",
                    )],
                }),
                "test://counter/state" => {
                    let counter_value = counter.lock().await;
                    Ok(ReadResourceResult {
                        contents: vec![ResourceContents::text(
                            format!("Current counter value: {}", *counter_value),
                            "test://counter/state",
                        )],
                    })
                }
                _ => Err(McpError::invalid_params("Unknown resource URI", None)),
            }
        }
    }
}

