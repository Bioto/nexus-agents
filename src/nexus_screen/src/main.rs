use chrono;
use clap::Parser;
use nexus_screen::{Cli, Commands, Result};
use simplelog::{
    ColorChoice, CombinedLogger, Config, LevelFilter, TermLogger, TerminalMode, WriteLogger,
    SharedLogger,
};
use std::env;
use std::fs::File;
use std::sync::OnceLock;

static LOG_INIT: OnceLock<String> = OnceLock::new();

fn get_log_level() -> LevelFilter {
    match env::var("RUST_LOG")
        .unwrap_or_else(|_| "info".to_string())
        .as_str()
    {
        "trace" => LevelFilter::Trace,
        "debug" => LevelFilter::Debug,
        "info" => LevelFilter::Info,
        "warn" => LevelFilter::Warn,
        "error" => LevelFilter::Error,
        _ => LevelFilter::Info,
    }
}

fn init_logging() -> String {
    LOG_INIT
        .get_or_init(|| {
            // Create log directory if it doesn't exist
            let log_dir = std::path::Path::new("logs");
            if let Err(e) = std::fs::create_dir_all(log_dir) {
                eprintln!("Warning: Could not create logs directory: {}", e);
            }

            // Create log file with timestamp
            let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
            let log_file = log_dir.join(format!("nexus-screen_{}.log", timestamp));
            let log_path = log_file.to_string_lossy().to_string();

            // Attempt to open log file
            let file = match File::create(&log_file) {
                Ok(f) => Some(f),
                Err(e) => {
                    eprintln!("Warning: Failed to create log file '{}': {}. Falling back to console-only logging.", log_path, e);
                    None
                }
            };

            let console_level = get_log_level();
            let mut loggers: Vec<Box<dyn SharedLogger>> = vec![
                TermLogger::new(
                    console_level,
                    Config::default(),
                    TerminalMode::Mixed,
                    ColorChoice::Auto,
                ),
            ];

            if let Some(file) = file {
                let file_level = LevelFilter::Info;
                loggers.push(WriteLogger::new(
                    file_level,
                    Config::default(),
                    file,
                ));
            }

            CombinedLogger::init(loggers)
                .expect("Failed to initialize logger");

            // Always print log location (even if fallback)
            println!("📝 Logging to: {} (console fallback if file unavailable)", log_path);

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
        Commands::Screenshot(args) => nexus_screen::run_screenshot(args)?,
        Commands::Record(args) => nexus_screen::run_record(args).await?,
    }

    Ok(())
}
