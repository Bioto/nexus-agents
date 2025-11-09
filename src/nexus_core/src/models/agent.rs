use crate::models::chat::{ChatCompletionRequest, ResponseFormat};
use crate::models::tool::Tool;
use crate::tools::ToolRegistry;
use quick_xml::events::{BytesEnd, BytesStart, BytesText, Event};
use quick_xml::Writer;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Cursor;
use uuid::Uuid;

/// Represents an agent with its configuration and executable tools
#[derive(Clone)]
pub struct Agent {
    pub name: String,
    pub description: String,
    pub system_prompt: String,
    pub tools: Vec<Tool>,
    pub(crate) tool_registry: ToolRegistry,
    pub(crate) response_format: Option<ResponseFormat>,
}

/// Serializable agent configuration without executable tools
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    pub name: String,
    pub description: String,
    pub system_prompt: String,
    pub tools: Vec<Tool>,
}

impl Agent {
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        system_prompt: impl Into<String>,
        tools: Vec<Tool>,
        tool_registry: ToolRegistry,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            system_prompt: system_prompt.into(),
            tools,
            tool_registry,
            response_format: None,
        }
    }

    pub fn with_response_format(
        name: impl Into<String>,
        description: impl Into<String>,
        system_prompt: impl Into<String>,
        tools: Vec<Tool>,
        tool_registry: ToolRegistry,
        response_format: Option<ResponseFormat>,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            system_prompt: system_prompt.into(),
            tools,
            tool_registry,
            response_format,
        }
    }

    /// Get tool definitions in OpenAI format
    pub fn get_tool_definitions(&self) -> Vec<serde_json::Value> {
        self.tools.iter().map(|t| t.to_openai_format()).collect()
    }

    /// Find a tool by name
    pub fn find_tool(&self, name: &str) -> Option<&Tool> {
        self.tools.iter().find(|t| t.name == name)
    }

    /// Get a reference to the tool registry
    pub fn tool_registry(&self) -> &ToolRegistry {
        &self.tool_registry
    }

    /// Get the response format configured for this agent
    pub fn response_format(&self) -> Option<&ResponseFormat> {
        self.response_format.as_ref()
    }

    /// Check if a model supports json_schema response format
    ///
    /// According to OpenAI's documentation, json_schema is only supported by:
    /// - gpt-4o
    /// - gpt-4-turbo
    /// - o1-preview
    /// - o1-mini
    /// Note: gpt-4o-mini and the default model (from DEFAULT_MODEL env var) only support json_object, not json_schema
    fn model_supports_json_schema(model: &str) -> bool {
        let model_lower = model.to_lowercase();
        model_lower == "gpt-4o"
            || model_lower == "gpt-4-turbo"
            || model_lower.starts_with("o1-")
            || model_lower.starts_with("gpt-4o-2024")
            || model_lower.starts_with("gpt-4-turbo-2024")
    }

    /// Apply this agent's configuration to a chat completion request
    ///
    /// This includes:
    /// - Tool definitions (only if the agent has tools)
    /// - Response format (if configured and supported by the model)
    ///
    /// If the model doesn't support json_schema response format, it will be
    /// removed from the request to avoid API errors.
    pub fn apply_to_request(&self, mut request: ChatCompletionRequest) -> ChatCompletionRequest {
        // Only add tools if the agent has any
        let tool_definitions = self.get_tool_definitions();
        if !tool_definitions.is_empty() {
            request = request.with_tools(tool_definitions);
        }

        // Add response format if configured and supported by the model
        if let Some(ref format) = self.response_format {
            match format {
                ResponseFormat::JsonObject => {
                    // json_object is supported by all models that support structured outputs
                    request = request.with_response_format(format.clone());
                }
                ResponseFormat::JsonSchema { .. } => {
                    // Only apply json_schema if the model supports it
                    if Self::model_supports_json_schema(&request.model) {
                        request = request.with_response_format(format.clone());
                    }
                    // Otherwise, silently omit it to avoid API errors
                }
            }
        }
        request
    }

    pub fn to_xml(
        &self,
        id: Option<&Uuid>,
        system_prompt: bool,
        writer: &mut Writer<Cursor<Vec<u8>>>,
    ) {
        writer
            .write_event(Event::Start(BytesStart::new("agent")))
            .unwrap();

        // Write agent ID if provided
        if let Some(agent_id) = id {
            writer
                .write_event(Event::Start(BytesStart::new("id")))
                .unwrap();
            writer
                .write_event(Event::Text(BytesText::new(&agent_id.to_string())))
                .unwrap();
            writer.write_event(Event::End(BytesEnd::new("id"))).unwrap();
        }

        // Write agent name
        writer
            .write_event(Event::Start(BytesStart::new("name")))
            .unwrap();
        writer
            .write_event(Event::Text(BytesText::new(&self.name)))
            .unwrap();

        writer
            .write_event(Event::End(BytesEnd::new("name")))
            .unwrap();

        // Write agent description
        writer
            .write_event(Event::Start(BytesStart::new("description")))
            .unwrap();
        writer
            .write_event(Event::Text(BytesText::new(&self.description)))
            .unwrap();
        writer
            .write_event(Event::End(BytesEnd::new("description")))
            .unwrap();

        if system_prompt {
            writer
                .write_event(Event::Start(BytesStart::new("system_prompt")))
                .unwrap();
            writer
                .write_event(Event::Text(BytesText::new(&self.system_prompt)))
                .unwrap();
            writer
                .write_event(Event::End(BytesEnd::new("system_prompt")))
                .unwrap();
        }

        writer
            .write_event(Event::End(BytesEnd::new("agent")))
            .unwrap();
    }
}

/// Builder for creating agents with their tool registries
pub struct AgentBuilder {
    name: String,
    description: String,
    system_prompt: String,
    tools: Vec<Tool>,
    tool_registry: ToolRegistry,
    response_format: Option<ResponseFormat>,
}

impl AgentBuilder {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: String::new(),
            system_prompt: String::new(),
            tools: Vec::new(),
            tool_registry: ToolRegistry::new(),
            response_format: None,
        }
    }

    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    pub fn system_prompt(mut self, system_prompt: impl Into<String>) -> Self {
        self.system_prompt = system_prompt.into();
        self
    }

    pub fn add_tool(mut self, tool: Tool) -> Self {
        self.tools.push(tool);
        self
    }

    pub fn tool_registry(mut self, registry: ToolRegistry) -> Self {
        self.tool_registry = registry;
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
    pub fn with_json_schema(mut self, schema: crate::models::chat::JsonSchema) -> Self {
        // Use the agent name as the schema name, or default to "response"
        let schema_name = if self.name.is_empty() {
            "response"
        } else {
            &self.name
        }
        .to_lowercase()
        .replace(" ", "_");
        self.response_format = Some(ResponseFormat::json_schema_with_name(schema, schema_name));
        self
    }

    pub fn build(self) -> Agent {
        Agent::with_response_format(
            self.name,
            self.description,
            self.system_prompt,
            self.tools,
            self.tool_registry,
            self.response_format,
        )
    }
}

/// Manages a collection of agents identified by UUIDs
#[derive(Default, Clone)]
pub struct AgentStore {
    agents: HashMap<Uuid, Agent>,
}

impl AgentStore {
    /// Create an empty store
    pub fn new() -> Self {
        Self {
            agents: HashMap::new(),
        }
    }

    /// Add an agent to the store, returning its generated UUID
    pub fn add_agent(&mut self, agent: Agent) -> Uuid {
        let mut id = Uuid::new_v4();

        while self.agents.contains_key(&id) {
            id = Uuid::new_v4();
        }

        self.agents.insert(id, agent);
        id
    }

    /// Retrieve an agent by UUID
    pub fn get_agent(&self, id: &Uuid) -> Option<&Agent> {
        self.agents.get(id)
    }

    /// Retrieve a mutable reference to an agent by UUID
    pub fn get_agent_mut(&mut self, id: &Uuid) -> Option<&mut Agent> {
        self.agents.get_mut(id)
    }

    /// Remove an agent from the store
    pub fn remove_agent(&mut self, id: &Uuid) -> Option<Agent> {
        self.agents.remove(id)
    }

    /// Iterate over all agents in the store
    pub fn iter(&self) -> impl Iterator<Item = (&Uuid, &Agent)> + '_ {
        self.agents.iter()
    }

    /// Number of agents currently stored
    pub fn len(&self) -> usize {
        self.agents.len()
    }

    /// Returns true when no agents are stored
    pub fn is_empty(&self) -> bool {
        self.agents.is_empty()
    }

    /// Returns the agent list in XML format
    pub fn to_xml(&self) -> String {
        let mut writer = Writer::new_with_indent(Cursor::new(Vec::new()), b' ', 2);

        writer
            .write_event(Event::Start(BytesStart::new("agent_list")))
            .unwrap();

        self.agents
            .iter()
            .for_each(|(id, agent)| agent.to_xml(Some(id), false, &mut writer));

        writer
            .write_event(Event::End(BytesEnd::new("agent_list")))
            .unwrap();

        String::from_utf8(writer.into_inner().into_inner()).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::tool::ToolParameter;
    use std::collections::HashMap;

    #[test]
    fn test_agent_creation() {
        let registry = ToolRegistry::new();
        let agent = Agent::new(
            "Test Agent",
            "A test agent",
            "You are a test assistant",
            vec![],
            registry,
        );
        assert_eq!(agent.name, "Test Agent");
        assert_eq!(agent.description, "A test agent");
        assert_eq!(agent.system_prompt, "You are a test assistant");
        assert_eq!(agent.tools.len(), 0);
    }

    #[test]
    fn test_agent_find_tool() {
        let mut params = HashMap::new();
        params.insert(
            "input".to_string(),
            ToolParameter::new("string", "Input value").with_required(true),
        );
        let tool = Tool::new("test_tool", "A test tool", params);

        let registry = ToolRegistry::new();
        let agent = Agent::new(
            "Test Agent",
            "A test agent",
            "You are a test assistant",
            vec![tool],
            registry,
        );

        let found_tool = agent.find_tool("test_tool");
        assert!(found_tool.is_some());
        assert_eq!(found_tool.unwrap().name, "test_tool");

        let no_tool = agent.find_tool("nonexistent");
        assert!(no_tool.is_none());
    }

    #[test]
    fn test_agent_get_tool_definitions() {
        let mut params = HashMap::new();
        params.insert(
            "input".to_string(),
            ToolParameter::new("string", "Input value").with_required(true),
        );
        let tool = Tool::new("test_tool", "A test tool", params);

        let registry = ToolRegistry::new();
        let agent = Agent::new(
            "Test Agent",
            "A test agent",
            "You are a test assistant",
            vec![tool],
            registry,
        );

        let definitions = agent.get_tool_definitions();
        assert_eq!(definitions.len(), 1);
        assert_eq!(definitions[0]["type"], "function");
        assert_eq!(definitions[0]["function"]["name"], "test_tool");
    }

    #[test]
    fn test_agent_builder() {
        let mut params = HashMap::new();
        params.insert(
            "input".to_string(),
            ToolParameter::new("string", "Input value").with_required(true),
        );
        let tool = Tool::new("test_tool", "A test tool", params);

        let agent = AgentBuilder::new("Test Agent")
            .description("A test agent")
            .system_prompt("You are a test assistant")
            .add_tool(tool)
            .build();

        assert_eq!(agent.name, "Test Agent");
        assert_eq!(agent.description, "A test agent");
        assert_eq!(agent.system_prompt, "You are a test assistant");
        assert_eq!(agent.tools.len(), 1);
        assert_eq!(agent.tools[0].name, "test_tool");
    }

    #[test]
    fn test_agent_store_add_agent_generates_unique_uuid() {
        let registry = ToolRegistry::new();
        let mut store = AgentStore::new();

        let agent_one = Agent::new(
            "Agent One",
            "First agent",
            "System prompt",
            vec![],
            registry.clone(),
        );

        let agent_two = Agent::new(
            "Agent Two",
            "Second agent",
            "System prompt",
            vec![],
            registry,
        );

        let id_one = store.add_agent(agent_one);
        let id_two = store.add_agent(agent_two);

        assert_ne!(id_one, id_two);
        assert_eq!(store.len(), 2);
    }

    #[test]
    fn test_agent_store_retrieves_agent() {
        let registry = ToolRegistry::new();
        let mut store = AgentStore::new();

        let agent = Agent::new("Agent", "Test agent", "System prompt", vec![], registry);

        let id = store.add_agent(agent);

        let retrieved = store.get_agent(&id).expect("agent should exist");
        assert_eq!(retrieved.name, "Agent");

        let removed = store.remove_agent(&id).expect("remove agent");
        assert_eq!(removed.name, "Agent");
        assert!(store.get_agent(&id).is_none());
        assert!(store.is_empty());
    }

    #[test]
    fn test_agent_with_response_format() {
        let registry = ToolRegistry::new();
        let format = ResponseFormat::json_object();
        let agent = Agent::with_response_format(
            "Test Agent",
            "A test agent",
            "You are a test assistant",
            vec![],
            registry,
            Some(format.clone()),
        );

        assert_eq!(agent.name, "Test Agent");
        assert!(agent.response_format().is_some());
        assert_eq!(agent.response_format().unwrap(), &format);
    }

    #[test]
    fn test_agent_builder_with_json_object() {
        let agent = AgentBuilder::new("JSON Agent")
            .description("An agent that outputs JSON")
            .system_prompt("You output JSON")
            .with_json_object()
            .build();

        assert_eq!(agent.name, "JSON Agent");
        assert!(agent.response_format().is_some());
        if let Some(ResponseFormat::JsonObject) = agent.response_format() {
            // Correct variant
        } else {
            panic!("Expected JsonObject variant");
        }
    }

    #[test]
    fn test_agent_builder_with_json_schema() {
        use crate::models::chat::JsonSchema;
        let schema = JsonSchema::new("object").with_properties(serde_json::json!({
            "result": {"type": "string"}
        }));

        let agent = AgentBuilder::new("Schema Agent")
            .description("An agent with schema")
            .system_prompt("You follow a schema")
            .with_json_schema(schema)
            .build();

        assert_eq!(agent.name, "Schema Agent");
        assert!(agent.response_format().is_some());
    }

    #[test]
    fn test_agent_apply_to_request() {
        use crate::models::chat::{JsonSchema, Message};
        let registry = ToolRegistry::new();
        let schema = JsonSchema::new("object").with_properties(serde_json::json!({
            "answer": {"type": "string"}
        }));
        let format = ResponseFormat::json_schema(schema);
        let agent = Agent::with_response_format(
            "Test Agent",
            "A test agent",
            "You are a test assistant",
            vec![],
            registry,
            Some(format),
        );

        let request = ChatCompletionRequest::new("gpt-4", vec![Message::user("Hello")]);
        let modified_request = agent.apply_to_request(request);

        assert!(modified_request.response_format.is_some());
        assert!(modified_request.tools.is_none());
    }
}
