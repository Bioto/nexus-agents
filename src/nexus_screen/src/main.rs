mod cli;
mod error;
mod services;
mod tui;

use clap::Parser;
use cli::{Cli, Commands};
use error::Result;
use simplelog::{CombinedLogger, Config, LevelFilter, WriteLogger};
use std::fs::File;
use std::sync::OnceLock;

static LOG_INIT: OnceLock<String> = OnceLock::new();

fn init_logging() -> String {
    LOG_INIT
        .get_or_init(|| {
            // Create log directory if it doesn't exist
            let log_dir = std::path::Path::new("logs");
            if !log_dir.exists() {
                let _ = std::fs::create_dir_all(log_dir);
            }

            // Create log file with timestamp
            let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
            let log_file = log_dir.join(format!("nexus-screen_{}.log", timestamp));
            let log_path = log_file.to_string_lossy().to_string();

            // Open log file for writing
            let file = File::create(&log_file).expect("Failed to create log file");

            // Configure logger to write to file
            CombinedLogger::init(vec![WriteLogger::new(
                LevelFilter::Info,
                Config::default(),
                file,
            )])
            .expect("Failed to initialize logger");

            // Print log location to stdout so it shows in terminal
            println!("📝 Logging to: {}", log_path);

            log_path
        })
        .clone()
}

#[tokio::main]
async fn main() -> Result<()> {
    // Parse CLI first so errors show in terminal
    let cli = Cli::parse();

    // Initialize logging after parsing (so CLI errors go to terminal)
    let _log_path = init_logging();

    match cli.command {
        Commands::Screenshot(args) => cli::run_screenshot(args)?,
        Commands::Record(args) => cli::run_record(args)?,
    }

    Ok(())
}
