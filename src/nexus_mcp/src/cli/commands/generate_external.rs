use crate::external_servers::generate_external_server_tools;
use clap::Args;
use std::path::PathBuf;

#[derive(Args, Debug)]
#[command(about = "Generate Python code API for external MCP servers from configuration file")]
pub struct GenerateExternalArgs {
    /// Path to the configuration file (TOML format)
    #[arg(short, long, default_value = "mcp-servers.toml")]
    pub config: PathBuf,

    /// Output directory path (default: servers)
    #[arg(short, long, default_value = "servers")]
    pub output: PathBuf,
}

pub async fn run_generate_external(
    args: GenerateExternalArgs,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("Generating Python code API for external MCP servers...");
    println!("Config file: {}", args.config.display());
    println!("Output directory: {}", args.output.display());

    generate_external_server_tools(&args.config, &args.output).await?;

    println!(
        "Successfully generated code API for external servers in directory {}",
        args.output.display()
    );
    Ok(())
}

