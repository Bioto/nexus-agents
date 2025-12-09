use crate::error::{RecorderError, Result};
use crate::services::{ScreenRecorder, ScreenRecordingConfig as RecordingConfig};
use clap::Args;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

/// CLI arguments for the record subcommand.
#[derive(Args)]
pub struct RecordArgs {
    /// Duration to record in seconds (default: until Ctrl+C)
    #[arg(short, long)]
    pub duration: Option<u64>,

    /// Output file path
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Frame rate in fps (default: 60)
    #[arg(short = 'f', long, default_value = "60")]
    pub fps: u32,

    /// Capture as fast as possible (ignore target FPS)
    #[arg(long)]
    pub fast: bool,

    /// Disable audio capture
    #[arg(long)]
    pub no_audio: bool,

    /// Monitor index to record (0-based, default: primary)
    #[arg(short = 'm', long)]
    pub monitor: Option<usize>,

    /// List available monitors and exit
    #[arg(short = 'l', long)]
    pub list_monitors: bool,
}

/// Runs the screen recording command based on args.
pub async fn run_record(args: RecordArgs) -> Result<()> {
    // Handle list monitors command
    if args.list_monitors {
        match ScreenRecorder::list_monitors() {
            Ok(monitors) => {
                println!("🖥️  Available monitors:\n");
                for monitor in &monitors {
                    let primary_marker = if monitor.is_primary { " [PRIMARY]" } else { "" };
                    println!(
                        "  {}. {}{}",
                        monitor.index, monitor.display_name, primary_marker
                    );
                }
                println!("\n💡 Tip: Use -m/--monitor with the index number (e.g., -m 0 or -m 1) to select a monitor");
            }
            Err(e) => {
                eprintln!("❌ Failed to enumerate monitors: {}", e);
                #[cfg(target_os = "linux")]
                {
                    eprintln!("\nNote: Make sure xrandr is installed (sudo apt-get install x11-xserver-utils)");
                }
                #[cfg(target_os = "macos")]
                {
                    eprintln!("\nNote: Make sure ffmpeg is installed with avfoundation support");
                }
                return Err(RecorderError::Other(format!(
                    "Failed to list monitors: {}",
                    e
                )));
            }
        }
        return Ok(());
    }

    // Determine output file path
    let output_path = if let Some(path) = args.output {
        path
    } else {
        // Generate default filename with timestamp in output directory
        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
        PathBuf::from(format!("output/screen_recording_{}.mp4", timestamp))
    };

    // Build recording configuration
    let config = RecordingConfig {
        framerate: args.fps,
        duration_secs: args.duration,
        output_path,
        monitor_index: args.monitor,
        window_id: None,
        window_title: None,
        include_audio: !args.no_audio,
        fast: args.fast,
        segment_duration_secs: None, // No video segmentation in CLI for now
    };

    // Create recorder with config
    let recorder = ScreenRecorder::new_with_config(config.clone()).map_err(|e| {
        RecorderError::ScreenCapture(format!("Failed to initialize recorder: {}", e))
    })?;

    println!("🎬 Starting screen recording...");
    println!("   Output: {}", config.output_path.display());
    if config.fast {
        println!("   Mode: Fast (capture as fast as possible)");
    } else {
        println!("   Frame rate: {} fps (target)", config.framerate);
    }
    if let Some(idx) = config.monitor_index {
        println!("   Monitor: {}", idx);
    }
    if config.include_audio {
        println!("   Audio: enabled");
    } else {
        println!("   Audio: disabled");
    }

    if config.duration_secs.is_some() {
        println!("   Duration: {:?} seconds", config.duration_secs);
        println!("\n⏹️  Recording will stop automatically...");
    } else {
        println!("\n⏹️  Press Ctrl+C to stop recording...");
    }

    // Create stop signal
    let stop_signal = Arc::new(AtomicBool::new(false));
    let stop_signal_clone = Arc::clone(&stop_signal);

    // Handle Ctrl+C gracefully
    if config.duration_secs.is_none() {
        ctrlc::set_handler(move || {
            println!("\n\n🛑 Stopping recording...");
            stop_signal_clone.store(true, std::sync::atomic::Ordering::Relaxed);
        })
        .map_err(|e| RecorderError::Other(format!("Failed to set Ctrl+C handler: {}", e)))?;
    }

    // Start recording in a blocking task to avoid blocking the async runtime
    let result = tokio::task::spawn_blocking(move || recorder.record(config, stop_signal)).await;

    match result {
        Ok(record_res) => {
            record_res
                .map_err(|e| RecorderError::VideoEncoding(format!("Recording failed: {}", e)))?;
        }
        Err(e) => {
            return Err(RecorderError::Other(format!(
                "Recording task failed: {}",
                e
            )));
        }
    }

    println!("\n✅ Recording saved successfully!");
    Ok(())
}
