use clap::Args;

/// Get the default model from DEFAULT_MODEL environment variable or fallback
pub fn default_model() -> String {
    std::env::var("DEFAULT_MODEL").unwrap_or_else(|_| "gpt-5-nano-2025-08-07".to_string())
}

/// Common parameters shared across commands
#[derive(Args)]
pub struct CommonArgs {
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

    /// Base URL for the API (defaults to LLM_BASE_URL or OPENAI_BASE_URL env var)
    #[arg(long)]
    pub base_url: Option<String>,

    /// API key (defaults to LLM_API_KEY or OPENAI_API_KEY env var)
    #[arg(long)]
    pub api_key: Option<String>,
}
