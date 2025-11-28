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
    #[must_use = "this returns the generated code, it doesn't have side effects"]
    pub fn schema_to_typed_dict(schema: &Value, type_name: &str) -> Result<String, SchemaError> {
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
    #[must_use = "this returns the Python type string, it doesn't have side effects"]
    pub fn json_type_to_python(prop: &Value) -> Result<String, SchemaError> {
        let type_str = prop
            .get("type")
            .and_then(|t| t.as_str())
            .unwrap_or("string");

        Ok(match type_str {
            "string" => "str".to_string(),
            "integer" => "int".to_string(),
            "number" => "float".to_string(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_json_type_to_python_string() {
        let prop = json!({"type": "string"});
        assert_eq!(SchemaConverter::json_type_to_python(&prop).unwrap(), "str");
    }

    #[test]
    fn test_json_type_to_python_integer() {
        let prop = json!({"type": "integer"});
        assert_eq!(SchemaConverter::json_type_to_python(&prop).unwrap(), "int");
    }

    #[test]
    fn test_json_type_to_python_number() {
        let prop = json!({"type": "number"});
        assert_eq!(
            SchemaConverter::json_type_to_python(&prop).unwrap(),
            "float"
        );
    }

    #[test]
    fn test_json_type_to_python_boolean() {
        let prop = json!({"type": "boolean"});
        assert_eq!(SchemaConverter::json_type_to_python(&prop).unwrap(), "bool");
    }

    #[test]
    fn test_json_type_to_python_array_with_items() {
        let prop = json!({"type": "array", "items": {"type": "string"}});
        assert_eq!(
            SchemaConverter::json_type_to_python(&prop).unwrap(),
            "list[str]"
        );
    }

    #[test]
    fn test_json_type_to_python_array_without_items() {
        let prop = json!({"type": "array"});
        assert_eq!(
            SchemaConverter::json_type_to_python(&prop).unwrap(),
            "list[Any]"
        );
    }

    #[test]
    fn test_json_type_to_python_object() {
        let prop = json!({"type": "object"});
        assert_eq!(
            SchemaConverter::json_type_to_python(&prop).unwrap(),
            "Dict[str, Any]"
        );
    }

    #[test]
    fn test_json_type_to_python_unknown_defaults_to_any() {
        let prop = json!({"type": "unknown_type"});
        assert_eq!(SchemaConverter::json_type_to_python(&prop).unwrap(), "Any");
    }

    #[test]
    fn test_json_type_to_python_missing_type_defaults_to_string() {
        let prop = json!({});
        assert_eq!(SchemaConverter::json_type_to_python(&prop).unwrap(), "str");
    }

    #[test]
    fn test_schema_to_typed_dict_empty_schema() {
        let schema = json!({});
        let result = SchemaConverter::schema_to_typed_dict(&schema, "EmptyInput").unwrap();
        assert_eq!(result, "class EmptyInput(TypedDict):\n    pass\n");
    }

    #[test]
    fn test_schema_to_typed_dict_with_required_fields() {
        let schema = json!({
            "properties": {
                "name": {"type": "string"},
                "age": {"type": "integer"}
            },
            "required": ["name"]
        });
        let result = SchemaConverter::schema_to_typed_dict(&schema, "PersonInput").unwrap();

        // name should be required, age should be optional
        assert!(result.contains("name: str"));
        assert!(result.contains("age: Optional[int]"));
        assert!(result.starts_with("class PersonInput(TypedDict):"));
    }

    #[test]
    fn test_schema_to_typed_dict_all_optional() {
        let schema = json!({
            "properties": {
                "color": {"type": "string"}
            }
        });
        let result = SchemaConverter::schema_to_typed_dict(&schema, "OptionalInput").unwrap();

        assert!(result.contains("color: Optional[str]"));
    }

    #[test]
    fn test_schema_to_typed_dict_nested_array() {
        let schema = json!({
            "properties": {
                "items": {"type": "array", "items": {"type": "integer"}}
            },
            "required": ["items"]
        });
        let result = SchemaConverter::schema_to_typed_dict(&schema, "ListInput").unwrap();

        assert!(result.contains("items: list[int]"));
    }
}
