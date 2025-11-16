use crate::client::{LLMClient, ResponsesClient};
use crate::models::{Agent, ChatCompletionRequest, FunctionCall, Message, Result, ToolCall};
use futures::StreamExt;
use log::{debug, error, info};
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
            println!("[AgentService] Making LLM call...");
            let response = self.client.chat(request.clone()).await?;
            println!(
                "[AgentService] LLM call completed. Response has {} choices",
                response.choices.len()
            );
            debug!("[AgentService] Full response: {:?}", response);

            if let Some(choice) = response.choices.first() {
                let message = &choice.message;
                println!(
                    "[AgentService] Got message from choice. Content: {:?}",
                    message.content
                );
                debug!("[AgentService] Message: {:?}", message.content);

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
                        println!("[AgentService] Executing tool: {}", tool_name);
                        info!("[AgentService] Executing tool: {}", tool_name);

                        let result = match serde_json::from_str::<serde_json::Value>(
                            &tool_call.function.arguments,
                        ) {
                            Ok(args) => {
                                println!(
                                    "[AgentService] Tool {} arguments parsed successfully",
                                    tool_name
                                );
                                debug!(
                                    "[AgentService] Tool {} arguments parsed successfully",
                                    tool_name
                                );

                                // Execute the tool in a blocking task to avoid blocking the async runtime
                                let registry = self.tool_registry.clone();
                                let tool_name_clone = tool_name.clone();
                                let args_clone = args.clone();

                                println!(
                                    "[AgentService] Spawning blocking task for tool: {}",
                                    tool_name
                                );
                                info!(
                                    "[AgentService] Spawning blocking task for tool: {}",
                                    tool_name
                                );

                                // Add a 60 second timeout to prevent hanging
                                println!(
                                    "[AgentService] Starting timeout (60s) for tool: {}",
                                    tool_name
                                );
                                match tokio::time::timeout(
                                    tokio::time::Duration::from_secs(60),
                                    tokio::task::spawn_blocking(move || {
                                        println!(
                                            "[AgentService] Blocking task started for tool: {}",
                                            tool_name_clone
                                        );
                                        debug!(
                                            "[AgentService] Blocking task started for tool: {}",
                                            tool_name_clone
                                        );
                                        let result = registry.execute(&tool_name_clone, args_clone);
                                        println!(
                                            "[AgentService] Blocking task completed for tool: {}",
                                            tool_name_clone
                                        );
                                        debug!(
                                            "[AgentService] Blocking task completed for tool: {}",
                                            tool_name_clone
                                        );
                                        result
                                    }),
                                )
                                .await
                                {
                                    Ok(Ok(Ok(result))) => {
                                        println!("[AgentService] Tool {} executed successfully (result length: {} chars)", tool_name, result.len());
                                        info!(
                                            "[AgentService] Tool {} executed successfully",
                                            tool_name
                                        );
                                        debug!(
                                            "[AgentService] Tool {} result length: {} characters",
                                            tool_name,
                                            result.len()
                                        );
                                        result
                                    }
                                    Ok(Ok(Err(e))) => {
                                        println!(
                                            "[AgentService] ERROR: Tool {} execution error: {}",
                                            tool_name, e
                                        );
                                        error!(
                                            "[AgentService] Tool {} execution error: {}",
                                            tool_name, e
                                        );
                                        has_error = true;
                                        format!("Error executing tool: {}", e)
                                    }
                                    Ok(Err(e)) => {
                                        println!(
                                            "[AgentService] ERROR: Tool {} task error: {}",
                                            tool_name, e
                                        );
                                        error!(
                                            "[AgentService] Tool {} task error: {}",
                                            tool_name, e
                                        );
                                        has_error = true;
                                        format!("Error in tool execution task: {}", e)
                                    }
                                    Err(_) => {
                                        println!("[AgentService] ERROR: Tool {} execution timed out after 60 seconds", tool_name);
                                        error!("[AgentService] Tool {} execution timed out after 60 seconds", tool_name);
                                        has_error = true;
                                        format!("Tool execution timed out after 60 seconds")
                                    }
                                }
                            }
                            Err(e) => {
                                println!("[AgentService] ERROR: Failed to parse arguments for tool {}: {}", tool_name, e);
                                error!(
                                    "[AgentService] Failed to parse arguments for tool {}: {}",
                                    tool_name, e
                                );
                                has_error = true;
                                format!("Error parsing arguments: {}", e)
                            }
                        };

                        // Add tool result to history
                        println!("[AgentService] Adding tool result to history: tool_call_id={}, name={}, result_length={}", 
                            tool_call.id, tool_name, result.len());
                        let tool_message =
                            Message::tool(tool_call.id.clone(), tool_name.clone(), result.clone());
                        println!("[AgentService] Tool message: role={:?}, content={:?}, tool_call_id={:?}, name={:?}", 
                            tool_message.role, tool_message.content, tool_message.tool_call_id, tool_message.name);
                        request.messages.push(tool_message);
                    }

                    // If any tool call had an error, stop the loop after adding the error results
                    if has_error {
                        // Make one final LLM call with the error messages
                        let response = self.client.chat(request.clone()).await?;
                        if let Some(choice) = response.choices.first() {
                            return Ok(choice.message.clone());
                        } else {
                            return Err(crate::models::Error::Other(
                                "No response from assistant after tool error".to_string(),
                            ));
                        }
                    }

                    // Debug: Print message history to check for duplicates
                    println!(
                        "[AgentService] Message history length: {}",
                        request.messages.len()
                    );
                    for (i, msg) in request.messages.iter().enumerate() {
                        println!("[AgentService] Message {}: role={:?}, has_tool_calls={:?}, tool_call_id={:?}", 
                            i, msg.role, msg.tool_calls.is_some(), msg.tool_call_id);
                    }

                    // After successful tool execution, request a final response
                    println!(
                        "[AgentService] Requesting final response using executed tool results"
                    );
                    let mut final_request = request.clone();

                    // Build a summary of the current tool outputs so the LLM can reference them
                    let mut summary = String::from("Tool results::\n");
                    for tool_call in tool_calls {
                        if let Some(tool_msg) = request
                            .messages
                            .iter()
                            .rev()
                            .find(|msg| msg.tool_call_id.as_deref() == Some(&tool_call.id))
                        {
                            let content = tool_msg
                                .content
                                .as_ref()
                                .map(|c| c.extract_text())
                                .unwrap_or_else(|| "(no output)".to_string());
                            summary.push_str(&format!(
                                "- {}:\n{}\n\n",
                                tool_call.function.name,
                                content.trim()
                            ));
                        }
                    }

                    summary.push_str("\nPlease summarize these execution results for the user, quote the key stdout or errors, and mention the script you ran.");
                    final_request.messages.push(Message::user(summary));
                    final_request.tools = None;
                    let response = self.client.chat(final_request).await?;
                    if let Some(choice) = response.choices.first() {
                        if choice.message.tool_calls.is_none() {
                            println!("[AgentService] Got final text response after tool execution");
                            return Ok(choice.message.clone());
                        }
                    }

                    println!("[AgentService] Still getting tool calls in final request, returning last tool result as response");
                    if let Some(last_tool_msg) = request
                        .messages
                        .iter()
                        .rev()
                        .find(|m| m.tool_call_id.is_some())
                    {
                        if let Some(content) = &last_tool_msg.content {
                            use crate::models::{Message, MessageContent, MessageRole};
                            return Ok(Message {
                                role: MessageRole::Assistant,
                                content: Some(MessageContent::String(content.extract_text())),
                                tool_calls: None,
                                tool_call_id: None,
                                name: None,
                            });
                        }
                    }
                    return Err(crate::models::Error::Other(
                        "Tool calls already executed but API still requesting them. This may indicate an API format issue.".to_string()
                    ));
                } else {
                    // No tool calls, return the final message
                    println!("[AgentService] No tool calls, returning final message");
                    return Ok(message.clone());
                }
            } else {
                println!(
                    "[AgentService] ERROR: Response has no choices! Response: {:?}",
                    response
                );
                error!("[AgentService] Response has no choices: {:?}", response);
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
                match client.chat_stream(current_request.clone()).await {
                    Ok(mut chunk_stream) => {
                        let mut accumulated_tool_calls = Vec::new();
                        let mut has_tool_calls = false;
                        let mut message_content = String::new();

                        let mut content_delta_sent = false;
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
                                                content_delta_sent = true;
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

                        // If we accumulated content but never sent any deltas (shouldn't happen, but safety check)
                        if !content_delta_sent && !message_content.is_empty() && !has_tool_calls {
                            let _ = tx
                                .send(Ok(AgentStreamEvent::ContentDelta(message_content.clone())));
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
                                println!("[AgentService] [Stream] Executing tool: {}", tool_name);
                                info!("[AgentService] [Stream] Executing tool: {}", tool_name);

                                let _ =
                                    tx.send(Ok(AgentStreamEvent::ToolExecuting(tool_name.clone())));

                                let result = match serde_json::from_str::<serde_json::Value>(
                                    &tool_call.function.arguments,
                                ) {
                                    Ok(args) => {
                                        println!("[AgentService] [Stream] Tool {} arguments parsed successfully", tool_name);
                                        debug!("[AgentService] [Stream] Tool {} arguments parsed successfully", tool_name);

                                        // Execute the tool in a blocking task to avoid blocking the async runtime
                                        let registry = agent_registry.clone();
                                        let tool_name_clone = tool_name.clone();
                                        let args_clone = args.clone();

                                        println!("[AgentService] [Stream] Spawning blocking task for tool: {}", tool_name);
                                        info!("[AgentService] [Stream] Spawning blocking task for tool: {}", tool_name);

                                        // Add a 60 second timeout to prevent hanging
                                        println!("[AgentService] [Stream] Starting timeout (60s) for tool: {}", tool_name);
                                        match tokio::time::timeout(
                                            tokio::time::Duration::from_secs(60),
                                            tokio::task::spawn_blocking(move || {
                                                println!("[AgentService] [Stream] Blocking task started for tool: {}", tool_name_clone);
                                                debug!("[AgentService] [Stream] Blocking task started for tool: {}", tool_name_clone);
                                                let result = registry.execute(&tool_name_clone, args_clone);
                                                println!("[AgentService] [Stream] Blocking task completed for tool: {}", tool_name_clone);
                                                debug!("[AgentService] [Stream] Blocking task completed for tool: {}", tool_name_clone);
                                                result
                                            })
                                        ).await {
                                            Ok(Ok(Ok(result))) => {
                                                println!("[AgentService] [Stream] Tool {} executed successfully (result length: {} chars)", tool_name, result.len());
                                                info!("[AgentService] [Stream] Tool {} executed successfully", tool_name);
                                                debug!("[AgentService] [Stream] Tool {} result length: {} characters", tool_name, result.len());
                                                result
                                            }
                                            Ok(Ok(Err(e))) => {
                                                println!("[AgentService] [Stream] ERROR: Tool {} execution error: {}", tool_name, e);
                                                error!("[AgentService] [Stream] Tool {} execution error: {}", tool_name, e);
                                                has_error = true;
                                                format!("Error executing tool: {}", e)
                                            }
                                            Ok(Err(e)) => {
                                                println!("[AgentService] [Stream] ERROR: Tool {} task error: {}", tool_name, e);
                                                error!("[AgentService] [Stream] Tool {} task error: {}", tool_name, e);
                                                has_error = true;
                                                format!("Error in tool execution task: {}", e)
                                            }
                                            Err(_) => {
                                                println!("[AgentService] [Stream] ERROR: Tool {} execution timed out after 60 seconds", tool_name);
                                                error!("[AgentService] [Stream] Tool {} execution timed out after 60 seconds", tool_name);
                                                has_error = true;
                                                format!("Tool execution timed out after 60 seconds")
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        println!("[AgentService] [Stream] ERROR: Failed to parse arguments for tool {}: {}", tool_name, e);
                                        error!("[AgentService] [Stream] Failed to parse arguments for tool {}: {}", tool_name, e);
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
                                match client.chat_stream(current_request.clone()).await {
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
