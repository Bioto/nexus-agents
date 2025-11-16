use clap::Args;
use nexus_core::client::ResponsesClient;
use nexus_core::factories::AgentFactory;
use nexus_core::load_env;
use nexus_core::models::{Message, Result};
use nexus_core::services::AgentService;
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

    println!("[test-python] Creating client and agent...");
    println!(
        "[test-python] Servers directory: {}",
        args.servers_dir.display()
    );
    println!("[test-python] Model: {}", model);

    // Create client
    let client = ResponsesClient::new(api_key, base_url);

    // Create MCP agent (same as mcp_agent command)
    let agent = AgentFactory::mcp_agent(&args.servers_dir);

    // Create agent service
    let service = AgentService::new(&client, &agent);

    // Create request
    let mut request =
        nexus_core::models::ChatCompletionRequest::new(model, vec![Message::user(&args.prompt)]);

    if let Some(temp) = args.temperature {
        request = request.with_temperature(temp);
    }
    if let Some(max) = args.max_tokens {
        request = request.with_max_tokens(max);
    }

    println!("[test-python] Sending prompt: {}", args.prompt);
    println!("[test-python] Waiting for response...\n");

    // Send the request and get response
    let response = service.chat(request).await?;

    println!("[test-python] Response received:");
    if let Some(content) = &response.content {
        println!("{}", content.extract_text());
    } else {
        println!("(No content in response)");
    }

    Ok(())
}
