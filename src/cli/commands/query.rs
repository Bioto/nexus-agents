use crate::client::Client;
use crate::models::{ChatCompletionRequest, Message, Result};
use clap::Args;
use tokio_stream::StreamExt;

#[derive(Args)]
pub struct QueryArgs {
    /// The message content to send
    #[arg(short, long)]
    pub message: Option<String>,

    /// System prompt to use
    #[arg(short, long)]
    pub system: Option<String>,

    /// Model to use (default: gpt-3.5-turbo)
    #[arg(long, default_value = "gpt-3.5-turbo")]
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

    /// Output format: json or text (default: text)
    #[arg(short, long, default_value = "text")]
    pub output: String,
}

pub async fn run_query(args: QueryArgs) -> Result<()> {
    // Get API key
    let api_key = args
        .api_key
        .or_else(|| std::env::var("OPENAI_API_KEY").ok())
        .ok_or_else(|| {
            crate::models::Error::Configuration(
                "API key not provided. Set OPENAI_API_KEY environment variable or use --api-key".to_string(),
            )
        })?;

    // Get base URL
    let base_url = args
        .base_url
        .or_else(|| std::env::var("OPENAI_BASE_URL").ok())
        .unwrap_or_else(|| "https://api.openai.com/v1".to_string());

    // Create client
    let client = Client::new(api_key, base_url);

    // Build messages
    let mut messages = Vec::new();

    if let Some(system) = args.system {
        messages.push(Message::system(system));
    }

    let user_message = if let Some(msg) = args.message {
        msg
    } else {
        // Read from stdin if no message provided
        use std::io::{self, Read};
        let mut buffer = String::new();
        io::stdin()
            .read_to_string(&mut buffer)
            .map_err(|e| crate::models::Error::Other(format!("Failed to read from stdin: {}", e)))?;
        buffer.trim().to_string()
    };

    if user_message.is_empty() {
        return Err(crate::models::Error::Configuration(
            "No message provided. Use --message or provide input via stdin".to_string(),
        ));
    }

    messages.push(Message::user(user_message));

    // Build request
    let mut request = ChatCompletionRequest::new(args.model, messages);

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

    // Send streaming request
    let mut stream = client.chat_completion_stream(request).await?;
    
    let mut full_content = String::new();
    let mut chunks = Vec::new();
    
    // Collect all chunks
    while let Some(chunk_result) = stream.next().await {
        match chunk_result {
            Ok(chunk) => {
                chunks.push(chunk.clone());
                if let Some(choice) = chunk.choices.first() {
                    if let Some(content) = &choice.delta.content {
                        full_content.push_str(content);
                        // Output text as it streams
                        if args.output == "text" {
                            print!("{}", content);
                            use std::io::Write;
                            std::io::stdout().flush().ok();
                        }
                    }
                }
            }
            Err(e) => {
                return Err(e);
            }
        }
    }
    
    // Output response
    match args.output.as_str() {
        "json" => {
            // For JSON, output the final complete response structure
            // We'll construct a response-like structure from chunks
            if let Some(first_chunk) = chunks.first() {
                let json = serde_json::json!({
                    "id": first_chunk.id,
                    "object": first_chunk.object,
                    "created": first_chunk.created,
                    "model": first_chunk.model,
                    "choices": [{
                        "index": 0,
                        "message": {
                            "role": "assistant",
                            "content": full_content
                        },
                        "finish_reason": chunks.last().and_then(|c| c.choices.first().and_then(|ch| ch.finish_reason.clone()))
                    }]
                });
                println!("{}", serde_json::to_string_pretty(&json).unwrap());
            }
        }
        "text" => {
            // Already output during streaming, just add newline
            println!();
        }
        _ => {
            return Err(crate::models::Error::Configuration(
                format!("Invalid output format: {}. Use 'json' or 'text'", args.output),
            ));
        }
    }

    Ok(())
}

