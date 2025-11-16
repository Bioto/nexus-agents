pub mod calculator;
pub mod python_exec;

use crate::models::tool::Tool;
use crate::models::Result;
use serde_json::Value;

/// Trait for executable tools
pub trait ExecutableTool: Send + Sync {
    /// Get the name of the tool
    fn name(&self) -> &str;

    /// Get the complete tool definition including parameters
    fn definition(&self) -> Tool;

    /// Execute the tool with the given arguments
    fn execute(&self, args: Value) -> Result<String>;
}

/// Registry for managing and executing tools
#[derive(Clone)]
pub struct ToolRegistry {
    tools: std::sync::Arc<std::collections::HashMap<String, std::sync::Arc<dyn ExecutableTool>>>,
}

impl ToolRegistry {
    /// Create a new empty tool registry
    pub fn new() -> Self {
        Self {
            tools: std::sync::Arc::new(std::collections::HashMap::new()),
        }
    }

    /// Register a tool (consumes self and returns new registry)
    pub fn register(self, tool: Box<dyn ExecutableTool>) -> Self {
        let name = tool.name().to_string();
        let mut tools = std::collections::HashMap::clone(&self.tools);
        tools.insert(name, std::sync::Arc::from(tool));
        Self {
            tools: std::sync::Arc::new(tools),
        }
    }

    /// Execute a tool by name with the given arguments
    pub fn execute(&self, name: &str, args: Value) -> Result<String> {
        let tool = self.tools.get(name).ok_or_else(|| {
            crate::models::Error::Configuration(format!("Tool not found: {}", name))
        })?;
        tool.execute(args)
    }

    /// Check if a tool exists
    pub fn has_tool(&self, name: &str) -> bool {
        self.tools.contains_key(name)
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct TestTool;

    impl ExecutableTool for TestTool {
        fn name(&self) -> &str {
            "test_tool"
        }

        fn definition(&self) -> Tool {
            Tool::new("test_tool", "A test tool", std::collections::HashMap::new())
        }

        fn execute(&self, args: Value) -> Result<String> {
            Ok(format!("Executed with args: {}", args))
        }
    }

    #[test]
    fn test_registry_creation() {
        let registry = ToolRegistry::new();
        assert!(!registry.has_tool("test_tool"));
    }

    #[test]
    fn test_registry_register() {
        let registry = ToolRegistry::new().register(Box::new(TestTool));
        assert!(registry.has_tool("test_tool"));
    }

    #[test]
    fn test_registry_execute() {
        let registry = ToolRegistry::new().register(Box::new(TestTool));

        let result = registry.execute("test_tool", json!({"test": "value"}));
        assert!(result.is_ok());
        assert!(result.unwrap().contains("Executed with args"));
    }

    #[test]
    fn test_registry_execute_nonexistent() {
        let registry = ToolRegistry::new();
        let result = registry.execute("nonexistent", json!({}));
        assert!(result.is_err());
    }
}
