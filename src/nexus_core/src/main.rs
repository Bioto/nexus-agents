use clap::Parser;
use nexus_core::cli::{run_chat, Cli, Commands};

#[tokio::main]
async fn main() {
    // Load environment variables from .env file (if it exists)
    let _ = dotenvy::dotenv();

    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Chat(args) => run_chat(args).await,
    };

    if let Err(e) = result {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
