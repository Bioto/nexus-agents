use nexus_core::{ChatCompletionRequest, Message, MessageContent, NexusApiService};
use std::path::PathBuf;

/// Test script that demonstrates file uploading and chat completion with files
///
/// This example demonstrates how to:
/// 1. Create a NexusApiService from environment variables
/// 2. Upload a file to the API
/// 3. Create a chat completion request that includes the uploaded file
/// 4. Send the request and receive a response
///
/// # Environment Variables Required
/// - `OPENAI_API_KEY`: Your OpenAI API key
/// - `OPENAI_BASE_URL`: (Optional) Base URL for the API (defaults to OpenAI)
/// - `DEFAULT_MODEL`: (Optional) Default model to use
///
/// # Usage
/// ```bash
/// # Upload a specific file
/// cargo run --example test_file_upload -- /path/to/file.txt
///
/// # Or create a test file and upload it
/// cargo run --example test_file_upload
/// ```
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize the library (loads .env file if present)
    nexus_core::init();

    println!("Creating NexusApiService from environment...");
    let service = NexusApiService::from_env()?;
    println!("Service created successfully!");

    // Determine which file to upload
    let file_path = if let Some(file_arg) = std::env::args().nth(1) {
        PathBuf::from(file_arg)
    } else {
        // Create a test file if no file is provided
        println!("\nNo file provided, creating a test file...");
        let test_file = PathBuf::from("test_upload.txt");
        std::fs::write(&test_file, "This is a test file for file upload functionality.\nIt contains some sample text that can be analyzed by the AI.")?;
        println!("Created test file: {}", test_file.display());
        test_file
    };

    // Verify file exists
    if !file_path.exists() {
        return Err(format!("File not found: {}", file_path.display()).into());
    }

    println!("\nUploading file: {}", file_path.display());

    // Upload the file using the client
    let uploaded_file = service.client().upload_file(&file_path).await?;
    println!("File uploaded successfully!");
    println!("  File ID: {}", uploaded_file.file_id);
    println!("  MIME Type: {}", uploaded_file.mime_type);

    // Get the model from environment or use a default
    let model = std::env::var("DEFAULT_MODEL")
        .unwrap_or_else(|_| "gpt-4o-mini".to_string());

    println!("\nCreating chat completion request with uploaded file...");
    println!("Model: {}", model);

    // Create message content with the uploaded file
    // You can include text along with the file, or just the file
    let file_content = MessageContent::with_file(
        "Please analyze this file and summarize its contents.",
        uploaded_file.file_id,
    );

    // Create a user message with the file content
    let user_message = Message::user_with_content(file_content);

    // Create a chat completion request
    let request = ChatCompletionRequest::new(model, vec![user_message]);

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

    // Clean up test file if we created it
    if file_path.file_name().and_then(|n| n.to_str()) == Some("test_upload.txt") {
        if let Err(e) = std::fs::remove_file(&file_path) {
            eprintln!("Warning: Failed to remove test file: {}", e);
        } else {
            println!("\nCleaned up test file.");
        }
    }

    println!("\nTest completed successfully!");

    Ok(())
}

