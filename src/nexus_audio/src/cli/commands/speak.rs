use crate::error::Result;
use crate::services::{TextToSpeech, TtsConfig};
use clap::Args;

#[derive(Args)]
pub struct SpeakArgs {
    /// Text to speak
    #[arg(required = true)]
    pub text: String,

    /// Voice to use (use --list-voices to see available voices)
    #[arg(short = 'V', long)]
    pub voice: Option<String>,

    /// Speech rate (0.0 to 1.0, where 0.5 is normal speed)
    /// Default: 0.5
    #[arg(short, long, default_value = "0.5")]
    pub rate: f32,

    /// Speech volume (0.0 to 1.0)
    /// Default: 1.0
    #[arg(short = 'v', long, default_value = "1.0")]
    pub volume: f32,

    /// Language code (e.g., "en-US", "en-GB")
    #[arg(short, long)]
    pub language: Option<String>,

    /// Don't wait for speech to complete (speak asynchronously)
    #[arg(short, long)]
    pub no_wait: bool,

    /// Interrupt any currently playing speech
    #[arg(short, long)]
    pub interrupt: bool,

    /// List all available voices and exit
    #[arg(long)]
    pub list_voices: bool,

    /// Use WebSocket RPC mode (faster, streaming) instead of HTTP REST
    #[arg(long)]
    pub websocket: bool,

    /// Kyutai TTS server endpoint URL
    #[arg(long)]
    pub endpoint: Option<String>,
}

pub async fn run_speak(args: SpeakArgs) -> Result<()> {
    // Handle list voices command
    if args.list_voices {
        let endpoint = args.endpoint.clone();
        let websocket_mode = args.websocket
            || endpoint
                .as_ref()
                .map(|e| e.starts_with("ws://") || e.starts_with("wss://"))
                .unwrap_or(false);

        let config = TtsConfig {
            endpoint,
            websocket: websocket_mode,
            voice: None,
            rate: None,
            volume: None,
            language: None,
        };
        let tts = TextToSpeech::with_config(config)?;
        let voices = tts.list_voices().await?;

        println!("🗣️  Available TTS voices:\n");
        if voices.is_empty() {
            println!("  No voices found.");
        } else {
            for (i, voice) in voices.iter().enumerate() {
                let gender_str = voice
                    .gender
                    .as_ref()
                    .map(|g| format!(" ({})", g))
                    .unwrap_or_default();
                println!(
                    "  {}. {} - {}{}",
                    i + 1,
                    voice.name,
                    voice.language,
                    gender_str
                );
            }
        }
        println!("\n💡 Tip: Use --voice with the voice name to select a specific voice");
        return Ok(());
    }

    // Validate rate and volume
    if args.rate < 0.0 || args.rate > 1.0 {
        return Err(crate::error::VoiceError::Configuration(
            "Rate must be between 0.0 and 1.0".to_string(),
        ));
    }

    if args.volume < 0.0 || args.volume > 1.0 {
        return Err(crate::error::VoiceError::Configuration(
            "Volume must be between 0.0 and 1.0".to_string(),
        ));
    }

    // Build TTS configuration (clone values since we'll use them again)
    // Determine mode: if endpoint starts with ws:// or wss://, use WebSocket mode
    let endpoint = args.endpoint.clone().or_else(|| {
        if args.websocket {
            Some("ws://localhost:8089/api/tts_streaming".to_string())
        } else {
            // Default to Moshi server (HTTP)
            Some("http://localhost:8089/api/tts_streaming".to_string())
        }
    });

    let websocket_mode = args.websocket
        || endpoint
            .as_ref()
            .map(|e| e.starts_with("ws://") || e.starts_with("wss://"))
            .unwrap_or(false);

    let config = TtsConfig {
        endpoint,
        websocket: websocket_mode,
        voice: args.voice.clone(),
        rate: Some(args.rate),
        volume: Some(args.volume),
        language: args.language.clone(),
    };

    // Create TTS instance (clone config since we'll use it again)
    let tts_config = config.clone();
    let mut tts = TextToSpeech::with_config(config)?;

    // Show current settings
    if let Ok(Some(voice)) = tts.current_voice() {
        println!("🗣️  Voice: {}", voice.name);
    }
    if let Ok(rate) = tts.rate() {
        println!("⚡ Rate: {:.2}", rate);
    }
    if let Ok(volume) = tts.volume() {
        println!("🔊 Volume: {:.2}", volume);
    }
    println!("\n💬 Speaking: \"{}\"", args.text);
    if args.no_wait {
        println!("   (asynchronous mode - not waiting for completion)");
    }
    println!();

    // Speak the text (use same config as above)

    if args.no_wait {
        tts.speak(&args.text, args.interrupt, &tts_config).await?;
        println!("✅ Speech started (running in background)");
    } else {
        tts.speak_sync(&args.text, args.interrupt, &tts_config)
            .await?;
        println!("✅ Speech completed");
    }

    Ok(())
}
