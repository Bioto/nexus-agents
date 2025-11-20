use crate::error::Result;
use clap::Args;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// CLI arguments for the capture subcommand.
#[derive(Args)]
pub struct CaptureArgs {
    /// Output file path (optional, defaults to stdout)
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Output format: json, text, or both
    #[arg(short, long, default_value = "text")]
    pub format: String,

    /// Capture keyboard events
    #[arg(short = 'k', long, default_value = "true")]
    pub keyboard: bool,

    /// Capture mouse events
    #[arg(short = 'm', long, default_value = "true")]
    pub mouse: bool,

    /// Include mouse move events (can be verbose)
    #[arg(long, default_value = "false")]
    pub mouse_moves: bool,

    /// Note: Database is now configured via ClickHouse environment variables
    /// (CLICKHOUSE_HOST, CLICKHOUSE_PORT, CLICKHOUSE_DATABASE, etc.)

    /// Show metrics summary every N seconds (0 to disable)
    #[arg(short = 's', long, default_value = "10")]
    pub metrics_interval: u64,
}

/// Runs the capture command based on args.
pub async fn run_capture(args: CaptureArgs) -> Result<()> {
    // Validate format
    let format = match args.format.as_str() {
        "json" | "text" | "both" => args.format.clone(),
        _ => {
            return Err(crate::error::LoggerError::Configuration(
                "Format must be 'json', 'text', or 'both'".to_string(),
            ));
        }
    };

    // Setup signal handler for graceful shutdown
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    ctrlc::set_handler(move || {
        println!("\n🛑 Shutting down capture...");
        r.store(false, Ordering::SeqCst);
    })
    .map_err(|e| {
        crate::error::LoggerError::Other(format!("Failed to set signal handler: {}", e))
    })?;

    println!("🎯 Starting input capture...");
    println!("   Keyboard: {}", if args.keyboard { "✓" } else { "✗" });
    println!("   Mouse: {}", if args.mouse { "✓" } else { "✗" });
    println!("   Mouse moves: {}", if args.mouse_moves { "✓" } else { "✗" });
    println!("   Format: {}", format);
    if let Some(ref output) = args.output {
        println!("   Output: {}", output.display());
    } else {
        println!("   Output: stdout");
    }
    println!("   Database: ClickHouse (configured via environment variables)");
    println!("   Metrics interval: {}s", args.metrics_interval);
    println!("\nPress Ctrl+C to stop\n");

    // Run the capture service (async)
    crate::services::capture::run_capture_service(
        args.keyboard,
        args.mouse,
        args.mouse_moves,
        format,
        args.output,
        args.metrics_interval,
        running,
    ).await?;

    println!("\n✅ Capture stopped.");
    Ok(())
}

