mod commands;

use clap::{Parser, Subcommand};

/// Nexus Py - Python-like CLI Shell for AI Agents
#[derive(Parser)]
#[command(name = "nexus-py")]
#[command(about = "A Python-like CLI shell for interacting with AI agents", long_about = None)]
#[command(version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Start an interactive shell
    Shell(commands::shell::ShellArgs),
    /// Execute a Python script with uv inline packages
    Py03(commands::py03::Py03Args),
    /// Execute Python code from a string
    ExecCode(commands::exec_code::ExecCodeArgs),
    /// Execute Python code in a Docker container
    DockerExec(commands::docker_exec::DockerExecArgs),
}

// Re-export command handlers for convenience
pub use commands::docker_exec::run_docker_exec;
pub use commands::exec_code::run_exec_code;
pub use commands::py03::run_py03;
pub use commands::shell::run_shell;
