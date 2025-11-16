use chrono;
use clap::Parser;
use nexus_core::cli::{run_chat, Cli, Commands};
use nexus_core::init;
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
            let log_file = log_dir.join(format!("nexus-core_{}.log", timestamp));
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
async fn main() {
    // Load environment variables from .env file (if it exists)
    init();

    let cli = Cli::parse();

    // Initialize logging after parsing (so CLI errors go to terminal)
    let _log_path = init_logging();

    let result = match cli.command {
        Commands::Chat(args) => run_chat(args).await,
    };

    if let Err(e) = result {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
