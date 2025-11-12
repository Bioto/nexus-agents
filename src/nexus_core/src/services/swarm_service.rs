use crate::client::ResponsesClient;
use crate::factories::AgentFactory;
use crate::models::task_manager::TaskManager;
use crate::models::{
    agent::AgentStore,
    chat::{ChatCompletionRequest, Message},
    tasks::{TaskDecomposition, TaskError, TaskId},
};
use crate::services::AgentService;
use std::collections::HashMap;
use tokio::sync::mpsc;
use uuid::Uuid;

/// Service for orchestrating multi-agent swarm task execution
pub struct SwarmService {
    client: ResponsesClient,
    agent_store: AgentStore,
    task_manager: TaskManager,
    completion_request: ChatCompletionRequest,
}

#[derive(Debug, thiserror::Error)]
pub enum SwarmError {
    #[error("task error: {0}")]
    Task(#[from] TaskError),
    #[error("agent service error: {0}")]
    AgentService(#[from] crate::models::Error),
    #[error("decomposition failed: {0}")]
    Decomposition(String),
    #[error("assignment failed: {0}")]
    Assignment(String),
    #[error("execution failed for task {0}: {1}")]
    Execution(TaskId, String),
}

/// Result of executing a single task
#[derive(Debug, Clone)]
pub struct TaskResult {
    pub task_id: TaskId,
    pub task_name: String,
    pub agent_id: Uuid,
    pub result: String,
}

/// Final result of swarm execution
#[derive(Debug, Clone)]
pub struct SwarmResult {
    pub task_results: Vec<TaskResult>,
    pub summary: String,
}

fn default_model_for_swarm() -> String {
    "gpt-4o-mini".to_string()
}

impl SwarmService {
    /// Create a new swarm service
    pub fn new(client: ResponsesClient, agent_store: AgentStore) -> Self {
        Self {
            client,
            agent_store,
            task_manager: TaskManager::new(),
            completion_request: ChatCompletionRequest::new(default_model_for_swarm(), vec![]),
        }
    }

    /// Execute a user request by decomposing into tasks and executing them
    pub async fn execute(
        &mut self,
        user_request: impl Into<String>,
        completion_request: &ChatCompletionRequest,
        status_tx: Option<mpsc::UnboundedSender<String>>,
    ) -> Result<SwarmResult, SwarmError> {
        self.completion_request = completion_request.clone();

        let request = user_request.into();

        // Helper to send status updates
        let send_status = |msg: &str| {
            if let Some(ref tx) = status_tx {
                let _ = tx.send(msg.to_string());
            }
        };

        // Step 1: Decompose request into tasks
        send_status("🔍 Decomposing request into tasks...");
        let decomposition = self.decompose_request(&request).await?;

        // Step 2: Add tasks to manager (with name-to-id mapping for dependencies)
        send_status(&format!(
            "📋 Registering {} tasks...",
            decomposition.tasks.len()
        ));
        let _name_to_id = self.register_tasks(decomposition)?;

        // Step 4: Execute tasks in dependency-aware batches
        send_status("⚙️ Executing tasks...");
        let task_results = self.execute_tasks(status_tx.clone()).await?;

        // Step 5: Generate summary
        send_status("📊 Generating summary...");
        let summary = self.generate_summary(&task_results).await?;

        send_status("✅ Swarm execution complete!");

        Ok(SwarmResult {
            task_results,
            summary,
        })
    }

    /// Decompose user request into tasks using the task decomposition agent
    async fn decompose_request(&self, request: &str) -> Result<TaskDecomposition, SwarmError> {
        let decomposition_agent = AgentFactory::swarm_task_decomposition(self.agent_store.clone());
        let service = AgentService::new(&self.client, &decomposition_agent);

        let chat_request = ChatCompletionRequest::new(
            self.completion_request.model.clone(), // Using a model that supports structured outputs
            vec![Message::user(request)],
        );

        let response = service.chat(chat_request).await?;

        // Parse structured output
        let content = response
            .content
            .as_ref()
            .map(|c| c.extract_text())
            .ok_or_else(|| SwarmError::Decomposition("No content in response".to_string()))?;

        let decomposition: TaskDecomposition = serde_json::from_str(&content).map_err(|e| {
            SwarmError::Decomposition(format!("Failed to parse decomposition: {}", e))
        })?;

        Ok(decomposition)
    }

    /// Register tasks from decomposition into the task manager
    fn register_tasks(
        &mut self,
        decomposition: TaskDecomposition,
    ) -> Result<HashMap<String, TaskId>, SwarmError> {
        let mut name_to_id = HashMap::new();

        // First pass: create all tasks without dependencies
        for task_def in &decomposition.tasks {
            let instructions = if task_def.instructions.trim().is_empty() {
                format!(
                    "Perform the task named \"{}\" based on the user's request and provide the result.",
                    task_def.name
                )
            } else {
                task_def.instructions.clone()
            };

            let task_id = self.task_manager.add_task(
                &task_def.name,
                instructions,
                task_def.priority,
                Vec::<TaskId>::new(),
            )?;
            name_to_id.insert(task_def.name.clone(), task_id);
        }

        // Second pass: update dependencies using name-to-id mapping
        for task_def in &decomposition.tasks {
            let task_id = name_to_id[&task_def.name];

            // Convert requirement names to IDs
            let requirement_ids: Vec<TaskId> = task_def
                .requirements
                .iter()
                .filter_map(|name| name_to_id.get(name).copied())
                .collect();

            // Update requirements
            {
                let task = self
                    .task_manager
                    .get_mut(&task_id)
                    .ok_or_else(|| SwarmError::Task(TaskError::UnknownTask(task_id)))?;
                task.requirements = requirement_ids.iter().cloned().collect();
            }

            // Update dependents
            for req_id in &requirement_ids {
                if let Some(dep_task) = self.task_manager.get_mut(req_id) {
                    dep_task.dependents.insert(task_id);
                }
            }
        }

        Ok(name_to_id)
    }

    /// Assign tasks to agents using the router agent
    async fn assign_tasks(&mut self) -> Result<(), SwarmError> {
        let router_agent =
            AgentFactory::swarm_router(self.task_manager.clone(), self.agent_store.clone());
        let service = AgentService::new(&self.client, &router_agent);

        // Get ready tasks
        let ready_tasks: Vec<(Uuid, String)> = self
            .task_manager
            .tasks()
            .filter(|t| !t.is_completed() && t.assigned_to.is_none())
            .filter_map(|t| {
                if self.task_manager.is_ready(t.id).unwrap_or(false) {
                    Some((t.id, t.name.clone()))
                } else {
                    None
                }
            })
            .collect();

        if ready_tasks.is_empty() {
            return Ok(());
        }

        let readiness_listing: Vec<String> = ready_tasks
            .iter()
            .map(|(id, name)| format!("{}: {}", id, name))
            .collect();
        let prompt = format!(
            "Assign the following ready tasks to available agents:\n{}",
            readiness_listing.join("\n")
        );

        let chat_request = ChatCompletionRequest::new(
            self.completion_request.model.clone(), // Using a model that supports structured outputs
            vec![Message::user(prompt)],
        );

        let response = service.chat(chat_request).await?;
        let content = response
            .content
            .as_ref()
            .map(|c| c.extract_text())
            .ok_or_else(|| SwarmError::Assignment("No content in response".to_string()))?;

        // Parse assignments - expecting JSON with task_assignments array
        let assignments: serde_json::Value = serde_json::from_str(&content)
            .map_err(|e| SwarmError::Assignment(format!("Failed to parse assignments: {}", e)))?;

        let available_agents: Vec<Uuid> = self.agent_store.iter().map(|(id, _)| *id).collect();
        if available_agents.is_empty() {
            return Err(SwarmError::Assignment(
                "No agents available to assign tasks".to_string(),
            ));
        }
        let mut next_agent_index = 0usize;

        if let Some(task_assignments) = assignments
            .get("task_assignments")
            .and_then(|v| v.as_array())
        {
            for assignment in task_assignments {
                if let Some(task_id_str) = assignment.get("task_id").and_then(|v| v.as_str()) {
                    match Uuid::parse_str(task_id_str) {
                        Ok(task_id) => {
                            if let Some(task) = self.task_manager.get(&task_id) {
                                if task.assigned_to.is_some() || task.is_completed() {
                                    continue;
                                }
                            }
                            let mut chosen_agent_id =
                                available_agents[next_agent_index % available_agents.len()];
                            if let Some(agent_id_str) =
                                assignment.get("agent_id").and_then(|v| v.as_str())
                            {
                                match Uuid::parse_str(agent_id_str) {
                                    Ok(candidate)
                                        if self.agent_store.get_agent(&candidate).is_some() =>
                                    {
                                        chosen_agent_id = candidate;
                                    }
                                    _ => {}
                                }
                            }
                            self.task_manager.assign_task(
                                task_id,
                                chosen_agent_id,
                                &self.agent_store,
                            )?;
                            next_agent_index += 1;
                        }
                        Err(_) => {}
                    }
                }
            }
        }

        // Ensure all ready tasks get an assignment (fallback)
        for (task_id, _task_name) in &ready_tasks {
            let already_assigned = self
                .task_manager
                .get(task_id)
                .map(|t| t.assigned_to.is_some())
                .unwrap_or(false);
            if !already_assigned {
                let chosen_agent_id = available_agents[next_agent_index % available_agents.len()];
                self.task_manager
                    .assign_task(*task_id, chosen_agent_id, &self.agent_store)?;
                next_agent_index += 1;
            }
        }

        Ok(())
    }

    /// Execute tasks in dependency-aware batches
    async fn execute_tasks(
        &mut self,
        status_tx: Option<mpsc::UnboundedSender<String>>,
    ) -> Result<Vec<TaskResult>, SwarmError> {
        let mut all_results = Vec::new();
        let mut completed_results: HashMap<TaskId, TaskResult> = HashMap::new();

        // Helper to send status updates
        let send_status = |msg: &str| {
            if let Some(ref tx) = status_tx {
                let _ = tx.send(msg.to_string());
            }
        };

        // Get prioritized batches
        let batches = self.task_manager.prioritized_batches()?;
        send_status(&format!("📦 Processing {} batches...", batches.len()));

        let available_agents: Vec<Uuid> = self.agent_store.iter().map(|(id, _)| *id).collect();
        if available_agents.is_empty() {
            return Err(SwarmError::Assignment(
                "No agents available to execute tasks".to_string(),
            ));
        }
        let mut fallback_agent_index = 0usize;

        for (batch_idx, batch) in batches.iter().enumerate() {
            let mut batch_results = Vec::new();
            send_status(&format!(
                "🔄 Executing batch {}/{} ({} tasks)...",
                batch_idx + 1,
                batches.len(),
                batch.len()
            ));

            for (task_idx, task_id) in batch.iter().enumerate() {
                let task = self
                    .task_manager
                    .get(task_id)
                    .ok_or_else(|| SwarmError::Task(TaskError::UnknownTask(*task_id)))?
                    .clone();

                send_status(&format!(
                    "  ⚡ Task {}/{}: {}",
                    task_idx + 1,
                    batch.len(),
                    task.name
                ));

                let agent_id = if let Some(agent_id) = task.assigned_to {
                    agent_id
                } else {
                    let fallback_agent =
                        available_agents[fallback_agent_index % available_agents.len()];
                    fallback_agent_index += 1;
                    self.task_manager
                        .assign_task(*task_id, fallback_agent, &self.agent_store)?;
                    fallback_agent
                };

                let agent = self
                    .agent_store
                    .get_agent(&agent_id)
                    .ok_or_else(|| SwarmError::Task(TaskError::AgentNotFound(agent_id)))?;

                let service = AgentService::new(&self.client, agent);

                // Build execution prompt with dependency results
                let mut prompt = format!(
                    "Execute the following task for the user.\n\nTask Name: {}\nInstructions:\n{}\n",
                    task.name, task.instructions
                );

                // Add dependency results if any
                let dependency_results: Vec<String> = task
                    .requirements
                    .iter()
                    .filter_map(|dep_id| {
                        completed_results
                            .get(dep_id)
                            .map(|result| format!("- {}: {}", result.task_name, result.result))
                    })
                    .collect();

                if !dependency_results.is_empty() {
                    prompt.push_str("\n\nPrevious task results:");
                    for dep_result in dependency_results {
                        prompt.push('\n');
                        prompt.push_str(&dep_result);
                    }
                }

                let request = ChatCompletionRequest::new(
                    self.completion_request.model.clone(), // Using a cheaper model for task execution (agents may have tools)
                    vec![Message::user(prompt)],
                );

                match service.chat(request).await {
                    Ok(response) => {
                        // Extract tool calls from the response if any
                        let mut result_content = String::new();
                        if let Some(tool_calls) = &response.tool_calls {
                            for tool_call in tool_calls {
                                result_content
                                    .push_str(&format!("🔧 {} → ", tool_call.function.name));
                            }
                        }
                        let content_text = response
                            .content
                            .as_ref()
                            .map(|c| c.extract_text())
                            .unwrap_or_default();
                        result_content.push_str(&content_text);

                        let result = TaskResult {
                            task_id: *task_id,
                            task_name: task.name.clone(),
                            agent_id,
                            result: result_content,
                        };
                        completed_results.insert(*task_id, result.clone());
                        batch_results.push(result);
                    }
                    Err(e) => return Err(SwarmError::Execution(*task_id, e.to_string())),
                }
            }

            // Mark completed tasks
            for result in &batch_results {
                let _ = self.task_manager.mark_completed(result.task_id);
            }

            all_results.extend(batch_results);
            send_status(&format!(
                "✅ Batch {}/{} completed ({} tasks done)",
                batch_idx + 1,
                batches.len(),
                all_results.len()
            ));

            if batch_idx + 1 < batches.len() {
                send_status("👥 Assigning next batch...");
                self.assign_tasks().await?;
            }
        }

        Ok(all_results)
    }

    /// Generate a summary of execution results
    async fn generate_summary(&self, results: &[TaskResult]) -> Result<String, SwarmError> {
        let mut summary = format!(
            "**Summary:** Successfully executed {} tasks\n\n",
            results.len()
        );
        summary.push_str("**Task Results:**\n");

        for (idx, result) in results.iter().enumerate() {
            summary.push_str(&format!(
                "{}. **{}**\n   - Agent: Calculator Agent\n   - Result: {}\n",
                idx + 1,
                result.task_name,
                result.result.trim()
            ));
        }

        Ok(summary)
    }
}
