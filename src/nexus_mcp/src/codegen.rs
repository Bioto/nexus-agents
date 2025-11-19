use crate::mcp_client::{McpClient, ToolDefinition};
use crate::schema::SchemaConverter;

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

impl From<crate::mcp_client::McpClientError> for CodegenError {
    fn from(err: crate::mcp_client::McpClientError) -> Self {
        match err {
            crate::mcp_client::McpClientError::HttpError(msg) => CodegenError::HttpError(msg),
            crate::mcp_client::McpClientError::ParseError(msg) => CodegenError::ParseError(msg),
            crate::mcp_client::McpClientError::ServerError(msg) => CodegenError::ServerError(msg),
        }
    }
}

impl From<crate::schema::SchemaError> for CodegenError {
    fn from(err: crate::schema::SchemaError) -> Self {
        match err {
            crate::schema::SchemaError::ParseError(msg) => CodegenError::ParseError(msg),
        }
    }
}

/// Generate Python code API for MCP tools
pub struct CodeGenerator {
    server_url: String,
    mcp_client: McpClient,
    headers: Option<std::collections::HashMap<String, String>>,
    server_name: String,
}

impl CodeGenerator {
    /// Create a new code generator
    pub fn new(server_url: impl Into<String>) -> Self {
        let server_url = server_url.into();
        Self {
            server_url: server_url.clone(),
            mcp_client: McpClient::new(server_url),
            headers: None,
            server_name: "nexus-mcp-server".to_string(),
        }
    }

    /// Create a new code generator with custom headers and server name
    pub fn with_config(
        server_url: impl Into<String>,
        server_name: impl Into<String>,
        headers: Option<std::collections::HashMap<String, String>>,
    ) -> Self {
        let server_url = server_url.into();
        let headers_clone = headers.clone();
        let mcp_client = if let Some(ref h) = headers_clone {
            McpClient::with_headers(server_url.clone(), h.clone())
        } else {
            McpClient::new(server_url.clone())
        };
        Self {
            server_url,
            mcp_client,
            headers,
            server_name: server_name.into(),
        }
    }

    /// Generate Python code files in directory structure
    pub async fn generate_code_files(
        &self,
        output_dir: &std::path::Path,
    ) -> Result<(), CodegenError> {
        let tools = self.mcp_client.fetch_tools().await?;

        // Create server directory (e.g., servers/context7)
        let server_dir = output_dir.join(&self.server_name);
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

    /// Generate MCP client code (shared across all tool files)
    fn generate_mcp_client_code(&self) -> Result<String, CodegenError> {
        let mut code = String::new();

        code.push_str("# uv: dependencies = [\"httpx\"]\n\n");
        code.push_str("\"\"\"\n");
        code.push_str("MCP Client - shared client for calling MCP tools via HTTP transport\n");
        code.push_str("Generated code - do not edit manually\n");
        code.push_str("\"\"\"\n\n");

        code.push_str("import httpx\n");
        code.push_str("import os\n");
        code.push_str("from typing import Any, Dict, Optional\n\n");

        // Normalize server URL
        let base_url = if self.server_url.ends_with("/mcp") {
            self.server_url.trim_end_matches("/mcp")
        } else {
            &self.server_url
        };
        code.push_str(&format!(
            "MCP_SERVER_URL = os.getenv(\"MCP_SERVER_URL\", \"{}\")\n\n",
            base_url
        ));

        code.push_str("# Session state for MCP initialization\n");
        code.push_str("_mcp_session_id: Optional[str] = None\n");
        code.push_str("_mcp_initialized = False\n\n");

        code.push_str("async def _ensure_mcp_initialized(client: httpx.AsyncClient) -> str:\n");
        code.push_str("    \"\"\"Initialize MCP session if not already initialized\"\"\"\n");
        code.push_str("    global _mcp_session_id, _mcp_initialized\n");
        code.push_str("    if _mcp_initialized and _mcp_session_id:\n");
        code.push_str("        return _mcp_session_id\n");
        code.push_str("    \n");
        code.push_str("    # Step 1: Initialize the MCP session\n");
        code.push_str("    init_request = {\n");
        code.push_str("        \"jsonrpc\": \"2.0\",\n");
        code.push_str("        \"id\": 1,\n");
        code.push_str("        \"method\": \"initialize\",\n");
        code.push_str("        \"params\": {\n");
        code.push_str("            \"protocolVersion\": \"2024-11-05\",\n");
        code.push_str("            \"capabilities\": {},\n");
        code.push_str("            \"clientInfo\": {\n");
        code.push_str("                \"name\": \"nexus-mcp-python-client\",\n");
        code.push_str("                \"version\": \"0.1.0\"\n");
        code.push_str("            }\n");
        code.push_str("        }\n");
        code.push_str("    }\n");
        code.push_str("    \n");
        code.push_str("    init_response = await client.post(\n");
        code.push_str("        f\"{MCP_SERVER_URL}/mcp\",\n");
        code.push_str("        json=init_request,\n");
        code.push_str("        headers={\n");
        code.push_str("            \"Accept\": \"application/json, text/event-stream\",\n");
        code.push_str("            \"Content-Type\": \"application/json\"\n");
        code.push_str("        }\n");
        code.push_str("    )\n");
        code.push_str("    init_response.raise_for_status()\n");
        code.push_str("    \n");
        code.push_str("    # Extract session ID from response headers\n");
        code.push_str("    _mcp_session_id = init_response.headers.get(\"mcp-session-id\")\n");
        code.push_str("    \n");
        code.push_str("    # Parse SSE format response (data: {...})\n");
        code.push_str("    init_text = init_response.text\n");
        code.push_str("    if isinstance(init_text, str):\n");
        code.push_str("        # Extract JSON from SSE format: data: {...}\n");
        code.push_str("        for line in init_text.split('\\n'):\n");
        code.push_str("            if line.startswith('data: '):\n");
        code.push_str("                import json\n");
        code.push_str("                data_json = json.loads(line[6:])  # Skip 'data: '\n");
        code.push_str("                if 'error' in data_json:\n");
        code.push_str("                    raise Exception(f\"MCP initialization error: {data_json['error']}\")\n");
        code.push_str("                break\n");
        code.push_str("    else:\n");
        code.push_str("        # Try to parse as JSON directly\n");
        code.push_str("        try:\n");
        code.push_str("            import json\n");
        code.push_str("            data_json = init_response.json()\n");
        code.push_str("            if 'error' in data_json:\n");
        code.push_str("                raise Exception(f\"MCP initialization error: {data_json['error']}\")\n");
        code.push_str("        except:\n");
        code.push_str("            pass\n");
        code.push_str("    \n");
        code.push_str("    # Step 2: Send initialized notification\n");
        code.push_str("    initialized_notification = {\n");
        code.push_str("        \"jsonrpc\": \"2.0\",\n");
        code.push_str("        \"method\": \"notifications/initialized\"\n");
        code.push_str("    }\n");
        code.push_str("    \n");
        code.push_str("    initialized_headers = {\n");
        code.push_str("        \"Accept\": \"application/json, text/event-stream\",\n");
        code.push_str("        \"Content-Type\": \"application/json\"\n");
        code.push_str("    }\n");
        code.push_str("    if _mcp_session_id:\n");
        code.push_str("        initialized_headers[\"mcp-session-id\"] = _mcp_session_id\n");
        code.push_str("    \n");
        code.push_str("    await client.post(\n");
        code.push_str("        f\"{MCP_SERVER_URL}/mcp\",\n");
        code.push_str("        json=initialized_notification,\n");
        code.push_str("        headers=initialized_headers\n");
        code.push_str("    )\n");
        code.push_str("    \n");
        code.push_str("    _mcp_initialized = True\n");
        code.push_str("    return _mcp_session_id or \"\"\n\n");

        code.push_str(
            "async def call_mcp_tool(tool_name: str, params: Dict[str, Any]) -> Dict[str, Any]:\n",
        );
        code.push_str("    \"\"\"Call an MCP tool via HTTP transport\"\"\"\n");
        code.push_str("    async with httpx.AsyncClient() as client:\n");
        code.push_str("        # Ensure MCP session is initialized\n");
        code.push_str("        session_id = await _ensure_mcp_initialized(client)\n");
        code.push_str("        \n");
        code.push_str("        # Make tool call request\n");
        code.push_str("        request = {\n");
        code.push_str("            \"jsonrpc\": \"2.0\",\n");
        code.push_str("            \"id\": 1,\n");
        code.push_str("            \"method\": \"tools/call\",\n");
        code.push_str("            \"params\": {\n");
        code.push_str("                \"name\": tool_name,\n");
        code.push_str("                \"arguments\": params\n");
        code.push_str("            }\n");
        code.push_str("        }\n");
        code.push_str("        \n");
        code.push_str("        headers = {\n");
        code.push_str("            \"Accept\": \"application/json, text/event-stream\",\n");
        code.push_str("            \"Content-Type\": \"application/json\"\n");
        code.push_str("        }\n");
        code.push_str("        if session_id:\n");
        code.push_str("            headers[\"mcp-session-id\"] = session_id\n");
        code.push_str("        \n");
        code.push_str("        response = await client.post(\n");
        code.push_str("            f\"{MCP_SERVER_URL}/mcp\",\n");
        code.push_str("            json=request,\n");
        code.push_str("            headers=headers\n");
        code.push_str("        )\n");
        code.push_str("        response.raise_for_status()\n");
        code.push_str("        \n");
        code.push_str("        # Parse SSE format response (data: {...})\n");
        code.push_str("        import json\n");
        code.push_str("        response_text = response.text\n");
        code.push_str("        # Extract JSON from SSE format: data: {...}\n");
        code.push_str("        for line in response_text.split('\\n'):\n");
        code.push_str("            line = line.strip()\n");
        code.push_str("            if line.startswith('data: '):\n");
        code.push_str("                result = json.loads(line[6:])  # Skip 'data: '\n");
        code.push_str("                if \"error\" in result:\n");
        code.push_str("                    raise Exception(f\"MCP tool error: {result['error']}\")\n");
        code.push_str("                return result.get(\"result\", {})\n");
        code.push_str("        # Fallback: try to parse as JSON directly\n");
        code.push_str("        try:\n");
        code.push_str("            result = response.json()\n");
        code.push_str("            if \"error\" in result:\n");
        code.push_str("                raise Exception(f\"MCP tool error: {result['error']}\")\n");
        code.push_str("            return result.get(\"result\", {})\n");
        code.push_str("        except:\n");
        code.push_str("            raise Exception(f\"Failed to parse MCP response: {response_text[:200]}\")\n");

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
        code.push_str("import os\n");
        code.push_str("import re\n");
        let base_url = if self.server_url.ends_with("/mcp") {
            self.server_url.trim_end_matches("/mcp")
        } else {
            &self.server_url
        };
        code.push_str(&format!(
            "MCP_SERVER_URL = os.getenv(\"MCP_SERVER_URL\", \"{}\")\n\n",
            base_url
        ));
        
        // Generate custom headers from environment variables if configured
        if let Some(ref headers) = self.headers {
            code.push_str("# Custom headers from configuration\n");
            code.push_str("def get_custom_headers() -> Dict[str, str]:\n");
            code.push_str("    headers = {}\n");
            for (key, default_value) in headers {
                // Convert header key to env var name (e.g., CONTEXT7_API_KEY -> CONTEXT7_API_KEY)
                let env_var = key.replace("-", "_").to_uppercase();
                code.push_str(&format!(
                    "    headers[\"{}\"] = os.getenv(\"{}\", \"{}\")\n",
                    key, env_var, default_value
                ));
            }
            code.push_str("    return headers\n\n");
        }

        code.push_str("# Session state for MCP initialization\n");
        code.push_str("_mcp_session_id: Optional[str] = None\n");
        code.push_str("_mcp_initialized = False\n\n");

        code.push_str("async def _ensure_mcp_initialized(client: httpx.AsyncClient) -> str:\n");
        code.push_str("    \"\"\"Initialize MCP session if not already initialized\"\"\"\n");
        code.push_str("    global _mcp_session_id, _mcp_initialized\n");
        code.push_str("    if _mcp_initialized and _mcp_session_id:\n");
        code.push_str("        return _mcp_session_id\n");
        code.push_str("    \n");
        code.push_str("    # Step 1: Initialize the MCP session\n");
        code.push_str("    init_request = {\n");
        code.push_str("        \"jsonrpc\": \"2.0\",\n");
        code.push_str("        \"id\": 1,\n");
        code.push_str("        \"method\": \"initialize\",\n");
        code.push_str("        \"params\": {\n");
        code.push_str("            \"protocolVersion\": \"2024-11-05\",\n");
        code.push_str("            \"capabilities\": {},\n");
        code.push_str("            \"clientInfo\": {\n");
        code.push_str("                \"name\": \"nexus-mcp-python-client\",\n");
        code.push_str("                \"version\": \"0.1.0\"\n");
        code.push_str("            }\n");
        code.push_str("        }\n");
        code.push_str("    }\n");
        code.push_str("    \n");
        code.push_str("    init_response = await client.post(\n");
        code.push_str("        f\"{MCP_SERVER_URL}/mcp\",\n");
        code.push_str("        json=init_request,\n");
        code.push_str("        headers={\n");
        code.push_str("            \"Accept\": \"application/json, text/event-stream\",\n");
        code.push_str("            \"Content-Type\": \"application/json\"\n");
        code.push_str("        }\n");
        code.push_str("    )\n");
        code.push_str("    init_response.raise_for_status()\n");
        code.push_str("    \n");
        code.push_str("    # Extract session ID from response headers\n");
        code.push_str("    _mcp_session_id = init_response.headers.get(\"mcp-session-id\")\n");
        code.push_str("    \n");
        code.push_str("    # Parse SSE format response (data: {...})\n");
        code.push_str("    init_text = init_response.text\n");
        code.push_str("    if isinstance(init_text, str):\n");
        code.push_str("        # Extract JSON from SSE format: data: {...}\n");
        code.push_str("        for line in init_text.split('\\n'):\n");
        code.push_str("            if line.startswith('data: '):\n");
        code.push_str("                import json\n");
        code.push_str("                data_json = json.loads(line[6:])  # Skip 'data: '\n");
        code.push_str("                if 'error' in data_json:\n");
        code.push_str("                    raise Exception(f\"MCP initialization error: {data_json['error']}\")\n");
        code.push_str("                break\n");
        code.push_str("    else:\n");
        code.push_str("        # Try to parse as JSON directly\n");
        code.push_str("        try:\n");
        code.push_str("            import json\n");
        code.push_str("            data_json = init_response.json()\n");
        code.push_str("            if 'error' in data_json:\n");
        code.push_str("                raise Exception(f\"MCP initialization error: {data_json['error']}\")\n");
        code.push_str("        except:\n");
        code.push_str("            pass\n");
        code.push_str("    \n");
        code.push_str("    # Step 2: Send initialized notification\n");
        code.push_str("    initialized_notification = {\n");
        code.push_str("        \"jsonrpc\": \"2.0\",\n");
        code.push_str("        \"method\": \"notifications/initialized\"\n");
        code.push_str("    }\n");
        code.push_str("    \n");
        code.push_str("    initialized_headers = {\n");
        code.push_str("        \"Accept\": \"application/json, text/event-stream\",\n");
        code.push_str("        \"Content-Type\": \"application/json\"\n");
        code.push_str("    }\n");
        code.push_str("    if _mcp_session_id:\n");
        code.push_str("        initialized_headers[\"mcp-session-id\"] = _mcp_session_id\n");
        code.push_str("    \n");
        code.push_str("    await client.post(\n");
        code.push_str("        f\"{MCP_SERVER_URL}/mcp\",\n");
        code.push_str("        json=initialized_notification,\n");
        code.push_str("        headers=initialized_headers\n");
        code.push_str("    )\n");
        code.push_str("    \n");
        code.push_str("    _mcp_initialized = True\n");
        code.push_str("    return _mcp_session_id or \"\"\n\n");

        code.push_str(
            "async def call_mcp_tool(tool_name: str, params: Dict[str, Any]) -> Dict[str, Any]:\n",
        );
        code.push_str("    \"\"\"Call an MCP tool via HTTP transport\"\"\"\n");
        code.push_str("    async with httpx.AsyncClient() as client:\n");
        code.push_str("        # Ensure MCP session is initialized\n");
        code.push_str("        session_id = await _ensure_mcp_initialized(client)\n");
        code.push_str("        \n");
        code.push_str("        # Make tool call request\n");
        code.push_str("        request = {\n");
        code.push_str("            \"jsonrpc\": \"2.0\",\n");
        code.push_str("            \"id\": 1,\n");
        code.push_str("            \"method\": \"tools/call\",\n");
        code.push_str("            \"params\": {\n");
        code.push_str("                \"name\": tool_name,\n");
        code.push_str("                \"arguments\": params\n");
        code.push_str("            }\n");
        code.push_str("        }\n");
        code.push_str("        \n");
        code.push_str("        headers = {\n");
        code.push_str("            \"Accept\": \"application/json, text/event-stream\",\n");
        code.push_str("            \"Content-Type\": \"application/json\"\n");
        code.push_str("        }\n");
        if let Some(_) = self.headers {
            code.push_str("        headers.update(get_custom_headers())\n");
        }
        code.push_str("        if session_id:\n");
        code.push_str("            headers[\"mcp-session-id\"] = session_id\n");
        code.push_str("        \n");
        code.push_str("        response = await client.post(\n");
        code.push_str("            f\"{MCP_SERVER_URL}/mcp\",\n");
        code.push_str("            json=request,\n");
        code.push_str("            headers=headers\n");
        code.push_str("        )\n");
        code.push_str("        response.raise_for_status()\n");
        code.push_str("        \n");
        code.push_str("        # Parse SSE format response (data: {...})\n");
        code.push_str("        response_text = response.text\n");
        code.push_str("        if isinstance(response_text, str):\n");
        code.push_str("            import json\n");
        code.push_str("            # Extract JSON from SSE format: data: {...}\n");
        code.push_str("            for line in response_text.split('\\n'):\n");
        code.push_str("                if line.startswith('data: '):\n");
        code.push_str("                    result = json.loads(line[6:])  # Skip 'data: '\n");
        code.push_str("                    if \"error\" in result:\n");
        code.push_str("                        raise Exception(f\"MCP tool error: {result['error']}\")\n");
        code.push_str("                    return result.get(\"result\", {})\n");
        code.push_str("            # Fallback: try to parse as JSON directly\n");
        code.push_str("            try:\n");
        code.push_str("                result = response.json()\n");
        code.push_str("                if \"error\" in result:\n");
        code.push_str("                    raise Exception(f\"MCP tool error: {result['error']}\")\n");
        code.push_str("                return result.get(\"result\", {})\n");
        code.push_str("            except:\n");
        code.push_str("                raise Exception(f\"Failed to parse MCP response: {response_text[:200]}\")\n");
        code.push_str("        else:\n");
        code.push_str("            result = response.json()\n");
        code.push_str("            if \"error\" in result:\n");
        code.push_str("                raise Exception(f\"MCP tool error: {result['error']}\")\n");
        code.push_str("            return result.get(\"result\", {})\n\n");
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
            let input_type = SchemaConverter::schema_to_typed_dict(
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
}
