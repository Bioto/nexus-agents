use clap::Parser;
use nexus_logger::{Cli, Commands, Result};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Log(args) => nexus_logger::run_log(args)?,
        Commands::Capture(args) => nexus_logger::run_capture(args).await?,
        Commands::Unified(args) => nexus_logger::run_unified(args).await?,
    }

    Ok(())
}
