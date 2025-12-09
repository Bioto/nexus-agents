//! Unified recording service for coordinated screen, audio, and input capture.
//!
//! This module provides the [`UnifiedRecordingService`] which orchestrates:
//! - Screen recording (video capture)
//! - Audio recording (microphone and desktop audio)
//! - Input capture (keyboard and mouse events)
//! - Event storage and timeline generation
//!
//! # Module Organization
//!
//! - [`config`]: Configuration types for all recording components
//! - [`service`]: Core service implementation
//! - [`timeline`]: Timeline display utilities

mod config;
mod service;
mod timeline;

// Re-export all public types from config
pub use config::{
    AudioRecordingConfig, DefaultEventCallback, EventCallback, InputCaptureConfig, OverlayLabel,
    ScreenRecordingConfig, UnifiedRecordingConfig, WebcamAnalysisConfig,
    DEFAULT_SEGMENT_DURATION_SECS,
};

// Re-export service types
pub use service::{RecordingSession, UnifiedRecordingService};

// Re-export timeline function
pub use timeline::print_timeline;
