use crate::cli::commands::tui::{self, ChatState};
use crate::client::Client;
use crate::factories::AgentFactory;
use crate::models::Result;
use crate::services::SwarmCoordinatorService;
use clap::Args;
use crossterm::terminal;
use ratatui::{Terminal, backend::CrosstermBackend};
use std::io;

/// Get the default model from DEFAULT_MODEL environment variable or fallback
fn default_model() -> String {
    std::env::var("DEFAULT_MODEL")
        .unwrap_or_else(|_| "gpt-5-nano-2025-08-07".to_string())
}

#[derive(Args)]
pub struct ChatArgs {
    /// System prompt to use
    #[arg(short, long)]
    pub system: Option<String>,

    /// Agent to use (e.g., "calculator")
    #[arg(short, long)]
    pub agent: Option<String>,

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

    /// Base URL for the API (defaults to OpenAI or OPENAI_BASE_URL env var)
    #[arg(long)]
    pub base_url: Option<String>,

    /// API key (defaults to OPENAI_API_KEY env var)
    #[arg(long)]
    pub api_key: Option<String>,

    /// Enable streaming responses (default: false)
    #[arg(long, default_value = "false")]
    pub stream: bool,
}

pub async fn run_chat(args: ChatArgs) -> Result<()> {
    // Get API key
    let api_key = args
        .api_key
        .or_else(|| std::env::var("OPENAI_API_KEY").ok())
        .ok_or_else(|| {
            crate::models::Error::Configuration(
                "API key not provided. Set OPENAI_API_KEY environment variable or use --api-key"
                    .to_string(),
            )
        })?;

    // Get base URL
    let base_url = args
        .base_url
        .or_else(|| std::env::var("OPENAI_BASE_URL").ok())
        .unwrap_or_else(|| "https://api.openai.com/v1".to_string());

    // Get model - use DEFAULT_MODEL env var if model is the default fallback, otherwise use provided value
    let model = if args.model == "gpt-5-nano-2025-08-07" {
        default_model()
    } else {
        args.model
    };

    // Create client
    let client = Client::new(api_key, base_url);

    // Load agent if specified
    let (agent, is_swarm) = if let Some(agent_name) = &args.agent {
        match agent_name.as_str() {
            "calculator" => (Some(AgentFactory::calculator()), false),
            "swarm" => (None, true), // Swarm mode - no agent, will create coordinator service
            _ => {
                return Err(crate::models::Error::Configuration(format!(
                    "Unknown agent: {}. Available agents: calculator, swarm",
                    agent_name
                )));
            }
        }
    } else {
        (None, false)
    };

    // Initialize message history
    let mut state = ChatState::new();

    // Add system prompt: agent's system prompt takes precedence
    if let Some(ref agent) = agent {
        state.set_agent_name(&agent.name);
        state.add_system(&agent.system_prompt);

        // Set tools for display
        let tools: Vec<(String, String)> = agent
            .tools
            .iter()
            .map(|t| (t.name.clone(), t.description.clone()))
            .collect();
        state.set_tools(tools);
    } else if let Some(system) = args.system {
        state.add_system(system);
    }

    // Setup terminal
    terminal::enable_raw_mode()
        .map_err(|e| crate::models::Error::Other(format!("Failed to enable raw mode: {}", e)))?;
    let mut stdout = io::stdout();
    crossterm::execute!(stdout, terminal::EnterAlternateScreen).map_err(|e| {
        crate::models::Error::Other(format!("Failed to enter alternate screen: {}", e))
    })?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)
        .map_err(|e| crate::models::Error::Other(format!("Failed to create terminal: {}", e)))?;

    // Clone args to avoid borrowing issues
    let temperature = args.temperature;
    let max_tokens = args.max_tokens;
    let top_p = args.top_p;
    let frequency_penalty = args.frequency_penalty;
    let presence_penalty = args.presence_penalty;
    let stream = args.stream;

    let result = if is_swarm {
        // Create swarm coordinator service
        let agent_store = AgentFactory::default_agent_store();
        let swarm_coordinator = SwarmCoordinatorService::new(client.clone(), agent_store);

        // Set the state to indicate we're using swarm
        state.set_agent_name("Swarm Coordinator");

        tui::run_swarm(
            &mut terminal,
            &mut state,
            &swarm_coordinator,
            stream,
            &model,
            temperature,
            max_tokens,
            top_p,
            frequency_penalty,
            presence_penalty,
        )
        .await
    } else {
        tui::run(
            &mut terminal,
            &mut state,
            &client,
            agent.as_ref(),
            stream,
            &model,
            temperature,
            max_tokens,
            top_p,
            frequency_penalty,
            presence_penalty,
        )
        .await
    };

    // Restore terminal
    terminal::disable_raw_mode().ok();
    crossterm::execute!(io::stdout(), terminal::LeaveAlternateScreen).ok();

    result
}
