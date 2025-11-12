use crate::load_env;
use crate::models::{
    ChatCompletionChunk, ChatCompletionRequest, ChatCompletionResponse, Error, Result,
};
use futures::StreamExt;
use reqwest::Client as HttpClient;
use std::pin::Pin;
use std::sync::Arc;
use tokio_stream::Stream;

/// Result from uploading a PDF file, includes file ID and MIME type
#[derive(Debug, Clone)]
pub struct UploadedPdf {
    pub file_id: String,
    pub mime_type: String,
}

/// Responses API client
#[derive(Clone)]
pub struct ResponsesClient {
    http_client: Arc<HttpClient>,
    base_url: String,
    api_key: String,
}

impl ResponsesClient {
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

    /// Send a non-streaming chat completion request using Responses API
    pub async fn responses_completion(
        &self,
        request: ChatCompletionRequest,
    ) -> Result<ChatCompletionResponse> {
        let url = format!("{}/responses", self.base_url);

        // Transform messages for Responses API format
        // Responses API uses different field names: input_text, input_image, input_file
        // Note: When files are present, text cannot be mixed with files in the same content array
        use crate::models::{ContentPart, MessageContent};
        let mut transformed_messages: Vec<serde_json::Value> = Vec::new();
        
        for msg in &request.messages {
            let mut message_json = serde_json::json!({
                "role": msg.role,
            });

            if let Some(content) = &msg.content {
                match content {
                    MessageContent::String(text) => {
                        // For simple text messages, use input_text
                        message_json["content"] = serde_json::json!([{
                            "type": "input_text",
                            "text": text
                        }]);
                        transformed_messages.push(message_json);
                    }
                    MessageContent::Array(parts) => {
                        // Check if there's a file in the parts
                        let has_file = parts.iter().any(|p| matches!(p, ContentPart::File { .. }));
                        
                        if has_file {
                            // When file is present, extract text separately
                            let text_parts: Vec<String> = parts
                                .iter()
                                .filter_map(|p| {
                                    if let ContentPart::Text { text } = p {
                                        Some(text.clone())
                                    } else {
                                        None
                                    }
                                })
                                .collect();
                            
                            // If there's text with the file, send it as a separate user message first
                            if !text_parts.is_empty() {
                                let combined_text = text_parts.join(" ");
                                transformed_messages.push(serde_json::json!({
                                    "role": "user",
                                    "content": [{
                                        "type": "input_text",
                                        "text": combined_text
                                    }]
                                }));
                            }
                            
                            // Now add the file(s) and images in a separate message
                            let file_parts: Vec<serde_json::Value> = parts
                                .iter()
                                .filter_map(|part| match part {
                                    ContentPart::File { file_id } => {
                                        Some(serde_json::json!({
                                            "type": "input_file",
                                            "file_id": file_id
                                        }))
                                    }
                                    ContentPart::ImageUrl { image_url } => {
                                        Some(serde_json::json!({
                                            "type": "input_image",
                                            "image_url": image_url
                                        }))
                                    }
                                    ContentPart::Text { .. } => None, // Already handled above
                                })
                                .collect();
                            
                            if !file_parts.is_empty() {
                                message_json["content"] = serde_json::Value::Array(file_parts);
                                transformed_messages.push(message_json);
                            }
                        } else {
                            // No file, can include text normally
                            let transformed_parts: Vec<serde_json::Value> = parts
                                .iter()
                                .map(|part| match part {
                                    ContentPart::Text { text } => {
                                        serde_json::json!({
                                            "type": "input_text",
                                            "text": text
                                        })
                                    }
                                    ContentPart::ImageUrl { image_url } => {
                                        serde_json::json!({
                                            "type": "input_image",
                                            "image_url": image_url
                                        })
                                    }
                                    ContentPart::File { file_id } => {
                                        serde_json::json!({
                                            "type": "input_file",
                                            "file_id": file_id
                                        })
                                    }
                                })
                                .collect();
                            message_json["content"] = serde_json::Value::Array(transformed_parts);
                            transformed_messages.push(message_json);
                        }
                    }
                }
            } else {
                // Message with no content, add as-is
                transformed_messages.push(message_json);
            }
        }

        // Build JSON request body for Responses API
        // In Responses API, 'messages' parameter is renamed to 'input'
        let mut request_body = serde_json::json!({
            "model": request.model,
            "input": transformed_messages,
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

        // Parse Responses API response and transform to ChatCompletionResponse format
        let response_json: serde_json::Value = response.json().await?;
        
        // Transform Responses API response to ChatCompletionResponse format
        // Responses API uses different field names and structure
        let id = response_json
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        
        let model = response_json
            .get("model")
            .and_then(|v| v.as_str())
            .unwrap_or(&request.model)
            .to_string();
        
        // Extract choices from the response
        use crate::models::{Choice, Message, MessageRole};
        let mut choices = Vec::new();
        
        // Try to extract from "output" array (Responses API format)
        if let Some(output) = response_json.get("output") {
            if let Some(output_array) = output.as_array() {
                for (index, item) in output_array.iter().enumerate() {
                    if let Some(content_type) = item.get("type").and_then(|v| v.as_str()) {
                        match content_type {
                            "message" => {
                                // Message type has content array with output_text items
                                if let Some(content_array) = item.get("content").and_then(|v| v.as_array()) {
                                    let mut text_parts = Vec::new();
                                    
                                    for content_item in content_array {
                                        if let Some(item_type) = content_item.get("type").and_then(|v| v.as_str()) {
                                            if item_type == "output_text" {
                                                if let Some(text) = content_item.get("text").and_then(|v| v.as_str()) {
                                                    text_parts.push(text);
                                                }
                                            }
                                        }
                                    }
                                    
                                    if !text_parts.is_empty() {
                                        let combined_text = text_parts.join("");
                                        let message = Message {
                                            role: MessageRole::Assistant,
                                            content: Some(MessageContent::String(combined_text)),
                                            tool_calls: None,
                                            tool_call_id: None,
                                            name: None,
                                        };
                                        
                                        choices.push(Choice {
                                            index: index as u32,
                                            message,
                                            finish_reason: Some("stop".to_string()),
                                        });
                                    }
                                }
                            }
                            "output_text" => {
                                // Direct output_text type (if it exists at top level)
                                let text = item
                                    .get("text")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string();
                                
                                let message = Message {
                                    role: MessageRole::Assistant,
                                    content: Some(MessageContent::String(text)),
                                    tool_calls: None,
                                    tool_call_id: None,
                                    name: None,
                                };
                                
                                choices.push(Choice {
                                    index: index as u32,
                                    message,
                                    finish_reason: Some("stop".to_string()),
                                });
                            }
                            "summary_text" => {
                                // Handle summary_text type as well
                                let text = item
                                    .get("text")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string();
                                
                                let message = Message {
                                    role: MessageRole::Assistant,
                                    content: Some(MessageContent::String(text)),
                                    tool_calls: None,
                                    tool_call_id: None,
                                    name: None,
                                };
                                
                                choices.push(Choice {
                                    index: index as u32,
                                    message,
                                    finish_reason: Some("stop".to_string()),
                                });
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
        
        // If no choices from output, try standard "choices" format
        if choices.is_empty() {
            if let Some(choices_array) = response_json.get("choices").and_then(|v| v.as_array()) {
                if let Ok(parsed_choices) = serde_json::from_value::<Vec<Choice>>(serde_json::Value::Array(choices_array.clone())) {
                    choices = parsed_choices;
                }
            }
        }
        
        // If still no choices, try to extract text from top-level fields
        if choices.is_empty() {
            // Some APIs might return text directly
            if let Some(text) = response_json.get("text").and_then(|v| v.as_str()) {
                let message = Message {
                    role: MessageRole::Assistant,
                    content: Some(MessageContent::String(text.to_string())),
                    tool_calls: None,
                    tool_call_id: None,
                    name: None,
                };
                
                choices.push(Choice {
                    index: 0,
                    message,
                    finish_reason: Some("stop".to_string()),
                });
            }
        }
        
        // Extract usage if present
        let usage = response_json
            .get("usage")
            .and_then(|v| serde_json::from_value(v.clone()).ok());
        
        // Create ChatCompletionResponse with transformed data
        let completion_response = ChatCompletionResponse {
            id,
            object: "chat.completion".to_string(),
            created: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            model,
            choices,
            usage,
        };
        
        Ok(completion_response)
    }

    /// Send a streaming chat completion request using Responses API
    pub async fn responses_completion_stream(
        &self,
        request: ChatCompletionRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<ChatCompletionChunk>> + Send>>> {
        let url = format!("{}/responses", self.base_url);
        
        // Transform messages for Responses API format
        // Responses API uses different field names: input_text, input_image, input_file
        // Note: When files are present, text cannot be mixed with files in the same content array
        use crate::models::{ContentPart, MessageContent};
        let mut transformed_messages: Vec<serde_json::Value> = Vec::new();
        
        for msg in &request.messages {
            let mut message_json = serde_json::json!({
                "role": msg.role,
            });

            if let Some(content) = &msg.content {
                match content {
                    MessageContent::String(text) => {
                        // For simple text messages, use input_text
                        message_json["content"] = serde_json::json!([{
                            "type": "input_text",
                            "text": text
                        }]);
                        transformed_messages.push(message_json);
                    }
                    MessageContent::Array(parts) => {
                        // Check if there's a file in the parts
                        let has_file = parts.iter().any(|p| matches!(p, ContentPart::File { .. }));
                        
                        if has_file {
                            // When file is present, extract text separately
                            let text_parts: Vec<String> = parts
                                .iter()
                                .filter_map(|p| {
                                    if let ContentPart::Text { text } = p {
                                        Some(text.clone())
                                    } else {
                                        None
                                    }
                                })
                                .collect();
                            
                            // If there's text with the file, send it as a separate user message first
                            if !text_parts.is_empty() {
                                let combined_text = text_parts.join(" ");
                                transformed_messages.push(serde_json::json!({
                                    "role": "user",
                                    "content": [{
                                        "type": "input_text",
                                        "text": combined_text
                                    }]
                                }));
                            }
                            
                            // Now add the file(s) and images in a separate message
                            let file_parts: Vec<serde_json::Value> = parts
                                .iter()
                                .filter_map(|part| match part {
                                    ContentPart::File { file_id } => {
                                        Some(serde_json::json!({
                                            "type": "input_file",
                                            "file_id": file_id
                                        }))
                                    }
                                    ContentPart::ImageUrl { image_url } => {
                                        Some(serde_json::json!({
                                            "type": "input_image",
                                            "image_url": image_url
                                        }))
                                    }
                                    ContentPart::Text { .. } => None, // Already handled above
                                })
                                .collect();
                            
                            if !file_parts.is_empty() {
                                message_json["content"] = serde_json::Value::Array(file_parts);
                                transformed_messages.push(message_json);
                            }
                        } else {
                            // No file, can include text normally
                            let transformed_parts: Vec<serde_json::Value> = parts
                                .iter()
                                .map(|part| match part {
                                    ContentPart::Text { text } => {
                                        serde_json::json!({
                                            "type": "input_text",
                                            "text": text
                                        })
                                    }
                                    ContentPart::ImageUrl { image_url } => {
                                        serde_json::json!({
                                            "type": "input_image",
                                            "image_url": image_url
                                        })
                                    }
                                    ContentPart::File { file_id } => {
                                        serde_json::json!({
                                            "type": "input_file",
                                            "file_id": file_id
                                        })
                                    }
                                })
                                .collect();
                            message_json["content"] = serde_json::Value::Array(transformed_parts);
                            transformed_messages.push(message_json);
                        }
                    }
                }
            } else {
                // Message with no content, add as-is
                transformed_messages.push(message_json);
            }
        }
        
        // Build JSON request body for Responses API
        // In Responses API, 'messages' parameter is renamed to 'input'
        let mut request_body = serde_json::json!({
            "model": request.model,
            "input": transformed_messages,
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
                                        // Try to parse as Responses API format first
                                        if let Ok(response_json) = serde_json::from_str::<serde_json::Value>(json_str) {
                                            // Check if this is a Responses API chunk
                                            if response_json.get("output").is_some() {
                                                // Transform Responses API chunk to ChatCompletionChunk format
                                                use crate::models::{ChatCompletionChunk, ChoiceDelta, MessageDelta, MessageRole};
                                                
                                                let mut choices = Vec::new();
                                                
                                                // Extract from output array
                                                if let Some(output) = response_json.get("output").and_then(|v| v.as_array()) {
                                                    for (index, item) in output.iter().enumerate() {
                                                        if let Some(content_type) = item.get("type").and_then(|v| v.as_str()) {
                                                            if content_type == "message" {
                                                                if let Some(content_array) = item.get("content").and_then(|v| v.as_array()) {
                                                                    for content_item in content_array {
                                                                        if let Some(item_type) = content_item.get("type").and_then(|v| v.as_str()) {
                                                                            if item_type == "output_text" {
                                                                                if let Some(text) = content_item.get("text").and_then(|v| v.as_str()) {
                                                                                    let delta = MessageDelta {
                                                                                        role: Some(MessageRole::Assistant),
                                                                                        content: Some(text.to_string()),
                                                                                        tool_calls: None,
                                                                                    };
                                                                                    
                                                                                    choices.push(ChoiceDelta {
                                                                                        index: index as u32,
                                                                                        delta,
                                                                                        finish_reason: None,
                                                                                    });
                                                                                }
                                                                            }
                                                                        }
                                                                    }
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                                
                                                if !choices.is_empty() {
                                                    let id = response_json
                                                        .get("id")
                                                        .and_then(|v| v.as_str())
                                                        .unwrap_or("")
                                                        .to_string();
                                                    
                                                    let model = response_json
                                                        .get("model")
                                                        .and_then(|v| v.as_str())
                                                        .unwrap_or("")
                                                        .to_string();
                                                    
                                                    let created = response_json
                                                        .get("created_at")
                                                        .and_then(|v| v.as_u64())
                                                        .unwrap_or(0);
                                                    
                                                    let chunk = ChatCompletionChunk {
                                                        id,
                                                        object: "chat.completion.chunk".to_string(),
                                                        created,
                                                        model,
                                                        choices,
                                                    };
                                                    
                                                    let _ = tx.send(Ok(chunk));
                                                    tokio::task::yield_now().await;
                                                }
                                            } else {
                                                // Try standard format
                                                if let Ok(chunk) = serde_json::from_str::<ChatCompletionChunk>(json_str) {
                                                    let _ = tx.send(Ok(chunk));
                                                    tokio::task::yield_now().await;
                                                }
                                            }
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

    /// Upload a PDF file to the API
    ///
    /// Returns the file information (ID and MIME type) that can be used in chat requests
    pub async fn upload_pdf(&self, file_path: &std::path::Path) -> Result<UploadedPdf> {
        use std::fs;
        use std::io::Read;

        let url = format!("{}/files", self.base_url);

        // Verify it's a PDF file
        let is_pdf = file_path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_lowercase() == "pdf")
            .unwrap_or(false);

        if !is_pdf {
            return Err(Error::Other(
                "File must be a PDF (.pdf extension required)".to_string(),
            ));
        }

        // Read file into bytes
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

        let file_name = file_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file.pdf")
            .to_string();

        // Create file part with PDF MIME type
        let file_part = reqwest::multipart::Part::bytes(file_data)
            .file_name(file_name.clone())
            .mime_str("application/pdf")
            .map_err(|e| Error::Other(format!("Failed to set MIME type: {:?}", e)))?;

        // Create the multipart form
        // PDFs use "assistants" purpose
        let form = reqwest::multipart::Form::new()
            .text("purpose", "assistants".to_string())
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
        let file_id = file_response
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Other("File upload response missing 'id' field".to_string()))?;

        Ok(UploadedPdf {
            file_id: file_id.to_string(),
            mime_type: "application/pdf".to_string(),
        })
    }

    /// Get the base URL
    pub fn base_url(&self) -> &str {
        &self.base_url
    }
}

