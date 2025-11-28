//! Shared types used across the nexus_mcp crate.

use serde_json::Value;

/// Tool definition structure representing an MCP tool.
#[derive(Debug, Clone)]
pub struct ToolDefinition {
    /// The name of the tool
    pub name: String,
    /// Human-readable description of what the tool does
    pub description: String,
    /// JSON Schema defining the tool's input parameters
    pub input_schema: Value,
}

