/// Logger library for AI agents.
/// Provides CLI interfaces and logging services.

pub mod cli;
pub mod error;
pub mod services;

/// CLI entry points and argument parsers.
pub use cli::{run_log, run_capture, run_unified, Cli, Commands};

/// Custom error types and Result alias for the crate.
pub use error::{Result, LoggerError};

/// Re-export unified recording service for other developers.
pub use services::unified_recording::{
    EventCallback, DefaultEventCallback, UnifiedRecordingService, UnifiedRecordingConfig,
    ScreenRecordingConfig, InputCaptureConfig, RecordingSession, OverlayLabel,
};

/// Re-export input event types.
pub use services::capture::InputEvent;

