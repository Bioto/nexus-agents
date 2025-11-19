use clap::Args;
use nexus_core::{
    load_env,
    models::{Error, Result},
    tools::{tool_discovery::ToolDiscovery, ExecutableTool},
};
use serde_json::{Map, Value};
use std::path::PathBuf;

#[derive(Args, Debug)]
#[command(about = "Search generated MCP tool files without entering the agent TUI")]
pub struct SearchMcpToolsArgs {
    /// Path to servers directory containing the generated MCP tool files
    #[arg(long, default_value = "servers")]
    pub servers_dir: PathBuf,

    /// Keyword to filter by tool/server name or docstring summary
    #[arg(short, long)]
    pub query: Option<String>,

    /// Detail level: name, summary, or full (default summary)
    #[arg(short, long)]
    pub detail: Option<String>,

    /// Maximum number of results to display (default 25, max 100)
    #[arg(short, long)]
    pub limit: Option<u32>,

    /// Emit raw JSON instead of a friendly table
    #[arg(long)]
    pub json: bool,
}

pub fn run_search_mcp_tools(args: SearchMcpToolsArgs) -> Result<()> {
    load_env();

    if !args.servers_dir.exists() {
        return Err(Error::Configuration(format!(
            "Servers directory does not exist: {}",
            args.servers_dir.display()
        )));
    }

    let detail_normalized = args
        .detail
        .as_deref()
        .map(|s| s.to_ascii_lowercase())
        .map(|detail| match detail.as_str() {
            "name" | "names" => "name".to_string(),
            "full" | "detail" => "full".to_string(),
            _ => "summary".to_string(),
        });

    let limit = args.limit.map(|v| v.min(100));

    let discovery = ToolDiscovery::new(&args.servers_dir);

    let mut payload = Map::new();
    if let Some(query) = args.query {
        if !query.trim().is_empty() {
            payload.insert("query".to_string(), Value::String(query));
        }
    }
    if let Some(detail) = detail_normalized {
        payload.insert("detail".to_string(), Value::String(detail));
    }
    if let Some(limit) = limit {
        payload.insert("limit".to_string(), Value::Number(limit.into()));
    }

    let response = discovery.execute(Value::Object(payload))?;
    let parsed: Value = serde_json::from_str(&response)
        .map_err(|e| Error::Other(format!("Failed to parse tool discovery response: {}", e)))?;

    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&parsed).unwrap_or(response)
        );
        return Ok(());
    }

    print_pretty_results(&parsed);
    Ok(())
}

fn print_pretty_results(value: &Value) {
    let detail = value
        .get("detail_level")
        .and_then(Value::as_str)
        .unwrap_or("summary");
    let count = value.get("count").and_then(Value::as_u64).unwrap_or(0);
    let total = value
        .get("total_available")
        .and_then(Value::as_u64)
        .unwrap_or(count);

    println!(
        "Found {} matching tools (showing {} of {}, detail: {})",
        total, count, total, detail
    );

    if let Some(results) = value.get("results").and_then(Value::as_array) {
        for (idx, entry) in results.iter().enumerate() {
            let name = entry
                .get("tool")
                .and_then(Value::as_str)
                .unwrap_or("<unknown>");
            let path = entry
                .get("path")
                .and_then(Value::as_str)
                .unwrap_or("<path unavailable>");
            println!("\n{}. {}", idx + 1, name);
            println!("   path   : {}", path);

            if let Some(server) = entry.get("server").and_then(Value::as_str) {
                println!("   server : {}", server);
            }
            if let Some(summary) = entry.get("summary").and_then(Value::as_str) {
                println!("   summary: {}", summary);
            }
            if let Some(docstring) = entry.get("docstring").and_then(Value::as_str) {
                println!("   doc    : {}", first_line(docstring));
            }
            if detail == "full" {
                if let Some(preview) = entry.get("preview").and_then(Value::as_str) {
                    println!("   preview:\n{}\n", indent_block(preview, "      "));
                }
            }
        }
    } else {
        println!("No results.");
    }
}

fn first_line(text: &str) -> String {
    text.lines()
        .next()
        .map(|line| line.trim().to_string())
        .unwrap_or_default()
}

fn indent_block(text: &str, prefix: &str) -> String {
    text.lines()
        .map(|line| format!("{}{}", prefix, line))
        .collect::<Vec<_>>()
        .join("\n")
}
