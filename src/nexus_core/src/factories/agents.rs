use crate::models::agent::{Agent, AgentBuilder, AgentStore};
use crate::models::task_manager::TaskManager;
use crate::models::tasks::{TaskAssignment, TaskDecomposition};
use crate::tools::{
    calculator::Calculator, python_exec::PythonExec, tool_discovery::ToolDiscovery, ExecutableTool,
    ToolRegistry,
};
use std::path::PathBuf;

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

    /// Create an MCP agent that can execute Python code with access to MCP server tools
    /// This will also generate Python tool files for any external MCP servers configured in mcp-servers.toml
    pub fn mcp_agent(servers_dir: impl Into<PathBuf>) -> Agent {
        let servers_path: PathBuf = servers_dir.into();
        
        // Generate code for external servers from config if it exists
        let config_path = servers_path
            .parent()
            .map(|p| p.join("mcp-servers.toml"))
            .or_else(|| Some(std::path::PathBuf::from("mcp-servers.toml")));
        
        if let Some(config_path) = config_path {
            if config_path.exists() {
                // Use tokio runtime to run async code generation
                if let Ok(handle) = tokio::runtime::Handle::try_current() {
                    // We're in an async context, spawn the task
                    let config_path_clone = config_path.clone();
                    let servers_path_clone = servers_path.clone();
                    handle.spawn(async move {
                        if let Err(e) = nexus_mcp::generate_external_server_tools(
                            &config_path_clone,
                            &servers_path_clone,
                        )
                        .await
                        {
                            eprintln!("Warning: Failed to generate external server tools: {}", e);
                        }
                    });
                } else {
                    // We're not in an async context, create a runtime
                    let rt = tokio::runtime::Runtime::new().unwrap_or_else(|_| {
                        tokio::runtime::Builder::new_current_thread()
                            .enable_all()
                            .build()
                            .expect("Failed to create tokio runtime")
                    });
                    
                    if let Err(e) = rt.block_on(nexus_mcp::generate_external_server_tools(
                        &config_path,
                        &servers_path,
                    )) {
                        eprintln!("Warning: Failed to generate external server tools: {}", e);
                    }
                }
            }
        }
        
        let python_exec = PythonExec::new(&servers_path);
        let discovery_tool = ToolDiscovery::new(&servers_path);
        let execute_definition = python_exec.definition();
        let discovery_definition = discovery_tool.definition();
        let registry = ToolRegistry::new()
            .register(Box::new(python_exec))
            .register(Box::new(discovery_tool));

        let servers_path_str = servers_path.to_string_lossy();

        let system_prompt = format!(
            "You are an MCP Agent with access to MCP server tools via Python code execution.\n\n\
            MCP tools are located in: {}\n\n\
            Available tools:\n\
            - search_mcp_tools: Discover available tools (use before writing code)\n\
            - execute_python: Execute Python code in a sandboxed environment\n\n\
            CRITICAL: All Python code MUST start with inline uv metadata dependencies.\n\
            This must be the FIRST line(s) of your code, before any imports or comments.\n\
            Format: # uv: dependencies = [\"package1\", \"package2\"]\n\
            If no external packages are needed, use: # uv: dependencies = []\n\
            Standard library modules (asyncio, importlib, pathlib, etc.) don't need to be listed.\n\n\
            Using MCP tools:\n\
            - Prefer import_tool(server_name, tool_name) helper (available in execution environment)\n\
            - All tool functions are async and take one argument: a dict with all required parameters\n\
            - Use search_mcp_tools to discover tool parameters before calling them\n\
            - Execute code using the execute_python tool\n\n\
            Example:\n\
            ```python
            # uv: dependencies = []\n\
            import asyncio\n\
            \n\
            tool = import_tool('nexus-mcp-server', 'echo')\n\
            \n\
            async def main():\n\
                result = await tool.echo({{'message': 'Hello'}})\n\
                print(result)\n\
            \n\
            asyncio.run(main())\n\
            ```\n\n\
            Execution policy:\n\
            - Use execute_python tool to run code (only approved execution method)\n\
            - Report stdout/stderr and errors back to the user\n\
            - Never claim execution unless you actually invoked the tool",
            servers_path_str
        );

        AgentBuilder::new("MCP Agent")
            .description("An agent that can interact with MCP servers by executing Python code with access to generated tool files")
            .system_prompt(system_prompt)
            .add_tool(execute_definition)
            .add_tool(discovery_definition)
            .tool_registry(registry)
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
        assert!(agent
            .system_prompt
            .contains("Task Decomposition Specialist"));

        // Verify structured output is configured
        assert!(agent.response_format().is_some());
    }
}
