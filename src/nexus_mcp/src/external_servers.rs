use crate::codegen::CodeGenerator;
use crate::config::MultiServerConfig;
use crate::error::NexusError;
use std::path::Path;

/// Generate Python tool files for all external MCP servers from configuration
pub async fn generate_external_server_tools(
    config_path: impl AsRef<Path>,
    output_dir: impl AsRef<Path>,
) -> Result<(), NexusError> {
    let config = MultiServerConfig::from_file(config_path)?;
    let output_path = output_dir.as_ref();

    for server in config.servers {
        if server.is_external() {
            let server_url = server.url.as_ref().unwrap();
            let server_name = server.name.clone();
            let headers = server.headers.clone();

            eprintln!("Generating tools for external server: {} ({})", server_name, server_url);

            let generator = CodeGenerator::with_config(server_url, &server_name, headers);
            generator
                .generate_code_files(output_path)
                .await
                .map_err(|e| NexusError::Server(format!("Failed to generate code for {}: {}", server_name, e)))?;

            eprintln!("Successfully generated tools for {}", server_name);
        }
    }

    Ok(())
}

