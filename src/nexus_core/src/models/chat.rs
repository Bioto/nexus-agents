use serde::{Deserialize, Serialize};

/// Role of a message in a chat completion
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole {
    System,
    User,
    Assistant,
    Tool,
}

/// Function call details in a tool call
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String,
}

/// Tool call information
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: FunctionCall,
}

/// Tool call delta for streaming (includes index field)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolCallDelta {
    pub index: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub call_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function: Option<FunctionCallDelta>,
}

/// Function call delta for streaming
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FunctionCallDelta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<String>,
}

/// Content part for multimodal messages
/// Uses standard OpenAI API type names: text, image_url, file
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type")]
pub enum ContentPart {
    /// Text content
    #[serde(rename = "text")]
    Text { text: String },
    /// Image URL (base64 encoded or URL)
    #[serde(rename = "image_url")]
    ImageUrl { image_url: ImageUrl },
    /// File reference (for uploaded files using file_id)
    /// Responses API uses "input_file" and expects file_id directly
    #[serde(rename = "input_file")]
    File { file_id: String },
}

/// Image URL for content parts
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImageUrl {
    pub url: String,
}

/// File reference for content parts
/// Note: Only file_id is sent to the API. MIME type is set during upload, not during reference.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileReference {
    pub file_id: String,
}

/// Message content - can be either a string or an array of content parts
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum MessageContent {
    /// Simple string content
    String(String),
    /// Array of content parts (for multimodal)
    Array(Vec<ContentPart>),
}

impl Default for MessageContent {
    fn default() -> Self {
        MessageContent::String(String::new())
    }
}

/// A message in a chat completion
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Message {
    pub role: MessageRole,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<MessageContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

impl Message {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::System,
            content: Some(MessageContent::String(content.into())),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::User,
            content: Some(MessageContent::String(content.into())),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        }
    }

    pub fn user_with_content(content: MessageContent) -> Self {
        Self {
            role: MessageRole::User,
            content: Some(content),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::Assistant,
            content: Some(MessageContent::String(content.into())),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        }
    }

    pub fn tool(
        tool_call_id: impl Into<String>,
        name: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        Self {
            role: MessageRole::Tool,
            content: Some(MessageContent::String(content.into())),
            tool_calls: None,
            tool_call_id: Some(tool_call_id.into()),
            name: Some(name.into()),
        }
    }

    pub fn assistant_with_tool_calls(tool_calls: Vec<ToolCall>) -> Self {
        Self {
            role: MessageRole::Assistant,
            content: None,
            tool_calls: Some(tool_calls),
            tool_call_id: None,
            name: None,
        }
    }
}

impl MessageContent {
    /// Create a content array with text and image
    pub fn with_image(text: impl Into<String>, image_base64: impl Into<String>) -> Self {
        let mut parts = Vec::new();
        parts.push(ContentPart::Text { text: text.into() });
        parts.push(ContentPart::ImageUrl {
            image_url: ImageUrl {
                url: format!("data:image/png;base64,{}", image_base64.into()),
            },
        });
        MessageContent::Array(parts)
    }

    /// Create a content array with text and file reference
    /// Uses the file ID from upload. MIME type was set during upload and should not be included here.
    pub fn with_file(text: impl Into<String>, file_id: impl Into<String>) -> Self {
        let mut parts = Vec::new();
        let text_str = text.into();
        if !text_str.is_empty() {
            parts.push(ContentPart::Text { text: text_str });
        }
        // For uploaded files, use type: "input_file" with file_id directly
        // (Responses API uses "input_file" and expects file_id directly, not nested)
        parts.push(ContentPart::File {
            file_id: file_id.into(),
        });
        MessageContent::Array(parts)
    }

    /// Get text content if it's a simple string
    pub fn as_string(&self) -> Option<&str> {
        match self {
            MessageContent::String(s) => Some(s),
            _ => None,
        }
    }

    /// Extract text content from either string or array (concatenates all text parts)
    pub fn extract_text(&self) -> String {
        match self {
            MessageContent::String(s) => s.clone(),
            MessageContent::Array(parts) => {
                let mut text = String::new();
                for part in parts {
                    if let ContentPart::Text { text: t } = part {
                        if !text.is_empty() {
                            text.push('\n');
                        }
                        text.push_str(t);
                    }
                }
                text
            }
        }
    }
}

/// Wrapper for JSON schema that includes name and strict mode
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JsonSchemaWrapper {
    /// Name of the schema
    pub name: String,
    /// The actual JSON schema
    pub schema: JsonSchema,
    /// Optional strict mode
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strict: Option<bool>,
}

/// JSON Schema for structured outputs
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JsonSchema {
    #[serde(rename = "type")]
    pub schema_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub additional_properties: Option<bool>,
    #[serde(flatten)]
    pub additional_fields: serde_json::Map<String, serde_json::Value>,
}

impl JsonSchema {
    /// Create a new JSON schema with the given type
    pub fn new(schema_type: impl Into<String>) -> Self {
        Self {
            schema_type: schema_type.into(),
            properties: None,
            required: None,
            additional_properties: None,
            additional_fields: serde_json::Map::new(),
        }
    }

    /// Set the properties of the schema
    pub fn with_properties(mut self, properties: serde_json::Value) -> Self {
        self.properties = Some(properties);
        self
    }

    /// Set the required fields
    pub fn with_required(mut self, required: Vec<String>) -> Self {
        self.required = Some(required);
        self
    }

    /// Set whether additional properties are allowed
    pub fn with_additional_properties(mut self, allowed: bool) -> Self {
        self.additional_properties = Some(allowed);
        self
    }
}

/// Response format for structured outputs
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResponseFormat {
    /// Request JSON object output (basic structured output)
    #[serde(rename = "json_object")]
    JsonObject,
    /// Request JSON output constrained by a JSON schema
    #[serde(rename = "json_schema")]
    JsonSchema {
        /// The JSON schema wrapper that contains name and schema
        json_schema: JsonSchemaWrapper,
    },
}

impl ResponseFormat {
    /// Create a basic JSON object response format
    pub fn json_object() -> Self {
        Self::JsonObject
    }

    /// Create a JSON schema response format
    pub fn json_schema(schema: JsonSchema) -> Self {
        Self::JsonSchema {
            json_schema: JsonSchemaWrapper {
                name: "response".to_string(),
                schema,
                strict: None,
            },
        }
    }

    /// Create a JSON schema response format with a name
    pub fn json_schema_with_name(schema: JsonSchema, name: impl Into<String>) -> Self {
        Self::JsonSchema {
            json_schema: JsonSchemaWrapper {
                name: name.into(),
                schema,
                strict: None,
            },
        }
    }

    /// Create a JSON schema response format with strict mode
    pub fn json_schema_strict(schema: JsonSchema, name: impl Into<String>, strict: bool) -> Self {
        Self::JsonSchema {
            json_schema: JsonSchemaWrapper {
                name: name.into(),
                schema,
                strict: Some(strict),
            },
        }
    }
}

/// Request for a chat completion
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frequency_penalty: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presence_penalty: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_format: Option<ResponseFormat>,
}

impl ChatCompletionRequest {
    pub fn new(model: impl Into<String>, messages: Vec<Message>) -> Self {
        Self {
            model: model.into(),
            messages,
            temperature: None,
            max_tokens: None,
            top_p: None,
            frequency_penalty: None,
            presence_penalty: None,
            stream: None,
            tools: None,
            response_format: None,
        }
    }

    pub fn with_temperature(mut self, temperature: f32) -> Self {
        self.temperature = Some(temperature);
        self
    }

    pub fn with_max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = Some(max_tokens);
        self
    }

    pub fn with_top_p(mut self, top_p: f32) -> Self {
        self.top_p = Some(top_p);
        self
    }

    pub fn with_frequency_penalty(mut self, penalty: f32) -> Self {
        self.frequency_penalty = Some(penalty);
        self
    }

    pub fn with_presence_penalty(mut self, penalty: f32) -> Self {
        self.presence_penalty = Some(penalty);
        self
    }

    pub fn with_stream(mut self, stream: bool) -> Self {
        self.stream = Some(stream);
        self
    }

    pub fn with_tools(mut self, tools: Vec<serde_json::Value>) -> Self {
        self.tools = Some(tools);
        self
    }

    /// Set the response format for structured outputs
    pub fn with_response_format(mut self, response_format: ResponseFormat) -> Self {
        self.response_format = Some(response_format);
        self
    }

    /// Request JSON object output (basic structured output)
    pub fn with_json_object(mut self) -> Self {
        self.response_format = Some(ResponseFormat::json_object());
        self
    }

    /// Request JSON output constrained by a JSON schema
    pub fn with_json_schema(mut self, schema: JsonSchema) -> Self {
        self.response_format = Some(ResponseFormat::json_schema(schema));
        self
    }
}

/// A choice in a chat completion response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Choice {
    pub index: u32,
    pub message: Message,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
}

/// Token usage information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Usage {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completion_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_tokens: Option<u32>,
}

/// Response from a chat completion request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatCompletionResponse {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub model: String,
    pub choices: Vec<Choice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
}

/// Streaming choice delta (partial content)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChoiceDelta {
    pub index: u32,
    pub delta: MessageDelta,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
}

/// Partial message content in streaming responses
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageDelta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<MessageRole>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCallDelta>>,
}

/// Streaming response chunk
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatCompletionChunk {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub model: String,
    pub choices: Vec<ChoiceDelta>,
}

/// A chat history that holds a sequence of messages for conversation management
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChatHistory {
    pub messages: Vec<Message>,
}

impl ChatHistory {
    pub fn new() -> Self {
        Self {
            messages: Vec::new(),
        }
    }

    pub fn add_message(&mut self, message: Message) {
        self.messages.push(message);
    }

    pub fn add_system(&mut self, content: impl Into<String>) {
        self.add_message(Message::system(content));
    }

    pub fn add_user(&mut self, content: impl Into<String>) {
        self.add_message(Message::user(content));
    }

    pub fn add_assistant(&mut self, content: impl Into<String>) {
        self.add_message(Message::assistant(content));
    }

    pub fn to_chat_request(&self, model: impl Into<String>) -> ChatCompletionRequest {
        ChatCompletionRequest::new(model, self.messages.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Get the default model from DEFAULT_MODEL environment variable or fallback
    fn default_test_model() -> String {
        std::env::var("DEFAULT_MODEL").unwrap_or_else(|_| "gpt-5-nano-2025-08-07".to_string())
    }

    #[test]
    fn test_message_creation() {
        let msg = Message::user("Hello");
        assert_eq!(msg.role, MessageRole::User);
        assert_eq!(
            msg.content,
            Some(MessageContent::String("Hello".to_string()))
        );
    }

    #[test]
    fn test_chat_completion_request_serialization() {
        let model = default_test_model();
        let request = ChatCompletionRequest::new(&model, vec![Message::user("Hello, world!")])
            .with_temperature(0.7)
            .with_max_tokens(100);

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains(&model));
        assert!(json.contains("Hello, world!"));
        assert!(json.contains("temperature"));
    }

    #[test]
    fn test_chat_completion_response_deserialization() {
        let model = default_test_model();
        let json = format!(
            r#"{{
            "id": "chatcmpl-123",
            "object": "chat.completion",
            "created": 1677652288,
            "model": "{}",
            "choices": [{{
                "index": 0,
                "message": {{
                    "role": "assistant",
                    "content": "Hello! How can I help you?"
                }},
                "finish_reason": "stop"
            }}],
            "usage": {{
                "prompt_tokens": 10,
                "completion_tokens": 8,
                "total_tokens": 18
            }}
        }}"#,
            model
        );

        let response: ChatCompletionResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(response.id, "chatcmpl-123");
        assert_eq!(response.choices.len(), 1);
        assert_eq!(
            response.choices[0].message.content,
            Some(MessageContent::String(
                "Hello! How can I help you?".to_string()
            ))
        );
    }

    #[test]
    fn test_chat_history() {
        let mut history = ChatHistory::new();
        history.add_system("You are a helpful assistant.");
        history.add_user("Hello!");
        history.add_assistant("Hi there!");

        assert_eq!(history.messages.len(), 3);
        assert_eq!(history.messages[0].role, MessageRole::System);
        assert_eq!(
            history.messages[0].content,
            Some(MessageContent::String(
                "You are a helpful assistant.".to_string()
            ))
        );
        assert_eq!(history.messages[1].role, MessageRole::User);
        assert_eq!(
            history.messages[1].content,
            Some(MessageContent::String("Hello!".to_string()))
        );
        assert_eq!(history.messages[2].role, MessageRole::Assistant);
        assert_eq!(
            history.messages[2].content,
            Some(MessageContent::String("Hi there!".to_string()))
        );

        let model = default_test_model();
        let request = history.to_chat_request(&model);
        assert_eq!(request.model, model);
        assert_eq!(request.messages.len(), 3);
        assert_eq!(request.messages, history.messages);
    }

    #[test]
    fn test_json_schema_creation() {
        let schema = JsonSchema::new("object")
            .with_properties(serde_json::json!({
                "name": {"type": "string"},
                "age": {"type": "number"}
            }))
            .with_required(vec!["name".to_string()])
            .with_additional_properties(false);

        assert_eq!(schema.schema_type, "object");
        assert!(schema.properties.is_some());
        assert!(schema.required.is_some());
        assert_eq!(schema.additional_properties, Some(false));
    }

    #[test]
    fn test_response_format_json_object() {
        let format = ResponseFormat::json_object();
        let json = serde_json::to_string(&format).unwrap();
        assert!(json.contains("json_object"));
    }

    #[test]
    fn test_response_format_json_schema() {
        let schema = JsonSchema::new("object").with_properties(serde_json::json!({
            "result": {"type": "string"}
        }));
        let format = ResponseFormat::json_schema(schema);
        let json = serde_json::to_string(&format).unwrap();
        assert!(json.contains("json_schema"));
        assert!(json.contains("result"));
    }

    #[test]
    fn test_chat_completion_request_with_json_object() {
        let request =
            ChatCompletionRequest::new("gpt-4", vec![Message::user("Hello")]).with_json_object();

        assert!(request.response_format.is_some());
        if let Some(ResponseFormat::JsonObject) = request.response_format {
            // Correct variant
        } else {
            panic!("Expected JsonObject variant");
        }

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("response_format"));
        assert!(json.contains("json_object"));
    }

    #[test]
    fn test_chat_completion_request_with_json_schema() {
        let schema = JsonSchema::new("object")
            .with_properties(serde_json::json!({
                "summary": {"type": "string"},
                "sentiment": {"type": "string", "enum": ["positive", "negative", "neutral"]}
            }))
            .with_required(vec!["summary".to_string()]);

        let request = ChatCompletionRequest::new("gpt-4", vec![Message::user("Analyze this")])
            .with_json_schema(schema);

        assert!(request.response_format.is_some());
        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("response_format"));
        assert!(json.contains("json_schema"));
        assert!(json.contains("summary"));
        assert!(json.contains("sentiment"));
    }

    #[test]
    fn test_response_format_serialization() {
        // Test JSON object format
        let format = ResponseFormat::json_object();
        let json = serde_json::to_string(&format).unwrap();
        let deserialized: ResponseFormat = serde_json::from_str(&json).unwrap();
        assert_eq!(format, deserialized);

        // Test JSON schema format
        let schema = JsonSchema::new("object");
        let format = ResponseFormat::json_schema(schema);
        let json = serde_json::to_string(&format).unwrap();
        let deserialized: ResponseFormat = serde_json::from_str(&json).unwrap();
        match (&format, &deserialized) {
            (
                ResponseFormat::JsonSchema { json_schema: w1 },
                ResponseFormat::JsonSchema { json_schema: w2 },
            ) => {
                assert_eq!(w1.schema.schema_type, w2.schema.schema_type);
                assert_eq!(w1.name, w2.name);
            }
            _ => panic!("Expected JsonSchema variants"),
        }
    }

    #[test]
    fn test_message_content_with_file() {
        let content = MessageContent::with_file("What's in this file?", "file-123");
        
        // Verify structure
        match &content {
            MessageContent::Array(parts) => {
                assert_eq!(parts.len(), 2);
                
                // Check text part
                if let ContentPart::Text { text } = &parts[0] {
                    assert_eq!(text, "What's in this file?");
                } else {
                    panic!("Expected first part to be Text");
                }
                
                // Check file part
                if let ContentPart::File { file_id } = &parts[1] {
                    assert_eq!(file_id, "file-123");
                } else {
                    panic!("Expected second part to be File");
                }
            }
            _ => panic!("Expected Array content"),
        }
        
        // Verify JSON serialization - should NOT contain mime_type
        let json = serde_json::to_string(&content).unwrap();
        assert!(json.contains("\"type\":\"text\""));
        assert!(json.contains("What's in this file?"));
        assert!(json.contains("\"type\":\"input_file\""));
        assert!(json.contains("\"file_id\":\"file-123\""));
        assert!(!json.contains("mime_type"), "MIME type should not be in file reference");
    }

    #[test]
    fn test_message_content_with_file_empty_text() {
        let content = MessageContent::with_file("", "file-456");
        
        // When text is empty, it should only have the file part
        match &content {
            MessageContent::Array(parts) => {
                assert_eq!(parts.len(), 1);
                
                // Check file part
                if let ContentPart::File { file_id } = &parts[0] {
                    assert_eq!(file_id, "file-456");
                } else {
                    panic!("Expected first part to be File");
                }
            }
            _ => panic!("Expected Array content"),
        }
    }
}
