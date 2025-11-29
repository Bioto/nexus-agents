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

    // Get API key - try LLM_API_KEY first, fall back to OPENAI_API_KEY for backward compatibility
    let api_key = args
        .api_key
        .or_else(|| std::env::var("LLM_API_KEY").ok())
        .or_else(|| std::env::var("OPENAI_API_KEY").ok())
        .ok_or_else(|| {
            nexus_core::models::Error::Configuration(
                "API key not provided. Set LLM_API_KEY (or OPENAI_API_KEY) environment variable or use --api-key"
                    .to_string(),
            )
        })?;

    // Get base URL - try LLM_BASE_URL first, fall back to OPENAI_BASE_URL for backward compatibility
    let base_url = args
        .base_url
        .or_else(|| std::env::var("LLM_BASE_URL").ok())
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

    // TEMPORARILY DISABLED TUI - Using simple console mode for raw logs
    println!("=== MCP Agent (Console Mode) ===");
    println!("Agent: {}", agent.name);
    println!("System Prompt: {}", agent.system_prompt);
    println!("Tools available: {}", agent.tools.len());
    for tool in &agent.tools {
        println!("  - {}: {}", tool.name, tool.description);
    }
    println!("\nType your message and press Enter (or 'quit' to exit):\n");

    use nexus_core::models::{ChatCompletionRequest, Message};
    use nexus_core::services::AgentService;
    use std::io::{self, BufRead};

    let agent_service = AgentService::new(&client, &agent);
    let mut messages = vec![Message::system(&agent.system_prompt)];

    let stdin = io::stdin();
    for line in stdin.lock().lines() {
        let user_input = line.map_err(|e| {
            nexus_core::models::Error::Other(format!("Failed to read input: {}", e))
        })?;

        if user_input.trim().is_empty() {
            continue;
        }

        if user_input.trim().eq_ignore_ascii_case("quit")
            || user_input.trim().eq_ignore_ascii_case("exit")
        {
            println!("Exiting...");
            break;
        }

        println!("\n[User] {}", user_input);
        messages.push(Message::user(&user_input));

        let mut request = ChatCompletionRequest::new(&model, messages.clone());

        if let Some(temp) = args.temperature {
            request = request.with_temperature(temp);
        }
        if let Some(max) = args.max_tokens {
            request = request.with_max_tokens(max);
        }
        if let Some(top_p) = args.top_p {
            request = request.with_top_p(top_p);
        }
        if let Some(freq) = args.frequency_penalty {
            request = request.with_frequency_penalty(freq);
        }
        if let Some(pres) = args.presence_penalty {
            request = request.with_presence_penalty(pres);
        }

        println!("[Agent] Processing...");
        match agent_service.chat(request).await {
            Ok(response) => {
                if let Some(ref content) = response.content {
                    let text = content.extract_text();
                    println!("[Agent] {}", text);
                } else {
                    println!("[Agent] (No content in response)");
                }
                messages.push(response);
            }
            Err(e) => {
                eprintln!("[Error] {}", e);
            }
        }
        println!("\n---\n");
    }

    Ok(())

    // ORIGINAL TUI CODE (commented out for debugging):
    /*
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
    */
}
