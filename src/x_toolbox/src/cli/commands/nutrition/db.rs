use crate::error::{Result, ToolboxError};
use super::commands::DbCommand;
use std::process::Command;

/// Handle database management commands
pub async fn handle_db_command(command: DbCommand) -> Result<()> {
    let compose_file = std::env::current_dir()
        .map_err(|e| ToolboxError::Io(e))?
        .join("src/x_toolbox/docker-compose.yml");

    let compose_file_str = compose_file
        .to_str()
        .ok_or_else(|| ToolboxError::Other("Invalid docker-compose.yml path".to_string()))?;

    match command {
        DbCommand::Start => {
            println!("Starting Postgres database...");
            let output = Command::new("docker")
                .args(&["compose", "-f", compose_file_str, "up", "-d"])
                .output()
                .map_err(|e| {
                    ToolboxError::Other(format!(
                        "Failed to execute docker compose: {}. Make sure Docker is installed and running.",
                        e
                    ))
                })?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(ToolboxError::Other(format!(
                    "Failed to start database: {}",
                    stderr
                )));
            }

            println!("Postgres database started successfully!");
            println!("Connection details:");
            println!("  Host: localhost");
            println!("  Port: 5432");
            println!("  Database: nutrition");
            println!("  User: postgres");
            println!("  Password: postgres");
            println!("\nSet these environment variables:");
            println!("  export POSTGRES_HOST=localhost");
            println!("  export POSTGRES_PORT=5432");
            println!("  export POSTGRES_DATABASE=nutrition");
            println!("  export POSTGRES_USER=postgres");
            println!("  export POSTGRES_PASSWORD=postgres");
        }
        DbCommand::Stop => {
            println!("Stopping Postgres database...");
            let output = Command::new("docker")
                .args(&["compose", "-f", compose_file_str, "down"])
                .output()
                .map_err(|e| {
                    ToolboxError::Other(format!(
                        "Failed to execute docker compose: {}. Make sure Docker is installed and running.",
                        e
                    ))
                })?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(ToolboxError::Other(format!(
                    "Failed to stop database: {}",
                    stderr
                )));
            }

            println!("Postgres database stopped successfully!");
        }
        DbCommand::Status => {
            let output = Command::new("docker")
                .args(&["compose", "-f", compose_file_str, "ps"])
                .output()
                .map_err(|e| {
                    ToolboxError::Other(format!(
                        "Failed to execute docker compose: {}. Make sure Docker is installed and running.",
                        e
                    ))
                })?;

            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                print!("{}", stdout);
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(ToolboxError::Other(format!(
                    "Failed to get database status: {}",
                    stderr
                )));
            }
        }
    }

    Ok(())
}

