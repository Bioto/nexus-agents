use crate::client::ResponsesClient;
use crate::factories::AgentFactory;
use crate::load_env;
use crate::models::{Message, Result};
use crate::services::AgentService;
use clap::Args;
use log::{info, warn};
use std::path::PathBuf;

#[derive(Args)]
pub struct TestPythonArgs {
    /// The prompt to send to the agent
    #[arg(required = true)]
    pub prompt: String,

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

    /// Base URL for the API (defaults to OpenAI or OPENAI_BASE_URL env var)
    #[arg(long)]
    pub base_url: Option<String>,

    /// API key (defaults to OPENAI_API_KEY env var)
    #[arg(long)]
    pub api_key: Option<String>,
}

pub async fn run_test_python(args: TestPythonArgs) -> Result<()> {
    // Load environment variables from .env file (if it exists)
    load_env();

    // Verify servers directory exists
    if !args.servers_dir.exists() {
        return Err(crate::models::Error::Configuration(format!(
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
            crate::models::Error::Configuration(
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

    info!("[test-python] Creating client and agent...");
    info!(
        "[test-python] Servers directory: {}",
        args.servers_dir.display()
    );
    info!("[test-python] Model: {}", model);

    // Create client
    let client = ResponsesClient::new(api_key, base_url);

    // Create MCP agent (same as mcp_agent command)
    let agent = AgentFactory::mcp_agent(&args.servers_dir);

    // Create agent service
    let service = AgentService::new(&client, &agent);

    // Create request
    let mut request =
        crate::models::ChatCompletionRequest::new(model, vec![Message::user(&args.prompt)]);

    if let Some(temp) = args.temperature {
        request = request.with_temperature(temp);
    }
    if let Some(max) = args.max_tokens {
        request = request.with_max_tokens(max);
    }

    info!("[test-python] Sending prompt: {}", args.prompt);
    info!("[test-python] Waiting for response...");

    // Send the request and get response
    let response = service.chat(request).await?;

    info!("[test-python] Response received:");
    if let Some(content) = &response.content {
        info!("{}", content.extract_text());
    } else {
        warn!("[test-python] No content in response");
    }

    Ok(())
}
