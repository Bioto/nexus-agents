/// Logger library for AI agents.
/// Provides CLI interfaces and logging services.

pub mod cli;
pub mod error;
pub mod services;

/// CLI entry points and argument parsers.
pub use cli::{run_log, run_capture, Cli, Commands};

/// Custom error types and Result alias for the crate.
pub use error::{Result, LoggerError};

