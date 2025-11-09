use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Represents a tool parameter with its type and description
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolParameter {
    #[serde(rename = "type")]
    pub param_type: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<bool>,
}

impl ToolParameter {
    pub fn new(param_type: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            param_type: param_type.into(),
            description: description.into(),
            required: None,
        }
    }

    pub fn with_required(mut self, required: bool) -> Self {
        self.required = Some(required);
        self
    }
}

/// Represents a tool/function that an agent can use
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Tool {
    pub name: String,
    pub description: String,
    pub parameters: HashMap<String, ToolParameter>,
}

impl Tool {
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        parameters: HashMap<String, ToolParameter>,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            parameters,
        }
    }

    /// Convert to OpenAI function calling format
    pub fn to_openai_format(&self) -> serde_json::Value {
        let mut properties = serde_json::Map::new();
        let mut required = Vec::new();

        for (name, param) in &self.parameters {
            let param_obj = serde_json::json!({
                "type": param.param_type,
                "description": param.description,
            });
            properties.insert(name.clone(), param_obj);

            if param.required.unwrap_or(false) {
                required.push(name.clone());
            }
        }

        let mut parameters_obj = serde_json::json!({
            "type": "object",
            "properties": properties,
        });

        if !required.is_empty() {
            parameters_obj["required"] = serde_json::json!(required);
        }

        serde_json::json!({
            "type": "function",
            "function": {
                "name": self.name,
                "description": self.description,
                "parameters": parameters_obj,
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_parameter_creation() {
        let param = ToolParameter::new("string", "A test parameter");
        assert_eq!(param.param_type, "string");
        assert_eq!(param.description, "A test parameter");
        assert_eq!(param.required, None);

        let required_param = param.with_required(true);
        assert_eq!(required_param.required, Some(true));
    }

    #[test]
    fn test_tool_creation() {
        let mut params = HashMap::new();
        params.insert(
            "input".to_string(),
            ToolParameter::new("string", "Input value").with_required(true),
        );

        let tool = Tool::new("test_tool", "A test tool", params);
        assert_eq!(tool.name, "test_tool");
        assert_eq!(tool.description, "A test tool");
        assert_eq!(tool.parameters.len(), 1);
    }

    #[test]
    fn test_tool_openai_format() {
        let mut params = HashMap::new();
        params.insert(
            "operation".to_string(),
            ToolParameter::new("string", "The operation").with_required(true),
        );
        params.insert(
            "value".to_string(),
            ToolParameter::new("number", "The value").with_required(false),
        );

        let tool = Tool::new("calculate", "Perform calculation", params);
        let openai_format = tool.to_openai_format();

        assert_eq!(openai_format["type"], "function");
        assert_eq!(openai_format["function"]["name"], "calculate");
        assert_eq!(
            openai_format["function"]["description"],
            "Perform calculation"
        );
        assert!(openai_format["function"]["parameters"]["properties"]["operation"].is_object());
        assert!(openai_format["function"]["parameters"]["properties"]["value"].is_object());

        let required = openai_format["function"]["parameters"]["required"]
            .as_array()
            .unwrap();
        assert_eq!(required.len(), 1);
        assert_eq!(required[0], "operation");
    }
}
