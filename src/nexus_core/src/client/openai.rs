use crate::models::{
    ChatCompletionChunk, ChatCompletionRequest, ChatCompletionResponse, Error, Result,
};
use futures::StreamExt;
use reqwest::Client as HttpClient;
use std::pin::Pin;
use std::sync::Arc;
use tokio_stream::Stream;

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
        let api_key = std::env::var("OPENAI_API_KEY").map_err(|_| {
            Error::Configuration("OPENAI_API_KEY environment variable not set".to_string())
        })?;

        let base_url = std::env::var("OPENAI_BASE_URL")
            .unwrap_or_else(|_| "https://api.openai.com/v1".to_string());

        Ok(Self::new(api_key, base_url))
    }

    /// Send a chat completion request
    pub async fn chat_completion(
        &self,
        request: ChatCompletionRequest,
    ) -> Result<ChatCompletionResponse> {
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
                    let api_error = serde_json::from_value::<crate::models::ApiError>(
                        serde_json::Value::Object(error_obj.clone()),
                    )
                    .unwrap_or_else(|_| crate::models::ApiError {
                        message: error_text.clone(),
                        error_type: None,
                        param: None,
                        code: None,
                    });
                    return Err(Error::Api(api_error));
                }
            }

            return Err(Error::Api(crate::models::ApiError {
                message: format!("HTTP {}: {}", status, error_text),
                error_type: Some("http_error".to_string()),
                param: None,
                code: Some(status.as_str().to_string()),
            }));
        }

        let completion_response: ChatCompletionResponse = response.json().await?;
        Ok(completion_response)
    }

    /// Send a streaming chat completion request
    ///
    /// Returns a stream of ChatCompletionChunk objects
    pub async fn chat_completion_stream(
        &self,
        request: ChatCompletionRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<ChatCompletionChunk>> + Send>>> {
        let url = format!("{}/chat/completions", self.base_url);

        let req = self
            .http_client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json");

        // Enable streaming
        let mut request_with_stream = request;
        request_with_stream.stream = Some(true);

        let response = req.json(&request_with_stream).send().await?;

        let status = response.status();

        if !status.is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());

            // Try to parse as API error
            if let Ok(api_error) = serde_json::from_str::<serde_json::Value>(&error_text) {
                if let Some(error_obj) = api_error.get("error").and_then(|e| e.as_object()) {
                    let api_error = serde_json::from_value::<crate::models::ApiError>(
                        serde_json::Value::Object(error_obj.clone()),
                    )
                    .unwrap_or_else(|_| crate::models::ApiError {
                        message: error_text.clone(),
                        error_type: None,
                        param: None,
                        code: None,
                    });
                    return Err(Error::Api(api_error));
                }
            }

            return Err(Error::Api(crate::models::ApiError {
                message: format!("HTTP {}: {}", status, error_text),
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
        let request =
            ChatCompletionRequest::new(model, vec![Message::user("Hello")]).with_temperature(0.7);

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
        let request = ChatCompletionRequest::new(model, vec![Message::user("Hello, world!")]);

        let response = client.chat_completion(request).await.unwrap();

        assert_eq!(response.id, "chatcmpl-123");
        assert_eq!(response.model, model);
        assert_eq!(response.choices.len(), 1);
        assert_eq!(
            response.choices[0].message.content,
            Some("Hello! How can I help you?".to_string())
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
