use clap::Parser;
use nexus_py::{Cli, Commands};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Shell(args) => nexus_py::run_shell(args)?,
        Commands::Py03(args) => nexus_py::run_py03(args)?,
        Commands::ExecCode(args) => nexus_py::run_exec_code(args)?,
        Commands::DockerExec(args) => nexus_py::run_docker_exec(args).await?,
    }

    Ok(())
}
