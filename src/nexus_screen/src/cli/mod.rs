mod commands;

use clap::{Parser, Subcommand};

/// Nexus Screen - Screen Interface for AI Agents
#[derive(Parser)]
#[command(name = "nexus-screen")]
#[command(about = "A screen interface for interacting with AI agents", long_about = None)]
#[command(version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Capture a screenshot of the screen
    Screenshot(commands::screenshot::ScreenshotArgs),
    /// Record the screen to a video file
    Record(commands::record::RecordArgs),
}

// Re-export command handlers for convenience
pub use commands::record::run_record;
pub use commands::screenshot::run_screenshot;
