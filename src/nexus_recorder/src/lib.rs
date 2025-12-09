/// Unified recording library for AI agents.
/// Provides audio recording, screen capture, input logging, and unified recording sessions.
pub mod cli;
pub mod error;
pub mod services;
pub mod tui;

/// CLI entry points and argument parsers.
pub use cli::{
    run_capture, run_listen, run_monitor, run_record, run_record_screen, run_report,
    run_screenshot, run_speak, run_splitter, run_test_voice, run_unified, Cli, Commands,
};

/// Custom error types and Result alias for the crate.
pub use error::{RecorderError, Result};

/// Core services for recording operations.
/// All services are re-exported from the services module for convenience.
pub use services::{
    // Webcam services
    list_v4l2_devices,
    mjpeg_to_rgb,
    show_device_info,
    yuv_to_rgb,
    yuyv_to_rgb,
    // Audio services
    AudioRecorder,
    AudioRecordingConfig,
    AudioStream,
    // Storage services
    BatchEvent,
    BatchInserterConfig,
    Database,
    // Unified recording
    DefaultEventCallback,
    DeviceInfo,
    EventCallback,
    EventWriterConfig,

    InputCaptureConfig,
    // Input capture services
    InputEvent,

    // Screen services
    MonitorInfo,
    OverlayLabel,
    PtzControl,
    PtzState,
    RecordingSession,
    ScreenRecorder,
    ScreenRecordingConfig,
    SplitterConfig,
    SplitterHandle,
    TextToSpeech,
    TtsConfig,
    UnifiedRecordingConfig,
    UnifiedRecordingService,

    VoiceListener,
    VoiceListenerConfig,

    WebcamController,
    WebcamDevice,
    WebcamDeviceInfo,
    WebcamRecorder,
    WebcamRecordingConfig,
    WebcamSplitter,
    WindowGeometry,
    WindowInfo,
    WindowInfoService,
};
