# uv: dependencies = ["httpx"]

"""
echo - Echoes the input string back to the user
Generated code - do not edit manually
This file is self-contained and can be executed independently.
"""

from typing import Any, Dict, Optional, TypedDict
import httpx

# === MCP Client Implementation (inline) ===
MCP_SERVER_URL = "http://127.0.0.1:8000"

async def call_mcp_tool(tool_name: str, params: Dict[str, Any]) -> Dict[str, Any]:
    """Call an MCP tool via HTTP transport"""
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

# Tool: echo
class EchoInput(TypedDict):
    message: str

"""Echoes the input string back to the user"""
async def echo(input: EchoInput) -> Dict[str, Any]:
    return await call_mcp_tool("echo", input)
