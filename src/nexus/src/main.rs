mod cli;

use clap::Parser;
use cli::{Cli, Commands};
use nexus_audio::{self, Commands as AudioCommands};
use nexus_core::{self, Commands as CoreCommands};
use nexus_gui::{self, Commands as GuiCommands};
use nexus_screen::{self, Commands as ScreenCommands};
use simplelog::{CombinedLogger, Config, LevelFilter, TermLogger, TerminalMode, ColorChoice, WriteLogger};
use std::fs::File;
use std::sync::OnceLock;
use std::env;
use chrono;

static LOG_INIT: OnceLock<String> = OnceLock::new();

fn get_log_level() -> LevelFilter {
    match env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string()).as_str() {
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
            let log_file = log_dir.join(format!("nexus_{}.log", timestamp));
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
                WriteLogger::new(
                    file_level,
                    Config::default(),
                    file,
                ),
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
        Commands::Audio { command } => match command {
            AudioCommands::Record(args) => nexus_audio::run_record(args)?,
            AudioCommands::Listen(args) => nexus_audio::run_listen(args)?,
            AudioCommands::Speak(args) => nexus_audio::run_speak(args).await?,
            AudioCommands::TestVoice(args) => nexus_audio::run_test_voice(args).await?,
        },
        Commands::Screen { command } => match command {
            ScreenCommands::Screenshot(args) => nexus_screen::run_screenshot(args)?,
            ScreenCommands::Record(args) => nexus_screen::run_record(args)?,
        },
        Commands::Gui { command } => match command {
            GuiCommands::Show(args) => nexus_gui::run_show(args)?,
        },
        Commands::Core { command } => match command {
            CoreCommands::Chat(args) => nexus_core::run_chat(args)
                .await
                .map_err(|e| anyhow::anyhow!(e.to_string()))?,
        },
    }

    Ok(())
}
