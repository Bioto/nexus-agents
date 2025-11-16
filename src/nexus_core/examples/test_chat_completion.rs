use nexus_core::{ChatCompletionRequest, Message, NexusApiService};

/// Test script that runs a chat completion via NexusApiService
///
/// This example demonstrates how to:
/// 1. Create a NexusApiService from environment variables
/// 2. Create a chat completion request
/// 3. Send the request and receive a response
/// 4. Extract and display the response content
///
/// # Environment Variables Required
/// - `OPENAI_API_KEY`: Your OpenAI API key
/// - `OPENAI_BASE_URL`: (Optional) Base URL for the API (defaults to OpenAI)
/// - `DEFAULT_MODEL`: (Optional) Default model to use
///
/// # Usage
/// ```bash
/// cargo run --example test_chat_completion
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

    // Send the request
    println!("\nSending request to API...");
    let response = service.chat(request).await?;

    // Extract and display the response
    println!("\n=== Response ===");
    if let Some(content) = response.content {
        println!("{}", content.extract_text());
    } else {
        println!("(No content in response)");
    }

    println!("\nTest completed successfully!");

    Ok(())
}





