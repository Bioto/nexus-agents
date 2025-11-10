mod commands;

use clap::{Parser, Subcommand};

/// Nexus GUI - Graphical Interface for AI Agents
#[derive(Parser)]
#[command(name = "nexus-gui")]
#[command(about = "A graphical interface for interacting with AI agents", long_about = None)]
#[command(version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Show a GUI component
    Show(commands::show::ShowArgs),
}

// Re-export command handlers for convenience
pub use commands::show::run_show;

