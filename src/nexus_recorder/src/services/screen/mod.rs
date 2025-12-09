//! Screen recording and window information services.
//!
//! This module provides screen recording, window detection, and monitor information.

pub mod screen_recorder;
pub mod window_info;

// Re-exports for convenience
pub use screen_recorder::{MonitorInfo, RecordingConfig as ScreenRecordingConfig, ScreenRecorder};
pub use window_info::{WindowGeometry, WindowInfo, WindowInfoService};
