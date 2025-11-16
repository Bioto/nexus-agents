use clap::Args;
use std::io::{self, Read};
use std::time::{SystemTime, UNIX_EPOCH};
use crate::service::docker::{DockerService, DockerConfig};

/// Escape XML special characters
fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// Format a Unix timestamp to a readable datetime string
fn format_timestamp(secs: u64) -> String {
    // Calculate time components from Unix timestamp
    // This is a simplified approach - for production, consider using chrono
    let total_secs = secs;
    let days = total_secs / 86400;
    let secs_in_day = total_secs % 86400;
    let hours = secs_in_day / 3600;
    let mins = (secs_in_day % 3600) / 60;
    let secs_remain = secs_in_day % 60;
    
    // Approximate year (Unix epoch started Jan 1, 1970)
    // This is a rough calculation - for accurate dates, use chrono
    let year = 1970 + (days / 365);
    let day_of_year = (days % 365) + 1; // Day of year (1-365)
    
    // Simple format: YYYY-DDD HH:MM:SS
    format!("{:04}-{:03} {:02}:{:02}:{:02} UTC", year, day_of_year, hours, mins, secs_remain)
}

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
    
    // Capture start time
    let start_time = SystemTime::now();
    let start_timestamp = start_time
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    
    // Format start time (simple format without external dependencies)
    let start_datetime = format_timestamp(start_timestamp);
    
    eprintln!("Executing code in Docker container...");
    
    let (stdout, stderr, exit_code) = docker.execute_python_code(&code).await?;
    
    // Capture end time and calculate duration
    let end_time = SystemTime::now();
    let duration = end_time
        .duration_since(start_time)
        .unwrap_or_default();
    
    let duration_secs = duration.as_secs_f64();
    let duration_ms = duration.as_millis();
    
    // Output everything in XML format
    println!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>");
    println!("<execution>");
    
    // Output section
    println!("  <output>");
    if !stdout.is_empty() {
        println!("    <stdout>{}</stdout>", escape_xml(&stdout));
    } else {
        println!("    <stdout></stdout>");
    }
    if !stderr.is_empty() {
        println!("    <stderr>{}</stderr>", escape_xml(&stderr));
    } else {
        println!("    <stderr></stderr>");
    }
    println!("  </output>");
    
    // Metrics section
    println!("  <metrics>");
    println!("    <start_time>{}</start_time>", escape_xml(&start_datetime));
    println!("    <duration_seconds>{:.3}</duration_seconds>", duration_secs);
    println!("    <duration_milliseconds>{}</duration_milliseconds>", duration_ms);
    println!("    <exit_code>{}</exit_code>", exit_code);
    println!("  </metrics>");
    
    println!("</execution>");

    // Exit with the container's exit code
    if exit_code != 0 {
        std::process::exit(exit_code);
    }

    Ok(())
}
