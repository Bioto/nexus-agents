use clap::Parser;
use nexus_sandbox::{Cli, Commands};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Shell(args) => nexus_sandbox::run_shell(args)?,
        Commands::Py03(args) => nexus_sandbox::run_py03(args)?,
        Commands::ExecCode(args) => nexus_sandbox::run_exec_code(args)?,
        Commands::DockerExec(args) => nexus_sandbox::run_docker_exec(args).await?,
    }

    Ok(())
}
