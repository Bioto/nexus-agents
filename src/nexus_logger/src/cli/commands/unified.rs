use crate::error::Result;
use crate::services::capture::InputEvent;
use crate::services::unified_recording::{
    DefaultEventCallback, EventCallback, UnifiedRecordingConfig,
    UnifiedRecordingService, ScreenRecordingConfig, InputCaptureConfig,
};
use chrono::DateTime;
use clap::Args;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// CLI arguments for the unified recording subcommand.
#[derive(Args)]
pub struct UnifiedArgs {
    /// Output video file path
    #[arg(short = 'o', long, default_value = "recording.mp4")]
    pub output: PathBuf,

    /// Frame rate for video recording
    #[arg(short = 'f', long, default_value = "30")]
    pub framerate: u32,

    /// Duration in seconds (0 = until stopped)
    #[arg(short = 'd', long, default_value = "0")]
    pub duration: u64,

    /// Monitor index (None = primary)
    #[arg(short = 'm', long)]
    pub monitor: Option<usize>,

    /// Disable audio recording
    #[arg(long)]
    pub no_audio: bool,

    /// Database path for storing events
    #[arg(short = 'D', long, default_value = "events.db")]
    pub database: PathBuf,

    /// Events output file (None = stdout)
    #[arg(short = 'e', long)]
    pub events: Option<PathBuf>,

    /// Events output format: json, text, or both
    #[arg(long, default_value = "text")]
    pub events_format: String,

    /// Disable keyboard capture
    #[arg(long)]
    pub no_keyboard: bool,

    /// Disable mouse capture
    #[arg(long)]
    pub no_mouse: bool,

    /// Enable mouse move capture (can be verbose)
    #[arg(long)]
    pub mouse_moves: bool,

    /// Enable verbose event callbacks (prints events with video timestamps)
    #[arg(long)]
    pub verbose: bool,
}

/// Runs the unified recording command based on args.
pub async fn run_unified(args: UnifiedArgs) -> Result<()> {
    // Validate events format
    match args.events_format.as_str() {
        "json" | "text" | "both" => {}
        _ => {
            return Err(crate::error::LoggerError::Configuration(
                "Events format must be 'json', 'text', or 'both'".to_string(),
            ));
        }
    }

    // Setup signal handler for graceful shutdown
    let running = Arc::new(AtomicBool::new(false));
    let r = running.clone();
    ctrlc::set_handler(move || {
        println!("\n🛑 Stopping unified recording...");
        r.store(true, Ordering::SeqCst);
    })
    .map_err(|e| {
        crate::error::LoggerError::Other(format!("Failed to set signal handler: {}", e))
    })?;

    // Create configuration
    let config = UnifiedRecordingConfig {
        screen_config: ScreenRecordingConfig {
            output_path: args.output.clone(),
            framerate: args.framerate,
            duration_secs: if args.duration > 0 {
                Some(args.duration)
            } else {
                None
            },
            monitor_index: args.monitor,
            include_audio: !args.no_audio,
        },
        input_config: InputCaptureConfig {
            output_file: args.events.clone(),
            format: args.events_format.clone(),
        },
        database_path: args.database.clone(),
        capture_keyboard: !args.no_keyboard,
        capture_mouse: !args.no_mouse,
        capture_mouse_moves: args.mouse_moves,
    };

    println!("🎬 Starting unified recording...");
    println!("   Video: {}", args.output.display());
    if let Some(ref events) = args.events {
        println!("   Events: {}", events.display());
    } else {
        println!("   Events: stdout");
    }
    println!("   Database: {}", args.database.display());
    println!("   Frame rate: {} fps", args.framerate);
    if args.duration > 0 {
        println!("   Duration: {} seconds", args.duration);
    } else {
        println!("   Duration: until stopped");
    }
    println!("   Keyboard: {}", if !args.no_keyboard { "✓" } else { "✗" });
    println!("   Mouse: {}", if !args.no_mouse { "✓" } else { "✗" });
    println!("   Mouse moves: {}", if args.mouse_moves { "✓" } else { "✗" });
    println!("   Audio: {}", if !args.no_audio { "✓" } else { "✗" });
    if args.verbose {
        println!("   Verbose callbacks: ✓");
    }
    println!("\nPress Ctrl+C to stop\n");

    // Create service with appropriate callback
    let service = if args.verbose {
        UnifiedRecordingService::with_callback(config, VerboseEventCallback)
    } else {
        UnifiedRecordingService::with_callback(config, DefaultEventCallback)
    };

    // Start recording
    let session = service.start_recording(running.clone()).await?;
    let session_id = session.session_id().to_string();
    let recording_start = session.recording_start();

    // Handle duration if specified
    if args.duration > 0 {
        // Wait for the specified duration, then stop
        tokio::time::sleep(tokio::time::Duration::from_secs(args.duration)).await;
        session.stop();
    }

    // Wait for recording to complete
    session.wait().await?;

    println!("\n✅ Unified recording complete!");
    println!("   Session ID: {}", session_id);
    println!("   Started at: {}", recording_start);
    println!("   Video: {}", args.output.display());
    println!("   Database: {}", args.database.display());

    Ok(())
}

/// Verbose callback that prints events with video timestamps.
struct VerboseEventCallback;

impl EventCallback for VerboseEventCallback {
    fn on_keyboard_event(
        &self,
        event: &InputEvent,
        video_timestamp: f64,
        _recording_start: DateTime<chrono::Utc>,
    ) -> bool {
        if let InputEvent::Keyboard { key, pressed, .. } = event {
            let action = if *pressed { "PRESS" } else { "RELEASE" };
            println!(
                "🎬 [{:8.3}s] KEYBOARD {}: {}",
                video_timestamp, action, key
            );
        }
        true
    }

    fn on_mouse_event(
        &self,
        event: &InputEvent,
        video_timestamp: f64,
        _recording_start: DateTime<chrono::Utc>,
    ) -> bool {
        match event {
            InputEvent::Mouse {
                event_type,
                button,
                x,
                y,
                ..
            } => {
                match event_type.as_str() {
                    "click" => {
                        println!(
                            "🎬 [{:8.3}s] MOUSE CLICK: {} at ({}, {})",
                            video_timestamp,
                            button.as_ref().unwrap_or(&"unknown".to_string()),
                            x.unwrap_or(0),
                            y.unwrap_or(0)
                        );
                    }
                    "release" => {
                        println!(
                            "🎬 [{:8.3}s] MOUSE RELEASE: {}",
                            video_timestamp,
                            button.as_ref().unwrap_or(&"unknown".to_string())
                        );
                    }
                    "move" => {
                        println!(
                            "🎬 [{:8.3}s] MOUSE MOVE: ({}, {})",
                            video_timestamp,
                            x.unwrap_or(0),
                            y.unwrap_or(0)
                        );
                    }
                    _ => {
                        println!(
                            "🎬 [{:8.3}s] MOUSE {}: {:?}",
                            video_timestamp, event_type, button
                        );
                    }
                }
            }
            _ => {}
        }
        true
    }
}

