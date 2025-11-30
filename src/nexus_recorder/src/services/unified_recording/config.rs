//! Configuration types for unified recording.
//!
//! This module contains all configuration structs and traits for the unified
//! recording service, including screen, audio, and input capture settings.

use crate::services::storage::BatchInserterConfig;

/// Simple configuration for webcam sentiment analysis.
/// Now uses the unified ProcessingService architecture.
#[derive(Clone, Debug)]
pub struct WebcamAnalysisConfig {
    /// Interval between frame analyses in seconds.
    pub interval_secs: u64,
    /// Whether analysis is enabled.
    pub enabled: bool,
}

impl WebcamAnalysisConfig {
    pub fn with_device(interval_secs: u64, _device_path: String) -> Self {
        Self {
            interval_secs,
            enabled: true,
        }
    }
}
use crate::services::input::InputEvent;
use crate::services::storage::EventWriterConfig;
use crate::services::webcam::WebcamRecordingConfig;
use std::time::Duration;

/// Default video segment duration in seconds (1 hour).
/// Used when segmenting long recordings into multiple files.
pub const DEFAULT_SEGMENT_DURATION_SECS: u64 = 3600;

/// Input event polling interval in milliseconds.
/// How often to check for new input events (keyboard/mouse).
pub const INPUT_POLL_INTERVAL_MS: u64 = 10;

/// Audio buffer timeout duration in milliseconds.
/// Used for timeouts when receiving audio chunks or events.
pub const AUDIO_BUFFER_DURATION_MS: u64 = 100;
use chrono::{DateTime, Utc};
use std::path::PathBuf;

/// Overlay label information
#[derive(Debug, Clone)]
pub struct OverlayLabel {
    /// Label text to display
    pub text: String,
    /// Video timestamp when this label should appear (seconds)
    pub timestamp: f64,
    /// Duration to show the label (seconds, None = show until next label)
    pub duration: Option<f64>,
    /// X position (None = auto)
    pub x: Option<u32>,
    /// Y position (None = auto)
    pub y: Option<u32>,
}

/// Callback trait for processing events during recording.
/// Implement this to receive events and timestamp video accordingly.
pub trait EventCallback: Send + Sync {
    /// Called when a keyboard event is captured.
    ///
    /// # Arguments
    /// * `event` - The keyboard event
    /// * `video_timestamp` - Current video timestamp in seconds
    /// * `recording_start` - When recording started (for absolute time calculations)
    ///
    /// Returns (should_store, optional_label) where:
    /// - should_store: true if the event should be stored, false to skip
    /// - optional_label: Some(label) to add overlay text, None for no overlay
    fn on_keyboard_event(
        &self,
        event: &InputEvent,
        video_timestamp: f64,
        recording_start: DateTime<Utc>,
    ) -> (bool, Option<OverlayLabel>);

    /// Called when a mouse event is captured.
    ///
    /// # Arguments
    /// * `event` - The mouse event
    /// * `video_timestamp` - Current video timestamp in seconds
    /// * `recording_start` - When recording started (for absolute time calculations)
    ///
    /// Returns (should_store, optional_label) where:
    /// - should_store: true if the event should be stored, false to skip
    /// - optional_label: Some(label) to add overlay text, None for no overlay
    fn on_mouse_event(
        &self,
        event: &InputEvent,
        video_timestamp: f64,
        recording_start: DateTime<Utc>,
    ) -> (bool, Option<OverlayLabel>);
}

/// Default callback implementation that accepts all events.
pub struct DefaultEventCallback;

impl EventCallback for DefaultEventCallback {
    fn on_keyboard_event(
        &self,
        event: &InputEvent,
        video_timestamp: f64,
        _recording_start: DateTime<Utc>,
    ) -> (bool, Option<OverlayLabel>) {
        // Generate labels for important keys
        if let InputEvent::Keyboard { key, pressed, .. } = event {
            if *pressed && (key == "Enter" || key == "Escape" || key == "Space" || key == "Tab") {
                return (
                    true,
                    Some(OverlayLabel {
                        text: format!("Key: {}", key),
                        timestamp: video_timestamp,
                        duration: Some(2.0),
                        x: None,
                        y: None,
                    }),
                );
            }
        }
        (true, None)
    }

    fn on_mouse_event(
        &self,
        event: &InputEvent,
        video_timestamp: f64,
        _recording_start: DateTime<Utc>,
    ) -> (bool, Option<OverlayLabel>) {
        // Generate labels for mouse clicks
        if let InputEvent::Mouse {
            event_type,
            button,
            x,
            y,
            ..
        } = event
        {
            if event_type == "click" {
                let btn_name = button.as_deref().unwrap_or("unknown");
                return (
                    true,
                    Some(OverlayLabel {
                        text: format!("Click: {}", btn_name),
                        timestamp: video_timestamp,
                        duration: Some(1.5),
                        x: x.map(|x| x as u32),
                        y: y.map(|y| y as u32),
                    }),
                );
            }
        }
        (true, None)
    }
}

/// Configuration for unified recording (screen + input).
#[derive(Clone, Debug)]
pub struct UnifiedRecordingConfig {
    /// Screen recording configuration (None = disabled)
    pub screen_config: Option<ScreenRecordingConfig>,
    /// Webcam recording configuration (None = disabled)
    pub webcam_config: Option<WebcamRecordingConfig>,
    /// Webcam sentiment analysis configuration (None = disabled)
    pub webcam_analysis_config: Option<WebcamAnalysisConfig>,
    /// Input capture configuration
    pub input_config: InputCaptureConfig,
    /// Audio recording configurations (can have multiple for mic + monitor)
    pub audio_configs: Vec<AudioRecordingConfig>,
    /// Database path for storing events
    pub database_path: PathBuf,
    /// Whether to capture keyboard events
    pub capture_keyboard: bool,
    /// Whether to capture mouse events
    pub capture_mouse: bool,
    /// Whether to capture mouse moves
    pub capture_mouse_moves: bool,
    /// Whether to add timestamp overlay to video
    pub show_timestamp: bool,
    /// Whether to add event labels to video
    pub show_labels: bool,
    /// Frames per second for post-recording context analysis (None = disabled)
    pub context_fps: Option<f64>,
    /// Event file writer configuration (buffered writes, rotation, compression)
    pub event_writer_config: Option<EventWriterConfig>,
    /// Batch inserter configuration (batched database inserts)
    pub batch_inserter_config: Option<BatchInserterConfig>,
}

impl Default for UnifiedRecordingConfig {
    fn default() -> Self {
        Self {
            screen_config: Some(ScreenRecordingConfig::default()),
            webcam_config: None,
            webcam_analysis_config: None,
            input_config: InputCaptureConfig::default(),
            audio_configs: Vec::new(),
            database_path: PathBuf::from("events.db"),
            capture_keyboard: true,
            capture_mouse: true,
            capture_mouse_moves: false,
            show_timestamp: true,
            show_labels: true,
            context_fps: None,
            event_writer_config: None, // Use legacy file writing by default
            batch_inserter_config: None, // Use individual inserts by default
        }
    }
}

/// Screen recording configuration.
#[derive(Clone, Debug)]
pub struct ScreenRecordingConfig {
    /// Output video file path
    pub output_path: PathBuf,
    /// Frame rate (FPS)
    pub framerate: u32,
    /// Duration in seconds (None = until stopped)
    pub duration_secs: Option<u64>,
    /// Monitor index (None = primary)
    pub monitor_index: Option<usize>,
    /// Include audio
    pub include_audio: bool,
    /// Video segment duration in seconds (None = no segmentation, Some(DEFAULT_SEGMENT_DURATION_SECS) = 1 hour segments)
    /// When enabled, creates files like: recording_000.mp4, recording_001.mp4, etc.
    pub segment_duration_secs: Option<u64>,
}

impl Default for ScreenRecordingConfig {
    fn default() -> Self {
        Self {
            output_path: PathBuf::from("output/recording.mp4"),
            framerate: 30,
            duration_secs: None,
            monitor_index: None,
            include_audio: true,
            segment_duration_secs: None, // No segmentation by default
        }
    }
}

/// Input capture configuration.
#[derive(Clone, Debug)]
pub struct InputCaptureConfig {
    /// Output file for events (None = stdout)
    pub output_file: Option<PathBuf>,
    /// Output format: "json", "text", or "both"
    pub format: String,
}

impl Default for InputCaptureConfig {
    fn default() -> Self {
        Self {
            output_file: None,
            format: "text".to_string(),
        }
    }
}

/// Audio recording configuration.
#[derive(Clone, Debug)]
pub struct AudioRecordingConfig {
    /// Whether to record audio
    pub enabled: bool,
    /// Output path for WAV file
    pub output_path: PathBuf,
    /// Sample rate in Hz (default 48000)
    pub sample_rate: u32,
    /// Number of channels (1 = mono, 2 = stereo, default 1 for mic, 2 for desktop)
    pub channels: u16,
    /// Specific microphone device name (None = default device)
    pub device_name: Option<String>,
    /// Whether to monitor desktop audio output (creates virtual loopback sink)
    /// When true, records desktop audio instead of microphone input
    pub monitor_desktop_audio: bool,
    /// Whether to transcribe audio (future feature)
    pub transcribe: bool,
    /// Path to Whisper model if transcribing (None = disabled)
    pub transcription_model_path: Option<PathBuf>,
}

impl Default for AudioRecordingConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            output_path: PathBuf::from("output/recording.wav"),
            sample_rate: 48000,
            channels: 1,
            device_name: None,
            monitor_desktop_audio: false,
            transcribe: false,
            transcription_model_path: None,
        }
    }
}

