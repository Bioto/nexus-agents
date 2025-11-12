use nexus_core::{ChatCompletionRequest, Message, NexusApiService};
use futures::StreamExt;
use std::io::{self, Write};

/// Test script that runs a streaming chat completion via NexusApiService
///
/// This example demonstrates how to:
/// 1. Create a NexusApiService from environment variables
/// 2. Create a chat completion request
/// 3. Send the request and receive a streaming response
/// 4. Process and display the response content as it arrives
///
/// # Environment Variables Required
/// - `OPENAI_API_KEY`: Your OpenAI API key
/// - `OPENAI_BASE_URL`: (Optional) Base URL for the API (defaults to OpenAI)
/// - `DEFAULT_MODEL`: (Optional) Default model to use
///
/// # Usage
/// ```bash
/// cargo run --example test_chat_streaming
/// ```
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize the library (loads .env file if present)
    nexus_core::init();

    println!("Creating NexusApiService from environment...");
    let service = NexusApiService::from_env()?;
    println!("Service created successfully!");

    // Get the model from environment or use a default
    let model = std::env::var("DEFAULT_MODEL")
        .unwrap_or_else(|_| "gpt-4o-mini".to_string());

    println!("\nCreating chat completion request...");
    println!("Model: {}", model);
    println!("Message: Hello! Can you tell me a fun fact about Rust programming?");

    // Create a chat completion request
    let request = ChatCompletionRequest::new(
        model,
        vec![Message::user("Hello! Can you tell me a fun fact about Rust programming?")],
    );

    // Send the request and get a stream
    println!("\nSending streaming request to API...");
    let mut stream = service.chat_stream(request).await?;

    // Process the stream and display content as it arrives
    println!("\n=== Streaming Response ===");
    let mut full_response = String::new();
    while let Some(result) = stream.next().await {
        match result {
            Ok(content) => {
                print!("{}", content);
                io::stdout().flush()?;
                full_response.push_str(&content);
            }
            Err(e) => {
                eprintln!("\nError: {}", e);
                return Err(e.into());
            }
        }
    }

    // Print the final accumulated response
    println!("\n\n=== Final Complete Response ===");
    println!("{}", full_response);

    println!("\nTest completed successfully!");

    Ok(())
}


