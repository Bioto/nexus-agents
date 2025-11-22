use crate::error::Result;
use crate::services::AudioRecorder;
use crate::services::RecordingConfig;
use clap::Args;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Args)]
pub struct MonitorArgs {
    /// Duration to record in seconds (default: until Ctrl+C)
    #[arg(short, long)]
    pub duration: Option<u64>,

    /// Output file path
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Sample rate in Hz (default: 48000 for better quality)
    #[arg(short = 'r', long, default_value = "48000")]
    pub sample_rate: u32,

    /// Number of channels (default: 2 - stereo for desktop audio)
    #[arg(short, long, default_value = "2")]
    pub channels: u16,

    /// Audio monitor device name (use --list-devices to see available)
    #[arg(long)]
    pub device: Option<String>,

    /// List all available audio monitor devices and exit
    #[arg(long)]
    pub list_devices: bool,
}

pub fn run_monitor(args: MonitorArgs) -> Result<()> {
    let recorder = AudioRecorder::new()?;

    // Handle list devices command
    if args.list_devices {
        let monitor_devices = recorder.list_monitor_devices()?;
        let output_devices = recorder.list_output_devices()?;
        
        println!("🔊 Available audio output devices:\n");
        for (i, device) in output_devices.iter().enumerate() {
            let default_marker = if device.default { " [DEFAULT]" } else { "" };
            println!("  {}. {}{}", i + 1, device.display_name, default_marker);
            if device.display_name != device.name {
                println!("     → {}", device.name);
            }
            // Show potential monitor source name
            println!("     📡 Monitor: \"Monitor of {}\"", device.name);
        }
        
        if !monitor_devices.is_empty() {
            println!("\n📡 Available monitor/loopback devices:\n");
            for (i, device) in monitor_devices.iter().enumerate() {
                let default_marker = if device.default { " [DEFAULT]" } else { "" };
                println!("  {}. {}{}", i + 1, device.display_name, default_marker);
                if device.display_name != device.name {
                    println!("     → {}", device.name);
                }
            }
        } else {
            println!("\n⚠️  No monitor devices found in input devices.");
            println!("\n💡 Tips:");
            println!("   - Try using the monitor names shown above (e.g., \"Monitor of <output device>\")");
            println!("   - On PulseAudio, you can also try: \"pulse\" or \"Monitor of PulseAudio\"");
            println!("   - On PipeWire, try: \"Monitor of PipeWire\"");
            println!("   - You may need to load PulseAudio monitor module: pactl load-module module-loopback");
        }
        
        println!("\n💡 Tip: Use the technical name (after →) with --device");
        return Ok(());
    }

    // Determine output file path
    let output_path = if let Some(path) = args.output {
        path
    } else {
        // Generate default filename with timestamp
        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
        PathBuf::from(format!("desktop_audio_{}.wav", timestamp))
    };

    // Determine monitor device - create loopback sink if not specified
    let (monitor_device, module_ids, previous_source) = if let Some(device) = args.device {
        (Some(device), Vec::new(), None)
    } else {
        // Create virtual loopback sink for monitoring
        #[cfg(target_os = "linux")]
        {
            // Create loopback sink and set it as default source (so CPAL can access it via default device)
            match AudioRecorder::create_loopback_sink(None) {
                Ok((monitor_name, module_ids)) => {
                    println!("✅ Created virtual loopback sink");
                    println!("   Monitor source: {} (created in PulseAudio)", monitor_name);
                    // Set the monitor source as default so CPAL can access it via default input device
                    match AudioRecorder::set_default_source(&monitor_name) {
                        Ok(prev_source) => {
                            println!("   Set as default source (will be restored after recording)");
                            // Use None to use default input device, which will now be our monitor source
                            (None, module_ids, Some(prev_source))
                        }
                        Err(e) => {
                            println!("⚠️  Warning: Could not set default source: {}", e);
                            println!("   Trying 'pulse' device as fallback...");
                            (Some("pulse".to_string()), module_ids, None)
                        }
                    }
                }
                Err(e) => {
                    println!("⚠️  Warning: Could not create loopback sink: {}", e);
                    println!("   Trying 'pulse' device as fallback...");
                    (Some("pulse".to_string()), Vec::new(), None)
                }
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            (None, Vec::new(), None)
        }
    };

    // Build recording configuration
    let config = RecordingConfig {
        sample_rate: args.sample_rate,
        channels: args.channels,
        duration: args.duration.map(Duration::from_secs),
        device_name: monitor_device,
    };

    println!("🔊 Starting desktop audio monitoring...");
    println!("   Output: {}", output_path.display());
    println!("   Sample rate: {} Hz", config.sample_rate);
    println!("   Channels: {}", config.channels);
    if let Some(ref device) = config.device_name {
        println!("   Device: {}", device);
    } else {
        println!("   Device: Default input device");
    }

    if config.duration.is_some() {
        println!("   Duration: {:?}", config.duration);
        println!("\n⏹️  Recording will stop automatically...");
    } else {
        println!("\n⏹️  Press Ctrl+C to stop recording...");
    }

    // Store module IDs and previous source for cleanup
    let module_ids_for_cleanup = module_ids.clone();
    #[cfg(target_os = "linux")]
    let previous_source_for_cleanup = previous_source.clone();
    
    // Handle Ctrl+C gracefully and clean up loopback sink
    if config.duration.is_none() {
        let module_ids_clone = module_ids_for_cleanup.clone();
        #[cfg(target_os = "linux")]
        let previous_source_clone = previous_source_for_cleanup.clone();
        ctrlc::set_handler(move || {
            println!("\n\n🛑 Stopping recording...");
            // Clean up PulseAudio modules and restore default source
            #[cfg(target_os = "linux")]
            {
                // Restore previous default source if we changed it
                if let Some(ref prev_source) = previous_source_clone {
                    let _ = AudioRecorder::set_default_source(prev_source);
                }
                // Clean up modules
                for module_id in &module_ids_clone {
                    if *module_id > 0 {
                        let _ = AudioRecorder::remove_pulseaudio_module(*module_id);
                    }
                }
            }
            std::process::exit(0);
        })
        .map_err(|e| {
            crate::error::VoiceError::Other(format!("Failed to set Ctrl+C handler: {}", e))
        })?;
    }

    // Start recording
    let result = recorder.record_to_file(config, &output_path);

    // Clean up PulseAudio modules and restore default source after recording
    #[cfg(target_os = "linux")]
    {
        // Restore previous default source if we changed it
        if let Some(ref prev_source) = previous_source_for_cleanup {
            let _ = AudioRecorder::set_default_source(prev_source);
            println!("🔄 Restored previous default source");
        }
        // Clean up modules
        for module_id in &module_ids {
            if *module_id > 0 {
                let _ = AudioRecorder::remove_pulseaudio_module(*module_id);
                println!("🧹 Cleaned up PulseAudio module {}", module_id);
            }
        }
    }

    match result {
        Ok(_) => {
            println!("\n✅ Desktop audio recording saved to: {}", output_path.display());
            Ok(())
        }
        Err(e) => Err(e),
    }
}

