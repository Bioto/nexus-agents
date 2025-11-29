/// Toolbox library for utility tools.
/// Provides CLI interfaces and tool services.
pub mod cli;
pub mod error;
pub mod nutrition;

/// CLI entry points and argument parsers.
pub use cli::{run_nutrition, Cli, Commands};

/// Custom error types and Result alias for the crate.
pub use error::{Result, ToolboxError};

/// Re-export nutrition MCP server for external use
pub use nutrition::NutritionMcpServer;
