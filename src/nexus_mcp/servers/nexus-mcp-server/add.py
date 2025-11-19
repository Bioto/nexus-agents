# uv: dependencies = ["httpx"]

"""
add - Adds two numbers together
Generated code - do not edit manually
This file is self-contained and can be executed independently.
"""

from typing import Any, Dict, Optional, TypedDict
import httpx

# === MCP Client Implementation (inline) ===
MCP_SERVER_URL = "http://192.168.1.63:8000"
print(f"MCP_SERVER_URL: {MCP_SERVER_URL}")

async def call_mcp_tool(tool_name: str, params: Dict[str, Any]) -> Dict[str, Any]:
    """Call an MCP tool via HTTP transport"""
    print(f"Calling MCP tool: {tool_name} with params: {params}")
    request = {
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {
            "name": tool_name,
            "arguments": params
        }
    }
    async with httpx.AsyncClient() as client:
        response = await client.post(
            f"{MCP_SERVER_URL}/mcp",
            json=request,
            headers={
                "Accept": "application/json, text/event-stream",
                "Content-Type": "application/json"
            }
        )
        response.raise_for_status()
        result = response.json()
        if "error" in result:
            raise Exception(f"MCP tool error: {result['error']}")
        return result.get("result", {})

# === Tool Definition ===

# Tool: add
class AddInput(TypedDict):
    a: float
    b: float

"""Adds two numbers together"""
async def add(input: AddInput) -> Dict[str, Any]:
    return await call_mcp_tool("add", input)
