use crate::error::Result;
use clap::Args;

/// CLI arguments for the log subcommand.
#[derive(Args)]
pub struct LogArgs {
    /// Message to log
    #[arg(short, long)]
    pub message: Option<String>,

    /// Log level (trace, debug, info, warn, error)
    #[arg(short, long, default_value = "info")]
    pub level: String,
}

/// Runs the log command based on args.
pub fn run_log(args: LogArgs) -> Result<()> {
    let message = args.message.unwrap_or_else(|| "No message provided".to_string());
    println!("[{}] {}", args.level.to_uppercase(), message);
    Ok(())
}

