use crate::client::ResponsesClient;
use crate::models::{Agent, ChatCompletionRequest, FunctionCall, Message, Result, ToolCall};
use futures::StreamExt;
use std::pin::Pin;
use tokio_stream::Stream;

/// Service for handling agent interactions with tool calling support
pub struct AgentService {
    client: ResponsesClient,
    tool_definitions: Vec<serde_json::Value>,
    tool_registry: crate::tools::ToolRegistry,
    response_format: Option<crate::models::chat::ResponseFormat>,
}

impl AgentService {
    pub fn new(client: &ResponsesClient, agent: &Agent) -> Self {
        Self {
            client: client.clone(),
            tool_definitions: agent.get_tool_definitions(),
            tool_registry: agent.tool_registry().clone(),
            response_format: agent.response_format().cloned(),
        }
    }

    /// Execute a chat request with automatic tool calling loop
    /// Returns the final assistant message after all tool calls are resolved
    pub async fn chat(&self, mut request: ChatCompletionRequest) -> Result<Message> {
        // Add tool definitions to the request only if there are tools
        if !self.tool_definitions.is_empty() {
            request = request.with_tools(self.tool_definitions.clone());
        }

        // Add response format if configured
        if let Some(ref format) = self.response_format {
            request = request.with_response_format(format.clone());
        }

        const MAX_TOOL_ROUNDS: usize = 5;
        let mut tool_round_count = 0;

        loop {
            // Make the LLM call
            let response = self.client.responses_completion(request.clone()).await?;

            if let Some(choice) = response.choices.first() {
                let message = &choice.message;

                // Check if the response has tool calls
                if let Some(tool_calls) = &message.tool_calls {
                    tool_round_count += 1;
                    if tool_round_count > MAX_TOOL_ROUNDS {
                        return Err(crate::models::Error::Other(format!(
                            "Maximum tool call rounds ({}) exceeded. This may indicate a loop.",
                            MAX_TOOL_ROUNDS
                        )));
                    }

                    // Add the assistant's tool call message to history
                    request.messages.push(message.clone());

                    // Execute each tool call
                    let mut has_error = false;
                    for tool_call in tool_calls {
                        let tool_name = &tool_call.function.name;

                        let result = match serde_json::from_str::<serde_json::Value>(
                            &tool_call.function.arguments,
                        ) {
                            Ok(args) => {
                                // Execute the tool
                                match self.tool_registry.execute(tool_name, args) {
                                    Ok(result) => result,
                                    Err(e) => {
                                        has_error = true;
                                        format!("Error executing tool: {}", e)
                                    }
                                }
                            }
                            Err(e) => {
                                has_error = true;
                                format!("Error parsing arguments: {}", e)
                            }
                        };

                        // Add tool result to history
                        request.messages.push(Message::tool(
                            tool_call.id.clone(),
                            tool_name.clone(),
                            result,
                        ));
                    }

                    // If any tool call had an error, stop the loop after adding the error results
                    if has_error {
                        // Make one final LLM call with the error messages
                        let response = self.client.responses_completion(request.clone()).await?;
                        if let Some(choice) = response.choices.first() {
                            return Ok(choice.message.clone());
                        } else {
                            return Err(crate::models::Error::Other(
                                "No response from assistant after tool error".to_string(),
                            ));
                        }
                    }

                    // Continue the loop to make another LLM call with tool results
                    continue;
                } else {
                    // No tool calls, return the final message
                    return Ok(message.clone());
                }
            } else {
                return Err(crate::models::Error::Other(
                    "No response from assistant".to_string(),
                ));
            }
        }
    }

    /// Execute a streaming chat request with automatic tool calling loop
    /// Returns a stream of message deltas and tool execution updates
    pub async fn chat_stream(
        &self,
        mut request: ChatCompletionRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<AgentStreamEvent>> + Send>>> {
        // Add tool definitions to the request only if there are tools
        if !self.tool_definitions.is_empty() {
            request = request.with_tools(self.tool_definitions.clone());
        }

        // Add response format if configured
        if let Some(ref format) = self.response_format {
            request = request.with_response_format(format.clone());
        }

        let client = self.client.clone();
        let agent_registry = self.tool_registry.clone();

        use tokio::sync::mpsc;
        let (tx, rx) = mpsc::unbounded_channel();

        tokio::spawn(async move {
            let mut current_request = request;
            const MAX_TOOL_ROUNDS: usize = 5;
            let mut tool_round_count = 0;

            loop {
                // Make the streaming LLM call
                match client.responses_completion_stream(current_request.clone()).await {
                    Ok(mut chunk_stream) => {
                        let mut accumulated_tool_calls = Vec::new();
                        let mut has_tool_calls = false;
                        let mut message_content = String::new();

                        while let Some(chunk_result) = chunk_stream.next().await {
                            match chunk_result {
                                Ok(chunk) => {
                                    if let Some(choice) = chunk.choices.first() {
                                        // Check for tool calls in delta
                                        if let Some(tool_call_deltas) = &choice.delta.tool_calls {
                                            has_tool_calls = true;
                                            // OpenAI streams tool calls with index field
                                            for delta in tool_call_deltas {
                                                let index = delta.index as usize;

                                                // Ensure we have enough slots
                                                while accumulated_tool_calls.len() <= index {
                                                    accumulated_tool_calls.push(ToolCall {
                                                        id: String::new(),
                                                        call_type: String::from("function"),
                                                        function: FunctionCall {
                                                            name: String::new(),
                                                            arguments: String::new(),
                                                        },
                                                    });
                                                }

                                                let tool_call = &mut accumulated_tool_calls[index];

                                                // Update fields from delta
                                                if let Some(ref id) = delta.id {
                                                    tool_call.id = id.clone();
                                                }
                                                if let Some(ref call_type) = delta.call_type {
                                                    tool_call.call_type = call_type.clone();
                                                }
                                                if let Some(ref func_delta) = delta.function {
                                                    if let Some(ref name) = func_delta.name {
                                                        tool_call.function.name = name.clone();
                                                    }
                                                    if let Some(ref args) = func_delta.arguments {
                                                        tool_call.function.arguments.push_str(args);
                                                    }
                                                }
                                            }
                                        }

                                        // Check for regular content
                                        if let Some(content) = &choice.delta.content {
                                            if !content.is_empty() {
                                                message_content.push_str(content);
                                                let _ = tx.send(Ok(
                                                    AgentStreamEvent::ContentDelta(content.clone()),
                                                ));
                                            }
                                        }
                                    }
                                }
                                Err(e) => {
                                    let _ = tx.send(Err(e));
                                    return;
                                }
                            }
                        }

                        // After stream ends, check if we have tool calls
                        if has_tool_calls && !accumulated_tool_calls.is_empty() {
                            tool_round_count += 1;
                            if tool_round_count > MAX_TOOL_ROUNDS {
                                let _ = tx.send(Err(crate::models::Error::Other(format!(
                                    "Maximum tool call rounds ({}) exceeded",
                                    MAX_TOOL_ROUNDS
                                ))));
                                return;
                            }

                            // Notify about tool calls
                            let _ = tx.send(Ok(AgentStreamEvent::ToolCallsStarted(
                                accumulated_tool_calls.clone(),
                            )));

                            // Add assistant message with tool calls to history
                            current_request
                                .messages
                                .push(Message::assistant_with_tool_calls(
                                    accumulated_tool_calls.clone(),
                                ));

                            // Execute tool calls
                            let mut has_error = false;
                            for tool_call in &accumulated_tool_calls {
                                let tool_name = &tool_call.function.name;

                                let _ =
                                    tx.send(Ok(AgentStreamEvent::ToolExecuting(tool_name.clone())));

                                let result = match serde_json::from_str::<serde_json::Value>(
                                    &tool_call.function.arguments,
                                ) {
                                    Ok(args) => match agent_registry.execute(tool_name, args) {
                                        Ok(result) => result,
                                        Err(e) => {
                                            has_error = true;
                                            format!("Error executing tool: {}", e)
                                        }
                                    },
                                    Err(e) => {
                                        has_error = true;
                                        format!("Failed to parse tool arguments: {}", e)
                                    }
                                };

                                let _ = tx.send(Ok(AgentStreamEvent::ToolResult {
                                    tool_name: tool_name.clone(),
                                    result: result.clone(),
                                }));

                                // Add tool result to history
                                current_request.messages.push(Message::tool(
                                    tool_call.id.clone(),
                                    tool_name.clone(),
                                    result,
                                ));
                            }

                            // If any tool had an error, make one final LLM call with the errors and then stop
                            if has_error {
                                match client.responses_completion_stream(current_request.clone()).await {
                                    Ok(mut final_stream) => {
                                        while let Some(chunk_result) = final_stream.next().await {
                                            match chunk_result {
                                                Ok(chunk) => {
                                                    if let Some(choice) = chunk.choices.first() {
                                                        if let Some(content) = &choice.delta.content
                                                        {
                                                            if !content.is_empty() {
                                                                let _ = tx.send(Ok(
                                                                    AgentStreamEvent::ContentDelta(
                                                                        content.clone(),
                                                                    ),
                                                                ));
                                                            }
                                                        }
                                                    }
                                                }
                                                Err(e) => {
                                                    let _ = tx.send(Err(e));
                                                    return;
                                                }
                                            }
                                        }
                                        let _ = tx.send(Ok(AgentStreamEvent::Done));
                                        return;
                                    }
                                    Err(e) => {
                                        let _ = tx.send(Err(e));
                                        return;
                                    }
                                }
                            }

                            // Continue loop to make another LLM call
                            continue;
                        } else {
                            // No tool calls, we're done
                            let _ = tx.send(Ok(AgentStreamEvent::Done));
                            break;
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(Err(e));
                        break;
                    }
                }
            }
        });

        let stream = tokio_stream::wrappers::UnboundedReceiverStream::new(rx);
        Ok(Box::pin(stream))
    }
}

/// Events emitted by the agent stream
#[derive(Debug, Clone)]
pub enum AgentStreamEvent {
    /// A delta of content from the assistant
    ContentDelta(String),
    /// Tool calls have been initiated
    ToolCallsStarted(Vec<crate::models::ToolCall>),
    /// A tool is being executed
    ToolExecuting(String),
    /// Result from a tool execution
    ToolResult { tool_name: String, result: String },
    /// Stream is complete
    Done,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::factories::AgentFactory;

    #[tokio::test]
    async fn test_agent_service_creation() {
        let client = ResponsesClient::new("test-key", "https://api.example.com");
        let agent = AgentFactory::calculator();
        let service = AgentService::new(&client, &agent);

        // Just verify it compiles and creates
        assert_eq!(service.tool_definitions.len(), 1);
    }
}
