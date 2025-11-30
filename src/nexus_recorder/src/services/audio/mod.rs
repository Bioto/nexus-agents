//! Audio recording and processing services.
//!
//! This module provides audio recording, text-to-speech, and voice listening capabilities.

pub mod audio_recorder;
pub mod tts;
pub mod voice_listener;

// Re-exports for convenience
pub use audio_recorder::{AudioRecorder, AudioStream, DeviceInfo, RecordingConfig as AudioRecordingConfig};
pub use tts::{TextToSpeech, TtsConfig};
pub use voice_listener::{VoiceListener, VoiceListenerConfig};

