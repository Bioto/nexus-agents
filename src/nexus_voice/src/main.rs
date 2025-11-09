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
            let log_file = log_dir.join(format!("nexus-voice_{}.log", timestamp));
            let log_path = log_file.to_string_lossy().to_string();

            // Open log file for writing (for both logger and stderr)
            let file = File::create(&log_file).expect("Failed to create log file");
            let stderr_file = File::create(&log_file).expect("Failed to create stderr log file");

            // Configure logger to write to file
            CombinedLogger::init(vec![WriteLogger::new(
                LevelFilter::Info,
                Config::default(),
                file,
            )])
            .expect("Failed to initialize logger");

            // Redirect stderr to log file to capture ALSA messages
            #[cfg(target_os = "linux")]
            {
                use std::os::unix::io::AsRawFd;
                let stderr_fd = stderr_file.as_raw_fd();
                unsafe {
                    // Duplicate the file descriptor to stderr (fd 2)
                    libc::dup2(stderr_fd, libc::STDERR_FILENO);
                }
                // Prevent the file from being closed when stderr_file goes out of scope
                std::mem::forget(stderr_file);
            }

            // Print log location to stdout (not stderr) so it shows in terminal
            println!("📝 Logging to: {}", log_path);

            log_path
        })
        .clone()
}

#[tokio::main]
async fn main() -> Result<()> {
    let _log_path = init_logging();

    let cli = Cli::parse();

    match cli.command {
        Commands::Record(args) => cli::run_record(args)?,
        Commands::Listen(args) => cli::run_listen(args)?,
    }

    Ok(())
}
