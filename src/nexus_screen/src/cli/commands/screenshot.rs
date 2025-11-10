use anyhow::Result;
use clap::Args;
use crate::services::screen_recorder::ScreenRecorder;
use std::path::PathBuf;

#[derive(Args, Debug)]
#[command(about = "Capture a screenshot and save to file")]
pub struct ScreenshotArgs {
    /// Output file path (default: screenshot.png)
    #[arg(short, long, default_value = "screenshot.png")]
    pub output: PathBuf,
}

pub fn run_screenshot(args: ScreenshotArgs) -> Result<()> {
    let recorder = ScreenRecorder::new()?;
    recorder.capture_screenshot_to_file(&args.output.to_string_lossy())?;
    log::info!("Screenshot saved to {}", args.output.display());
    println!("✅ Screenshot saved to: {}", args.output.display());
    Ok(())
}
