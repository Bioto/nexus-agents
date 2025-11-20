/// Screen capture and recording library for AI agents.
/// Provides CLI interfaces, services for screen/window info, and TUI support.
pub mod cli;
pub mod error;
pub mod services;
pub mod tui;

/// CLI entry points and argument parsers.
pub use cli::{run_record, run_screenshot, Cli, Commands};

/// Custom error types and Result alias for the crate.
pub use error::{Result, ScreenError};

/// Core services for screen operations.
pub use services::{
    screen_recorder::{RecordingConfig, ScreenRecorder},
    window_info::{WindowGeometry, WindowInfo, WindowInfoService},
};
