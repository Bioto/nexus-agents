use crate::error::{RecorderError, Result};
use crate::services::{AudioRecorder, AudioRecordingConfig as RecordingConfig};
use clap::Args;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Args)]
pub struct RecordArgs {
    /// Duration to record in seconds (default: until Ctrl+C)
    #[arg(short, long)]
    pub duration: Option<u64>,

    /// Output file path
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Sample rate in Hz (default: 48000 for better quality)
    #[arg(short = 'r', long, default_value = "48000")]
    pub sample_rate: u32,

    /// Number of channels (default: 1 - mono)
    #[arg(short, long, default_value = "1")]
    pub channels: u16,

    /// Audio input device name (use --list-devices to see available)
    #[arg(long)]
    pub device: Option<String>,

    /// List all available audio input devices and exit
    #[arg(long)]
    pub list_devices: bool,
}

pub fn run_record(args: RecordArgs) -> Result<()> {
    let recorder = AudioRecorder::new()?;

    // Handle list devices command
    if args.list_devices {
        let devices = recorder.list_input_devices()?;
        println!("📡 Available audio input devices:\n");
        for (i, device) in devices.iter().enumerate() {
            let default_marker = if device.default { " [DEFAULT]" } else { "" };
            println!("  {}. {}{}", i + 1, device.display_name, default_marker);
            if device.display_name != device.name {
                println!("     → {}", device.name);
            }
        }
        println!("\n💡 Tip: Use the technical name (after →) with --device");
        return Ok(());
    }

    // Determine output file path
    let output_path = if let Some(path) = args.output {
        path
    } else {
        // Generate default filename with timestamp in output directory
        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
        PathBuf::from(format!("output/recording_{}.wav", timestamp))
    };

    // Build recording configuration
    let config = RecordingConfig {
        sample_rate: args.sample_rate,
        channels: args.channels,
        duration: args.duration.map(Duration::from_secs),
        device_name: args.device,
    };

    println!("🎤 Starting recording...");
    println!("   Output: {}", output_path.display());
    println!("   Sample rate: {} Hz", config.sample_rate);
    println!("   Channels: {}", config.channels);
    if let Some(ref device) = config.device_name {
        println!("   Device: {}", device);
    }

    if config.duration.is_some() {
        println!("   Duration: {:?}", config.duration);
        println!("\n⏹️  Recording will stop automatically...");
    } else {
        println!("\n⏹️  Press Ctrl+C to stop recording...");
    }

    // Handle Ctrl+C gracefully
    if config.duration.is_none() {
        ctrlc::set_handler(move || {
            println!("\n\n🛑 Stopping recording...");
            std::process::exit(0);
        })
        .map_err(|e| RecorderError::Other(format!("Failed to set Ctrl+C handler: {}", e)))?;
    }

    // Start recording
    recorder.record_to_file(config, &output_path)?;

    println!("\n✅ Recording saved to: {}", output_path.display());
    Ok(())
}
