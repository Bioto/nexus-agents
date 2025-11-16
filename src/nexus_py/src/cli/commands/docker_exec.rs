use clap::Args;
use std::io::{self, Read};
use crate::service::docker::{DockerService, DockerConfig};

#[derive(Args, Debug)]
#[command(about = "Execute Python code in a Docker container")]
pub struct DockerExecArgs {
    /// Python code to execute (if not provided, reads from stdin)
    #[arg(short, long)]
    pub code: Option<String>,
    
    /// Docker image name (default: nexus_py)
    #[arg(long, default_value = "nexus_py")]
    pub image: Option<String>,
    
    /// Docker image tag (default: latest)
    #[arg(long, default_value = "latest")]
    pub tag: Option<String>,
    
    /// Path to Dockerfile (default: src/nexus_py/.docker/Dockerfile)
    #[arg(long)]
    pub dockerfile: Option<String>,
    
    /// Build context directory (default: .)
    #[arg(long)]
    pub build_context: Option<String>,
}

pub async fn run_docker_exec(args: DockerExecArgs) -> Result<(), Box<dyn std::error::Error>> {
    // Get code from argument or stdin
    let code = if let Some(code) = args.code {
        code
    } else {
        // Read from stdin
        let mut buffer = String::new();
        io::stdin().read_to_string(&mut buffer)?;
        buffer
    };

    if code.trim().is_empty() {
        eprintln!("Error: No code provided. Use --code <code> or pipe code via stdin.");
        std::process::exit(1);
    }

    // Create Docker configuration
    let image_name = args.image.as_ref().map(|s| s.clone()).unwrap_or_else(|| "nexus_py".to_string());
    let image_tag = args.tag.as_ref().map(|s| s.clone()).unwrap_or_else(|| "latest".to_string());
    let config = DockerConfig {
        image_name: image_name.clone(),
        image_tag: image_tag.clone(),
        dockerfile_path: args.dockerfile.as_ref().map(|s| s.clone()).unwrap_or_else(|| {
            "src/nexus_py/.docker/Dockerfile".to_string()
        }),
        build_context: args.build_context.as_ref().map(|s| s.clone()).unwrap_or_else(|| ".".to_string()),
    };

    // Create Docker service
    eprintln!("Connecting to Docker daemon...");
    let docker = DockerService::new(config).await
        .map_err(|e| {
            eprintln!("Error: Failed to connect to Docker daemon. Is Docker running?");
            e
        })?;
    eprintln!("Connected to Docker daemon.");

    // Execute Python code in container
    // Note: We don't check for image existence here - Docker will return a clear error
    // if the image doesn't exist when we try to create the container
    eprintln!("Executing code in Docker container...");
    let (stdout, stderr, exit_code) = docker.execute_python_code(&code).await?;

    // Print stdout
    if !stdout.is_empty() {
        print!("{}", stdout);
    }

    // Print stderr
    if !stderr.is_empty() {
        eprint!("{}", stderr);
    }

    // Exit with the container's exit code
    if exit_code != 0 {
        std::process::exit(exit_code);
    }

    Ok(())
}
