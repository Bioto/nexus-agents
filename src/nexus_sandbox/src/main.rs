use clap::Parser;
use nexus_sandbox::{Cli, Commands};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Shell(args) => nexus_sandbox::run_shell(args)?,
        Commands::Py03(args) => nexus_sandbox::run_py03(args)?,
        Commands::ExecCode(args) => nexus_sandbox::run_exec_code(args)?,
        Commands::DockerExec(args) => nexus_sandbox::run_docker_exec(args).await?,
    }

    Ok(())
}
