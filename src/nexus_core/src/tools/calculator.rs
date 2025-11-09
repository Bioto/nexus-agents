use crate::models::tool::{Tool, ToolParameter};
use crate::models::{Error, Result};
use crate::tools::ExecutableTool;
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;

/// Calculator tool that performs basic arithmetic operations
pub struct Calculator;

#[derive(Debug, Deserialize)]
struct CalculatorArgs {
    operation: String,
    a: f64,
    b: f64,
}

impl Calculator {
    pub fn new() -> Self {
        Self
    }

    fn perform_calculation(operation: &str, a: f64, b: f64) -> Result<f64> {
        match operation.to_lowercase().as_str() {
            "add" => Ok(a + b),
            "subtract" => Ok(a - b),
            "multiply" => Ok(a * b),
            "divide" => {
                if b == 0.0 {
                    Err(Error::Configuration("Cannot divide by zero".to_string()))
                } else {
                    Ok(a / b)
                }
            }
            _ => Err(Error::Configuration(format!(
                "Unknown operation: {}. Supported operations: add, subtract, multiply, divide",
                operation
            ))),
        }
    }
}

impl Default for Calculator {
    fn default() -> Self {
        Self::new()
    }
}

impl ExecutableTool for Calculator {
    fn name(&self) -> &str {
        "calculate"
    }

    fn definition(&self) -> Tool {
        let mut params = HashMap::new();
        params.insert(
            "operation".to_string(),
            ToolParameter::new(
                "string",
                "The mathematical operation to perform: add, subtract, multiply, divide",
            )
            .with_required(true),
        );
        params.insert(
            "a".to_string(),
            ToolParameter::new("number", "The first number").with_required(true),
        );
        params.insert(
            "b".to_string(),
            ToolParameter::new("number", "The second number").with_required(true),
        );

        Tool::new(
            "calculate",
            "Perform basic mathematical operations (add, subtract, multiply, divide)",
            params,
        )
    }

    fn execute(&self, args: Value) -> Result<String> {
        let calc_args: CalculatorArgs = serde_json::from_value(args)
            .map_err(|e| Error::Configuration(format!("Invalid calculator arguments: {}", e)))?;

        let result = Self::perform_calculation(&calc_args.operation, calc_args.a, calc_args.b)?;

        Ok(format!(
            "The result of {} {} {} is {}",
            calc_args.a, calc_args.operation, calc_args.b, result
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_calculator_name() {
        let calc = Calculator::new();
        assert_eq!(calc.name(), "calculate");
    }

    #[test]
    fn test_calculator_add() {
        let calc = Calculator::new();
        let args = json!({
            "operation": "add",
            "a": 5.0,
            "b": 3.0
        });
        let result = calc.execute(args).unwrap();
        assert!(result.contains("8"));
    }

    #[test]
    fn test_calculator_subtract() {
        let calc = Calculator::new();
        let args = json!({
            "operation": "subtract",
            "a": 10.0,
            "b": 4.0
        });
        let result = calc.execute(args).unwrap();
        assert!(result.contains("6"));
    }

    #[test]
    fn test_calculator_multiply() {
        let calc = Calculator::new();
        let args = json!({
            "operation": "multiply",
            "a": 6.0,
            "b": 7.0
        });
        let result = calc.execute(args).unwrap();
        assert!(result.contains("42"));
    }

    #[test]
    fn test_calculator_divide() {
        let calc = Calculator::new();
        let args = json!({
            "operation": "divide",
            "a": 20.0,
            "b": 4.0
        });
        let result = calc.execute(args).unwrap();
        assert!(result.contains("5"));
    }

    #[test]
    fn test_calculator_divide_by_zero() {
        let calc = Calculator::new();
        let args = json!({
            "operation": "divide",
            "a": 10.0,
            "b": 0.0
        });
        let result = calc.execute(args);
        assert!(result.is_err());
    }

    #[test]
    fn test_calculator_invalid_operation() {
        let calc = Calculator::new();
        let args = json!({
            "operation": "power",
            "a": 2.0,
            "b": 3.0
        });
        let result = calc.execute(args);
        assert!(result.is_err());
    }

    #[test]
    fn test_calculator_invalid_args() {
        let calc = Calculator::new();
        let args = json!({
            "operation": "add"
            // missing a and b
        });
        let result = calc.execute(args);
        assert!(result.is_err());
    }

    #[test]
    fn test_calculator_case_insensitive() {
        let calc = Calculator::new();
        let args = json!({
            "operation": "ADD",
            "a": 2.0,
            "b": 3.0
        });
        let result = calc.execute(args).unwrap();
        assert!(result.contains("5"));
    }
}
