/// Unified recording library for AI agents.
/// Provides audio recording, screen capture, input logging, and unified recording sessions.

pub mod cli;
pub mod error;
pub mod services;
pub mod tui;

/// CLI entry points and argument parsers.
pub use cli::{
    run_capture, run_listen, run_monitor, run_record, run_record_screen, run_report,
    run_screenshot, run_speak, run_test_voice, run_unified, Cli, Commands,
};

/// Custom error types and Result alias for the crate.
pub use error::{RecorderError, Result};

/// Core services for recording operations.
/// All services are re-exported from the services module for convenience.
pub use services::{
    // Audio services
    AudioRecorder, AudioRecordingConfig, AudioStream, DeviceInfo,
    TextToSpeech, TtsConfig,
    VoiceListener, VoiceListenerConfig,
    
    // Screen services
    MonitorInfo, ScreenRecordingConfig, ScreenRecorder,
    WindowGeometry, WindowInfo, WindowInfoService,
    
    // Input capture services
    InputEvent,
    
    // Unified recording
    DefaultEventCallback, EventCallback, InputCaptureConfig, OverlayLabel, RecordingSession,
    UnifiedRecordingConfig, UnifiedRecordingService,
    
    // Storage services
    BatchEvent, BatchInserterConfig, Database, EventWriterConfig,
};
