use crate::load_env;
use crate::models::{
    ChatCompletionChunk, ChatCompletionRequest, ChatCompletionResponse, Error, Result,
};
use futures::StreamExt;
use reqwest::Client as HttpClient;
use std::pin::Pin;
use std::sync::Arc;
use tokio_stream::Stream;

/// Result from uploading a file, includes file ID and MIME type
#[derive(Debug, Clone)]
pub struct UploadedFile {
    pub file_id: String,
    pub mime_type: String,
}

/// OpenAI-compatible API client
#[derive(Clone)]
pub struct Client {
    http_client: Arc<HttpClient>,
    base_url: String,
    api_key: String,
}

impl Client {
    /// Create a new client with the given API key and base URL
    pub fn new(api_key: impl Into<String>, base_url: impl Into<String>) -> Self {
        Self {
            http_client: Arc::new(HttpClient::new()),
            base_url: base_url.into(),
            api_key: api_key.into(),
        }
    }

    /// Create a new client with default OpenAI base URL
    pub fn with_default_url(api_key: impl Into<String>) -> Self {
        Self::new(api_key, "https://api.openai.com/v1")
    }

    /// Create a client from environment variables
    ///
    /// Reads `OPENAI_API_KEY` for the API key and optionally
    /// `OPENAI_BASE_URL` for the base URL (defaults to OpenAI's URL)
    pub fn from_env() -> Result<Self> {
        load_env();

        let api_key = std::env::var("OPENAI_API_KEY").map_err(|_| {
            Error::Configuration("OPENAI_API_KEY environment variable not set".to_string())
        })?;

        let base_url = std::env::var("OPENAI_BASE_URL")
            .unwrap_or_else(|_| "https://api.openai.com/v1".to_string());

        Ok(Self::new(api_key, base_url))
    }

    /// Check if request contains file references that require Responses API
    fn has_file_references(request: &ChatCompletionRequest) -> bool {
        use crate::models::{ContentPart, MessageContent};
        request.messages.iter().any(|msg| {
            if let Some(MessageContent::Array(parts)) = &msg.content {
                parts.iter().any(|part| matches!(part, ContentPart::File { .. }))
            } else {
                false
            }
        })
    }
    /// Send a chat completion request using Responses API (for requests with files)
    async fn chat_completion_with_responses_api(
        &self,
        request: ChatCompletionRequest,
    ) -> Result<ChatCompletionResponse> {
        let url = format!("{}/responses", self.base_url);

        // Build JSON request body for Responses API
        // In Responses API, 'messages' parameter is renamed to 'input'
        let mut request_body = serde_json::json!({
            "model": request.model,
            "input": request.messages,
        });

        // Add optional parameters
        if let Some(temp) = request.temperature {
            request_body["temperature"] = serde_json::Value::Number(serde_json::Number::from_f64(temp as f64).unwrap());
        }
        if let Some(max) = request.max_tokens {
            request_body["max_tokens"] = serde_json::Value::Number(serde_json::Number::from(max));
        }
        if let Some(top_p) = request.top_p {
            request_body["top_p"] = serde_json::Value::Number(serde_json::Number::from_f64(top_p as f64).unwrap());
        }
        if let Some(freq) = request.frequency_penalty {
            request_body["frequency_penalty"] = serde_json::Value::Number(serde_json::Number::from_f64(freq as f64).unwrap());
        }
        if let Some(pres) = request.presence_penalty {
            request_body["presence_penalty"] = serde_json::Value::Number(serde_json::Number::from_f64(pres as f64).unwrap());
        }
        if request.stream == Some(true) {
            request_body["stream"] = serde_json::Value::Bool(true);
        }
        if let Some(ref tools) = request.tools {
            request_body["tools"] = serde_json::Value::Array(tools.clone());
        }
        if let Some(ref response_format) = request.response_format {
            request_body["response_format"] = serde_json::to_value(response_format)
                .map_err(|e| Error::Other(format!("Failed to serialize response_format: {}", e)))?;
        }

        let response = self
            .http_client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await?;

        let status = response.status();

        if !status.is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());

            // Try to parse as API error
            if let Ok(api_error) = serde_json::from_str::<serde_json::Value>(&error_text) {
                if let Some(error_obj) = api_error.get("error").and_then(|e| e.as_object()) {
                    let mut api_error = serde_json::from_value::<crate::models::ApiError>(
                        serde_json::Value::Object(error_obj.clone()),
                    )
                    .unwrap_or_else(|_| crate::models::ApiError {
                        message: error_text.clone(),
                        error_type: None,
                        param: None,
                        code: None,
                    });

                    // Enhance error message with diagnostic info
                    let model_info = format!(" (model: {})", request.model);
                    let url_info = format!(" (URL: {})", url);
                    let status_info = format!(" [HTTP {}]", status);
                    api_error.message = format!(
                        "{}{}{}{}",
                        api_error.message, status_info, model_info, url_info
                    );

                    return Err(Error::Api(api_error));
                }
            }

            // Fallback error with diagnostic info
            let model_info = format!(" (model: {})", request.model);
            let url_info = format!(" (URL: {})", url);
            return Err(Error::Api(crate::models::ApiError {
                message: format!("HTTP {}: {}{}{}", status, error_text, model_info, url_info),
                error_type: Some("http_error".to_string()),
                param: None,
                code: Some(status.as_str().to_string()),
            }));
        }

        let completion_response: ChatCompletionResponse = response.json().await?;
        Ok(completion_response)
    }

    /// Send a chat completion request
    pub async fn chat_completion(
        &self,
        request: ChatCompletionRequest,
    ) -> Result<ChatCompletionResponse> {
        // Use Responses API if request contains file references
        if Self::has_file_references(&request) {
            return self.chat_completion_with_responses_api(request).await;
        }

        let url = format!("{}/chat/completions", self.base_url);

        let response = self
            .http_client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await?;

        let status = response.status();

        if !status.is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());

            // Try to parse as API error
            if let Ok(api_error) = serde_json::from_str::<serde_json::Value>(&error_text) {
                if let Some(error_obj) = api_error.get("error").and_then(|e| e.as_object()) {
                    let mut api_error = serde_json::from_value::<crate::models::ApiError>(
                        serde_json::Value::Object(error_obj.clone()),
                    )
                    .unwrap_or_else(|_| crate::models::ApiError {
                        message: error_text.clone(),
                        error_type: None,
                        param: None,
                        code: None,
                    });

                    // Enhance error message with diagnostic info
                    let model_info = format!(" (model: {})", request.model);
                    let url_info = format!(" (URL: {})", url);
                    let status_info = format!(" [HTTP {}]", status);
                    api_error.message = format!(
                        "{}{}{}{}",
                        api_error.message, status_info, model_info, url_info
                    );

                    return Err(Error::Api(api_error));
                }
            }

            // Fallback error with diagnostic info
            let model_info = format!(" (model: {})", request.model);
            let url_info = format!(" (URL: {})", url);
            return Err(Error::Api(crate::models::ApiError {
                message: format!("HTTP {}: {}{}{}", status, error_text, model_info, url_info),
                error_type: Some("http_error".to_string()),
                param: None,
                code: Some(status.as_str().to_string()),
            }));
        }

        let completion_response: ChatCompletionResponse = response.json().await?;
        Ok(completion_response)
    }

    /// Send a streaming chat completion request using Responses API (for requests with files)
    async fn chat_completion_stream_with_responses_api(
        &self,
        request: ChatCompletionRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<ChatCompletionChunk>> + Send>>> {
        let url = format!("{}/responses", self.base_url);
        // Build JSON request body for Responses API
        // In Responses API, 'messages' parameter is renamed to 'input'
        let mut request_body = serde_json::json!({
            "model": request.model,
            "input": request.messages,
            "stream": true,
        });

        // Add optional parameters
        if let Some(temp) = request.temperature {
            request_body["temperature"] = serde_json::Value::Number(
                serde_json::Number::from_f64(temp as f64)
                    .ok_or_else(|| Error::Other("Invalid temperature value".to_string()))?
            );
        }
        if let Some(max) = request.max_tokens {
            request_body["max_tokens"] = serde_json::Value::Number(serde_json::Number::from(max));
        }
        if let Some(top_p) = request.top_p {
            request_body["top_p"] = serde_json::Value::Number(
                serde_json::Number::from_f64(top_p as f64)
                    .ok_or_else(|| Error::Other("Invalid top_p value".to_string()))?
            );
        }
        if let Some(freq) = request.frequency_penalty {
            request_body["frequency_penalty"] = serde_json::Value::Number(
                serde_json::Number::from_f64(freq as f64)
                    .ok_or_else(|| Error::Other("Invalid frequency_penalty value".to_string()))?
            );
        }
        if let Some(pres) = request.presence_penalty {
            request_body["presence_penalty"] = serde_json::Value::Number(
                serde_json::Number::from_f64(pres as f64)
                    .ok_or_else(|| Error::Other("Invalid presence_penalty value".to_string()))?
            );
        }
        if let Some(ref tools) = request.tools {
            request_body["tools"] = serde_json::Value::Array(tools.clone());
        }
        if let Some(ref response_format) = request.response_format {
            request_body["response_format"] = serde_json::to_value(response_format)
                .map_err(|e| Error::Other(format!("Failed to serialize response_format: {}", e)))?;
        }

        let response = self
            .http_client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await?;

        let status = response.status();

        if !status.is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());

            // Try to parse as API error
            if let Ok(api_error) = serde_json::from_str::<serde_json::Value>(&error_text) {
                if let Some(error_obj) = api_error.get("error").and_then(|e| e.as_object()) {
                    let mut api_error = serde_json::from_value::<crate::models::ApiError>(
                        serde_json::Value::Object(error_obj.clone()),
                    )
                    .unwrap_or_else(|_| crate::models::ApiError {
                        message: error_text.clone(),
                        error_type: None,
                        param: None,
                        code: None,
                    });

                    // Enhance error message with diagnostic info
                    let model_info = format!(" (model: {})", request.model);
                    let url_info = format!(" (URL: {})", url);
                    let status_info = format!(" [HTTP {}]", status);
                    api_error.message = format!(
                        "{}{}{}{}",
                        api_error.message, status_info, model_info, url_info
                    );

                    return Err(Error::Api(api_error));
                }
            }

            // Fallback error with diagnostic info
            let model_info = format!(" (model: {})", request.model);
            let url_info = format!(" (URL: {})", url);
            return Err(Error::Api(crate::models::ApiError {
                message: format!("HTTP {}: {}{}{}", status, error_text, model_info, url_info),
                error_type: Some("http_error".to_string()),
                param: None,
                code: Some(status.as_str().to_string()),
            }));
        }

        use tokio::sync::mpsc;

        let (tx, rx) = mpsc::unbounded_channel();
        let mut buffer = Vec::new();

        tokio::spawn(async move {
            let mut bytes_stream = response.bytes_stream();
            while let Some(chunk_result) = bytes_stream.next().await {
                match chunk_result {
                    Ok(chunk) => {
                        buffer.extend_from_slice(chunk.as_ref());

                        // Parse complete lines
                        let mut line_start = 0;
                        let mut found_done = false;

                        for (i, &byte) in buffer.iter().enumerate() {
                            if byte == b'\n' {
                                let line = &buffer[line_start..i];
                                if let Ok(line_str) = std::str::from_utf8(line) {
                                    let line_str = line_str.trim();

                                    if line_str == "data: [DONE]" {
                                        found_done = true;
                                        break;
                                    }

                                    if line_str.starts_with("data: ") {
                                        let json_str = &line_str[6..];
                                        if let Ok(chunk) =
                                            serde_json::from_str::<ChatCompletionChunk>(json_str)
                                        {
                                            let _ = tx.send(Ok(chunk));
                                            // Yield to allow incremental processing by consumers
                                            tokio::task::yield_now().await;
                                        }
                                    }
                                }
                                line_start = i + 1;
                            }
                        }

                        if found_done {
                            break;
                        }

                        // Keep incomplete line
                        buffer.drain(..line_start);
                    }
                    Err(e) => {
                        let _ = tx.send(Err(Error::Network(e)));
                        break;
                    }
                }
            }
        });

        let stream = tokio_stream::wrappers::UnboundedReceiverStream::new(rx);
        Ok(Box::pin(stream))
    }

    /// Send a streaming chat completion request
    ///
    /// Returns a stream of ChatCompletionChunk objects
    pub async fn chat_completion_stream(
        &self,
        request: ChatCompletionRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<ChatCompletionChunk>> + Send>>> {
        // Use Responses API if request contains file references
        if Self::has_file_references(&request) {
            return self.chat_completion_stream_with_responses_api(request).await;
        }

        let url = format!("{}/chat/completions", self.base_url);

        // Enable streaming
        let mut request_with_stream = request.clone();
        request_with_stream.stream = Some(true);

        let response = self
            .http_client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request_with_stream)
            .send()
            .await?;

        let status = response.status();

        if !status.is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());

            // Try to parse as API error
            if let Ok(api_error) = serde_json::from_str::<serde_json::Value>(&error_text) {
                if let Some(error_obj) = api_error.get("error").and_then(|e| e.as_object()) {
                    let mut api_error = serde_json::from_value::<crate::models::ApiError>(
                        serde_json::Value::Object(error_obj.clone()),
                    )
                    .unwrap_or_else(|_| crate::models::ApiError {
                        message: error_text.clone(),
                        error_type: None,
                        param: None,
                        code: None,
                    });

                    // Enhance error message with diagnostic info
                    let model_info = format!(" (model: {})", request_with_stream.model);
                    let url_info = format!(" (URL: {})", url);
                    let status_info = format!(" [HTTP {}]", status);
                    api_error.message = format!(
                        "{}{}{}{}",
                        api_error.message, status_info, model_info, url_info
                    );

                    return Err(Error::Api(api_error));
                }
            }

            // Fallback error with diagnostic info
            let model_info = format!(" (model: {})", request_with_stream.model);
            let url_info = format!(" (URL: {})", url);
            return Err(Error::Api(crate::models::ApiError {
                message: format!("HTTP {}: {}{}{}", status, error_text, model_info, url_info),
                error_type: Some("http_error".to_string()),
                param: None,
                code: Some(status.as_str().to_string()),
            }));
        }

        use tokio::sync::mpsc;

        let (tx, rx) = mpsc::unbounded_channel();
        let mut buffer = Vec::new();

        tokio::spawn(async move {
            let mut bytes_stream = response.bytes_stream();
            while let Some(chunk_result) = bytes_stream.next().await {
                match chunk_result {
                    Ok(chunk) => {
                        buffer.extend_from_slice(chunk.as_ref());

                        // Parse complete lines
                        let mut line_start = 0;
                        let mut found_done = false;

                        for (i, &byte) in buffer.iter().enumerate() {
                            if byte == b'\n' {
                                let line = &buffer[line_start..i];
                                if let Ok(line_str) = std::str::from_utf8(line) {
                                    let line_str = line_str.trim();

                                    if line_str == "data: [DONE]" {
                                        found_done = true;
                                        break;
                                    }

                                    if line_str.starts_with("data: ") {
                                        let json_str = &line_str[6..];
                                        if let Ok(chunk) =
                                            serde_json::from_str::<ChatCompletionChunk>(json_str)
                                        {
                                            let _ = tx.send(Ok(chunk));
                                            // Yield to allow incremental processing by consumers
                                            tokio::task::yield_now().await;
                                        }
                                    }
                                }
                                line_start = i + 1;
                            }
                        }

                        if found_done {
                            break;
                        }

                        // Keep incomplete line
                        buffer.drain(..line_start);
                    }
                    Err(e) => {
                        let _ = tx.send(Err(Error::Network(e)));
                        break;
                    }
                }
            }
        });

        let stream = tokio_stream::wrappers::UnboundedReceiverStream::new(rx);
        Ok(Box::pin(stream))
    }

    /// Send a streaming chat completion request that yields content text deltas
    ///
    /// Returns a stream of content strings from the assistant's response chunks.
    /// The stream ends after the final chunk.
    pub async fn chat_completion_text_stream(
        &self,
        request: ChatCompletionRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<String>> + Send>>> {
        let chunk_stream = self.chat_completion_stream(request).await?;

        let text_stream = chunk_stream.filter_map(|chunk_result| async move {
            match chunk_result {
                Ok(chunk) => chunk
                    .choices
                    .first()
                    .and_then(|choice| choice.delta.content.clone())
                    .map(Ok),
                Err(e) => Some(Err(e)),
            }
        });

        Ok(Box::pin(text_stream))
    }

    /// Get the base URL
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Upload a file to the API
    ///
    /// Returns the file information (ID and MIME type) that can be used in chat requests
    pub async fn upload_file(&self, file_path: &std::path::Path) -> Result<UploadedFile> {
        use std::fs;
        use std::io::Read;

        let url = format!("{}/files", self.base_url);

        // Determine MIME type from extension
        let mime_type_str = get_mime_type(file_path);
        
        // Parse MIME type string to mime::Mime type
        let mime_type: mime::Mime = mime_type_str.parse()
            .map_err(|e| Error::Other(format!("Invalid MIME type '{}': {:?}", mime_type_str, e)))?;

        // Read file into bytes to set proper MIME type
        let mut file = fs::File::open(file_path).map_err(|e| {
            Error::Other(format!(
                "Failed to open file {}: {}",
                file_path.display(),
                e
            ))
        })?;
        
        let mut file_data = Vec::new();
        file.read_to_end(&mut file_data).map_err(|e| {
            Error::Other(format!(
                "Failed to read file {}: {}",
                file_path.display(),
                e
            ))
        })?;

        let mut file_name = file_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file")
            .to_string();

        // Ensure filename has an extension for Responses API type detection
        // If no extension, add one based on MIME type
        if file_path.extension().is_none() {
            let extension = match mime_type_str.as_str() {
                "application/pdf" => ".pdf",
                "text/plain" => ".txt",
                "text/markdown" => ".md",
                "application/json" => ".json",
                "text/csv" => ".csv",
                "application/xml" => ".xml",
                "image/jpeg" => ".jpg",
                "image/png" => ".png",
                "image/gif" => ".gif",
                "image/webp" => ".webp",
                _ => ".txt", // Default to .txt for unknown text types
            };
            file_name.push_str(extension);
        }

        // Create file part with explicit MIME type
        // Ensure file_name includes extension for proper type detection
        let file_part = reqwest::multipart::Part::bytes(file_data)
            .file_name(file_name.clone())
            .mime_str(mime_type.as_ref())
            .map_err(|e| Error::Other(format!("Failed to set MIME type '{}': {:?}", mime_type, e)))?;

        // Determine purpose based on file type
        // "vision" is for images, "assistants" is for other files (PDFs, text, etc.)
        let purpose = if is_image_file(file_path) {
            "vision"
        } else {
            "assistants"
        };

        // Create the multipart form
        let form = reqwest::multipart::Form::new()
            .text("purpose", purpose.to_string())
            .part("file", file_part);

        let response = self
            .http_client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .multipart(form)
            .send()
            .await?;

        let status = response.status();

        if !status.is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());

            return Err(Error::Api(crate::models::ApiError {
                message: format!("HTTP {}: {}", status, error_text),
                error_type: Some("http_error".to_string()),
                param: None,
                code: Some(status.as_str().to_string()),
            }));
        }

        let file_response: serde_json::Value = response.json().await?;

        // Log the full response for debugging
        eprintln!("File upload response: {}", serde_json::to_string_pretty(&file_response).unwrap_or_else(|_| "Failed to serialize response".to_string()));

        let file_id = file_response
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Other("File upload response missing 'id' field".to_string()))?;

        Ok(UploadedFile {
            file_id: file_id.to_string(),
            mime_type: mime_type.to_string(),
        })
    }

    /// Retrieve file details from the API
    /// This can be used to verify the file was uploaded correctly, including MIME type
    pub async fn get_file(&self, file_id: &str) -> Result<serde_json::Value> {
        let url = format!("{}/files/{}", self.base_url, file_id);

        let response = self
            .http_client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .await?;

        let status = response.status();

        if !status.is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());

            return Err(Error::Api(crate::models::ApiError {
                message: format!("HTTP {}: {}", status, error_text),
                error_type: Some("http_error".to_string()),
                param: None,
                code: Some(status.as_str().to_string()),
            }));
        }

        let file_details: serde_json::Value = response.json().await?;
        Ok(file_details)
    }
}

/// Check if a file is an image based on its extension
fn is_image_file(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| {
            let ext_lower = ext.to_lowercase();
            matches!(
                ext_lower.as_str(),
                "jpg" | "jpeg" | "png" | "gif" | "webp" | "bmp" | "svg"
            )
        })
        .unwrap_or(false)
}

/// Get MIME type based on file extension
/// OpenAI API supports: text/plain, application/pdf, text/markdown, text/csv, application/json, etc.
/// 
/// For files without extensions (like "Makefile"), we try to infer from the filename
fn get_mime_type(path: &std::path::Path) -> String {
    // First try to get from extension
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        let ext_lower = ext.to_lowercase();
        match ext_lower.as_str() {
            "jpg" | "jpeg" => return "image/jpeg".to_string(),
            "png" => return "image/png".to_string(),
            "gif" => return "image/gif".to_string(),
            "webp" => return "image/webp".to_string(),
            "bmp" => return "image/bmp".to_string(),
            "svg" => return "image/svg+xml".to_string(),
            "pdf" => return "application/pdf".to_string(),
            "txt" => return "text/plain".to_string(),
            "md" | "markdown" => return "text/markdown".to_string(),
            "json" => return "application/json".to_string(),
            "xml" => return "application/xml".to_string(),
            "csv" => return "text/csv".to_string(),
            "xlsx" => return "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet".to_string(),
            _ => {}
        }
    }
    
    // For files without extensions, try to infer from filename
    if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
        let name_lower = file_name.to_lowercase();
        // Common files without extensions
        if name_lower == "makefile" || name_lower.starts_with("makefile") {
            return "text/plain".to_string();
        }
        if name_lower == "dockerfile" || name_lower.starts_with("dockerfile") {
            return "text/plain".to_string();
        }
        if name_lower == ".gitignore" || name_lower == ".gitattributes" || name_lower == ".env" {
            return "text/plain".to_string();
        }
    }
    
    // Default to text/plain for unknown types
    // This is safer than application/octet-stream which may not be accepted
    "text/plain".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Message, MessageRole};
    use futures::StreamExt;
    use mockito::{Mock, Server};

    /// Get the default model from DEFAULT_MODEL environment variable or fallback
    fn default_test_model() -> String {
        std::env::var("DEFAULT_MODEL").unwrap_or_else(|_| "gpt-5-nano-2025-08-07".to_string())
    }

    #[tokio::test]
    async fn test_client_creation() {
        let client = Client::new("test-key", "https://api.example.com/v1");
        assert_eq!(client.base_url(), "https://api.example.com/v1");
    }

    #[tokio::test]
    async fn test_client_with_default_url() {
        let client = Client::with_default_url("test-key");
        assert_eq!(client.base_url(), "https://api.openai.com/v1");
    }

    #[test]
    fn test_client_request_builder() {
        let model = default_test_model();
        let model_clone = model.clone();
        let request = ChatCompletionRequest::new(model_clone, vec![Message::user("Hello")])
            .with_temperature(0.7);

        assert_eq!(request.model, model);
        assert_eq!(request.temperature, Some(0.7));
    }

    #[test]
    fn test_from_env_success() {
        // Set environment variables
        unsafe {
            std::env::set_var("OPENAI_API_KEY", "test-env-key");
            std::env::set_var("OPENAI_BASE_URL", "https://custom.example.com/v1");
        }

        let client = Client::from_env().unwrap();
        assert_eq!(client.base_url(), "https://custom.example.com/v1");

        // Clean up
        unsafe {
            std::env::remove_var("OPENAI_API_KEY");
            std::env::remove_var("OPENAI_BASE_URL");
        }
    }

    #[test]
    fn test_from_env_with_default_url() {
        unsafe {
            std::env::set_var("OPENAI_API_KEY", "test-env-key");
            std::env::remove_var("OPENAI_BASE_URL");
        }

        let client = Client::from_env().unwrap();
        assert_eq!(client.base_url(), "https://api.openai.com/v1");

        unsafe {
            std::env::remove_var("OPENAI_API_KEY");
        }
    }

    #[test]
    fn test_from_env_missing_api_key() {
        unsafe {
            std::env::remove_var("OPENAI_API_KEY");
        }

        let result = Client::from_env();
        assert!(result.is_err());
        if let Err(Error::Configuration(msg)) = result {
            assert!(msg.contains("OPENAI_API_KEY"));
        } else {
            panic!("Expected Configuration error");
        }
    }

    #[tokio::test]
    async fn test_chat_completion_success() {
        let mut server = Server::new_async().await;
        let mock = create_success_mock(&mut server).await;

        let client = Client::new("test-key", server.url());
        let model = default_test_model();
        let model_clone = model.clone();
        let request = ChatCompletionRequest::new(model_clone, vec![Message::user("Hello, world!")]);

        let response = client.chat_completion(request).await.unwrap();

        assert_eq!(response.id, "chatcmpl-123");
        assert_eq!(response.model, model);
        assert_eq!(response.choices.len(), 1);
        assert_eq!(
            response.choices[0].message.content,
            Some(crate::models::MessageContent::String(
                "Hello! How can I help you?".to_string()
            ))
        );
        assert_eq!(response.choices[0].message.role, MessageRole::Assistant);

        if let Some(usage) = response.usage {
            assert_eq!(usage.prompt_tokens, 10);
            assert_eq!(usage.completion_tokens, 8);
            assert_eq!(usage.total_tokens, 18);
        } else {
            panic!("Expected usage information");
        }

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_chat_completion_api_error() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("POST", "/chat/completions")
            .match_header("Authorization", "Bearer test-key")
            .match_header("Content-Type", "application/json")
            .with_status(400)
            .with_body(
                r#"{
                "error": {
                    "message": "Invalid request",
                    "type": "invalid_request_error",
                    "param": "model",
                    "code": "invalid_model"
                }
            }"#,
            )
            .create_async()
            .await;

        let client = Client::new("test-key", server.url());
        let request = ChatCompletionRequest::new("invalid-model", vec![Message::user("Hello")]);

        let result = client.chat_completion(request).await;
        assert!(result.is_err());

        if let Err(Error::Api(api_error)) = result {
            assert_eq!(api_error.message, "Invalid request");
            assert_eq!(
                api_error.error_type,
                Some("invalid_request_error".to_string())
            );
            assert_eq!(api_error.param, Some("model".to_string()));
            assert_eq!(api_error.code, Some("invalid_model".to_string()));
        } else {
            panic!("Expected Api error");
        }

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_chat_completion_http_error() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("POST", "/chat/completions")
            .match_header("Authorization", "Bearer test-key")
            .with_status(500)
            .with_body("Internal Server Error")
            .create_async()
            .await;

        let client = Client::new("test-key", server.url());
        let model = default_test_model();
        let request = ChatCompletionRequest::new(model, vec![Message::user("Hello")]);

        let result = client.chat_completion(request).await;
        assert!(result.is_err());

        if let Err(Error::Api(api_error)) = result {
            assert!(api_error.message.contains("HTTP 500"));
            assert!(api_error.message.contains("Internal Server Error"));
            assert_eq!(api_error.error_type, Some("http_error".to_string()));
            assert_eq!(api_error.code, Some("500".to_string()));
        } else {
            panic!("Expected Api error");
        }

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_chat_completion_network_error() {
        // Use an invalid URL to trigger a network error
        let client = Client::new("test-key", "http://127.0.0.1:1");
        let request =
            ChatCompletionRequest::new("gpt-5-nano-2025-08-07", vec![Message::user("Hello")]);

        let result = client.chat_completion(request).await;
        assert!(result.is_err());

        if let Err(Error::Network(_)) = result {
            // Expected network error
        } else {
            panic!("Expected Network error");
        }
    }

    #[tokio::test]
    async fn test_chat_completion_stream_success() {
        let mut server = Server::new_async().await;
        let model = default_test_model();
        let stream_body = format!("data: {{\"id\":\"chatcmpl-123\",\"object\":\"chat.completion.chunk\",\"created\":1677652288,\"model\":\"{}\",\"choices\":[{{\"index\":0,\"delta\":{{\"role\":\"assistant\",\"content\":\"Hello\"}},\"finish_reason\":null}}]}}\n\ndata: {{\"id\":\"chatcmpl-123\",\"object\":\"chat.completion.chunk\",\"created\":1677652288,\"model\":\"{}\",\"choices\":[{{\"index\":0,\"delta\":{{\"content\":\"!\"}},\"finish_reason\":null}}]}}\n\ndata: {{\"id\":\"chatcmpl-123\",\"object\":\"chat.completion.chunk\",\"created\":1677652288,\"model\":\"{}\",\"choices\":[{{\"index\":0,\"delta\":{{}},\"finish_reason\":\"stop\"}}]}}\n\ndata: [DONE]\n", model, model, model);

        let mock = server
            .mock("POST", "/chat/completions")
            .match_header("Authorization", "Bearer test-key")
            .match_header("Content-Type", "application/json")
            .with_status(200)
            .with_header("content-type", "text/event-stream")
            .with_body(stream_body)
            .create_async()
            .await;

        let client = Client::new("test-key", server.url());
        let model = default_test_model();
        let request = ChatCompletionRequest::new(model, vec![Message::user("Hello")]);

        let mut stream = client.chat_completion_stream(request).await.unwrap();

        let mut chunks = Vec::new();
        while let Some(result) = stream.next().await {
            chunks.push(result.unwrap());
        }

        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].id, "chatcmpl-123");
        assert_eq!(chunks[0].choices.len(), 1);
        assert_eq!(
            chunks[0].choices[0].delta.content,
            Some("Hello".to_string())
        );

        assert_eq!(chunks[1].choices[0].delta.content, Some("!".to_string()));

        assert_eq!(chunks[2].choices[0].finish_reason, Some("stop".to_string()));

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_chat_completion_stream_api_error() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("POST", "/chat/completions")
            .match_header("Authorization", "Bearer test-key")
            .with_status(401)
            .with_body(
                r#"{
                "error": {
                    "message": "Invalid API key",
                    "type": "invalid_request_error",
                    "code": "invalid_api_key"
                }
            }"#,
            )
            .create_async()
            .await;

        let client = Client::new("test-key", server.url());
        let model = default_test_model();
        let request = ChatCompletionRequest::new(model, vec![Message::user("Hello")]);

        let result = client.chat_completion_stream(request).await;
        assert!(result.is_err());

        if let Err(Error::Api(api_error)) = result {
            assert_eq!(api_error.message, "Invalid API key");
            assert_eq!(api_error.code, Some("invalid_api_key".to_string()));
        } else {
            panic!("Expected Api error");
        }

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_chat_completion_stream_http_error() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("POST", "/chat/completions")
            .match_header("Authorization", "Bearer test-key")
            .with_status(503)
            .with_body("Service Unavailable")
            .create_async()
            .await;

        let client = Client::new("test-key", server.url());
        let model = default_test_model();
        let request = ChatCompletionRequest::new(model, vec![Message::user("Hello")]);

        let result = client.chat_completion_stream(request).await;
        assert!(result.is_err());

        if let Err(Error::Api(api_error)) = result {
            assert!(api_error.message.contains("HTTP 503"));
            assert_eq!(api_error.error_type, Some("http_error".to_string()));
        } else {
            panic!("Expected Api error");
        }

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_chat_completion_stream_partial_lines() {
        let mut server = Server::new_async().await;
        // Simulate partial line that gets completed in next chunk
        let model = default_test_model();
        let stream_body = format!("data: {{\"id\":\"chatcmpl-123\",\"object\":\"chat.completion.chunk\",\"created\":1677652288,\"model\":\"{}\",\"choices\":[{{\"index\":0,\"delta\":{{\"content\":\"Hello\"}}}}]}}\n\ndata: {{\"id\":\"chatcmpl-123\",\"object\":\"chat.completion.chunk\",\"created\":1677652288,\"model\":\"{}\",\"choices\":[{{\"index\":0,\"delta\":{{\"content\":\" world\"}}}}]}}\n\ndata: [DONE]\n", model, model);

        let mock = server
            .mock("POST", "/chat/completions")
            .match_header("Authorization", "Bearer test-key")
            .with_status(200)
            .with_header("content-type", "text/event-stream")
            .with_body(stream_body)
            .create_async()
            .await;

        let client = Client::new("test-key", server.url());
        let model = default_test_model();
        let request = ChatCompletionRequest::new(model, vec![Message::user("Hello")]);

        let mut stream = client.chat_completion_stream(request).await.unwrap();

        let mut chunks = Vec::new();
        while let Some(result) = stream.next().await {
            chunks.push(result.unwrap());
        }

        assert_eq!(chunks.len(), 2);
        assert_eq!(
            chunks[0].choices[0].delta.content,
            Some("Hello".to_string())
        );
        assert_eq!(
            chunks[1].choices[0].delta.content,
            Some(" world".to_string())
        );

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_chat_completion_stream_ignores_invalid_lines() {
        let mut server = Server::new_async().await;
        // Include invalid lines that should be ignored
        let model = default_test_model();
        let stream_body = format!("invalid line\n\ndata: {{\"id\":\"chatcmpl-123\",\"object\":\"chat.completion.chunk\",\"created\":1677652288,\"model\":\"{}\",\"choices\":[{{\"index\":0,\"delta\":{{\"content\":\"Hello\"}}}}]}}\n\n: comment line\n\ndata: [DONE]\n", model);

        let mock = server
            .mock("POST", "/chat/completions")
            .match_header("Authorization", "Bearer test-key")
            .with_status(200)
            .with_header("content-type", "text/event-stream")
            .with_body(stream_body)
            .create_async()
            .await;

        let client = Client::new("test-key", server.url());
        let model = default_test_model();
        let request = ChatCompletionRequest::new(model, vec![Message::user("Hello")]);

        let mut stream = client.chat_completion_stream(request).await.unwrap();

        let mut chunks = Vec::new();
        while let Some(result) = stream.next().await {
            chunks.push(result.unwrap());
        }

        // Should only parse the valid data line
        assert_eq!(chunks.len(), 1);
        assert_eq!(
            chunks[0].choices[0].delta.content,
            Some("Hello".to_string())
        );

        mock.assert_async().await;
    }

    // Helper function to create a successful chat completion mock
    async fn create_success_mock(server: &mut Server) -> Mock {
        let model = default_test_model();
        let response_body = serde_json::json!({
            "id": "chatcmpl-123",
            "object": "chat.completion",
            "created": 1677652288,
            "model": model,
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "Hello! How can I help you?"
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 8,
                "total_tokens": 18
            }
        });

        server
            .mock("POST", "/chat/completions")
            .match_header("Authorization", "Bearer test-key")
            .match_header("Content-Type", "application/json")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(response_body.to_string())
            .create_async()
            .await
    }
}
