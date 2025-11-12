use crate::client::Client;
use crate::models::{Agent, ChatCompletionRequest, Message, Result};
use crate::services::AgentService;
use std::pin::Pin;
use tokio_stream::Stream;

/// Configuration for chat completion requests
///
/// This struct allows you to specify all the parameters that can be used
/// when making chat completion requests.
#[derive(Debug, Clone)]
pub struct ChatConfig {
    /// Model to use
    pub model: String,
    /// Temperature parameter (0.0 to 2.0)
    pub temperature: Option<f32>,
    /// Maximum tokens in the response
    pub max_tokens: Option<u32>,
    /// Top-p parameter
    pub top_p: Option<f32>,
    /// Frequency penalty
    pub frequency_penalty: Option<f32>,
    /// Presence penalty
    pub presence_penalty: Option<f32>,
}

impl ChatConfig {
    /// Create a new ChatConfig with the given model
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            temperature: None,
            max_tokens: None,
            top_p: None,
            frequency_penalty: None,
            presence_penalty: None,
        }
    }

    /// Set the temperature parameter
    pub fn with_temperature(mut self, temperature: f32) -> Self {
        self.temperature = Some(temperature);
        self
    }

    /// Set the maximum tokens
    pub fn with_max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = Some(max_tokens);
        self
    }

    /// Set the top-p parameter
    pub fn with_top_p(mut self, top_p: f32) -> Self {
        self.top_p = Some(top_p);
        self
    }

    /// Set the frequency penalty
    pub fn with_frequency_penalty(mut self, penalty: f32) -> Self {
        self.frequency_penalty = Some(penalty);
        self
    }

    /// Set the presence penalty
    pub fn with_presence_penalty(mut self, penalty: f32) -> Self {
        self.presence_penalty = Some(penalty);
        self
    }

    /// Apply this configuration to a ChatCompletionRequest
    fn apply_to_request(&self, mut request: ChatCompletionRequest) -> ChatCompletionRequest {
        request.model = self.model.clone();
        if let Some(temp) = self.temperature {
            request = request.with_temperature(temp);
        }
        if let Some(max) = self.max_tokens {
            request = request.with_max_tokens(max);
        }
        if let Some(top_p) = self.top_p {
            request = request.with_top_p(top_p);
        }
        if let Some(freq) = self.frequency_penalty {
            request = request.with_frequency_penalty(freq);
        }
        if let Some(pres) = self.presence_penalty {
            request = request.with_presence_penalty(pres);
        }
        request
    }
}

impl Default for ChatConfig {
    fn default() -> Self {
        Self::new(
            std::env::var("DEFAULT_MODEL")
                .unwrap_or_else(|_| "gpt-5-nano-2025-08-07".to_string()),
        )
    }
}

/// Unified service for interacting with agents and the API
///
/// This service provides a clean interface for third-party code to:
/// - Make direct API calls without agents (simple chat completions)
/// - Make agent calls with automatic tool execution
/// - Support both streaming and non-streaming responses
///
/// # Example
///
/// ```no_run
/// use nexus_core::{Client, NexusApiService, ChatCompletionRequest, Message};
///
/// #[tokio::main]
/// async fn main() -> Result<(), Box<dyn std::error::Error>> {
///     // Create client
///     let client = Client::from_env()?;
///
///     // Create service
///     let service = NexusApiService::new(client);
///
///     // Non-agent call (direct API)
///     let request = ChatCompletionRequest::new(
///         "gpt-4o-mini",
///         vec![Message::user("Hello!")]
///     );
///     let response = service.chat(request).await?;
///     println!("Response: {}", response.content.unwrap().extract_text());
///
///     Ok(())
/// }
/// ```
///
/// # Example with Agent
///
/// ```no_run
/// use nexus_core::{Client, NexusApiService, Agent, ChatCompletionRequest, Message};
///
/// #[tokio::main]
/// async fn main() -> Result<(), Box<dyn std::error::Error>> {
///     let client = Client::from_env()?;
///     let service = NexusApiService::new(client);
///
///     // Create an agent with tools
///     let agent = Agent::new(
///         "Calculator",
///         "A calculator agent",
///         "You are a helpful calculator",
///         vec![],
///         crate::tools::ToolRegistry::new(),
///     );
///
///     // Agent call (with automatic tool execution)
///     let request = ChatCompletionRequest::new(
///         "gpt-4o-mini",
///         vec![Message::user("What is 2 + 2?")]
///     );
///     let response = service.chat_with_agent(&agent, request).await?;
///     println!("Response: {}", response.content.unwrap().extract_text());
///
///     Ok(())
/// }
/// ```
pub struct NexusApiService {
    client: Client,
}

impl NexusApiService {
    /// Create a new NexusApiService with the given client
    pub fn new(client: Client) -> Self {
        Self { client }
    }

    /// Create a new NexusApiService from environment variables
    ///
    /// Reads `OPENAI_API_KEY` and optionally `OPENAI_BASE_URL` from environment
    pub fn from_env() -> Result<Self> {
        let client = Client::from_env()?;
        Ok(Self::new(client))
    }

    /// Make a direct API call without an agent (no tool calling)
    ///
    /// This is a simple chat completion request that goes directly to the API
    /// without any agent configuration or tool execution.
    ///
    /// # Arguments
    ///
    /// * `request` - The chat completion request
    ///
    /// # Returns
    ///
    /// The assistant's message from the API response
    ///
    /// # Example
    ///
    /// ```no_run
    /// use nexus_core::{NexusApiService, ChatCompletionRequest, Message};
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let service = NexusApiService::from_env()?;
    /// let request = ChatCompletionRequest::new(
    ///     "gpt-4o-mini",
    ///     vec![Message::user("Hello!")]
    /// );
    /// let response = service.chat(request).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn chat(&self, request: ChatCompletionRequest) -> Result<Message> {
        let response = self.client.chat_completion(request).await?;

        response
            .choices
            .first()
            .map(|choice| choice.message.clone())
            .ok_or_else(|| {
                crate::models::Error::Other("No response from API".to_string())
            })
    }

    /// Make a streaming API call without an agent
    ///
    /// Returns a stream of content deltas from the assistant's response.
    ///
    /// # Arguments
    ///
    /// * `request` - The chat completion request
    ///
    /// # Returns
    ///
    /// A stream of content strings (deltas)
    ///
    /// # Example
    ///
    /// ```no_run
    /// use nexus_core::{NexusApiService, ChatCompletionRequest, Message};
    /// use futures::StreamExt;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let service = NexusApiService::from_env()?;
    /// let request = ChatCompletionRequest::new(
    ///     "gpt-4o-mini",
    ///     vec![Message::user("Tell me a story")]
    /// );
    /// let mut stream = service.chat_stream(request).await?;
    ///
    /// while let Some(result) = stream.next().await {
    ///     match result {
    ///         Ok(content) => print!("{}", content),
    ///         Err(e) => eprintln!("Error: {}", e),
    ///     }
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn chat_stream(
        &self,
        request: ChatCompletionRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<String>> + Send>>> {
        self.client.chat_completion_text_stream(request).await
    }

    /// Make a chat request with an agent (supports tool calling)
    ///
    /// This method uses the agent's configuration and automatically handles
    /// tool execution in a loop until the agent provides a final response.
    ///
    /// # Arguments
    ///
    /// * `agent` - The agent to use for this request
    /// * `request` - The chat completion request
    ///
    /// # Returns
    ///
    /// The final assistant message after all tool calls are resolved
    ///
    /// # Example
    ///
    /// ```no_run
    /// use nexus_core::{NexusApiService, Agent, ChatCompletionRequest, Message};
    /// use nexus_core::tools::ToolRegistry;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let service = NexusApiService::from_env()?;
    ///
    /// // Create an agent with tools
    /// let agent = Agent::new(
    ///     "Calculator",
    ///     "A calculator agent",
    ///     "You are a helpful calculator",
    ///     vec![],
    ///     ToolRegistry::new(),
    /// );
    ///
    /// let request = ChatCompletionRequest::new(
    ///     "gpt-4o-mini",
    ///     vec![Message::user("What is 2 + 2?")]
    /// );
    /// let response = service.chat_with_agent(&agent, request).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn chat_with_agent(
        &self,
        agent: &Agent,
        request: ChatCompletionRequest,
    ) -> Result<Message> {
        let agent_service = AgentService::new(&self.client, agent);
        agent_service.chat(request).await
    }

    /// Make a streaming chat request with an agent (supports tool calling)
    ///
    /// Returns a stream of events including content deltas, tool execution
    /// updates, and completion status.
    ///
    /// # Arguments
    ///
    /// * `agent` - The agent to use for this request
    /// * `request` - The chat completion request
    ///
    /// # Returns
    ///
    /// A stream of `AgentStreamEvent` items
    ///
    /// # Example
    ///
    /// ```no_run
    /// use nexus_core::{NexusApiService, Agent, ChatCompletionRequest, Message, AgentStreamEvent};
    /// use nexus_core::tools::ToolRegistry;
    /// use futures::StreamExt;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let service = NexusApiService::from_env()?;
    ///
    /// let agent = Agent::new(
    ///     "Calculator",
    ///     "A calculator agent",
    ///     "You are a helpful calculator",
    ///     vec![],
    ///     ToolRegistry::new(),
    /// );
    ///
    /// let request = ChatCompletionRequest::new(
    ///     "gpt-4o-mini",
    ///     vec![Message::user("Calculate 2 + 2")]
    /// );
    /// let mut stream = service.chat_with_agent_stream(&agent, request).await?;
    ///
    /// while let Some(result) = stream.next().await {
    ///     match result {
    ///         Ok(AgentStreamEvent::ContentDelta(content)) => {
    ///             print!("{}", content);
    ///         }
    ///         Ok(AgentStreamEvent::ToolExecuting(tool_name)) => {
    ///             println!("\n[Executing tool: {}]", tool_name);
    ///         }
    ///         Ok(AgentStreamEvent::Done) => {
    ///             println!("\n[Done]");
    ///             break;
    ///         }
    ///         Err(e) => {
    ///             eprintln!("Error: {}", e);
    ///             break;
    ///         }
    ///         _ => {}
    ///     }
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn chat_with_agent_stream(
        &self,
        agent: &Agent,
        request: ChatCompletionRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<crate::services::AgentStreamEvent>> + Send>>>
    {
        let agent_service = AgentService::new(&self.client, agent);
        agent_service.chat_stream(request).await
    }

    /// Get a reference to the underlying client
    ///
    /// This allows direct access to the client for advanced use cases
    pub fn client(&self) -> &Client {
        &self.client
    }

    /// Make a direct API call with configuration parameters
    ///
    /// This is a convenience method that allows you to specify all chat parameters
    /// directly without building a ChatCompletionRequest manually.
    ///
    /// # Arguments
    ///
    /// * `config` - Chat configuration (model, temperature, etc.)
    /// * `messages` - The conversation messages
    ///
    /// # Returns
    ///
    /// The assistant's message from the API response
    ///
    /// # Example
    ///
    /// ```no_run
    /// use nexus_core::{NexusApiService, ChatConfig, Message};
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let service = NexusApiService::from_env()?;
    /// let config = ChatConfig::new("gpt-4o-mini")
    ///     .with_temperature(0.7)
    ///     .with_max_tokens(100);
    /// let response = service.chat_with_config(config, vec![Message::user("Hello!")]).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn chat_with_config(
        &self,
        config: ChatConfig,
        messages: Vec<Message>,
    ) -> Result<Message> {
        let request = ChatCompletionRequest::new(config.model.clone(), messages);
        let request = config.apply_to_request(request);
        self.chat(request).await
    }

    /// Make a streaming API call with configuration parameters
    ///
    /// # Arguments
    ///
    /// * `config` - Chat configuration (model, temperature, etc.)
    /// * `messages` - The conversation messages
    ///
    /// # Returns
    ///
    /// A stream of content strings (deltas)
    ///
    /// # Example
    ///
    /// ```no_run
    /// use nexus_core::{NexusApiService, ChatConfig, Message};
    /// use futures::StreamExt;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let service = NexusApiService::from_env()?;
    /// let config = ChatConfig::new("gpt-4o-mini").with_temperature(0.7);
    /// let mut stream = service.chat_stream_with_config(config, vec![Message::user("Hello")]).await?;
    ///
    /// while let Some(result) = stream.next().await {
    ///     match result {
    ///         Ok(content) => print!("{}", content),
    ///         Err(e) => eprintln!("Error: {}", e),
    ///     }
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn chat_stream_with_config(
        &self,
        config: ChatConfig,
        messages: Vec<Message>,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<String>> + Send>>> {
        let request = ChatCompletionRequest::new(config.model.clone(), messages);
        let request = config.apply_to_request(request);
        self.chat_stream(request).await
    }

    /// Make a chat request with an agent using configuration parameters
    ///
    /// # Arguments
    ///
    /// * `agent` - The agent to use for this request
    /// * `config` - Chat configuration (model, temperature, etc.)
    /// * `messages` - The conversation messages
    ///
    /// # Returns
    ///
    /// The final assistant message after all tool calls are resolved
    ///
    /// # Example
    ///
    /// ```no_run
    /// use nexus_core::{NexusApiService, Agent, ChatConfig, Message};
    /// use nexus_core::tools::ToolRegistry;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let service = NexusApiService::from_env()?;
    ///
    /// let agent = Agent::new(
    ///     "Calculator",
    ///     "A calculator agent",
    ///     "You are a helpful calculator",
    ///     vec![],
    ///     ToolRegistry::new(),
    /// );
    ///
    /// let config = ChatConfig::new("gpt-4o-mini")
    ///     .with_temperature(0.5)
    ///     .with_max_tokens(200);
    /// let response = service.chat_with_agent_config(&agent, config, vec![Message::user("What is 2 + 2?")]).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn chat_with_agent_config(
        &self,
        agent: &Agent,
        config: ChatConfig,
        messages: Vec<Message>,
    ) -> Result<Message> {
        let request = ChatCompletionRequest::new(config.model.clone(), messages);
        let request = config.apply_to_request(request);
        self.chat_with_agent(agent, request).await
    }

    /// Make a streaming chat request with an agent using configuration parameters
    ///
    /// # Arguments
    ///
    /// * `agent` - The agent to use for this request
    /// * `config` - Chat configuration (model, temperature, etc.)
    /// * `messages` - The conversation messages
    ///
    /// # Returns
    ///
    /// A stream of `AgentStreamEvent` items
    ///
    /// # Example
    ///
    /// ```no_run
    /// use nexus_core::{NexusApiService, Agent, ChatConfig, Message, AgentStreamEvent};
    /// use nexus_core::tools::ToolRegistry;
    /// use futures::StreamExt;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let service = NexusApiService::from_env()?;
    ///
    /// let agent = Agent::new(
    ///     "Calculator",
    ///     "A calculator agent",
    ///     "You are a helpful calculator",
    ///     vec![],
    ///     ToolRegistry::new(),
    /// );
    ///
    /// let config = ChatConfig::new("gpt-4o-mini").with_temperature(0.5);
    /// let mut stream = service.chat_with_agent_stream_config(&agent, config, vec![Message::user("Calculate 2 + 2")]).await?;
    ///
    /// while let Some(result) = stream.next().await {
    ///     match result {
    ///         Ok(AgentStreamEvent::ContentDelta(content)) => {
    ///             print!("{}", content);
    ///         }
    ///         Ok(AgentStreamEvent::Done) => break,
    ///         Err(e) => {
    ///             eprintln!("Error: {}", e);
    ///             break;
    ///         }
    ///         _ => {}
    ///     }
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn chat_with_agent_stream_config(
        &self,
        agent: &Agent,
        config: ChatConfig,
        messages: Vec<Message>,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<crate::services::AgentStreamEvent>> + Send>>>
    {
        let request = ChatCompletionRequest::new(config.model.clone(), messages);
        let request = config.apply_to_request(request);
        self.chat_with_agent_stream(agent, request).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::factories::AgentFactory;
    use crate::models::{MessageContent, MessageRole};
    use futures::StreamExt;
    use mockito::{Mock, Server};

    /// Get the default model from DEFAULT_MODEL environment variable or fallback
    fn default_test_model() -> String {
        std::env::var("DEFAULT_MODEL").unwrap_or_else(|_| "gpt-5-nano-2025-08-07".to_string())
    }

    /// Helper function to create a successful chat completion mock
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

    #[tokio::test]
    async fn test_nexus_api_service_creation() {
        let client = Client::new("test-key", "https://api.example.com");
        let service = NexusApiService::new(client);
        assert_eq!(service.client().base_url(), "https://api.example.com");
    }

    #[tokio::test]
    async fn test_chat_direct_success() {
        let mut server = Server::new_async().await;
        let mock = create_success_mock(&mut server).await;

        let client = Client::new("test-key", server.url());
        let service = NexusApiService::new(client);
        let model = default_test_model();
        let request = ChatCompletionRequest::new(model, vec![Message::user("Hello!")]);

        let response = service.chat(request).await.unwrap();

        assert_eq!(response.role, MessageRole::Assistant);
        assert_eq!(
            response.content,
            Some(MessageContent::String("Hello! How can I help you?".to_string()))
        );

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_chat_direct_no_response() {
        let mut server = Server::new_async().await;
        let model = default_test_model();
        let response_body = serde_json::json!({
            "id": "chatcmpl-123",
            "object": "chat.completion",
            "created": 1677652288,
            "model": model,
            "choices": []
        });

        let mock = server
            .mock("POST", "/chat/completions")
            .match_header("Authorization", "Bearer test-key")
            .with_status(200)
            .with_body(response_body.to_string())
            .create_async()
            .await;

        let client = Client::new("test-key", server.url());
        let service = NexusApiService::new(client);
        let request = ChatCompletionRequest::new(model, vec![Message::user("Hello!")]);

        let result = service.chat(request).await;
        assert!(result.is_err());
        if let Err(crate::models::Error::Other(msg)) = result {
            assert!(msg.contains("No response from API"));
        } else {
            panic!("Expected 'No response from API' error");
        }

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_chat_stream_success() {
        let mut server = Server::new_async().await;
        let model = default_test_model();
        let stream_body = format!(
            "data: {{\"id\":\"chatcmpl-123\",\"object\":\"chat.completion.chunk\",\"created\":1677652288,\"model\":\"{}\",\"choices\":[{{\"index\":0,\"delta\":{{\"role\":\"assistant\",\"content\":\"Hello\"}},\"finish_reason\":null}}]}}\n\ndata: {{\"id\":\"chatcmpl-123\",\"object\":\"chat.completion.chunk\",\"created\":1677652288,\"model\":\"{}\",\"choices\":[{{\"index\":0,\"delta\":{{\"content\":\"!\"}},\"finish_reason\":null}}]}}\n\ndata: [DONE]\n",
            model, model
        );

        let mock = server
            .mock("POST", "/chat/completions")
            .match_header("Authorization", "Bearer test-key")
            .with_status(200)
            .with_header("content-type", "text/event-stream")
            .with_body(stream_body)
            .create_async()
            .await;

        let client = Client::new("test-key", server.url());
        let service = NexusApiService::new(client);
        let request = ChatCompletionRequest::new(model, vec![Message::user("Hello")]);

        let mut stream = service.chat_stream(request).await.unwrap();

        let mut chunks = Vec::new();
        while let Some(result) = stream.next().await {
            chunks.push(result.unwrap());
        }

        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0], "Hello");
        assert_eq!(chunks[1], "!");

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_chat_with_agent_success() {
        let mut server = Server::new_async().await;
        let mock = create_success_mock(&mut server).await;

        let client = Client::new("test-key", server.url());
        let service = NexusApiService::new(client);
        let agent = AgentFactory::calculator();
        let model = default_test_model();
        let request = ChatCompletionRequest::new(model, vec![Message::user("What is 2 + 2?")]);

        let response = service.chat_with_agent(&agent, request).await.unwrap();

        assert_eq!(response.role, MessageRole::Assistant);
        assert!(response.content.is_some());

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_chat_with_agent_stream() {
        let mut server = Server::new_async().await;
        let model = default_test_model();
        let stream_body = format!(
            "data: {{\"id\":\"chatcmpl-123\",\"object\":\"chat.completion.chunk\",\"created\":1677652288,\"model\":\"{}\",\"choices\":[{{\"index\":0,\"delta\":{{\"role\":\"assistant\",\"content\":\"The answer\"}},\"finish_reason\":null}}]}}\n\ndata: {{\"id\":\"chatcmpl-123\",\"object\":\"chat.completion.chunk\",\"created\":1677652288,\"model\":\"{}\",\"choices\":[{{\"index\":0,\"delta\":{{\"content\":\" is 4\"}},\"finish_reason\":null}}]}}\n\ndata: [DONE]\n",
            model, model
        );

        let mock = server
            .mock("POST", "/chat/completions")
            .match_header("Authorization", "Bearer test-key")
            .with_status(200)
            .with_header("content-type", "text/event-stream")
            .with_body(stream_body)
            .create_async()
            .await;

        let client = Client::new("test-key", server.url());
        let service = NexusApiService::new(client);
        let agent = AgentFactory::calculator();
        let request = ChatCompletionRequest::new(model, vec![Message::user("What is 2 + 2?")]);

        let mut stream = service.chat_with_agent_stream(&agent, request).await.unwrap();

        let mut events = Vec::new();
        while let Some(result) = stream.next().await {
            events.push(result);
        }

        // Should have content deltas and a Done event
        assert!(!events.is_empty());
        let has_content = events.iter().any(|e| {
            matches!(e, Ok(crate::services::AgentStreamEvent::ContentDelta(_)))
        });
        assert!(has_content);

        mock.assert_async().await;
    }

    #[test]
    fn test_service_has_client_access() {
        let client = Client::new("test-key", "https://api.example.com");
        let service = NexusApiService::new(client);
        assert_eq!(service.client().base_url(), "https://api.example.com");
    }

    #[test]
    fn test_from_env_missing_api_key() {
        unsafe {
            std::env::remove_var("OPENAI_API_KEY");
        }

        let result = NexusApiService::from_env();
        assert!(result.is_err());
        if let Err(crate::models::Error::Configuration(msg)) = result {
            assert!(msg.contains("OPENAI_API_KEY"));
        } else {
            panic!("Expected Configuration error");
        }
    }

    #[test]
    fn test_chat_config_creation() {
        let config = ChatConfig::new("gpt-4o-mini");
        assert_eq!(config.model, "gpt-4o-mini");
        assert!(config.temperature.is_none());
        assert!(config.max_tokens.is_none());
    }

    #[test]
    fn test_chat_config_builder() {
        let config = ChatConfig::new("gpt-4o-mini")
            .with_temperature(0.7)
            .with_max_tokens(100)
            .with_top_p(0.9)
            .with_frequency_penalty(0.5)
            .with_presence_penalty(0.3);

        assert_eq!(config.model, "gpt-4o-mini");
        assert_eq!(config.temperature, Some(0.7));
        assert_eq!(config.max_tokens, Some(100));
        assert_eq!(config.top_p, Some(0.9));
        assert_eq!(config.frequency_penalty, Some(0.5));
        assert_eq!(config.presence_penalty, Some(0.3));
    }

    #[test]
    fn test_chat_config_default() {
        let config = ChatConfig::default();
        // Should use DEFAULT_MODEL env var or fallback
        assert!(!config.model.is_empty());
    }

    #[tokio::test]
    async fn test_chat_with_config_success() {
        let mut server = Server::new_async().await;
        let mock = create_success_mock(&mut server).await;

        let client = Client::new("test-key", server.url());
        let service = NexusApiService::new(client);
        let model = default_test_model();
        let config = ChatConfig::new(model.clone())
            .with_temperature(0.7)
            .with_max_tokens(100);

        let response = service
            .chat_with_config(config, vec![Message::user("Hello!")])
            .await
            .unwrap();

        assert_eq!(response.role, MessageRole::Assistant);
        assert!(response.content.is_some());

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_chat_with_agent_config_success() {
        let mut server = Server::new_async().await;
        let mock = create_success_mock(&mut server).await;

        let client = Client::new("test-key", server.url());
        let service = NexusApiService::new(client);
        let agent = AgentFactory::calculator();
        let model = default_test_model();
        let config = ChatConfig::new(model)
            .with_temperature(0.5)
            .with_max_tokens(200);

        let response = service
            .chat_with_agent_config(&agent, config, vec![Message::user("What is 2 + 2?")])
            .await
            .unwrap();

        assert_eq!(response.role, MessageRole::Assistant);
        assert!(response.content.is_some());

        mock.assert_async().await;
    }
}

