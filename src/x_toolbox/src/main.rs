use clap::Parser;
use x_toolbox::{Cli, Commands, Result};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    if let Some(command) = cli.command {
        match command {
            Commands::Nutrition(args) => x_toolbox::run_nutrition(args).await?,
        }
    }

    Ok(())
}
