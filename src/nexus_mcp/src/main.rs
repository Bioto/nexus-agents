use clap::Parser;
use nexus_mcp::{Cli, Commands};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Shell(args) => nexus_mcp::run_shell(args)?,
        Commands::GenerateCode(args) => {
            nexus_mcp::run_generate_code(args).await?;
        }
        Commands::StartServers(args) => {
            nexus_mcp::run_start_servers(args)
                .await
                .map_err(|e| -> Box<dyn std::error::Error> { Box::new(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())) })?;
        }
    }

    Ok(())
}
