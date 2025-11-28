//! Code generator for MCP tools.

use crate::client::McpClient;
use crate::error::NexusError;
use crate::types::ToolDefinition;
use crate::utils::{to_pascal_case, to_snake_case};

use super::python_templates;
use super::schema_converter::SchemaConverter;

use std::collections::HashMap;
use std::path::Path;

/// Generate Python code API for MCP tools.
pub struct CodeGenerator {
    server_url: String,
    mcp_client: McpClient,
    headers: Option<HashMap<String, String>>,
    server_name: String,
}

impl CodeGenerator {
    /// Create a new code generator.
    pub fn new(server_url: impl Into<String>) -> Self {
        let server_url = server_url.into();
        Self {
            mcp_client: McpClient::new(server_url.clone()),
            server_url,
            headers: None,
            server_name: "nexus-mcp-server".to_string(),
        }
    }

    /// Create a new code generator with custom headers and server name.
    pub fn with_config(
        server_url: impl Into<String>,
        server_name: impl Into<String>,
        headers: Option<HashMap<String, String>>,
    ) -> Self {
        let server_url = server_url.into();
        let mcp_client = match &headers {
            Some(h) => McpClient::with_headers(server_url.clone(), h.clone()),
            None => McpClient::new(server_url.clone()),
        };
        Self {
            server_url,
            mcp_client,
            headers,
            server_name: server_name.into(),
        }
    }

    /// Generate Python code files in directory structure.
    pub async fn generate_code_files(&self, output_dir: &Path) -> Result<(), NexusError> {
        let tools = self.mcp_client.fetch_tools().await?;

        // Create server directory (e.g., servers/context7)
        let server_dir = output_dir.join(&self.server_name);
        std::fs::create_dir_all(&server_dir)?;

        // Generate shared MCP client
        let base_url = self.normalize_base_url();
        let client_code = python_templates::generate_mcp_client_module(&base_url);
        let client_path = output_dir.join("_mcp_client.py");
        std::fs::write(&client_path, client_code)?;

        // Generate individual tool files
        let mut tool_exports = Vec::new();
        for tool in &tools {
            let tool_code = self.generate_tool_file_code(tool)?;
            let function_name = to_snake_case(&tool.name);
            let file_name = format!("{}.py", function_name);
            let file_path = server_dir.join(&file_name);

            std::fs::write(&file_path, tool_code)?;
            tool_exports.push((function_name, tool.name.clone()));
        }

        // Generate index.py that re-exports all tools
        let index_code = python_templates::generate_index_file(&tool_exports);
        let index_path = server_dir.join("index.py");
        std::fs::write(&index_path, index_code)?;

        // Generate __init__.py
        let init_code = python_templates::generate_init_file(&tool_exports);
        let init_path = server_dir.join("__init__.py");
        std::fs::write(&init_path, init_code)?;

        Ok(())
    }

    /// Normalize the server URL by removing trailing /mcp if present.
    fn normalize_base_url(&self) -> String {
        if self.server_url.ends_with("/mcp") {
            self.server_url.trim_end_matches("/mcp").to_string()
        } else {
            self.server_url.clone()
        }
    }

    /// Generate a single tool file (self-contained with inline MCP client).
    fn generate_tool_file_code(&self, tool: &ToolDefinition) -> Result<String, NexusError> {
        let base_url = self.normalize_base_url();
        let function_name = to_snake_case(&tool.name);

        // Check if tool has parameters
        let has_params = tool
            .input_schema
            .get("properties")
            .and_then(|p| p.as_object())
            .map(|obj| !obj.is_empty())
            .unwrap_or(false);

        // Generate TypedDict if tool has parameters
        let typed_dict_code = if has_params {
            let input_type_name = format!("{}Input", to_pascal_case(&tool.name));
            Some(
                SchemaConverter::schema_to_typed_dict(&tool.input_schema, &input_type_name)
                    .map_err(|e| NexusError::Parse(e.to_string()))?,
            )
        } else {
            None
        };

        // Generate function code
        let function_code = if has_params {
            let input_type_name = format!("{}Input", to_pascal_case(&tool.name));
            format!(
                "async def {}(input: {}) -> Dict[str, Any]:\n    return await call_mcp_tool(\"{}\", input)\n",
                function_name, input_type_name, tool.name
            )
        } else {
            format!(
                "async def {}() -> Dict[str, Any]:\n    return await call_mcp_tool(\"{}\", {{}})\n",
                function_name, tool.name
            )
        };

        Ok(python_templates::generate_tool_file(
            &tool.name,
            &tool.description,
            &base_url,
            self.headers.as_ref(),
            typed_dict_code.as_deref(),
            &function_code,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn generator() -> CodeGenerator {
        CodeGenerator::new("http://localhost:8000")
    }

    #[test]
    fn test_normalize_base_url_with_mcp() {
        let gen = CodeGenerator::new("http://localhost:8000/mcp");
        assert_eq!(gen.normalize_base_url(), "http://localhost:8000");
    }

    #[test]
    fn test_normalize_base_url_without_mcp() {
        let gen = generator();
        assert_eq!(gen.normalize_base_url(), "http://localhost:8000");
    }

    #[test]
    fn test_with_config() {
        let mut headers = HashMap::new();
        headers.insert("X-Api-Key".to_string(), "test-key".to_string());

        let gen = CodeGenerator::with_config(
            "http://example.com",
            "test-server",
            Some(headers.clone()),
        );

        assert_eq!(gen.server_url, "http://example.com");
        assert_eq!(gen.server_name, "test-server");
        assert!(gen.headers.is_some());
    }
}

