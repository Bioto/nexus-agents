pub mod commands;

use clap::{Parser, Subcommand};

/// Nexus Exporter - Document Export CLI for AI Agents
#[derive(Parser)]
#[command(name = "nexus-exporter")]
#[command(about = "A CLI for exporting documents in various formats (PDF, etc.)", long_about = None)]
#[command(version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Generate a PDF document
    Pdf(commands::pdf::PdfArgs),
}

// Re-export command handlers for convenience
pub use commands::pdf::run_pdf;
