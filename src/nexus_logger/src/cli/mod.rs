mod commands;

use clap::{Parser, Subcommand};

/// Nexus Logger - Logging Interface for AI Agents
#[derive(Parser)]
#[command(name = "nexus-logger")]
#[command(about = "A logging interface for AI agents", long_about = None)]
#[command(version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Log a message
    Log(commands::log::LogArgs),
    /// Capture keyboard and mouse input
    Capture(commands::capture::CaptureArgs),
    /// Unified recording (screen + input capture with callbacks)
    Unified(commands::unified::UnifiedArgs),
    /// Generate a report of all collected data from ClickHouse
    Report(commands::report::ReportArgs),
}

// Re-export command handlers for convenience
pub use commands::capture::run_capture;
pub use commands::log::run_log;
pub use commands::report::run_report;
pub use commands::unified::run_unified;
