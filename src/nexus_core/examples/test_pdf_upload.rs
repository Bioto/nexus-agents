use nexus_core::client::LLMClient;
use nexus_core::{ChatCompletionRequest, Message, MessageContent, NexusApiService};
use std::path::PathBuf;

/// Test script that demonstrates PDF file uploading and chat completion with PDF files
///
/// This example demonstrates how to:
/// 1. Create a NexusApiService from environment variables
/// 2. Upload a PDF file to the API
/// 3. Create a chat completion request that includes the uploaded PDF
/// 4. Send the request and receive a response
///
/// # Environment Variables Required
/// - `OPENAI_API_KEY`: Your OpenAI API key
/// - `OPENAI_BASE_URL`: (Optional) Base URL for the API (defaults to OpenAI)
/// - `DEFAULT_MODEL`: (Optional) Default model to use
///
/// # Usage
/// ```bash
/// # Upload the default PDF file (sample-local-pdf.pdf in project root)
/// cargo run --example test_pdf_upload --package nexus-core
///
/// # Upload a specific PDF file
/// cargo run --example test_pdf_upload --package nexus-core -- /path/to/file.pdf
/// ```
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize the library (loads .env file if present)
    nexus_core::init();

    println!("Creating NexusApiService from environment...");
    let service = NexusApiService::from_env()?;
    println!("Service created successfully!");

    // Determine which PDF file to upload
    let file_path = if let Some(file_arg) = std::env::args().nth(1) {
        PathBuf::from(file_arg)
    } else {
        // Use the default PDF file in the project root
        let project_root = std::env::current_dir()?;
        let default_pdf = project_root.join("sample-local-pdf.pdf");
        
        if !default_pdf.exists() {
            return Err(format!(
                "Default PDF file not found: {}\nPlease provide a PDF file path as an argument.",
                default_pdf.display()
            ).into());
        }
        
        println!("\nUsing default PDF file: {}", default_pdf.display());
        default_pdf
    };

    // Verify file exists
    if !file_path.exists() {
        return Err(format!("File not found: {}", file_path.display()).into());
    }

    // Verify it's a PDF file
    if let Some(ext) = file_path.extension() {
        if ext.to_str().unwrap_or("").to_lowercase() != "pdf" {
            eprintln!("Warning: File does not have .pdf extension: {}", file_path.display());
        }
    } else {
        eprintln!("Warning: File has no extension: {}", file_path.display());
    }

    println!("\nUploading PDF file: {}", file_path.display());

    // Upload the file using the client
    let uploaded_file = service.client().upload_pdf(&file_path).await?;
    println!("PDF file uploaded successfully!");
    println!("  File ID: {}", uploaded_file.file_id);
    println!("  MIME Type: {}", uploaded_file.mime_type);

    // Get the model from environment or use a default
    let model = std::env::var("DEFAULT_MODEL")
        .unwrap_or_else(|_| "gpt-4o-mini".to_string());

    println!("\nCreating chat completion request with uploaded PDF...");
    println!("Model: {}", model);

    // Create message content with the uploaded PDF file
    // You can include text along with the file, or just the file
    let file_content = MessageContent::with_file(
        "Please analyze this PDF file and summarize its contents.",
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

    println!("\nTest completed successfully!");

    Ok(())
}


