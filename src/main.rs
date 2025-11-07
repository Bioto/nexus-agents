use clap::Parser;
use nexus_agents::cli::{run_chat, run_query, Cli, Commands};

#[tokio::main]
async fn main() {
    // Load environment variables from .env file (if it exists)
    let _ = dotenvy::dotenv();

    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Query(args) => run_query(args).await,
        Commands::Chat(args) => run_chat(args).await,
    };

    if let Err(e) = result {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
