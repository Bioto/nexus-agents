mod cli;

use chrono;
use clap::Parser;
use cli::{Cli, Commands};
use nexus_core::{self, Commands as CoreCommands};
use nexus_exporter::{self, Commands as ExporterCommands};
use nexus_gui::{self, Commands as GuiCommands};
use nexus_mcp::{self, Commands as McpCommands};
use nexus_recorder::{self, Commands as RecorderCommands};
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
        Commands::Recorder { command } => match command {
            RecorderCommands::Record(args) => nexus_recorder::run_record(args)?,
            RecorderCommands::Monitor(args) => nexus_recorder::run_monitor(args)?,
            RecorderCommands::Listen(args) => nexus_recorder::run_listen(args)?,
            RecorderCommands::Speak(args) => nexus_recorder::run_speak(args).await?,
            RecorderCommands::TestVoice(args) => nexus_recorder::run_test_voice(args).await?,
            RecorderCommands::Screenshot(args) => nexus_recorder::run_screenshot(args)?,
            RecorderCommands::RecordScreen(args) => nexus_recorder::run_record_screen(args).await?,
            RecorderCommands::Capture(args) => nexus_recorder::run_capture(args).await?,
            RecorderCommands::Unified(args) => nexus_recorder::run_unified(args).await?,
            RecorderCommands::Report(args) => nexus_recorder::run_report(args).await?,
        },
        Commands::Gui { command } => match command {
            GuiCommands::Show(args) => nexus_gui::run_show(args)?,
        },
        Commands::Core { command } => match command {
            CoreCommands::Chat(args) => nexus_core::run_chat(args)
                .await
                .map_err(|e| anyhow::anyhow!(e.to_string()))?,
            CoreCommands::TestPython(args) => nexus_core::run_test_python(args)
                .await
                .map_err(|e| anyhow::anyhow!(e.to_string()))?,
        },
        Commands::Mcp { command } => match command {
            McpCommands::Shell(args) => {
                nexus_mcp::run_shell(args).map_err(|e| anyhow::anyhow!(e.to_string()))?
            }
            McpCommands::GenerateCode(args) => nexus_mcp::run_generate_code(args)
                .await
                .map_err(|e| anyhow::anyhow!(e.to_string()))?,
            McpCommands::GenerateExternal(args) => nexus_mcp::run_generate_external(args)
                .await
                .map_err(|e| anyhow::anyhow!(e.to_string()))?,
            McpCommands::StartServers(args) => nexus_mcp::run_start_servers(args)
                .await
                .map_err(|e| anyhow::anyhow!(e.to_string()))?,
        },
        Commands::Exporter { command } => match command {
            ExporterCommands::Pdf(args) => {
                nexus_exporter::run_pdf(args).map_err(|e| anyhow::anyhow!(e.to_string()))?
            }
        },
        Commands::McpAgent(args) => cli::commands::mcp_agent::run_mcp_agent(args)
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?,
        Commands::SearchMcpTools(args) => {
            cli::commands::search_mcp_tools::run_search_mcp_tools(args)
                .map_err(|e| anyhow::anyhow!(e.to_string()))?
        }
        Commands::TestMcpFlow(args) => cli::commands::test_mcp_flow::run_test_mcp_flow(args)
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?,
    }

    Ok(())
}
