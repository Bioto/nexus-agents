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
pub mod screen;
pub mod input;
pub mod storage;
pub mod context;
pub mod video;
pub mod unified_recording;

// Re-exports for convenience (maintaining backward compatibility)
pub use audio::{
    AudioRecorder, AudioRecordingConfig, AudioStream, DeviceInfo, TextToSpeech, TtsConfig,
    VoiceListener, VoiceListenerConfig,
};
pub use screen::{MonitorInfo, ScreenRecordingConfig, ScreenRecorder, WindowGeometry, WindowInfo, WindowInfoService};
pub use input::InputEvent;
pub use storage::{BatchEvent, BatchInserterConfig, Database, EventWriterConfig};
pub use context::{ClickContextEvent, ClickContextHandle, ClickContextService, ProcessingConfig, ProcessingHandle, ProcessingJob, ProcessingService};
pub use video::{create_overlay_channel, format_timestamp, get_current_overlay, OverlayReceiver, OverlaySender, VideoOverlay};
pub use unified_recording::{
    AudioRecordingConfig as UnifiedAudioConfig, DefaultEventCallback, EventCallback,
    InputCaptureConfig, OverlayLabel, RecordingSession, ScreenRecordingConfig as UnifiedScreenConfig,
    UnifiedRecordingConfig, UnifiedRecordingService, print_timeline,
};
