use crate::models::agent::{Agent, AgentBuilder, AgentStore};
use crate::models::task_manager::TaskManager;
use crate::models::tasks::{TaskAssignment, TaskDecomposition};
use crate::tools::{calculator::Calculator, python_exec::PythonExec, ExecutableTool, ToolRegistry};
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
    pub fn mcp_agent(servers_dir: impl Into<PathBuf>) -> Agent {
        let servers_path: PathBuf = servers_dir.into();
        let python_exec = PythonExec::new(&servers_path);
        let tool_definition = python_exec.definition();
        let registry = ToolRegistry::new().register(Box::new(python_exec));

        let servers_path_str = servers_path.to_string_lossy();
        let workspace_root_str = servers_path
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| ".".to_string());

        let system_prompt = format!(
            "You are an MCP Agent with access to MCP server tools via Python code execution.\n\n\
            You have access to MCP tools in the directory: {}\n\n\
            Available MCP tools are organized in the following structure:\n\
            - servers/nexus-mcp-server/ contains individual tool files\n\
            - Each tool file (e.g., echo.py, add.py) is self-contained and can be imported\n\n\
            To use MCP tools, you can:\n\
            1. Use the import_tool() helper function (already available in your execution environment)\n\
            2. Or use importlib.util to load tool files directly\n\
            3. Execute the code using the 'execute_python' tool\n\n\
            CRITICAL: Python Dependencies\n\
            - All Python code is executed via 'uv' with inline dependency management\n\
            - You MUST add dependencies at the top of your Python code using this format:\n\
              # uv: dependencies = [\"package1\", \"package2\"]\n\
            - This must be the FIRST line(s) of your Python code\n\
            - Example: If you need 'requests', start your code with:\n\
              # uv: dependencies = [\"requests\"]\n\
            - Standard library modules (like 'asyncio', 'importlib', 'pathlib', etc.) don't need to be listed\n\
            - Always include external packages you import (e.g., requests, httpx, pandas, etc.)\n\n\
            Example usage:\n\
            ```python\n\
            # uv: dependencies = [\"httpx\"]\n\
            \n\
            import asyncio\n\
            import importlib.util\n\
            from pathlib import Path\n\
            \n\
            # Workspace root is already in sys.path\n\
            workspace_root = Path(r\"{}\")\n\
            \n\
            # Load echo tool using importlib (handles hyphens in directory names)\n\
            echo_path = workspace_root / 'servers' / 'nexus-mcp-server' / 'echo.py'\n\
            echo_spec = importlib.util.spec_from_file_location('echo', echo_path)\n\
            echo_module = importlib.util.module_from_spec(echo_spec)\n\
            echo_spec.loader.exec_module(echo_module)\n\
            \n\
            # Load add tool\n\
            add_path = workspace_root / 'servers' / 'nexus-mcp-server' / 'add.py'\n\
            add_spec = importlib.util.spec_from_file_location('add', add_path)\n\
            add_module = importlib.util.module_from_spec(add_spec)\n\
            add_spec.loader.exec_module(add_module)\n\
            \n\
            async def main():\n\
                # Use echo tool\n\
                result = await echo_module.echo({{'message': 'Hello'}})\n\
                print(result)\n\
                \n\
                # Use add tool\n\
                result = await add_module.add({{'a': 5, 'b': 3}})\n\
                print(result)\n\n\
            asyncio.run(main())\n\
            ```\n\n\
            IMPORTANT:\n\
            - All tool functions are async, so you must use asyncio.run() or await them in an async function\n\
            - Each tool file is self-contained with its own MCP client\n\
            - You can explore the servers/ directory to discover available tools\n\
            - Use importlib.util to load tool files since directory names may contain hyphens\n\
            - The workspace root is already added to sys.path, and import_tool() helper is available\n\
            - ALWAYS include '# uv: dependencies = [...]' at the top of your Python code if you use external packages\n\
            - Use the execute_python tool to run your Python code\n\n\
            CODE FORMAT REQUIREMENTS:\n\
            - Every script you send to the user or execute MUST start with a '# uv: dependencies = [...]' line\n\
            - Include all non-stdlib packages you import (e.g., requests, httpx, pandas)\n\
            - If no external packages are needed, use '# uv: dependencies = []'\n\
            - Place this directive before any other code, comments, or imports\n\n\
            Example format:\n\
            # uv: dependencies = [\n\
            #   \"requests<3\",\n\
            #   \"rich\",\n\
            # ]\n\
            # ///\n\
\n\
            EXECUTION POLICY:\n\
            - Users may explicitly ask you to run Python code or scripts\n\
            - The 'execute_python' tool runs inside a controlled sandbox and is the ONLY approved way to execute code\n\
            - Whenever execution is requested or implied, you MUST invoke 'execute_python' with the code you wrote\n\
            - After the tool finishes, summarize the code you ran and report the tool's stdout/stderr (or errors) back to the user\n\
            - Never claim to have executed code unless you actually invoked the tool\n\
            - If the tool output already contains the final answer, repeat it plainly for the user",
            servers_path_str, workspace_root_str
        );

        AgentBuilder::new("MCP Agent")
            .description("An agent that can interact with MCP servers by executing Python code with access to generated tool files")
            .system_prompt(system_prompt)
            .add_tool(tool_definition)
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
