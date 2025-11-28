//! Parameter and result types for MCP server tools and prompts.

use rmcp::schemars;
use serde::{Deserialize, Serialize};

// ============================================================================
// Tool Parameter Types
// ============================================================================

/// Parameters for the echo tool.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct EchoParams {
    /// The message to echo back.
    pub message: String,
}

/// Parameters for the add tool.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AddParams {
    /// First number to add.
    pub a: f64,
    /// Second number to add.
    pub b: f64,
}

/// Result of the add tool.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AddResult {
    /// The sum of a and b.
    pub result: f64,
    /// The operation performed.
    pub operation: String,
}

/// Parameters for the generate_code_api tool.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GenerateCodeApiParams {
    /// MCP server URL (default: http://127.0.0.1:8000)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_url: Option<String>,
    /// Output directory path (default: servers)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_dir: Option<String>,
}

// ============================================================================
// Prompt Parameter Types
// ============================================================================

/// Parameters for the greeting prompt.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GreetingParams {
    /// Name to greet.
    pub name: String,
}

/// Parameters for the code review prompt.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CodeReviewParams {
    /// The code to review.
    pub code: String,
    /// The programming language of the code.
    pub language: String,
    /// What to focus on during review.
    pub focus: String,
}

/// Parameters for the analyze prompt.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AnalyzeParams {
    /// The topic to analyze.
    pub topic: String,
    /// Additional context for the analysis.
    pub context: String,
}
