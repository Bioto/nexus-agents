mod cli;
mod components;

use clap::Parser;
use cli::{Cli, Commands};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Show(args) => cli::run_show(args)?,
    }

    Ok(())
}
