pub mod commands;
pub mod common;

use clap::{Parser, Subcommand};

/// LLM Agent Framework CLI
#[derive(Parser)]
#[command(name = "nexus-agents")]
#[command(about = "A CLI tool for interacting with OpenAI-compatible LLM endpoints", long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Start an interactive chat session with conversation history
    Chat(commands::chat::ChatArgs),
    /// Test execute_python tool with a one-off prompt
    TestPython(commands::test_python::TestPythonArgs),
}

// Re-export command handlers for convenience
pub use commands::chat::run_chat;
pub use commands::test_python::run_test_python;
