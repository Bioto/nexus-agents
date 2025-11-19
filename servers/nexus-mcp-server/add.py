
# /// script
# requires-python = ">=3.10"
# dependencies = [
#     "httpx",
# ]
# ///

# uv: dependencies = ["httpx"]

"""
add - Adds two numbers together
Generated code - do not edit manually
This file is self-contained and can be executed independently.
"""

from typing import Any, Dict, Optional, TypedDict
import httpx
import os

# === MCP Client Implementation (inline) ===
MCP_SERVER_URL = os.getenv("MCP_SERVER_URL", "http://172.17.0.1:8000")

# Session state for MCP initialization
_mcp_session_id: Optional[str] = None
_mcp_initialized = False

async def _ensure_mcp_initialized(client: httpx.AsyncClient) -> str:
    """Initialize MCP session if not already initialized"""
    global _mcp_session_id, _mcp_initialized
    if _mcp_initialized and _mcp_session_id:
        return _mcp_session_id
    
    # Step 1: Initialize the MCP session
    init_request = {
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {
                "name": "nexus-mcp-python-client",
                "version": "0.1.0"
            }
        }
    }
    
    init_response = await client.post(
        f"{MCP_SERVER_URL}/mcp",
        json=init_request,
        headers={
            "Accept": "application/json, text/event-stream",
            "Content-Type": "application/json"
        }
    )
    init_response.raise_for_status()
    
    # Extract session ID from response headers
    _mcp_session_id = init_response.headers.get("mcp-session-id")
    
    # Parse SSE format response (data: {...})
    import json
    init_text = init_response.text
    # Extract JSON from SSE format: data: {...}
    for line in init_text.split('\n'):
        line = line.strip()
        if line.startswith('data: '):
            data_json = json.loads(line[6:])  # Skip 'data: '
            if 'error' in data_json:
                raise Exception(f"MCP initialization error: {data_json['error']}")
            break
    
    # Step 2: Send initialized notification
    initialized_notification = {
        "jsonrpc": "2.0",
        "method": "notifications/initialized"
    }
    
    initialized_headers = {
        "Accept": "application/json, text/event-stream",
        "Content-Type": "application/json"
    }
    if _mcp_session_id:
        initialized_headers["mcp-session-id"] = _mcp_session_id
    
    await client.post(
        f"{MCP_SERVER_URL}/mcp",
        json=initialized_notification,
        headers=initialized_headers
    )
    
    _mcp_initialized = True
    return _mcp_session_id or ""

async def call_mcp_tool(tool_name: str, params: Dict[str, Any]) -> Dict[str, Any]:
    """Call an MCP tool via HTTP transport"""
    async with httpx.AsyncClient() as client:
        # Ensure MCP session is initialized
        session_id = await _ensure_mcp_initialized(client)
        
        # Make tool call request
        request = {
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {
                "name": tool_name,
                "arguments": params
            }
        }
        
        headers = {
            "Accept": "application/json, text/event-stream",
            "Content-Type": "application/json"
        }
        if session_id:
            headers["mcp-session-id"] = session_id
        
        full_url = f"{MCP_SERVER_URL}/mcp"
        print(f"[call_mcp_tool] MCP_SERVER_URL: {MCP_SERVER_URL}")
        print(f"[call_mcp_tool] Full URL: {full_url}")
        print(f"[call_mcp_tool] Calling tool '{tool_name}' with params: {params}")
        print(f"[call_mcp_tool] Request payload: {request}")
        
        try:
            print(f"[call_mcp_tool] Sending POST request to: {full_url}")
            response = await client.post(
                full_url,
                json=request,
                headers=headers,
                timeout=10.0
            )
            print(f"[call_mcp_tool] Response status: {response.status_code}")
            print(f"[call_mcp_tool] Response headers: {dict(response.headers)}")
            print(f"[call_mcp_tool] Response text (first 500 chars): {response.text[:500]}")
            response.raise_for_status()
            
            # Parse SSE format response (data: {...})
            import json
            response_text = response.text
            # Extract JSON from SSE format: data: {...}
            for line in response_text.split('\n'):
                line = line.strip()
                if line.startswith('data: '):
                    result = json.loads(line[6:])  # Skip 'data: '
                    if "error" in result:
                        raise Exception(f"MCP tool error: {result['error']}")
                    return result.get("result", {})
            # Fallback: try to parse as JSON directly
            try:
                result = response.json()
                if "error" in result:
                    raise Exception(f"MCP tool error: {result['error']}")
                return result.get("result", {})
            except:
                raise Exception(f"Failed to parse MCP response: {response_text[:200]}")
        except httpx.ConnectError as e:
            print(f"[call_mcp_tool] Connection error: {e}")
            print(f"[call_mcp_tool] Failed to connect to: {full_url}")
            raise
        except Exception as e:
            print(f"[call_mcp_tool] Error: {type(e).__name__}: {e}")
            raise

# === Tool Definition ===

# Tool: add
class AddInput(TypedDict):
    a: float
    b: float

"""Adds two numbers together"""
async def add(input: AddInput) -> Dict[str, Any]:
    return await call_mcp_tool("add", input)
