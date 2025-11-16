use clap::Args;
use nexus_core::client::ResponsesClient;
use nexus_core::factories::AgentFactory;
use nexus_core::load_env;
use nexus_core::models::Result;
use std::path::PathBuf;

#[derive(Args)]
pub struct McpAgentArgs {
    /// Path to servers directory containing MCP tool files (default: servers)
    #[arg(long, default_value = "servers")]
    pub servers_dir: PathBuf,

    /// Model to use (default: from DEFAULT_MODEL env var or gpt-5-nano-2025-08-07)
    #[arg(long, default_value = "gpt-5-nano-2025-08-07")]
    pub model: String,

    /// Temperature parameter (0.0 to 2.0)
    #[arg(short, long)]
    pub temperature: Option<f32>,

    /// Maximum tokens in the response
    #[arg(short = 'M', long)]
    pub max_tokens: Option<u32>,

    /// Top-p parameter
    #[arg(long)]
    pub top_p: Option<f32>,

    /// Frequency penalty
    #[arg(long)]
    pub frequency_penalty: Option<f32>,

    /// Presence penalty
    #[arg(long)]
    pub presence_penalty: Option<f32>,

    /// Enable streaming responses (default: false)
    #[arg(long, default_value = "false")]
    pub stream: bool,

    /// Base URL for the API (defaults to OpenAI or OPENAI_BASE_URL env var)
    #[arg(long)]
    pub base_url: Option<String>,

    /// API key (defaults to OPENAI_API_KEY env var)
    #[arg(long)]
    pub api_key: Option<String>,
}

pub async fn run_mcp_agent(args: McpAgentArgs) -> Result<()> {
    // Load environment variables from .env file (if it exists)
    load_env();

    // Verify servers directory exists
    if !args.servers_dir.exists() {
        return Err(nexus_core::models::Error::Configuration(format!(
            "Servers directory does not exist: {}. Please generate MCP tools first using 'nexus-mcp generate-code'",
            args.servers_dir.display()
        )));
    }

    // Get API key
    let api_key = args
        .api_key
        .or_else(|| std::env::var("OPENAI_API_KEY").ok())
        .ok_or_else(|| {
            nexus_core::models::Error::Configuration(
                "API key not provided. Set OPENAI_API_KEY environment variable or use --api-key"
                    .to_string(),
            )
        })?;

    // Get base URL
    let base_url = args
        .base_url
        .or_else(|| std::env::var("OPENAI_BASE_URL").ok())
        .unwrap_or_else(|| "https://api.openai.com/v1".to_string());

    // Get model - use DEFAULT_MODEL env var if model is the default fallback
    let model = if args.model == "gpt-5-nano-2025-08-07" {
        std::env::var("DEFAULT_MODEL").unwrap_or_else(|_| "gpt-5-nano-2025-08-07".to_string())
    } else {
        args.model
    };

    // Create client
    let client = ResponsesClient::new(api_key, base_url);

    // Create MCP agent
    let agent = AgentFactory::mcp_agent(&args.servers_dir);

    // Use the chat TUI with our custom agent
    use crossterm::terminal;
    use nexus_core::cli::commands::tui::{self, ChatState};
    use ratatui::{backend::CrosstermBackend, Terminal};
    use std::io;

    // Initialize message history
    let mut state = ChatState::new();
    state.set_agent_name(&agent.name);
    state.add_system(&agent.system_prompt);

    // Set tools for display
    let tools: Vec<(String, String)> = agent
        .tools
        .iter()
        .map(|t| (t.name.clone(), t.description.clone()))
        .collect();
    state.set_tools(tools);

    // Setup terminal
    terminal::enable_raw_mode().map_err(|e| {
        nexus_core::models::Error::Other(format!("Failed to enable raw mode: {}", e))
    })?;
    let mut stdout = io::stdout();
    crossterm::execute!(stdout, terminal::EnterAlternateScreen).map_err(|e| {
        nexus_core::models::Error::Other(format!("Failed to enter alternate screen: {}", e))
    })?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).map_err(|e| {
        nexus_core::models::Error::Other(format!("Failed to create terminal: {}", e))
    })?;

    let temperature = args.temperature;
    let max_tokens = args.max_tokens;
    let top_p = args.top_p;
    let frequency_penalty = args.frequency_penalty;
    let presence_penalty = args.presence_penalty;
    let stream = args.stream;

    let result = tui::run(
        &mut terminal,
        &mut state,
        &client,
        Some(&agent),
        stream,
        &model,
        temperature,
        max_tokens,
        top_p,
        frequency_penalty,
        presence_penalty,
    )
    .await;

    // Restore terminal
    terminal::disable_raw_mode().ok();
    crossterm::execute!(io::stdout(), terminal::LeaveAlternateScreen).ok();

    result
}
