mod commands;

use clap::{Parser, Subcommand};

/// Nexus Voice - Voice Interface for AI Agents
#[derive(Parser)]
#[command(name = "nexus-audio")]
#[command(about = "A voice interface for interacting with AI agents", long_about = None)]
#[command(version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Record audio to a WAV file
    Record(commands::record::RecordArgs),
    /// Continuously listen and transcribe speech using Whisper
    Listen(commands::listen_simple::ListenArgs),
    /// Convert text to speech and play it
    Speak(commands::speak::SpeakArgs),
    /// Listen for "this is a test" and respond with TTS
    TestVoice(commands::test_voice::TestVoiceArgs),
}

// Re-export command handlers for convenience
pub use commands::record::run_record;
// pub use commands::listen::run_listen;  // Temporarily using simple version
pub use commands::listen_simple::run_listen;
pub use commands::speak::run_speak;
pub use commands::test_voice::run_test_voice;
