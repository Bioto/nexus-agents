use crate::error::{Result, ScreenError};
use crate::services::{ScreenRecorder, RecordingConfig};
use clap::Args;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

#[derive(Args)]
pub struct RecordArgs {
    /// Duration to record in seconds (default: until Ctrl+C)
    #[arg(short, long)]
    pub duration: Option<u64>,

    /// Output file path
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Frame rate in fps (default: 30, note: actual capture rate may be lower)
    #[arg(short = 'f', long, default_value = "60")]
    pub fps: u32,
    
    /// Capture as fast as possible (ignore target FPS, maximize frame count)
    #[arg(long)]
    pub fast: bool,

    /// Disable audio capture
    #[arg(long)]
    pub no_audio: bool,

    /// Monitor index to record (0-based, default: primary monitor)
    #[arg(short = 'm', long)]
    pub monitor: Option<usize>,

    /// List available monitors and exit
    #[arg(short = 'l', long)]
    pub list_monitors: bool,
}

pub fn run_record(args: RecordArgs) -> Result<()> {
    // Handle list monitors command
    if args.list_monitors {
        let monitors = xcap::Monitor::all()
            .map_err(|e| ScreenError::Screen(format!("Failed to enumerate monitors: {}", e)))?;
        println!("🖥️  Available monitors:\n");
        for (i, monitor) in monitors.iter().enumerate() {
            let is_primary = monitor.is_primary().unwrap_or(false);
            let primary_marker = if is_primary { " [PRIMARY]" } else { "" };
            let width = monitor.width().unwrap_or(0);
            let height = monitor.height().unwrap_or(0);
            println!("  {}. {}x{}{}", i, width, height, primary_marker);
        }
        println!("\n💡 Tip: Use -m/--monitor to select a monitor by index");
        return Ok(());
    }

    // Determine output file path
    let output_path = if let Some(path) = args.output {
        path
    } else {
        // Generate default filename with timestamp
        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
        PathBuf::from(format!("screen_recording_{}.mp4", timestamp))
    };

    // Build recording configuration
    let config = RecordingConfig {
        framerate: args.fps,
        duration_secs: args.duration,
        output_path,
        monitor_index: args.monitor,
        include_audio: !args.no_audio,
        fast: args.fast,
    };

    // Create recorder with config
    let recorder = ScreenRecorder::new_with_config(config.clone())
        .map_err(|e| ScreenError::Screen(format!("Failed to initialize recorder: {}", e)))?;

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
        .map_err(|e| {
            ScreenError::Other(format!("Failed to set Ctrl+C handler: {}", e))
        })?;
    }

    // Start recording
    recorder.record(config, stop_signal)
        .map_err(|e| ScreenError::VideoEncoding(format!("Recording failed: {}", e)))?;

    println!("\n✅ Recording saved successfully!");
    Ok(())
}
