//! MCP protocol client for fetching tool definitions.

use crate::error::NexusError;
use crate::types::ToolDefinition;
use serde_json::{json, Value};
use std::collections::HashMap;

/// MCP protocol client for fetching tool definitions.
pub struct McpClient {
    server_url: String,
    client: reqwest::Client,
    headers: HashMap<String, String>,
}

impl McpClient {
    /// Create a new MCP client.
    pub fn new(server_url: impl Into<String>) -> Self {
        Self {
            server_url: server_url.into(),
            client: reqwest::Client::new(),
            headers: HashMap::new(),
        }
    }

    /// Create a new MCP client with custom headers.
    pub fn with_headers(server_url: impl Into<String>, headers: HashMap<String, String>) -> Self {
        Self {
            server_url: server_url.into(),
            client: reqwest::Client::new(),
            headers,
        }
    }

    /// Fetch tool definitions from MCP server.
    pub async fn fetch_tools(&self) -> Result<Vec<ToolDefinition>, NexusError> {
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

        let mut init_request_builder = self
            .client
            .post(&url)
            .header("Accept", "application/json, text/event-stream")
            .header("Content-Type", "application/json");

        // Add custom headers
        for (key, value) in &self.headers {
            init_request_builder = init_request_builder.header(key, value);
        }

        let init_response = init_request_builder.json(&init_request).send().await?;

        if !init_response.status().is_success() {
            let status = init_response.status();
            let error_text = init_response
                .text()
                .await
                .unwrap_or_else(|_| "Unable to read error response".to_string());
            return Err(NexusError::Http(format!(
                "MCP server returned error during initialization: {} - {}",
                status, error_text
            )));
        }

        // Extract session ID from response headers
        let session_id = init_response
            .headers()
            .get("mcp-session-id")
            .and_then(|h| h.to_str().ok())
            .map(|s| s.to_string());

        // Parse SSE format response
        let init_text = init_response
            .text()
            .await
            .map_err(|e| NexusError::Http(e.to_string()))?;

        // If response is empty or doesn't match SSE format, try to parse as JSON directly
        let init_json = if init_text.trim().is_empty() {
            return Err(NexusError::Parse(
                "Empty response from MCP server. Check authentication headers.".to_string(),
            ));
        } else if init_text.trim().starts_with('{') {
            // Direct JSON response
            serde_json::from_str(&init_text).map_err(|e| {
                NexusError::Parse(format!(
                    "Failed to parse JSON response: {} - Response: {}",
                    e,
                    &init_text[..init_text.len().min(500)]
                ))
            })?
        } else {
            // Try SSE format
            self.parse_sse_response(&init_text)?
        };

        if let Some(error) = init_json.get("error") {
            return Err(NexusError::Server(format!(
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

        // Add custom headers
        for (key, value) in &self.headers {
            initialized_request = initialized_request.header(key, value);
        }

        // Add session ID if we have one
        if let Some(ref sid) = session_id {
            initialized_request = initialized_request.header("mcp-session-id", sid);
        }

        if let Err(e) = initialized_request
            .json(&initialized_notification)
            .send()
            .await
        {
            eprintln!("[WARN] Failed to send initialized notification: {}", e);
        }

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

        // Add custom headers
        for (key, value) in &self.headers {
            tools_request_builder = tools_request_builder.header(key, value);
        }

        // Add session ID if we have one
        if let Some(ref sid) = session_id {
            tools_request_builder = tools_request_builder.header("mcp-session-id", sid);
        }

        let tools_response = tools_request_builder.json(&tools_request).send().await?;

        if !tools_response.status().is_success() {
            return Err(NexusError::Http(format!(
                "MCP server returned error: {}",
                tools_response.status()
            )));
        }

        // Parse SSE format response
        let tools_text = tools_response
            .text()
            .await
            .map_err(|e| NexusError::Http(e.to_string()))?;

        let tools_json = self.parse_sse_response(&tools_text)?;

        // Handle JSON-RPC response
        if let Some(error) = tools_json.get("error") {
            return Err(NexusError::Server(format!("MCP server error: {}", error)));
        }

        let result = tools_json
            .get("result")
            .ok_or_else(|| NexusError::Parse("Missing 'result' in response".to_string()))?;

        let tools = result
            .get("tools")
            .and_then(|t| t.as_array())
            .ok_or_else(|| NexusError::Parse("Missing 'tools' array in result".to_string()))?;

        let mut tool_defs = Vec::new();
        for tool in tools {
            tool_defs.push(self.parse_tool_definition(tool)?);
        }

        Ok(tool_defs)
    }

    /// Parse a single tool definition from JSON.
    fn parse_tool_definition(&self, tool: &Value) -> Result<ToolDefinition, NexusError> {
        let name = tool
            .get("name")
            .and_then(|n| n.as_str())
            .ok_or_else(|| NexusError::Parse("Missing 'name' in tool".to_string()))?
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

    /// Parse SSE (Server-Sent Events) format response.
    ///
    /// Looks for lines starting with "data: " and extracts JSON.
    /// Handles multiple data lines by accumulating them if needed, or picking the last valid one.
    fn parse_sse_response(&self, text: &str) -> Result<Value, NexusError> {
        // First, try to parse as direct JSON (some servers return JSON directly)
        if let Ok(json) = serde_json::from_str::<Value>(text.trim()) {
            return Ok(json);
        }

        let mut json_data = String::new();

        // Try SSE format
        for line in text.lines() {
            let line = line.trim();
            if let Some(data) = line.strip_prefix("data: ") {
                json_data.push_str(data);
            }
        }

        if !json_data.is_empty() {
            // Try to parse the accumulated data
            if let Ok(json) = serde_json::from_str::<Value>(&json_data) {
                return Ok(json);
            }

            // If that failed, maybe it was multiple independent JSON objects?
            // Try to parse the last one found
            for line in text.lines().rev() {
                let line = line.trim();
                if let Some(data) = line.strip_prefix("data: ") {
                    if let Ok(json) = serde_json::from_str::<Value>(data) {
                        return Ok(json);
                    }
                }
            }

            return Err(NexusError::Parse(format!(
                "Failed to parse SSE JSON data: {}",
                json_data
            )));
        }

        // If neither worked, return error with response preview
        let preview = if text.len() > 500 {
            format!("{}...", &text[..500])
        } else {
            text.to_string()
        };
        Err(NexusError::Parse(format!(
            "No 'data: ' line found in SSE response and not valid JSON. Response preview: {}",
            preview
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn client() -> McpClient {
        McpClient::new("http://localhost:8000")
    }

    #[test]
    fn test_parse_sse_response_direct_json() {
        let client = client();
        let input = r#"{"jsonrpc": "2.0", "id": 1, "result": {"tools": []}}"#;
        let result = client.parse_sse_response(input).unwrap();

        assert_eq!(result["jsonrpc"], "2.0");
        assert_eq!(result["id"], 1);
    }

    #[test]
    fn test_parse_sse_response_with_whitespace() {
        let client = client();
        let input = r#"
            {"jsonrpc": "2.0", "id": 1, "result": {}}
        "#;
        let result = client.parse_sse_response(input).unwrap();

        assert_eq!(result["jsonrpc"], "2.0");
    }

    #[test]
    fn test_parse_sse_response_sse_format() {
        let client = client();
        let input = "data: {\"jsonrpc\": \"2.0\", \"id\": 1, \"result\": {\"tools\": []}}";
        let result = client.parse_sse_response(input).unwrap();

        assert_eq!(result["jsonrpc"], "2.0");
        assert!(result["result"]["tools"].is_array());
    }

    #[test]
    fn test_parse_sse_response_sse_with_event_lines() {
        let client = client();
        let input = "event: message\ndata: {\"jsonrpc\": \"2.0\", \"id\": 1}\n\n";
        let result = client.parse_sse_response(input).unwrap();

        assert_eq!(result["jsonrpc"], "2.0");
    }

    #[test]
    fn test_parse_sse_response_multiple_data_lines() {
        let client = client();
        // Some servers split JSON across multiple data lines
        let input = "data: {\"jsonrpc\": \"2.0\",\ndata:  \"id\": 1}";
        let result = client.parse_sse_response(input).unwrap();

        assert_eq!(result["jsonrpc"], "2.0");
        assert_eq!(result["id"], 1);
    }

    #[test]
    fn test_parse_sse_response_invalid_json() {
        let client = client();
        let input = "this is not json at all";
        let result = client.parse_sse_response(input);

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, NexusError::Parse(_)));
    }

    #[test]
    fn test_parse_sse_response_empty_data_line() {
        let client = client();
        let input = "data: ";
        let result = client.parse_sse_response(input);

        // Empty data line should fail to parse
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_tool_definition() {
        let client = client();
        let tool_json = json!({
            "name": "test_tool",
            "description": "A test tool",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "arg1": {"type": "string"}
                }
            }
        });

        let tool_def = client.parse_tool_definition(&tool_json).unwrap();

        assert_eq!(tool_def.name, "test_tool");
        assert_eq!(tool_def.description, "A test tool");
        assert!(tool_def.input_schema["properties"]["arg1"].is_object());
    }

    #[test]
    fn test_parse_tool_definition_missing_name() {
        let client = client();
        let tool_json = json!({
            "description": "A tool without a name"
        });

        let result = client.parse_tool_definition(&tool_json);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_tool_definition_optional_fields() {
        let client = client();
        let tool_json = json!({
            "name": "minimal_tool"
        });

        let tool_def = client.parse_tool_definition(&tool_json).unwrap();

        assert_eq!(tool_def.name, "minimal_tool");
        assert_eq!(tool_def.description, ""); // Should default to empty
        assert_eq!(tool_def.input_schema, json!({})); // Should default to empty object
    }
}
