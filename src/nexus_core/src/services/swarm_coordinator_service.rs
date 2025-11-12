use crate::client::Client;
use crate::factories::AgentFactory;
use crate::models::{
    agent::AgentStore,
    chat::{ChatCompletionRequest, Message, MessageRole},
    Result,
};
use crate::services::{AgentService, SwarmService};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};

/// Service that coordinates swarm execution and handles follow-up questions
#[derive(Clone)]
pub struct SwarmCoordinatorService {
    client: Client,
    _agent_store: Arc<AgentStore>,
    swarm_service: Arc<Mutex<SwarmService>>,
    coordinator_agent: crate::models::Agent,
}

impl SwarmCoordinatorService {
    /// Create a new swarm coordinator service
    pub fn new(client: Client, agent_store: AgentStore) -> Self {
        let swarm_service = Arc::new(Mutex::new(SwarmService::new(
            client.clone(),
            agent_store.clone(),
        )));
        let coordinator_agent = AgentFactory::swarm_coordinator(agent_store.clone());
        let agent_store_arc = Arc::new(agent_store);

        Self {
            client,
            _agent_store: agent_store_arc,
            swarm_service,
            coordinator_agent,
        }
    }

    /// Handle a chat request - detects if it's a new request or follow-up
    pub async fn chat(
        &self,
        request: ChatCompletionRequest,
        status_tx: Option<mpsc::UnboundedSender<String>>,
    ) -> Result<Message> {
        let has_assistant_message = request
            .messages
            .iter()
            .any(|m| matches!(m.role, MessageRole::Assistant));

        if !has_assistant_message {
            // Extract the last user request
            let user_request = request
                .messages
                .iter()
                .filter(|m| matches!(m.role, MessageRole::User))
                .last()
                .and_then(|m| m.content.as_ref())
                .cloned()
                .unwrap_or_default();

            // Execute swarm
            let mut swarm = self.swarm_service.lock().await;
            match swarm.execute(&user_request, &request.clone(), status_tx).await {
                Ok(result) => {
                    // Format the swarm results for display
                    let mut response = String::new();
                    response.push_str("🤖🤖🤖 **SWARM EXECUTION COMPLETE** 🤖🤖🤖\n\n");
                    response.push_str(&result.summary);
                    response.push_str("Please refer to the detailed task report above for agent assignments and results.\n");
                    response.push_str(
                        "\n(Feel free to ask follow-up questions about the execution details.)",
                    );
                    Ok(Message::assistant(response))
                }
                Err(e) => Ok(Message::assistant(format!(
                    "I encountered an error executing the swarm: {}. Please try again or ask a follow-up question.",
                    e
                ))),
            }
        } else {
            // Follow-up question - use normal agent service
            let service = AgentService::new(&self.client, &self.coordinator_agent);
            service.chat(request).await
        }
    }

    /// Handle a streaming chat request
    pub async fn chat_stream(
        &self,
        request: ChatCompletionRequest,
        status_tx: Option<mpsc::UnboundedSender<String>>,
    ) -> Result<
        std::pin::Pin<
            Box<
                dyn futures::Stream<Item = Result<crate::services::agent_service::AgentStreamEvent>>
                    + Send,
            >,
        >,
    > {
        use crate::services::agent_service::AgentStreamEvent;
        use futures::stream;

        let has_assistant_message = request
            .messages
            .iter()
            .any(|m| matches!(m.role, MessageRole::Assistant));

        if !has_assistant_message {
            // Run swarm execution in non-streaming mode and convert to stream
            let result = self.chat(request, status_tx).await;
            match result {
                Ok(message) => {
                    let content = message.content.unwrap_or_default();
                    let events = vec![
                        Ok(AgentStreamEvent::ContentDelta(content)),
                        Ok(AgentStreamEvent::Done),
                    ];
                    Ok(Box::pin(stream::iter(events)))
                }
                Err(e) => {
                    let events = vec![
                        Ok(AgentStreamEvent::ContentDelta(format!("Error: {}", e))),
                        Ok(AgentStreamEvent::Done),
                    ];
                    Ok(Box::pin(stream::iter(events)))
                }
            }
        } else {
            // Follow-up question - use normal agent service
            let service = AgentService::new(&self.client, &self.coordinator_agent);
            service.chat_stream(request).await
        }
    }
}
