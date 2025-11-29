//! Python code templates for MCP client generation.

use std::collections::HashMap;

/// Generate the MCP client module code.
///
/// This creates the shared `_mcp_client.py` file that handles MCP protocol
/// communication.
#[must_use]
pub fn generate_mcp_client_module(base_url: &str) -> String {
    let mut code = String::with_capacity(4096);

    code.push_str("# uv: dependencies = [\"httpx\"]\n\n");
    code.push_str("\"\"\"\n");
    code.push_str("MCP Client - shared client for calling MCP tools via HTTP transport\n");
    code.push_str("Generated code - do not edit manually\n");
    code.push_str("\"\"\"\n\n");

    code.push_str("import httpx\n");
    code.push_str("import os\n");
    code.push_str("from typing import Any, Dict, Optional\n\n");

    code.push_str(&format!(
        "MCP_SERVER_URL = os.getenv(\"MCP_SERVER_URL\", \"{}\")\n\n",
        base_url
    ));

    code.push_str(&generate_session_state());
    code.push_str(&generate_ensure_initialized(None));
    code.push_str(&generate_call_tool_function(None));

    code
}

/// Generate the session state variables.
fn generate_session_state() -> String {
    let mut code = String::new();
    code.push_str("# Session state for MCP initialization\n");
    code.push_str("_mcp_session_id: Optional[str] = None\n");
    code.push_str("_mcp_initialized = False\n\n");
    code
}

/// Generate the `_ensure_mcp_initialized` function.
fn generate_ensure_initialized(custom_headers_fn: Option<&str>) -> String {
    let mut code = String::new();

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
    code.push_str(&generate_sse_parsing("init"));
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
    if custom_headers_fn.is_some() {
        code.push_str("    initialized_headers.update(get_custom_headers())\n");
    }
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

    code
}

/// Generate SSE parsing logic.
fn generate_sse_parsing(var_prefix: &str) -> String {
    let text_var = format!("{}_text", var_prefix);
    let response_var = format!("{}_response", var_prefix);

    let mut code = String::new();
    code.push_str(&format!(
        "    # Parse SSE format response (data: {{...}})\n"
    ));
    code.push_str(&format!("    {} = {}.text\n", text_var, response_var));
    code.push_str(&format!("    if isinstance({}, str):\n", text_var));
    code.push_str("        # Extract JSON from SSE format: data: {...}\n");
    code.push_str(&format!("        for line in {}.split('\\n'):\n", text_var));
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
    code.push_str(&format!(
        "            data_json = {}.json()\n",
        response_var
    ));
    code.push_str("            if 'error' in data_json:\n");
    code.push_str(
        "                raise Exception(f\"MCP initialization error: {data_json['error']}\")\n",
    );
    code.push_str("        except:\n");
    code.push_str("            pass\n");
    code
}

/// Generate the `call_mcp_tool` function.
fn generate_call_tool_function(custom_headers: Option<&HashMap<String, String>>) -> String {
    let mut code = String::new();

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
    if custom_headers.is_some() {
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
    code.push_str(&generate_response_parsing());

    code
}

/// Generate response parsing logic for tool calls.
fn generate_response_parsing() -> String {
    let mut code = String::new();
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
    code.push_str(
        "            raise Exception(f\"Failed to parse MCP response: {response_text[:200]}\")\n",
    );
    code
}

/// Generate a self-contained tool file with inline MCP client.
#[must_use]
pub fn generate_tool_file(
    tool_name: &str,
    tool_description: &str,
    base_url: &str,
    custom_headers: Option<&HashMap<String, String>>,
    typed_dict_code: Option<&str>,
    function_code: &str,
) -> String {
    let mut code = String::with_capacity(8192);

    code.push_str("# uv: dependencies = [\"httpx\"]\n\n");
    code.push_str("\"\"\"\n");
    code.push_str(&format!("{} - {}\n", tool_name, tool_description));
    code.push_str("Generated code - do not edit manually\n");
    code.push_str("This file is self-contained and can be executed independently.\n");
    code.push_str("\"\"\"\n\n");

    code.push_str("from typing import Any, Dict, Optional, TypedDict\n");
    code.push_str("import httpx\n\n");

    // Include MCP client code inline
    code.push_str("# === MCP Client Implementation (inline) ===\n");
    code.push_str("import os\n");
    code.push_str(&format!(
        "MCP_SERVER_URL = os.getenv(\"MCP_SERVER_URL\", \"{}\")\n\n",
        base_url
    ));

    // Generate custom headers function if needed
    if let Some(headers) = custom_headers {
        code.push_str(&generate_custom_headers_function(headers));
    }

    code.push_str(&generate_session_state());
    code.push_str(&generate_ensure_initialized(
        custom_headers.map(|_| "get_custom_headers"),
    ));
    code.push_str(&generate_call_tool_function(custom_headers));

    code.push_str("# === Tool Definition ===\n\n");
    code.push_str(&format!("# Tool: {}\n", tool_name));

    if let Some(typed_dict) = typed_dict_code {
        code.push_str(typed_dict);
        code.push_str("\n\n");
    }

    if !tool_description.is_empty() {
        code.push_str(&format!("\"\"\"{}\"\"\"\n", tool_description));
    }

    code.push_str(function_code);

    code
}

/// Generate custom headers function for external servers.
fn generate_custom_headers_function(headers: &HashMap<String, String>) -> String {
    let mut code = String::new();
    code.push_str("# Custom headers from configuration\n");
    code.push_str("def get_custom_headers() -> Dict[str, str]:\n");
    code.push_str("    headers = {}\n");
    for (key, default_value) in headers {
        let env_var = key.replace('-', "_").to_uppercase();
        code.push_str(&format!(
            "    headers[\"{}\"] = os.getenv(\"{}\", \"{}\")\n",
            key, env_var, default_value
        ));
    }
    code.push_str("    return headers\n\n");
    code
}

/// Generate the index.py file that re-exports all tools.
#[must_use]
pub fn generate_index_file(tool_exports: &[(String, String)]) -> String {
    let mut code = String::new();

    code.push_str("\"\"\"\n");
    code.push_str("Index file - re-exports all tools from this server\n");
    code.push_str("Generated code - do not edit manually\n");
    code.push_str("\"\"\"\n\n");

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

    code
}

/// Generate the `__init__.py` file.
#[must_use]
pub fn generate_init_file(tool_exports: &[(String, String)]) -> String {
    let mut code = String::new();

    code.push_str("\"\"\"\n");
    code.push_str("Nexus MCP Server tools\n");
    code.push_str("Generated code - do not edit manually\n");
    code.push_str("\"\"\"\n\n");

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

    code
}
