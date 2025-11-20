use crate::service::docker::{DockerConfig, DockerService};
use clap::Args;
use std::io::{self, Read};
use std::time::{SystemTime, UNIX_EPOCH};
use chrono::{DateTime, Utc};
use tracing::{error, info};

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
    let dt = DateTime::<Utc>::from_timestamp(secs as i64, 0)
        .unwrap_or_else(Utc::now);
    dt.format("%Y-%m-%d %H:%M:%S UTC").to_string()
}

#[derive(Args, Debug)]
#[command(about = "Execute Python code in a Docker container")]
pub struct DockerExecArgs {
    /// Python code to execute (if not provided, reads from stdin)
    #[arg(short, long)]
    pub code: Option<String>,

    /// Docker image name (default: nexus_sandbox)
    #[arg(long, default_value = "nexus_sandbox")]
    pub image: Option<String>,

    /// Docker image tag (default: latest)
    #[arg(long, default_value = "latest")]
    pub tag: Option<String>,

    /// Path to Dockerfile (default: src/nexus_sandbox/.docker/Dockerfile)
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
        error!("No code provided. Use --code <code> or pipe code via stdin.");
        std::process::exit(1);
    }

    // Create Docker configuration
    let image_name = args.image.as_deref().unwrap_or("nexus_sandbox");
    let image_tag = args.tag.as_deref().unwrap_or("latest");

    let config = DockerConfig {
        image_name: image_name.to_string(),
        image_tag: image_tag.to_string(),
        dockerfile_path: args.dockerfile.clone().unwrap_or_else(|| "src/nexus_sandbox/.docker/Dockerfile".to_string()),
        build_context: args.build_context.clone().unwrap_or_else(|| ".".to_string()),
    };

    // Create Docker service
    info!("Connecting to Docker daemon...");
    let docker = DockerService::new(config).await.map_err(|e| {
        error!("Failed to connect to Docker daemon. Is Docker running?");
        e
    })?;
    info!("Connected to Docker daemon.");

    // Execute Python code in container
    // Note: We don't check for image existence here - Docker will return a clear error
    // if the image doesn't exist when we try to create the container

    // Capture start time
    let start_time = SystemTime::now();
    let start_timestamp = start_time
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    // Format start time
    let start_datetime = format_timestamp(start_timestamp);

    info!("Executing code in Docker container...");

    let (stdout, stderr, exit_code) = docker.execute_python_code(&code).await?;

    // Capture end time and calculate duration
    let end_time = SystemTime::now();
    let duration = end_time.duration_since(start_time).unwrap_or_default();

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
    println!(
        "    <start_time>{}</start_time>",
        escape_xml(&start_datetime)
    );
    println!(
        "    <duration_seconds>{:.3}</duration_seconds>",
        duration_secs
    );
    println!(
        "    <duration_milliseconds>{}</duration_milliseconds>",
        duration_ms
    );
    println!("    <exit_code>{}</exit_code>", exit_code);
    println!("  </metrics>");

    println!("</execution>");

    // Exit with the container's exit code
    if exit_code != 0 {
        std::process::exit(exit_code);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_escape_xml() {
        assert_eq!(escape_xml("normal string"), "normal string");
        assert_eq!(escape_xml("a < b"), "a &lt; b");
        assert_eq!(escape_xml("a > b"), "a &gt; b");
        assert_eq!(escape_xml("a & b"), "a &amp; b");
        assert_eq!(escape_xml("\"quotes\""), "&quot;quotes&quot;");
        assert_eq!(escape_xml("'single quotes'"), "&apos;single quotes&apos;");
        assert_eq!(escape_xml("<tag>content</tag>"), "&lt;tag&gt;content&lt;/tag&gt;");
    }
}
