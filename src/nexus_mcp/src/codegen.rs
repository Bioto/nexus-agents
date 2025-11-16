use serde_json::{json, Value};

/// Error type for code generation
#[derive(Debug)]
pub enum CodegenError {
    HttpError(String),
    ParseError(String),
    ServerError(String),
}

impl std::fmt::Display for CodegenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CodegenError::HttpError(msg) => write!(f, "HTTP error: {}", msg),
            CodegenError::ParseError(msg) => write!(f, "Parse error: {}", msg),
            CodegenError::ServerError(msg) => write!(f, "Server error: {}", msg),
        }
    }
}

impl std::error::Error for CodegenError {}

/// Generate Python code API for MCP tools
pub struct CodeGenerator {
    server_url: String,
    client: reqwest::Client,
}

impl CodeGenerator {
    /// Create a new code generator
    pub fn new(server_url: impl Into<String>) -> Self {
        Self {
            server_url: server_url.into(),
            client: reqwest::Client::new(),
        }
    }

    /// Fetch tool definitions from MCP server
    async fn fetch_tools(&self) -> Result<Vec<ToolDefinition>, CodegenError> {
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
                CodegenError::HttpError(format!("Failed to connect to MCP server: {}", e))
            })?;

        if !init_response.status().is_success() {
            return Err(CodegenError::HttpError(format!(
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
            CodegenError::ParseError(format!("Failed to read init response: {}", e))
        })?;

        let init_json = self.parse_sse_response(&init_text)?;

        if let Some(error) = init_json.get("error") {
            return Err(CodegenError::ServerError(format!(
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
            .map_err(|e| CodegenError::HttpError(format!("Failed to request tools: {}", e)))?;

        if !tools_response.status().is_success() {
            return Err(CodegenError::HttpError(format!(
                "MCP server returned error: {}",
                tools_response.status()
            )));
        }

        // Parse SSE format response
        let tools_text = tools_response.text().await.map_err(|e| {
            CodegenError::ParseError(format!("Failed to read tools response: {}", e))
        })?;

        let tools_json = self.parse_sse_response(&tools_text)?;

        // Handle JSON-RPC response
        if let Some(error) = tools_json.get("error") {
            return Err(CodegenError::ServerError(format!(
                "MCP server error: {}",
                error
            )));
        }

        let result = tools_json
            .get("result")
            .ok_or_else(|| CodegenError::ParseError("Missing 'result' in response".to_string()))?;

        let tools = result
            .get("tools")
            .and_then(|t| t.as_array())
            .ok_or_else(|| {
                CodegenError::ParseError("Missing 'tools' array in result".to_string())
            })?;

        let mut tool_defs = Vec::new();
        for tool in tools {
            tool_defs.push(self.parse_tool_definition(tool)?);
        }

        Ok(tool_defs)
    }

    /// Parse a single tool definition from JSON
    fn parse_tool_definition(&self, tool: &Value) -> Result<ToolDefinition, CodegenError> {
        let name = tool
            .get("name")
            .and_then(|n| n.as_str())
            .ok_or_else(|| CodegenError::ParseError("Missing 'name' in tool".to_string()))?
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

    /// Generate Python code for all tools
    pub async fn generate_code(&self) -> Result<String, CodegenError> {
        let tools = self.fetch_tools().await?;
        self.generate_python_code(&tools)
    }

    /// Generate Python code files in directory structure
    pub async fn generate_code_files(
        &self,
        output_dir: &std::path::Path,
    ) -> Result<(), CodegenError> {
        let tools = self.fetch_tools().await?;

        // Create server directory (e.g., servers/nexus-mcp-server)
        let server_name = "nexus-mcp-server";
        let server_dir = output_dir.join(server_name);
        std::fs::create_dir_all(&server_dir)
            .map_err(|e| CodegenError::ParseError(format!("Failed to create directory: {}", e)))?;

        // Generate shared MCP client
        let client_code = self.generate_mcp_client_code()?;
        let client_path = output_dir.join("_mcp_client.py");
        std::fs::write(&client_path, client_code)
            .map_err(|e| CodegenError::ParseError(format!("Failed to write client file: {}", e)))?;

        // Generate individual tool files
        let mut tool_exports = Vec::new();
        for tool in &tools {
            let tool_code = self.generate_tool_file_code(tool)?;
            let function_name = self.to_snake_case(&tool.name);
            let file_name = format!("{}.py", function_name);
            let file_path = server_dir.join(&file_name);

            std::fs::write(&file_path, tool_code).map_err(|e| {
                CodegenError::ParseError(format!("Failed to write tool file: {}", e))
            })?;

            tool_exports.push((function_name, tool.name.clone()));
        }

        // Generate index.py that re-exports all tools
        let index_code = self.generate_index_code(&tool_exports)?;
        let index_path = server_dir.join("index.py");
        std::fs::write(&index_path, index_code)
            .map_err(|e| CodegenError::ParseError(format!("Failed to write index file: {}", e)))?;

        // Generate __init__.py
        let init_code = self.generate_init_code(&tool_exports)?;
        let init_path = server_dir.join("__init__.py");
        std::fs::write(&init_path, init_code).map_err(|e| {
            CodegenError::ParseError(format!("Failed to write __init__ file: {}", e))
        })?;

        Ok(())
    }

    /// Generate Python code from tool definitions (legacy single-file method)
    fn generate_python_code(&self, tools: &[ToolDefinition]) -> Result<String, CodegenError> {
        let mut code = String::new();

        // Header with uv inline dependency
        code.push_str("# uv: dependencies = [\"httpx\"]\n\n");
        code.push_str("\"\"\"\n");
        code.push_str("MCP Tools API for nexus-mcp-server\n");
        code.push_str("Generated code - do not edit manually\n");
        code.push_str("\"\"\"\n\n");

        // Imports
        code.push_str("# === MCP Client Implementation ===\n");
        code.push_str("import httpx\n");
        code.push_str("import json\n");
        code.push_str("from typing import Any, Dict, Optional, TypedDict\n\n");

        // MCP Client
        // Normalize server URL - remove /mcp if present since we'll append it
        let base_url = if self.server_url.ends_with("/mcp") {
            self.server_url.trim_end_matches("/mcp")
        } else {
            &self.server_url
        };
        code.push_str(&format!("MCP_SERVER_URL = \"{}\"\n\n", base_url));
        code.push_str(
            "async def call_mcp_tool(tool_name: str, params: Dict[str, Any]) -> Dict[str, Any]:\n",
        );
        code.push_str("    \"\"\"Call an MCP tool via HTTP transport\"\"\"\n");
        code.push_str("    request = {\n");
        code.push_str("        \"jsonrpc\": \"2.0\",\n");
        code.push_str("        \"id\": 1,\n");
        code.push_str("        \"method\": \"tools/call\",\n");
        code.push_str("        \"params\": {\n");
        code.push_str("            \"name\": tool_name,\n");
        code.push_str("            \"arguments\": params\n");
        code.push_str("        }\n");
        code.push_str("    }\n");
        code.push_str("    async with httpx.AsyncClient() as client:\n");
        code.push_str("        response = await client.post(\n");
        code.push_str("            f\"{MCP_SERVER_URL}/mcp\",\n");
        code.push_str("            json=request,\n");
        code.push_str("            headers={\n");
        code.push_str("                \"Accept\": \"application/json, text/event-stream\",\n");
        code.push_str("                \"Content-Type\": \"application/json\"\n");
        code.push_str("            }\n");
        code.push_str("        )\n");
        code.push_str("        response.raise_for_status()\n");
        code.push_str("        result = response.json()\n");
        code.push_str("        if \"error\" in result:\n");
        code.push_str("            raise Exception(f\"MCP tool error: {result['error']}\")\n");
        code.push_str("        return result.get(\"result\", {})\n\n");

        // Tool definitions
        code.push_str("# === Tool Definitions ===\n\n");

        for tool in tools {
            code.push_str(&self.generate_tool_code(tool)?);
            code.push_str("\n\n");
        }

        Ok(code)
    }

    /// Generate MCP client code (shared across all tool files)
    fn generate_mcp_client_code(&self) -> Result<String, CodegenError> {
        let mut code = String::new();

        code.push_str("# uv: dependencies = [\"httpx\"]\n\n");
        code.push_str("\"\"\"\n");
        code.push_str("MCP Client - shared client for calling MCP tools via HTTP transport\n");
        code.push_str("Generated code - do not edit manually\n");
        code.push_str("\"\"\"\n\n");

        code.push_str("import httpx\n");
        code.push_str("from typing import Any, Dict\n\n");

        // Normalize server URL
        let base_url = if self.server_url.ends_with("/mcp") {
            self.server_url.trim_end_matches("/mcp")
        } else {
            &self.server_url
        };
        code.push_str(&format!("MCP_SERVER_URL = \"{}\"\n\n", base_url));

        code.push_str(
            "async def call_mcp_tool(tool_name: str, params: Dict[str, Any]) -> Dict[str, Any]:\n",
        );
        code.push_str("    \"\"\"Call an MCP tool via HTTP transport\"\"\"\n");
        code.push_str("    request = {\n");
        code.push_str("        \"jsonrpc\": \"2.0\",\n");
        code.push_str("        \"id\": 1,\n");
        code.push_str("        \"method\": \"tools/call\",\n");
        code.push_str("        \"params\": {\n");
        code.push_str("            \"name\": tool_name,\n");
        code.push_str("            \"arguments\": params\n");
        code.push_str("        }\n");
        code.push_str("    }\n");
        code.push_str("    async with httpx.AsyncClient() as client:\n");
        code.push_str("        response = await client.post(\n");
        code.push_str("            f\"{MCP_SERVER_URL}/mcp\",\n");
        code.push_str("            json=request,\n");
        code.push_str("            headers={\n");
        code.push_str("                \"Accept\": \"application/json, text/event-stream\",\n");
        code.push_str("                \"Content-Type\": \"application/json\"\n");
        code.push_str("            }\n");
        code.push_str("        )\n");
        code.push_str("        response.raise_for_status()\n");
        code.push_str("        result = response.json()\n");
        code.push_str("        if \"error\" in result:\n");
        code.push_str("            raise Exception(f\"MCP tool error: {result['error']}\")\n");
        code.push_str("        return result.get(\"result\", {})\n");

        Ok(code)
    }

    /// Generate a single tool file (self-contained with inline MCP client)
    fn generate_tool_file_code(&self, tool: &ToolDefinition) -> Result<String, CodegenError> {
        let mut code = String::new();

        code.push_str("# uv: dependencies = [\"httpx\"]\n\n");
        code.push_str("\"\"\"\n");
        code.push_str(&format!("{} - {}\n", tool.name, tool.description));
        code.push_str("Generated code - do not edit manually\n");
        code.push_str("This file is self-contained and can be executed independently.\n");
        code.push_str("\"\"\"\n\n");

        code.push_str("from typing import Any, Dict, Optional, TypedDict\n");
        code.push_str("import httpx\n\n");

        // Include MCP client code inline
        code.push_str("# === MCP Client Implementation (inline) ===\n");
        let base_url = if self.server_url.ends_with("/mcp") {
            self.server_url.trim_end_matches("/mcp")
        } else {
            &self.server_url
        };
        code.push_str(&format!("MCP_SERVER_URL = \"{}\"\n\n", base_url));

        code.push_str(
            "async def call_mcp_tool(tool_name: str, params: Dict[str, Any]) -> Dict[str, Any]:\n",
        );
        code.push_str("    \"\"\"Call an MCP tool via HTTP transport\"\"\"\n");
        code.push_str("    request = {\n");
        code.push_str("        \"jsonrpc\": \"2.0\",\n");
        code.push_str("        \"id\": 1,\n");
        code.push_str("        \"method\": \"tools/call\",\n");
        code.push_str("        \"params\": {\n");
        code.push_str("            \"name\": tool_name,\n");
        code.push_str("            \"arguments\": params\n");
        code.push_str("        }\n");
        code.push_str("    }\n");
        code.push_str("    async with httpx.AsyncClient() as client:\n");
        code.push_str("        response = await client.post(\n");
        code.push_str("            f\"{MCP_SERVER_URL}/mcp\",\n");
        code.push_str("            json=request,\n");
        code.push_str("            headers={\n");
        code.push_str("                \"Accept\": \"application/json, text/event-stream\",\n");
        code.push_str("                \"Content-Type\": \"application/json\"\n");
        code.push_str("            }\n");
        code.push_str("        )\n");
        code.push_str("        response.raise_for_status()\n");
        code.push_str("        result = response.json()\n");
        code.push_str("        if \"error\" in result:\n");
        code.push_str("            raise Exception(f\"MCP tool error: {result['error']}\")\n");
        code.push_str("        return result.get(\"result\", {})\n\n");
        code.push_str("# === Tool Definition ===\n\n");

        // Parse input schema to generate TypedDict (only if tool has parameters)
        let has_params = tool
            .input_schema
            .get("properties")
            .and_then(|p| p.as_object())
            .map(|obj| !obj.is_empty())
            .unwrap_or(false);

        code.push_str(&format!("# Tool: {}\n", tool.name));

        if has_params {
            let input_type = self.schema_to_typed_dict(
                &tool.input_schema,
                &format!("{}Input", self.to_pascal_case(&tool.name)),
            )?;
            code.push_str(&input_type);
            code.push_str("\n\n");
        }

        // Function docstring
        if !tool.description.is_empty() {
            code.push_str(&format!("\"\"\"{}\"\"\"\n", tool.description));
        }

        // Function signature and body
        let function_name = self.to_snake_case(&tool.name);

        if has_params {
            let input_type_name = format!("{}Input", self.to_pascal_case(&tool.name));
            code.push_str(&format!(
                "async def {}(input: {}) -> Dict[str, Any]:\n",
                function_name, input_type_name
            ));
            code.push_str(&format!(
                "    return await call_mcp_tool(\"{}\", input)\n",
                tool.name
            ));
        } else {
            code.push_str(&format!(
                "async def {}() -> Dict[str, Any]:\n",
                function_name
            ));
            code.push_str(&format!(
                "    return await call_mcp_tool(\"{}\", {{}})\n",
                tool.name
            ));
        }

        Ok(code)
    }

    /// Generate index.py that re-exports all tools
    fn generate_index_code(
        &self,
        tool_exports: &[(String, String)],
    ) -> Result<String, CodegenError> {
        let mut code = String::new();

        code.push_str("\"\"\"\n");
        code.push_str("Index file - re-exports all tools from this server\n");
        code.push_str("Generated code - do not edit manually\n");
        code.push_str("\"\"\"\n\n");

        // Import all tools
        for (function_name, _) in tool_exports {
            code.push_str(&format!(
                "from .{} import {}\n",
                function_name, function_name
            ));
        }

        code.push_str("\n");
        code.push_str("__all__ = [\n");
        for (function_name, _) in tool_exports {
            code.push_str(&format!("    \"{}\",\n", function_name));
        }
        code.push_str("]\n");

        Ok(code)
    }

    /// Generate __init__.py
    fn generate_init_code(
        &self,
        tool_exports: &[(String, String)],
    ) -> Result<String, CodegenError> {
        let mut code = String::new();

        code.push_str("\"\"\"\n");
        code.push_str("Nexus MCP Server tools\n");
        code.push_str("Generated code - do not edit manually\n");
        code.push_str("\"\"\"\n\n");

        // Import all tools
        for (function_name, _) in tool_exports {
            code.push_str(&format!(
                "from .{} import {}\n",
                function_name, function_name
            ));
        }

        code.push_str("\n");
        code.push_str("__all__ = [\n");
        for (function_name, _) in tool_exports {
            code.push_str(&format!("    \"{}\",\n", function_name));
        }
        code.push_str("]\n");

        Ok(code)
    }

    /// Generate Python code for a single tool (for legacy single-file method)
    fn generate_tool_code(&self, tool: &ToolDefinition) -> Result<String, CodegenError> {
        let mut code = String::new();

        // Parse input schema to generate TypedDict (only if tool has parameters)
        let has_params = tool
            .input_schema
            .get("properties")
            .and_then(|p| p.as_object())
            .map(|obj| !obj.is_empty())
            .unwrap_or(false);

        code.push_str(&format!("# Tool: {}\n", tool.name));

        if has_params {
            let input_type = self.schema_to_typed_dict(
                &tool.input_schema,
                &format!("{}Input", self.to_pascal_case(&tool.name)),
            )?;
            code.push_str(&input_type);
            code.push_str("\n\n");
        }

        // Function docstring
        if !tool.description.is_empty() {
            code.push_str(&format!("\"\"\"{}\"\"\"\n", tool.description));
        }

        // Function signature and body
        let function_name = self.to_snake_case(&tool.name);

        // Check if the tool has any parameters
        let has_params = tool
            .input_schema
            .get("properties")
            .and_then(|p| p.as_object())
            .map(|obj| !obj.is_empty())
            .unwrap_or(false);

        if has_params {
            let input_type_name = format!("{}Input", self.to_pascal_case(&tool.name));
            code.push_str(&format!(
                "async def {}(input: {}) -> Dict[str, Any]:\n",
                function_name, input_type_name
            ));
            code.push_str(&format!(
                "    return await call_mcp_tool(\"{}\", input)\n",
                tool.name
            ));
        } else {
            code.push_str(&format!(
                "async def {}() -> Dict[str, Any]:\n",
                function_name
            ));
            code.push_str(&format!(
                "    return await call_mcp_tool(\"{}\", {{}})\n",
                tool.name
            ));
        }

        Ok(code)
    }

    /// Convert JSON Schema to Python TypedDict
    fn schema_to_typed_dict(
        &self,
        schema: &Value,
        type_name: &str,
    ) -> Result<String, CodegenError> {
        // Handle empty schema or missing properties
        let properties = schema.get("properties").and_then(|p| p.as_object());

        if properties.is_none() {
            return Ok(format!("class {}(TypedDict):\n    pass\n", type_name));
        }

        let properties = properties.unwrap();
        let required = schema
            .get("required")
            .and_then(|r| r.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .map(|s| s.to_string())
                    .collect::<std::collections::HashSet<_>>()
            })
            .unwrap_or_default();

        let mut fields = Vec::new();
        for (name, prop) in properties {
            let python_type = self.json_type_to_python(prop)?;
            let is_required = required.contains(name);

            if is_required {
                fields.push(format!("    {}: {}", name, python_type));
            } else {
                fields.push(format!("    {}: Optional[{}]", name, python_type));
            }
        }

        let mut code = format!("class {}(TypedDict):\n", type_name);
        if fields.is_empty() {
            code.push_str("    pass\n");
        } else {
            code.push_str(&fields.join("\n"));
        }

        Ok(code)
    }

    /// Convert JSON Schema type to Python type
    fn json_type_to_python(&self, prop: &Value) -> Result<String, CodegenError> {
        let type_str = prop
            .get("type")
            .and_then(|t| t.as_str())
            .unwrap_or("string");

        Ok(match type_str {
            "string" => "str".to_string(),
            "number" | "integer" => {
                // Check for integer specifically
                if type_str == "integer"
                    || prop.get("type").and_then(|t| t.as_str()) == Some("integer")
                {
                    "int".to_string()
                } else {
                    "float".to_string()
                }
            }
            "boolean" => "bool".to_string(),
            "array" => {
                let items = prop.get("items");
                if let Some(items) = items {
                    let item_type = self.json_type_to_python(items)?;
                    format!("list[{}]", item_type)
                } else {
                    "list[Any]".to_string()
                }
            }
            "object" => "Dict[str, Any]".to_string(),
            _ => "Any".to_string(),
        })
    }

    /// Convert to PascalCase
    fn to_pascal_case(&self, s: &str) -> String {
        let mut result = String::new();
        let mut capitalize = true;
        for c in s.chars() {
            if c == '_' || c == '-' {
                capitalize = true;
            } else if capitalize {
                result.push(c.to_uppercase().next().unwrap_or(c));
                capitalize = false;
            } else {
                result.push(c);
            }
        }
        result
    }

    /// Convert to snake_case
    fn to_snake_case(&self, s: &str) -> String {
        let mut result = String::new();
        for (i, c) in s.chars().enumerate() {
            if c.is_uppercase() && i > 0 {
                result.push('_');
            }
            result.push(c.to_lowercase().next().unwrap_or(c));
        }
        result
    }

    /// Parse SSE (Server-Sent Events) format response
    /// Looks for lines starting with "data: " and extracts JSON
    fn parse_sse_response(&self, text: &str) -> Result<Value, CodegenError> {
        for line in text.lines() {
            let line = line.trim();
            if let Some(json_str) = line.strip_prefix("data: ") {
                return serde_json::from_str(json_str).map_err(|e| {
                    CodegenError::ParseError(format!("Failed to parse SSE JSON: {}", e))
                });
            }
        }
        Err(CodegenError::ParseError(
            "No 'data: ' line found in SSE response".to_string(),
        ))
    }
}

/// Tool definition structure
#[derive(Debug, Clone)]
struct ToolDefinition {
    name: String,
    description: String,
    input_schema: Value,
}
