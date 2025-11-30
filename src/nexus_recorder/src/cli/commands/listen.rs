use crate::error::{RecorderError, Result};
use crate::services::{AudioRecorder, VoiceListener, VoiceListenerConfig};
use clap::Args;
use std::path::PathBuf;

#[derive(Args)]
pub struct ListenArgs {
    /// Target audio sample rate in Hz.
    ///
    /// Default: 16000 (recommended for Whisper compatibility; values above this may waste compute, below reduces voice quality).
    /// Use typical 44100/48000 for hi-fi, but 16000 is optimal for speech recognition.
    #[arg(short = 'r', long, default_value = "16000")]
    pub sample_rate: u32,

    /// Number of recording channels.
    ///
    /// Set to 1 for mono input (default and optimal for ASR); set to 2 for stereo.
    /// If your microphone uses two channels, but only needs one, prefer mono to reduce CPU and memory use.
    #[arg(short, long, default_value = "1")]
    pub channels: u16,

    /// Audio input device name to record from.
    ///
    /// Specify a substring or full device name. Defaults to system default.
    /// List available devices via `--list-devices` to pick the precise device (use value after → for technical name).
    #[arg(long)]
    pub device: Option<String>,

    /// Root mean squared (RMS) amplitude threshold for basic voice detection.
    ///
    /// Range: 0.0–1.0 (normalized float). Lower detects very soft voices, higher ignores background/electrical noise.
    /// Default: 0.02 (good for normal close-mic). If picking up false triggers, raise this slightly.
    #[arg(short, long, default_value = "0.02")]
    pub threshold: f32,

    /// Zero-Crossing Rate threshold to further suppress non-voice events (like keyboard clicks).
    ///
    /// Range: 0.0–1.0. Lower values are stricter; higher means more events pass as "voice".
    /// Only segments with ZCR below this are eligible for transcription events.
    /// Default: 0.25 (recommended; tune for your environment if false positives/negatives).
    #[arg(long, default_value = "0.25")]
    pub zcr_threshold: f32,

    /// Minimum detected dominant frequency (Hz) allowed for a segment to count as voice.
    ///
    /// Useful for filtering out low-frequency non-speech such as hum or rumble.
    /// Default: 85 Hz (close to average adult male vocal floor; raise to ignore lower-frequency noise).
    #[arg(long, default_value = "85")]
    pub min_voice_freq: f32,

    /// Maximum detected dominant frequency (Hz) allowed to count as voice.
    ///
    /// Used to filter out high-frequency events like hisses and sharp clicks.
    /// Default: 255 Hz (covers normal speech; above this is unlikely to be voice).
    #[arg(long, default_value = "255")]
    pub max_voice_freq: f32,

    /// Milliseconds of silence before concluding a speech segment.
    ///
    /// If the voice detector sees at least this much silence, it considers a phrase/utterance finished and triggers a transcription.
    /// Default: 5000 ms (5s); reduce for shorter phrases, increase for longer natural pauses.
    #[arg(long, default_value = "5000")]
    pub silence_duration_ms: u32,

    /// Minimum speech duration (ms) required before a chunk is transcribed.
    ///
    /// Ignore segments shorter than this threshold (eliminates false starts and coughs).
    /// Default: 1000 ms (one second); decrease to capture very short utterances, increase for only full sentences.
    #[arg(long, default_value = "1000")]
    pub min_speech_ms: u32,

    /// Path to Whisper model file
    #[arg(short, long, default_value = ".models/ggml-small-fp16.bin")]
    pub model: PathBuf,

    /// List all available audio input devices and exit
    #[arg(long)]
    pub list_devices: bool,

    /// Show detailed audio metrics for debugging
    #[arg(short, long)]
    pub verbose: bool,
}

pub fn run_listen(args: ListenArgs) -> Result<()> {
    // Handle list devices command
    if args.list_devices {
        let recorder = AudioRecorder::new()?;
        let devices = recorder.list_input_devices()?;

        // Separate CPAL-enumerated devices from ALSA-only devices
        let mut cpal_devices = Vec::new();
        let mut alsa_only_devices = Vec::new();

        #[cfg(target_os = "linux")]
        {
            let alsa_devices = AudioRecorder::list_alsa_devices();
            let cpal_device_names: std::collections::HashSet<String> =
                devices.iter().map(|d| d.name.clone()).collect();

            for device in devices.iter() {
                // Check if this device name matches an ALSA pattern
                let is_alsa_pattern = device.name.contains("CARD=")
                    || device.name.contains("hw:")
                    || device.name.contains("sysdefault:")
                    || device.name.contains("plughw:")
                    || device.name.contains("front:")
                    || device.name.contains("dsnoop:");

                if is_alsa_pattern && !cpal_device_names.contains(&device.name) {
                    // This is an ALSA device that CPAL enumerated
                    cpal_devices.push(device.clone());
                } else if !is_alsa_pattern {
                    // This is a CPAL device (pulse, pipewire, etc.)
                    cpal_devices.push(device.clone());
                }
            }

            // Find ALSA devices that aren't in CPAL enumeration
            for alsa_device in alsa_devices {
                if !cpal_device_names.contains(&alsa_device.name) {
                    alsa_only_devices.push(alsa_device);
                }
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            cpal_devices = devices;
        }

        println!("📡 Available audio input devices:\n");
        for (i, device) in cpal_devices.iter().enumerate() {
            let default_marker = if device.default { " [DEFAULT]" } else { "" };
            println!("  {}. {}{}", i + 1, device.display_name, default_marker);
            if device.display_name != device.name {
                println!("     → {}", device.name);
            }
        }

        #[cfg(target_os = "linux")]
        {
            if !alsa_only_devices.is_empty() {
                println!("\n📡 Additional ALSA devices (not enumerated by CPAL):\n");
                let start_num = cpal_devices.len() + 1;
                for (i, device) in alsa_only_devices.iter().enumerate() {
                    println!("  {}. {}{}", start_num + i, device.display_name, "");
                    if device.display_name != device.name {
                        println!("     → {}", device.name);
                    }
                    println!("     ⚠️  May require exact ALSA device name to use");
                }
            }
        }

        println!("\n💡 Tip: Use the technical name (after →) with --device");
        println!("💡 If your device isn't listed, try: --device sysdefault:CARD=<CardName>");
        return Ok(());
    }

    // Build voice listener configuration
    let config = VoiceListenerConfig {
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
        verbose: args.verbose,
    };

    println!(
        "🔄 Loading Whisper model from: {}",
        config.model_path.display()
    );

    // Create voice listener
    let mut listener = VoiceListener::new(config.clone())?;

    println!("🎤 Starting continuous listening...");
    println!("   Sample rate: {} Hz", config.sample_rate);
    println!("   Channels: {}", config.channels);
    println!("   Energy threshold: {}", config.energy_threshold);
    println!(
        "   Voice frequency range: {}-{} Hz",
        config.min_voice_freq, config.max_voice_freq
    );
    println!("   ZCR threshold: {}", config.zcr_threshold);
    if let Some(ref device) = config.device_name {
        println!("   Device: {}", device);
    }
    println!("\n⏹️  Press Ctrl+C to stop...\n");

    // Handle Ctrl+C - we'll rely on the listener's internal running flag
    ctrlc::set_handler(move || {
        println!("\n\n🛑 Stopping...");
        std::process::exit(0);
    })
    .map_err(|e| RecorderError::Other(format!("Failed to set Ctrl+C handler: {}", e)))?;

    // Start voice listener with transcription callback
    listener.start(|transcription| {
        println!("💬 {}", transcription);
        println!();
    })?;

    println!("👂 Listening... (speak to transcribe)\n");
    println!("💡 Using advanced voice detection (filters out typing/clicks)");
    println!(
        "   Will wait {:.1}s of silence before transcribing",
        config.silence_duration_ms as f32 / 1000.0
    );

    if config.verbose {
        println!("\n📊 Verbose mode - showing metrics:");
        println!("   ✓/✗ = overall detection | E = energy | Z = ZCR | F = frequency");
        println!(
            "   Thresholds: E>{:.3}, Z<{:.3}, F={}-{}Hz",
            config.energy_threshold,
            config.zcr_threshold,
            config.min_voice_freq,
            config.max_voice_freq
        );
    }
    println!();

    let mut last_voice_detected = false;
    let mut voice_start_metrics = None;

    // Process audio with voice detection
    listener.listen(|metrics| {
        if config.verbose {
            let status = if metrics.is_voice { "✓" } else { "✗" };
            let energy_ok = if metrics.energy > config.energy_threshold {
                "✓"
            } else {
                "✗"
            };
            let zcr_ok = if metrics.zero_crossing_rate < config.zcr_threshold {
                "✓"
            } else {
                "✗"
            };
            let freq_ok = if metrics.dominant_frequency == 0.0
                || (metrics.dominant_frequency >= config.min_voice_freq
                    && metrics.dominant_frequency <= config.max_voice_freq)
            {
                "✓"
            } else {
                "✗"
            };

            println!(
                "{} E:{:.3}{} Z:{:.3}{} F:{:3.0}Hz{} | Voice: {}",
                status,
                metrics.energy,
                energy_ok,
                metrics.zero_crossing_rate,
                zcr_ok,
                metrics.dominant_frequency,
                freq_ok,
                if metrics.is_voice { "YES" } else { "NO" }
            );
        }

        // Track voice detection transitions
        if metrics.is_voice && !last_voice_detected {
            voice_start_metrics = Some((
                metrics.energy,
                metrics.zero_crossing_rate,
                metrics.dominant_frequency,
            ));
            if let Some((e, z, f)) = voice_start_metrics {
                println!(
                    "🎙️  Voice detected (energy: {:.3}, zcr: {:.3}, freq: {:.0}Hz)...",
                    e, z, f
                );
            }
        }
        last_voice_detected = metrics.is_voice;
    })?;

    Ok(())
}
