use crate::services::NotetakerService;
use crate::Result;
use clap::Args;
use log::info;

/// Process an existing unified recording session and generate notes.
#[derive(Args)]
pub struct ProcessArgs {
    /// Session identifier to load from ClickHouse
    #[arg(long)]
    pub session_id: String,

    /// Maximum number of contextual events to feed into the prompt
    #[arg(long, default_value_t = 200)]
    pub max_events: usize,

    /// Override model name (falls back to DEFAULT_MODEL or latest default)
    #[arg(long)]
    pub model: Option<String>,
}

/// Run the processing flow without initiating any new recording.
pub async fn run_process(args: ProcessArgs) -> Result<()> {
    nexus_core::init();

    let service = NotetakerService::new(args.model.clone()).await?;
    println!("📥 Loading session {} …", args.session_id);
    let data = service.load_session(&args.session_id).await?;
    info!(
        "Loaded session {} with {} events",
        args.session_id,
        data.events.len()
    );

    println!(
        "✅ Session loaded. Start: {}, events: {} (limiting to {}).",
        data.start_time,
        data.events.len(),
        args.max_events
    );

    let notes = service.summarize(data, args.max_events).await?;
    println!("\n📝 Structured notes:\n\n{}\n", notes);

    Ok(())
}
