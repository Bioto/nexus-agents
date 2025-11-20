use crate::codegen::CodeGenerator;
use crate::error::NexusError;
use clap::Args;
use std::fs;
use std::path::PathBuf;

#[derive(Args, Debug)]
#[command(about = "Generate Python code API for MCP tools")]
pub struct GenerateCodeArgs {
    /// MCP server URL (default: http://127.0.0.1:8000)
    #[arg(long, default_value = "http://127.0.0.1:8000")]
    pub server_url: String,

    /// Output directory path (default: servers)
    #[arg(short, long, default_value = "servers")]
    pub output: PathBuf,

    /// Overwrite existing file without prompting
    #[arg(long)]
    pub overwrite: bool,
}

pub async fn run_generate_code(args: GenerateCodeArgs) -> Result<(), NexusError> {
    println!("Generating Python code API for MCP tools...");
    println!("Server URL: {}", args.server_url);
    println!("Output directory: {}", args.output.display());

    // Check if directory exists and has content
    if args.output.exists() && !args.overwrite {
        let has_content = fs::read_dir(&args.output)
            .map(|mut entries| entries.next().is_some())
            .unwrap_or(false);
        if has_content {
            return Err(NexusError::Config(format!(
                "Directory {} already exists and has content. Use --overwrite to replace it.",
                args.output.display()
            )));
        }
    }

    // Create output directory if it doesn't exist
    fs::create_dir_all(&args.output).map_err(|e| NexusError::Io(e))?;

    // Generate code files in directory structure
    let generator = CodeGenerator::new(&args.server_url);
    generator
        .generate_code_files(&args.output)
        .await?;

    println!(
        "Successfully generated code API in directory {}",
        args.output.display()
    );
    Ok(())
}
