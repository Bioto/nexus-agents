use serde_json::Value;

/// Error type for schema conversion
#[derive(Debug)]
pub enum SchemaError {
    ParseError(String),
}

impl std::fmt::Display for SchemaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SchemaError::ParseError(msg) => write!(f, "Parse error: {}", msg),
        }
    }
}

impl std::error::Error for SchemaError {}

/// Converter for JSON Schema to Python types
pub struct SchemaConverter;

impl SchemaConverter {
    /// Convert JSON Schema to Python TypedDict
    pub fn schema_to_typed_dict(
        schema: &Value,
        type_name: &str,
    ) -> Result<String, SchemaError> {
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
            let python_type = Self::json_type_to_python(prop)?;
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
    pub fn json_type_to_python(prop: &Value) -> Result<String, SchemaError> {
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
                    let item_type = Self::json_type_to_python(items)?;
                    format!("list[{}]", item_type)
                } else {
                    "list[Any]".to_string()
                }
            }
            "object" => "Dict[str, Any]".to_string(),
            _ => "Any".to_string(),
        })
    }
}

