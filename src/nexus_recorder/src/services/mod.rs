//! Service modules for recording operations.
//!
//! This module organizes services into logical groups:
//! - Audio: recording, TTS, voice listening
//! - Screen: screen recording, window information
//! - Input: keyboard and mouse capture
//! - Storage: database, batch insertion, event writing
//! - Context: AI-powered context analysis
//! - Video: overlay and annotation
//! - Unified: unified recording sessions

// Service modules
pub mod audio;
pub mod context;
pub mod input;
pub mod screen;
pub mod storage;
pub mod unified_recording;
pub mod video;
pub mod webcam;

// Re-exports for convenience (maintaining backward compatibility)
pub use audio::{
    AudioRecorder, AudioRecordingConfig, AudioStream, DeviceInfo, TextToSpeech, TtsConfig,
    VoiceListener, VoiceListenerConfig,
};
pub use context::{
    ClickContextEvent, ClickContextHandle, ClickContextService, ProcessingConfig, ProcessingHandle,
    ProcessingJob, ProcessingService,
};
pub use input::InputEvent;
pub use screen::{
    MonitorInfo, ScreenRecorder, ScreenRecordingConfig, WindowGeometry, WindowInfo,
    WindowInfoService,
};
pub use storage::{BatchEvent, BatchInserterConfig, Database, EventWriterConfig};
pub use unified_recording::{
    print_timeline, AudioRecordingConfig as UnifiedAudioConfig, DefaultEventCallback,
    EventCallback, InputCaptureConfig, OverlayLabel, RecordingSession,
    ScreenRecordingConfig as UnifiedScreenConfig, UnifiedRecordingConfig, UnifiedRecordingService,
};
pub use video::{
    create_overlay_channel, format_timestamp, get_current_overlay, OverlayReceiver, OverlaySender,
    VideoOverlay,
};
pub use webcam::{
    list_v4l2_devices, mjpeg_to_rgb, show_device_info, yuv_to_rgb, yuyv_to_rgb, PtzControl,
    PtzState, SplitterConfig, SplitterHandle, WebcamController, WebcamDevice, WebcamDeviceInfo,
    WebcamRecorder, WebcamRecordingConfig, WebcamSplitter,
};
