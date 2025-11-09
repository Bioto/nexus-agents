use crate::models::agent::{Agent, AgentBuilder, AgentStore};
use crate::models::tasks::{TaskAssignment, TaskDecomposition};
use crate::models::task_manager::TaskManager;
use crate::tools::{
    ExecutableTool, ToolRegistry, calculator::Calculator
};

/// Factory for creating pre-configured agents with their tool registries
pub struct AgentFactory;

impl AgentFactory {
    /// Create a preconfigured agent store with available agents
    pub fn default_agent_store() -> AgentStore {
        let mut store = AgentStore::new();

        // Add calculator agent
        let calculator = Self::calculator();
        store.add_agent(calculator);

        // Add generic agent
        let generic = Self::generic_agent();
        store.add_agent(generic);

        // Add more agents here as they become available

        store
    }

    pub fn generic_agent() -> Agent {
        AgentBuilder::new("Generic Agent")
            .description("An agent that can perform generic tasks")
            .system_prompt("You are a helpful assistant that can perform generic tasks")
            .build()
    }

    /// Create a swarm coordinator agent
    pub fn swarm_coordinator(agent_store: AgentStore) -> Agent {
        let prompt = format!(
            "You are a Swarm Coordinator for follow-up questions.\n\
            You do NOT have any tools available. You can only discuss and answer questions about \
            the results from previous swarm executions that are shown in the conversation history.\n\
            If the user asks you to perform a new task or calculation, explain that you need to \
            start a new swarm execution for that.\n\n\
            Available agents in the swarm:\n{}",
            agent_store.to_xml()
        );

        AgentBuilder::new("Swarm Coordinator")
            .description("Orchestrates multi-agent task execution and handles follow-up questions")
            .system_prompt(prompt)
            .build()
    }

    /// Create a calculator agent with basic math capabilities
    pub fn calculator() -> Agent {
        let calculator = Calculator::new();
        let tool_definition = calculator.definition();
        let registry = ToolRegistry::new().register(Box::new(calculator));

        AgentBuilder::new("Calculator Agent")
            .description("An agent that can perform basic mathematical calculations")
            .system_prompt(
                "You are a helpful assistant with calculator capabilities. \
                You have ONE tool available called 'calculate' that performs basic math operations.\n\n\
                To use it, you MUST call the 'calculate' tool with these parameters:\n\
                - operation: one of 'add', 'subtract', 'multiply', 'divide'\n\
                - a: first number\n\
                - b: second number\n\n\
                IMPORTANT: You do NOT have separate 'add', 'subtract', etc. tools. \
                You only have the 'calculate' tool that takes an operation parameter.\n\n\
                Example: To add 2 + 3, use calculate with operation='add', a=2, b=3"
            )
            .add_tool(tool_definition)
            .tool_registry(registry)
            .build()
    }

    /// Create a router agent that can route requests to the appropriate agent
    pub fn swarm_router(task_manager: TaskManager, agent_store: AgentStore) -> Agent {
        let prompt = include_str!("../agents/swarm_router_prompt.md")
            .replace("{task_list}", &task_manager.to_xml())
            .replace("{agent_list}", &agent_store.to_xml())
            .replace(
                "{output_format}",
                &serde_json::to_string_pretty(&TaskAssignment::to_output_format()).unwrap(),
            );

        AgentBuilder::new("Swarm Router Agent")
            .description("An agent that can route requests to the appropriate agent")
            .system_prompt(prompt)
            .with_json_schema(TaskAssignment::to_output_format())
            .build()
    }

    /// Create a task decomposition agent that breaks down user requests into actionable tasks
    pub fn swarm_task_decomposition(agent_store: AgentStore) -> Agent {
        let prompt = include_str!("../agents/swarm_task_prompt.md")
            .replace("{agent_list}", &agent_store.to_xml())
            .replace(
                "{output_format}",
                &serde_json::to_string_pretty(&TaskDecomposition::to_output_format()).unwrap(),
            );

        AgentBuilder::new("Task Decomposition Agent")
            .description("An agent that analyzes user requests and decomposes them into structured, actionable tasks")
            .system_prompt(prompt)
            .with_json_schema(TaskDecomposition::to_output_format())
            .build()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculator_agent_factory() {
        let agent = AgentFactory::calculator();

        assert_eq!(agent.name, "Calculator Agent");
        assert_eq!(agent.tools.len(), 1);

        let tool = &agent.tools[0];
        assert_eq!(tool.name, "calculate");
        assert_eq!(tool.parameters.len(), 3);

        // Verify registry has the tool
        assert!(agent.tool_registry().has_tool("calculate"));

        // Verify it can execute
        let result = agent.tool_registry().execute(
            "calculate",
            serde_json::json!({
                "operation": "add",
                "a": 2.0,
                "b": 3.0
            }),
        );
        assert!(result.is_ok());
        assert!(result.unwrap().contains("5"));
    }

    #[test]
    fn test_task_decomposition_agent_factory() {
        let agent_store = AgentStore::new();
        let agent = AgentFactory::swarm_task_decomposition(agent_store);

        assert_eq!(agent.name, "Task Decomposition Agent");
        assert!(agent.description.contains("decomposes"));
        assert!(
            agent
                .system_prompt
                .contains("Task Decomposition Specialist")
        );

        // Verify structured output is configured
        assert!(agent.response_format().is_some());
    }
}
