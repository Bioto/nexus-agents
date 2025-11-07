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
    /// Send a chat completion request
    Query(commands::query::QueryArgs),
    /// Start an interactive chat session with conversation history
    Chat(commands::chat::ChatArgs),
}

// Re-export command handlers for convenience
pub use commands::chat::run_chat;
pub use commands::query::run_query;
