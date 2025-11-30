use crate::error::{RecorderError, Result};
use crate::services::{TextToSpeech, TtsConfig, VoiceListener, VoiceListenerConfig};
use clap::Args;
use std::path::PathBuf;
use std::sync::mpsc;

#[derive(Args)]
pub struct TestVoiceArgs {
    /// Target audio sample rate in Hz.
    #[arg(short = 'r', long, default_value = "16000")]
    pub sample_rate: u32,

    /// Number of recording channels.
    #[arg(short, long, default_value = "1")]
    pub channels: u16,

    /// Audio input device name to record from.
    #[arg(long)]
    pub device: Option<String>,

    /// RMS amplitude threshold for voice detection.
    #[arg(short, long, default_value = "0.02")]
    pub threshold: f32,

    /// Zero-Crossing Rate threshold.
    #[arg(long, default_value = "0.25")]
    pub zcr_threshold: f32,

    /// Minimum detected dominant frequency (Hz).
    #[arg(long, default_value = "85")]
    pub min_voice_freq: f32,

    /// Maximum detected dominant frequency (Hz).
    #[arg(long, default_value = "255")]
    pub max_voice_freq: f32,

    /// Milliseconds of silence before concluding a speech segment.
    #[arg(long, default_value = "5000")]
    pub silence_duration_ms: u32,

    /// Minimum speech duration (ms) required before transcription.
    #[arg(long, default_value = "1000")]
    pub min_speech_ms: u32,

    /// Path to Whisper model file
    #[arg(short, long, default_value = ".models/ggml-small-fp16.bin")]
    pub model: PathBuf,

    /// Text to speak back when the phrase is detected
    #[arg(
        long,
        default_value = "I heard you say this is a test! I heard you say this is a test! I heard you say this is a test! I heard you say this is a test! I heard you say this is a test! I heard you say this is a test! "
    )]
    pub response: String,

    /// Voice to use for TTS response
    #[arg(short = 'V', long)]
    pub voice: Option<String>,

    /// Speech rate for TTS response (0.0 to 1.0)
    #[arg(long, default_value = "0.5")]
    pub rate: f32,

    /// Speech volume for TTS response (0.0 to 1.0)
    #[arg(short = 'v', long, default_value = "1.0")]
    pub volume: f32,

    /// Use WebSocket RPC mode (faster, streaming) instead of HTTP REST
    #[arg(long)]
    pub websocket: bool,

    /// Kyutai TTS server endpoint URL
    #[arg(long)]
    pub endpoint: Option<String>,
}

pub async fn run_test_voice(args: TestVoiceArgs) -> Result<()> {
    // Build voice listener configuration
    let listener_config = VoiceListenerConfig {
        sample_rate: args.sample_rate,
        channels: args.channels,
        device_name: args.device,
        energy_threshold: args.threshold,
        zcr_threshold: args.zcr_threshold,
        min_voice_freq: args.min_voice_freq,
        max_voice_freq: args.max_voice_freq,
        silence_duration_ms: args.silence_duration_ms,
        min_speech_ms: args.min_speech_ms,
        model_path: args.model,
        verbose: false,
    };

    println!(
        "🔄 Loading Whisper model from: {}",
        listener_config.model_path.display()
    );

    // Create voice listener
    let mut listener = VoiceListener::new(listener_config.clone())?;

    println!("🎤 Starting voice test...");
    println!("   Listening for: \"this is a test\"");
    println!("   Will respond with: \"{}\"", args.response);
    println!("\n⏹️  Press Ctrl+C to stop...\n");

    // Handle Ctrl+C
    ctrlc::set_handler(move || {
        println!("\n\n🛑 Stopping...");
        std::process::exit(0);
    })
    .map_err(|e| RecorderError::Other(format!("Failed to set Ctrl+C handler: {}", e)))?;

    // Build TTS configuration
    // Default to WebSocket mode for better performance (can be overridden with --endpoint)
    let endpoint = args.endpoint.clone().or_else(|| {
        // Default to WebSocket for better performance
        Some("ws://localhost:8089/api/tts_streaming".to_string())
    });

    // Determine WebSocket mode: explicit flag, or auto-detect from endpoint URL
    let websocket_mode = args.websocket
        || endpoint
            .as_ref()
            .map(|e| e.starts_with("ws://") || e.starts_with("wss://"))
            .unwrap_or(true); // Default to true (WebSocket) if no endpoint specified

    let tts_config = TtsConfig {
        endpoint,
        websocket: websocket_mode,
        voice: args.voice.clone(),
        rate: Some(args.rate),
        volume: Some(args.volume),
        language: None,
    };

    // Start listening and get transcription channel
    let rx = listener.start_with_channel()?;

    println!("👂 Listening... (say \"this is a test\")\n");

    // Channel to signal when phrase is detected
    let (tx, phrase_rx) = mpsc::channel();

    // Process transcriptions in a separate thread
    let response_text = args.response.clone();
    std::thread::spawn(move || {
        println!("🔍 Debug: Transcription processing thread started");
        while let Ok(result) = rx.recv() {
            let raw_text = result.text.trim();
            let transcription = raw_text.to_lowercase();

            println!("💬 Heard (raw): \"{}\"", raw_text);
            println!("🔍 Debug: Lowercase version: \"{}\"", transcription);
            println!("🔍 Debug: Looking for: \"this is a test\"");
            println!(
                "🔍 Debug: Contains check: {}",
                transcription.contains("this is a test")
            );

            // Check if transcription contains "this is a test"
            if transcription.contains("this is a test") {
                println!("\n✅ Phrase detected!\n");
                let _ = tx.send(response_text);
                break;
            } else {
                println!("🔍 Debug: Phrase not found, continuing to listen...\n");
            }
        }
        println!("🔍 Debug: Transcription processing thread exiting");
    });

    // Run listen() in a separate thread (it blocks)
    // Don't wait for it - we'll start TTS immediately when phrase is detected
    let _listen_handle = std::thread::spawn(move || {
        println!("🔍 Debug: Listen thread started");
        let result = listener.listen(|_metrics| {
            // We don't need verbose metrics for this command
        });
        println!("🔍 Debug: Listen thread finished: {:?}", result);
    });

    // Wait for phrase detection
    println!("🔍 Debug: Waiting for phrase detection...");
    let response_text = tokio::task::spawn_blocking(move || {
        println!("🔍 Debug: Blocking on phrase_rx.recv()...");
        phrase_rx.recv().ok()
    })
    .await
    .map_err(|e| RecorderError::Other(format!("Task error: {:?}", e)))?;
    println!(
        "🔍 Debug: Received from phrase_rx: {:?}",
        response_text.is_some()
    );

    // Speak the response immediately if phrase was detected (don't wait for listener to stop)
    if let Some(text) = response_text {
        println!("🗣️  Speaking response...\n");
        let mut tts = TextToSpeech::with_config(tts_config.clone())?;
        // Start speaking immediately - streaming will begin as soon as first chunk arrives
        tts.speak_sync(&text, true, &tts_config).await?;
        println!("✅ Response spoken successfully!");
    } else {
        println!("⚠️  Listener stopped without detecting phrase");
    }

    Ok(())
}
