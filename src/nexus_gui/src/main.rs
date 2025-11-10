use clap::Parser;
use nexus_gui::{Cli, Commands};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Show(args) => nexus_gui::run_show(args)?,
    }

    Ok(())
}
