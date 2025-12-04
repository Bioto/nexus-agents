mod commands;

use clap::{Parser, Subcommand};

/// Nexus Recorder - Unified Recording Interface for AI Agents
#[derive(Parser)]
#[command(name = "nexus-recorder")]
#[command(about = "Unified recording interface for audio, screen, and input capture", long_about = None)]
#[command(version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    // Audio commands
    /// Record audio to a WAV file
    Record(commands::record_audio::RecordArgs),
    /// Monitor and record desktop audio output
    Monitor(commands::monitor::MonitorArgs),
    /// Continuously listen and transcribe speech using Whisper
    Listen(commands::listen::ListenArgs),
    /// Convert text to speech and play it
    Speak(commands::speak::SpeakArgs),
    /// Listen for "this is a test" and respond with TTS
    TestVoice(commands::test_voice::TestVoiceArgs),

    // Screen commands
    /// Capture a screenshot of the screen
    Screenshot(commands::screenshot::ScreenshotArgs),
    /// Record the screen to a video file
    RecordScreen(commands::record_screen::RecordArgs),

    // Input capture commands
    /// Capture keyboard and mouse input
    Capture(commands::capture::CaptureArgs),

    // Unified recording
    /// Unified recording (screen + input capture with callbacks)
    Unified(commands::unified::UnifiedArgs),

    // Report
    /// Generate a report of all collected data from ClickHouse
    Report(commands::report::ReportArgs),

    // Webcam splitter
    /// Split webcam to multiple virtual cameras (requires v4l2loopback)
    Splitter(commands::splitter::SplitterArgs),
}

// Re-export command handlers for convenience
// Audio commands
pub use commands::listen::run_listen;
pub use commands::monitor::run_monitor;
pub use commands::record_audio::run_record;
pub use commands::speak::run_speak;
pub use commands::test_voice::run_test_voice;

// Screen commands
pub use commands::record_screen::run_record as run_record_screen;
pub use commands::screenshot::run_screenshot;

// Input capture commands
pub use commands::capture::run_capture;

// Unified recording
pub use commands::unified::run_unified;

// Report
pub use commands::report::run_report;

// Webcam splitter
pub use commands::splitter::run_splitter;

