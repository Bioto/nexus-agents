use chrono;
use clap::Parser;
use nexus_recorder::{Cli, Commands};
use simplelog::{
    ColorChoice, CombinedLogger, Config, LevelFilter, TermLogger, TerminalMode, WriteLogger,
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
            if !log_dir.exists() {
                let _ = std::fs::create_dir_all(log_dir);
            }

            // Create log file with timestamp
            let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
            let log_file = log_dir.join(format!("nexus_recorder_{}.log", timestamp));
            let log_path = log_file.to_string_lossy().to_string();

            // Open log file for writing
            let file = File::create(&log_file).expect("Failed to create log file");

            let console_level = get_log_level();
            let file_level = LevelFilter::Info;

            // Configure logger to write to console and file
            CombinedLogger::init(vec![
                TermLogger::new(
                    console_level,
                    Config::default(),
                    TerminalMode::Mixed,
                    ColorChoice::Auto,
                ),
                WriteLogger::new(file_level, Config::default(), file),
            ])
            .expect("Failed to initialize logger");

            // Print log location to stdout so it shows in terminal
            println!("📝 Logging to: {}", log_path);

            log_path
        })
        .clone()
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Initialize logging after parsing (so CLI errors go to terminal)
    let _log_path = init_logging();

    match cli.command {
        // Audio commands
        Commands::Record(args) => nexus_recorder::run_record(args)?,
        Commands::Monitor(args) => nexus_recorder::run_monitor(args)?,
        Commands::Listen(args) => nexus_recorder::run_listen(args)?,
        Commands::Speak(args) => nexus_recorder::run_speak(args).await?,
        Commands::TestVoice(args) => nexus_recorder::run_test_voice(args).await?,

        // Screen commands
        Commands::Screenshot(args) => nexus_recorder::run_screenshot(args)?,
        Commands::RecordScreen(args) => nexus_recorder::run_record_screen(args).await?,

        // Input capture commands
        Commands::Capture(args) => nexus_recorder::run_capture(args).await?,

        // Unified recording
        Commands::Unified(args) => nexus_recorder::run_unified(args).await?,

        // Report
        Commands::Report(args) => nexus_recorder::run_report(args).await?,

        // Webcam splitter
        Commands::Splitter(args) => nexus_recorder::run_splitter(args)?,
    }

    Ok(())
}

