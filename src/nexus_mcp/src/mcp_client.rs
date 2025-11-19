use serde_json::{json, Value};

/// Error type for MCP client operations
#[derive(Debug)]
pub enum McpClientError {
    HttpError(String),
    ParseError(String),
    ServerError(String),
}

impl std::fmt::Display for McpClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            McpClientError::HttpError(msg) => write!(f, "HTTP error: {}", msg),
            McpClientError::ParseError(msg) => write!(f, "Parse error: {}", msg),
            McpClientError::ServerError(msg) => write!(f, "Server error: {}", msg),
        }
    }
}

impl std::error::Error for McpClientError {}

/// Tool definition structure
#[derive(Debug, Clone)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

/// MCP protocol client for fetching tool definitions
pub struct McpClient {
    server_url: String,
    client: reqwest::Client,
}

impl McpClient {
    /// Create a new MCP client
    pub fn new(server_url: impl Into<String>) -> Self {
        Self {
            server_url: server_url.into(),
            client: reqwest::Client::new(),
        }
    }

    /// Fetch tool definitions from MCP server
    pub async fn fetch_tools(&self) -> Result<Vec<ToolDefinition>, McpClientError> {
        // Handle both cases: server_url with or without /mcp
        let url = if self.server_url.ends_with("/mcp") {
            self.server_url.clone()
        } else {
            format!("{}/mcp", self.server_url)
        };

        // Step 1: Initialize the MCP session
        let init_request = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {
                    "name": "nexus-mcp-codegen",
                    "version": "0.1.0"
                }
            }
        });

        let init_response = self
            .client
            .post(&url)
            .header("Accept", "application/json, text/event-stream")
            .header("Content-Type", "application/json")
            .json(&init_request)
            .send()
            .await
            .map_err(|e| {
                McpClientError::HttpError(format!("Failed to connect to MCP server: {}", e))
            })?;

        if !init_response.status().is_success() {
            return Err(McpClientError::HttpError(format!(
                "MCP server returned error during initialization: {}",
                init_response.status()
            )));
        }

        // Extract session ID from response headers
        let session_id = init_response
            .headers()
            .get("mcp-session-id")
            .and_then(|h| h.to_str().ok())
            .map(|s| s.to_string());

        // Parse SSE format response
        let init_text = init_response.text().await.map_err(|e| {
            McpClientError::ParseError(format!("Failed to read init response: {}", e))
        })?;

        let init_json = self.parse_sse_response(&init_text)?;

        if let Some(error) = init_json.get("error") {
            return Err(McpClientError::ServerError(format!(
                "MCP initialization error: {}",
                error
            )));
        }

        // Step 2: Send initialized notification (required by MCP protocol)
        let initialized_notification = json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        });

        let mut initialized_request = self
            .client
            .post(&url)
            .header("Accept", "application/json, text/event-stream")
            .header("Content-Type", "application/json");

        // Add session ID if we have one
        if let Some(ref sid) = session_id {
            initialized_request = initialized_request.header("mcp-session-id", sid);
        }

        let _ = initialized_request
            .json(&initialized_notification)
            .send()
            .await;

        // Step 3: Request tools list
        let tools_request = json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list",
            "params": {}
        });

        let mut tools_request_builder = self
            .client
            .post(&url)
            .header("Accept", "application/json, text/event-stream")
            .header("Content-Type", "application/json");

        // Add session ID if we have one
        if let Some(ref sid) = session_id {
            tools_request_builder = tools_request_builder.header("mcp-session-id", sid);
        }

        let tools_response = tools_request_builder
            .json(&tools_request)
            .send()
            .await
            .map_err(|e| McpClientError::HttpError(format!("Failed to request tools: {}", e)))?;

        if !tools_response.status().is_success() {
            return Err(McpClientError::HttpError(format!(
                "MCP server returned error: {}",
                tools_response.status()
            )));
        }

        // Parse SSE format response
        let tools_text = tools_response.text().await.map_err(|e| {
            McpClientError::ParseError(format!("Failed to read tools response: {}", e))
        })?;

        let tools_json = self.parse_sse_response(&tools_text)?;

        // Handle JSON-RPC response
        if let Some(error) = tools_json.get("error") {
            return Err(McpClientError::ServerError(format!(
                "MCP server error: {}",
                error
            )));
        }

        let result = tools_json
            .get("result")
            .ok_or_else(|| McpClientError::ParseError("Missing 'result' in response".to_string()))?;

        let tools = result
            .get("tools")
            .and_then(|t| t.as_array())
            .ok_or_else(|| {
                McpClientError::ParseError("Missing 'tools' array in result".to_string())
            })?;

        let mut tool_defs = Vec::new();
        for tool in tools {
            tool_defs.push(self.parse_tool_definition(tool)?);
        }

        Ok(tool_defs)
    }

    /// Parse a single tool definition from JSON
    fn parse_tool_definition(&self, tool: &Value) -> Result<ToolDefinition, McpClientError> {
        let name = tool
            .get("name")
            .and_then(|n| n.as_str())
            .ok_or_else(|| McpClientError::ParseError("Missing 'name' in tool".to_string()))?
            .to_string();

        let description = tool
            .get("description")
            .and_then(|d| d.as_str())
            .unwrap_or("")
            .to_string();

        let input_schema = tool
            .get("inputSchema")
            .cloned()
            .unwrap_or_else(|| json!({}));

        Ok(ToolDefinition {
            name,
            description,
            input_schema,
        })
    }

    /// Parse SSE (Server-Sent Events) format response
    /// Looks for lines starting with "data: " and extracts JSON
    fn parse_sse_response(&self, text: &str) -> Result<Value, McpClientError> {
        for line in text.lines() {
            let line = line.trim();
            if let Some(json_str) = line.strip_prefix("data: ") {
                return serde_json::from_str(json_str).map_err(|e| {
                    McpClientError::ParseError(format!("Failed to parse SSE JSON: {}", e))
                });
            }
        }
        Err(McpClientError::ParseError(
            "No 'data: ' line found in SSE response".to_string(),
        ))
    }
}

