use clap::Args;
use nexus_core::{
    load_env,
    models::{Error, Result},
    tools::{python_exec::PythonExec, tool_discovery::ToolDiscovery, ExecutableTool},
};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use tokio::task;

#[derive(Args, Debug)]
#[command(about = "Search for an MCP tool and execute it via execute_python")]
pub struct TestMcpFlowArgs {
    /// Path to servers directory containing the generated MCP tool files
    #[arg(long, default_value = "servers")]
    pub servers_dir: PathBuf,

    /// Keyword to pass to search_mcp_tools (same as --query on the CLI)
    #[arg(long)]
    pub query: String,

    /// 1-based index of the search result to execute
    #[arg(long, default_value_t = 1)]
    pub pick: usize,

    /// JSON payload to send to the tool function (defaults to an empty object)
    #[arg(long)]
    pub payload: Option<String>,
}

pub async fn run_test_mcp_flow(args: TestMcpFlowArgs) -> Result<()> {
    load_env();

    if args.pick == 0 {
        return Err(Error::Configuration(
            "--pick must be at least 1".to_string(),
        ));
    }

    if args.pick > 100 {
        return Err(Error::Configuration(
            "--pick cannot be greater than 100 (search_mcp_tools limit)".to_string(),
        ));
    }

    if !args.servers_dir.exists() {
        return Err(Error::Configuration(format!(
            "Servers directory does not exist: {}",
            args.servers_dir.display()
        )));
    }

    let discovery = ToolDiscovery::new(&args.servers_dir);
    let mut discovery_payload = serde_json::Map::new();
    discovery_payload.insert("query".into(), Value::String(args.query.clone()));
    discovery_payload.insert("detail".into(), Value::String("full".into()));
    discovery_payload.insert(
        "limit".into(),
        Value::Number(serde_json::Number::from(args.pick as u64)),
    );

    println!(
        "[test-mcp-flow] Searching for tools with query '{}'...",
        args.query
    );
    let response = discovery.execute(Value::Object(discovery_payload))?;
    let parsed: Value = serde_json::from_str(&response)
        .map_err(|e| Error::Other(format!("Failed to parse tool discovery response: {}", e)))?;
    let results = parsed
        .get("results")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::Other("Tool discovery response missing results array".to_string()))?;

    if results.is_empty() {
        return Err(Error::Other(format!(
            "No tools matched query '{}'",
            args.query
        )));
    }

    let index = args.pick - 1;
    let selected = results.get(index).ok_or_else(|| {
        Error::Other(format!(
            "Result index {} is out of range (only {} result(s) available)",
            args.pick,
            results.len()
        ))
    })?;

    let qualified_name = selected
        .get("tool")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::Other("Selected result missing 'tool' field".to_string()))?;
    let (server_name, tool_name) = qualified_name.split_once('/').ok_or_else(|| {
        Error::Other(format!(
            "Tool name '{}' does not include server/tool structure",
            qualified_name
        ))
    })?;
    let python_function = tool_name;

    let tool_path_str = selected
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::Other("Selected result missing 'path' field".to_string()))?;

    let servers_dir = args
        .servers_dir
        .canonicalize()
        .map_err(|e| Error::Other(format!("Failed to canonicalize servers dir: {}", e)))?;
    let workspace_root = servers_dir.parent().ok_or_else(|| {
        Error::Other(format!(
            "Servers directory {} has no parent workspace",
            servers_dir.display()
        ))
    })?;

    let tool_path = resolve_tool_path(workspace_root, tool_path_str);
    println!(
        "[test-mcp-flow] Selected tool: {} (path: {})",
        qualified_name,
        tool_path.display()
    );

    let dependency_directive = extract_dependency_directive(&tool_path)
        .unwrap_or_else(|| "# uv: dependencies = []".into());

    let payload_value: Value = match args.payload {
        Some(ref raw) => serde_json::from_str(raw)
            .map_err(|e| Error::Other(format!("Failed to parse payload JSON '{}': {}", raw, e)))?,
        None => {
            // Try to generate default test values from the tool's TypedDict
            match generate_default_payload(&tool_path) {
                Some(default) => {
                    println!(
                        "[test-mcp-flow] No payload provided, using generated defaults: {}",
                        serde_json::to_string(&default).unwrap_or_default()
                    );
                    default
                }
                None => {
                    eprintln!(
                        "[test-mcp-flow] WARNING: No payload provided and could not generate defaults."
                    );
                    eprintln!(
                        "[test-mcp-flow] Tool may require parameters. Use --payload '{{...}}' to provide them."
                    );
                    Value::Object(Default::default())
                }
            }
        }
    };
    let payload_json =
        serde_json::to_string(&payload_value).map_err(|e| Error::Other(e.to_string()))?;

    let python_code = build_python_snippet(
        &dependency_directive,
        server_name,
        tool_name,
        python_function,
        &payload_json,
    );

    println!("[test-mcp-flow] Executing tool via execute_python...");
    let executor = PythonExec::new(&args.servers_dir);
    let execution_result =
        task::spawn_blocking(move || executor.execute(json!({ "code": python_code })))
            .await
            .map_err(|e| Error::Other(format!("Failed to join execute_python task: {}", e)))??;
    println!(
        "[test-mcp-flow] Tool execution completed.\n{}\n",
        execution_result
    );

    Ok(())
}

fn resolve_tool_path(workspace_root: &Path, tool_path: &str) -> PathBuf {
    let candidate = PathBuf::from(tool_path);
    if candidate.is_absolute() {
        candidate
    } else {
        workspace_root.join(candidate)
    }
}

fn extract_dependency_directive(path: &Path) -> Option<String> {
    let contents = fs::read_to_string(path).ok()?;
    contents.lines().find_map(|line| {
        let trimmed = line.trim_start();
        if trimmed.starts_with("# uv: dependencies") {
            Some(trimmed.to_string())
        } else {
            None
        }
    })
}

/// Generate default test payload from TypedDict definition in Python file
fn generate_default_payload(path: &Path) -> Option<Value> {
    let contents = fs::read_to_string(path).ok()?;

    // Look for TypedDict class definition (e.g., "class AddInput(TypedDict):")
    let mut in_typed_dict = false;
    let mut payload = serde_json::Map::new();

    for line in contents.lines() {
        let trimmed = line.trim();

        // Check if this is a TypedDict class definition
        if trimmed.starts_with("class ") && trimmed.contains("TypedDict") {
            in_typed_dict = true;
            continue;
        }

        // If we're in a TypedDict, look for field definitions
        if in_typed_dict {
            // Stop if we hit a blank line (after first field) or docstring or function definition
            if trimmed.is_empty() && !payload.is_empty() {
                break;
            }
            if trimmed.starts_with("\"\"\"")
                || trimmed.starts_with("async def")
                || trimmed.starts_with("def ")
            {
                break;
            }

            // Skip comments
            if trimmed.starts_with('#') {
                continue;
            }

            // Parse field definition: "field_name: type"
            if let Some(colon_pos) = trimmed.find(':') {
                let field_name = trimmed[..colon_pos].trim();
                let field_type = trimmed[colon_pos + 1..].trim();

                // Skip if field name is empty or contains invalid characters
                if field_name.is_empty()
                    || !field_name
                        .chars()
                        .next()
                        .map(|c| c.is_alphabetic())
                        .unwrap_or(false)
                {
                    continue;
                }

                // Generate default value based on type
                let default_value = match field_type {
                    t if t.contains("float") || t.contains("int") || t.contains("number") => {
                        Value::Number(serde_json::Number::from(1))
                    }
                    t if t.contains("str") || t.contains("string") || t.contains("String") => {
                        Value::String("test".to_string())
                    }
                    t if t.contains("bool") || t.contains("Bool") => Value::Bool(true),
                    t if t.contains("list") || t.contains("List") || t.contains("Array") => {
                        Value::Array(vec![])
                    }
                    t if t.contains("dict") || t.contains("Dict") || t.contains("Mapping") => {
                        Value::Object(Default::default())
                    }
                    _ => Value::String("test".to_string()), // Default fallback
                };

                payload.insert(field_name.to_string(), default_value);
            }
        }
    }

    if payload.is_empty() {
        None
    } else {
        Some(Value::Object(payload))
    }
}

fn build_python_snippet(
    dependency_directive: &str,
    server_name: &str,
    tool_module: &str,
    function_name: &str,
    payload_json: &str,
) -> String {
    let server_literal = serde_json::to_string(server_name).expect("server name serialization");
    let module_literal = serde_json::to_string(tool_module).expect("tool name serialization");
    let function_literal =
        serde_json::to_string(function_name).expect("function name serialization");

    format!(
        "{dependency_directive}

import asyncio
import json

tool_module = import_tool({server_literal}, {module_literal})
tool_fn = getattr(tool_module, {function_literal})
payload = json.loads(r'''{payload_json}''')

async def main():
    result = await tool_fn(payload)
    print(json.dumps(result, indent=2, sort_keys=True))

asyncio.run(main())
"
    )
}
