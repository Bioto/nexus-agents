mod commands;

use clap::{Parser, Subcommand};

/// Nexus Notetaker - consumes unified recording sessions and produces notes.
#[derive(Parser)]
#[command(name = "nexus-notetaker")]
#[command(about = "Process recorded sessions and produce structured notes", long_about = None)]
#[command(version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Process an existing recording session and generate structured notes
    Process(commands::process::ProcessArgs),
}

// Re-export command runners
pub use commands::process::run_process;
