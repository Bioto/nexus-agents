/// Nexus Exporter - Document export library for AI agents.
/// Provides PDF generation and other document export capabilities.
pub mod cli;
pub mod error;
pub mod services;

/// CLI entry points and argument parsers.
pub use cli::{run_pdf, Cli, Commands};

/// Custom error types and Result alias for the crate.
pub use error::{ExporterError, Result};

/// Core services for export operations.
pub use services::{PdfExportConfig, PdfExporter};
