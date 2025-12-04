//! CLI command for webcam splitting.

use crate::error::Result;
use crate::services::webcam::splitter::{SplitterConfig, WebcamSplitter};
use clap::Args;
use log::info;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Split a webcam feed to multiple virtual cameras.
#[derive(Args, Debug)]
pub struct SplitterArgs {
    /// Input device path (real camera)
    #[arg(short, long, default_value = "/dev/video0")]
    pub input: String,

    /// Output device paths (virtual cameras, comma-separated)
    #[arg(short, long, default_value = "/dev/video10,/dev/video11")]
    pub output: String,

    /// Frame rate
    #[arg(short, long, default_value = "30")]
    pub framerate: u32,

    /// Video width (auto-detect if not specified)
    #[arg(long)]
    pub width: Option<u32>,

    /// Video height (auto-detect if not specified)
    #[arg(long)]
    pub height: Option<u32>,
}

/// Run the webcam splitter command.
#[allow(clippy::missing_errors_doc)]
pub fn run_splitter(args: SplitterArgs) -> Result<()> {
    info!("Starting webcam splitter...");
    info!("  Input: {}", args.input);
    info!("  Output: {}", args.output);

    let output_devices: Vec<String> = args
        .output
        .split(',')
        .map(|s| s.trim().to_string())
        .collect();

    let config = SplitterConfig {
        input_device: args.input,
        output_devices,
        framerate: args.framerate,
        width: args.width,
        height: args.height,
        input_format: None,
    };

    let mut splitter = WebcamSplitter::new(config)?;

    // Set up Ctrl+C handler
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();

    ctrlc::set_handler(move || {
        info!("Received Ctrl+C, stopping splitter...");
        r.store(false, Ordering::SeqCst);
    })
    .expect("Error setting Ctrl+C handler");

    info!("Splitter running. Press Ctrl+C to stop.");
    info!("Other apps can now use the virtual cameras!");

    // Run the splitter (blocking)
    splitter.run()?;

    info!("Splitter stopped.");
    Ok(())
}
