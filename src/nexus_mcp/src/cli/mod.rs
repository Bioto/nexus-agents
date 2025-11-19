pub mod commands;

use clap::{Parser, Subcommand};

/// Nexus MCP - MCP CLI for AI Agents
#[derive(Parser)]
#[command(name = "nexus-mcp")]
#[command(about = "A CLI for interacting with MCP (Model Context Protocol)", long_about = None)]
#[command(version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Start an interactive shell
    Shell(commands::shell::ShellArgs),
    /// Generate Python code API for MCP tools
    GenerateCode(commands::generate_code::GenerateCodeArgs),
    /// Start multiple MCP servers from a configuration file
    StartServers(commands::start_servers::StartServersArgs),
}

// Re-export command handlers for convenience
pub use commands::generate_code::run_generate_code;
pub use commands::shell::run_shell;
pub use commands::start_servers::run_start_servers;
