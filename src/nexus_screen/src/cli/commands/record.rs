use crate::error::{Result, ScreenError};
use crate::services::{ScreenRecorder, RecordingConfig};
use clap::Args;
use std::path::PathBuf;
use std::time::Duration;
use xcap::Monitor;

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
    // Handle --list-monitors flag
    if args.list_monitors {
        list_available_monitors()?;
        return Ok(());
    }

    let recorder = ScreenRecorder::new()?;

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
        fps: args.fps,
        duration: args.duration.map(Duration::from_secs),
        output_path: output_path.clone(),
        include_audio: !args.no_audio,
        monitor_index: args.monitor,
    };

    println!("🎥 Starting screen recording...");
    println!("   Output: {}", config.output_path.display());
    println!("   Frame rate: {} fps", config.fps);
    println!("   Audio: {}", if config.include_audio { "enabled" } else { "disabled" });
    if let Some(idx) = config.monitor_index {
        println!("   Monitor: {}", idx);
    } else {
        println!("   Monitor: Primary");
    }

    if config.duration.is_some() {
        println!("   Duration: {:?}", config.duration);
        println!("\n⏹️  Recording will stop automatically...");
    } else {
        println!("\n⏹️  Press Ctrl+C to stop recording...");
    }

    // Handle Ctrl+C gracefully
    // Note: We don't set a handler here since the recording loop in the service
    // uses an AtomicBool that can be checked. The Ctrl+C will naturally terminate
    // the process, and the service will handle cleanup.

    // Start recording
    recorder.record_to_file(config)?;

    println!("\n✅ Recording saved to: {}", output_path.display());
    Ok(())
}

/// List all available monitors
fn list_available_monitors() -> Result<()> {
    let monitors = Monitor::all()
        .map_err(|e| ScreenError::ScreenCapture(format!("Failed to get monitors: {}", e)))?;
    
    if monitors.is_empty() {
        println!("No monitors found.");
        return Ok(());
    }
    
    println!("📺 Available monitors:");
    println!();
    
    for (idx, monitor) in monitors.iter().enumerate() {
        let width = monitor.width().unwrap_or(0);
        let height = monitor.height().unwrap_or(0);
        let x = monitor.x().unwrap_or(0);
        let y = monitor.y().unwrap_or(0);
        
        println!("Monitor {}: {}x{} at position ({}, {})", 
            idx, width, height, x, y);
        
        // Try to get monitor name if available
        if let Ok(name) = monitor.name() {
            println!("  Name: {}", name);
        }
        
        // Mark if it's the first monitor (typically primary)
        if idx == 0 {
            println!("  Primary: Yes (first monitor)");
        }
        
        println!();
    }
    
    println!("To record a specific monitor, use: --monitor <index>");
    println!("Example: cargo run -p nexus-screen -- record --monitor 1");
    
    Ok(())
}

