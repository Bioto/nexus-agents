use crate::models::tool::{Tool, ToolParameter};
use crate::models::{Error, Result};
use crate::tools::ExecutableTool;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Detail level for tool discovery responses
#[derive(Clone, Copy)]
enum DetailLevel {
    NameOnly,
    Summary,
    Full,
}

impl DetailLevel {
    fn from_str(value: Option<&str>) -> Self {
        match value.map(|v| v.to_ascii_lowercase()) {
            Some(ref v) if v == "name" || v == "names" => DetailLevel::NameOnly,
            Some(ref v) if v == "full" || v == "detail" => DetailLevel::Full,
            _ => DetailLevel::Summary,
        }
    }

    fn as_str(&self) -> &'static str {
        match self {
            DetailLevel::NameOnly => "name",
            DetailLevel::Summary => "summary",
            DetailLevel::Full => "full",
        }
    }
}

/// Metadata about a discovered MCP tool file
struct ToolMetadata {
    qualified_name: String,
    server: String,
    relative_path: String,
    summary: Option<String>,
    docstring: Option<String>,
    preview: Option<String>,
}

pub struct ToolDiscovery {
    servers_dir: PathBuf,
}

impl ToolDiscovery {
    pub fn new(servers_dir: impl Into<PathBuf>) -> Self {
        Self {
            servers_dir: servers_dir.into(),
        }
    }

    fn ensure_servers_dir(&self) -> Result<PathBuf> {
        if !self.servers_dir.exists() {
            return Err(Error::Configuration(format!(
                "Servers directory does not exist: {}",
                self.servers_dir.display()
            )));
        }
        self.servers_dir
            .canonicalize()
            .map_err(|e| Error::Other(format!("Failed to access servers directory: {}", e)))
    }

    fn workspace_root(&self) -> Option<PathBuf> {
        self.servers_dir.parent().map(|p| p.to_path_buf())
    }

    fn collect_metadata(&self) -> Result<Vec<ToolMetadata>> {
        let servers_dir = self.ensure_servers_dir()?;
        let workspace_root = self.workspace_root();
        let mut tools = Vec::new();

        for entry in fs::read_dir(&servers_dir).map_err(|e| {
            Error::Other(format!(
                "Failed to read servers directory {}: {}",
                servers_dir.display(),
                e
            ))
        })? {
            let entry = entry
                .map_err(|e| Error::Other(format!("Failed to read directory entry: {}", e)))?;
            let path = entry.path();
            let file_type = entry
                .file_type()
                .map_err(|e| Error::Other(format!("Failed to inspect entry type: {}", e)))?;

            if file_type.is_dir() {
                let server_name = entry.file_name().to_string_lossy().to_string();
                tools.extend(self.read_server(&server_name, &path, workspace_root.as_ref())?);
            }
        }

        Ok(tools)
    }

    fn read_server(
        &self,
        server_name: &str,
        server_path: &Path,
        workspace_root: Option<&PathBuf>,
    ) -> Result<Vec<ToolMetadata>> {
        let mut tools = Vec::new();

        for entry in fs::read_dir(server_path).map_err(|e| {
            Error::Other(format!(
                "Failed to read server directory {}: {}",
                server_path.display(),
                e
            ))
        })? {
            let entry = entry
                .map_err(|e| Error::Other(format!("Failed to read server file entry: {}", e)))?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("py") {
                continue;
            }

            let file_name = match path.file_stem().and_then(|s| s.to_str()) {
                Some(name) => name.to_string(),
                None => continue,
            };

            if ["__init__", "index"].contains(&file_name.as_str()) {
                continue;
            }

            let contents = fs::read_to_string(&path).map_err(|e| {
                Error::Other(format!(
                    "Failed to read tool file {}: {}",
                    path.display(),
                    e
                ))
            })?;

            let docstring = extract_docstring(&contents);
            let summary = docstring
                .as_ref()
                .and_then(|s| s.lines().next())
                .map(|line| line.trim().to_string());

            let relative_path = workspace_root
                .and_then(|root| path.strip_prefix(root).ok())
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|| path.to_string_lossy().to_string());

            tools.push(ToolMetadata {
                qualified_name: format!("{}/{}", server_name, file_name),
                server: server_name.to_string(),
                relative_path,
                summary,
                docstring,
                preview: Some(truncate_preview(&contents)),
            });
        }

        Ok(tools)
    }
}

impl ExecutableTool for ToolDiscovery {
    fn name(&self) -> &str {
        "search_mcp_tools"
    }

    fn definition(&self) -> Tool {
        let mut parameters = HashMap::new();
        parameters.insert(
            "query".to_string(),
            ToolParameter::new(
                "string",
                "Optional keyword to filter by tool name, server name, or description",
            ),
        );
        parameters.insert(
            "detail".to_string(),
            ToolParameter::new(
                "string",
                "Detail level: 'name' (names only), 'summary' (default), or 'full'",
            ),
        );
        parameters.insert(
            "limit".to_string(),
            ToolParameter::new(
                "integer",
                "Maximum number of results to return (default 25, max 100)",
            ),
        );

        Tool::new(
            "search_mcp_tools",
            "Search the generated MCP tool files by keyword and detail level.",
            parameters,
        )
    }

    fn execute(&self, args: Value) -> Result<String> {
        let query = args
            .get("query")
            .and_then(|v| v.as_str())
            .map(|s| s.to_ascii_lowercase())
            .unwrap_or_default();
        let detail = DetailLevel::from_str(args.get("detail").and_then(|v| v.as_str()));
        let limit = args
            .get("limit")
            .and_then(|v| v.as_u64())
            .map(|v| v.min(100) as usize)
            .unwrap_or(25);

        let mut metadata = self.collect_metadata()?;

        if !query.is_empty() {
            metadata.retain(|tool| {
                let name = tool.qualified_name.to_ascii_lowercase();
                let server = tool.server.to_ascii_lowercase();
                let summary = tool
                    .summary
                    .as_deref()
                    .map(|s| s.to_ascii_lowercase())
                    .unwrap_or_default();
                name.contains(&query) || server.contains(&query) || summary.contains(&query)
            });
        }

        metadata.sort_by(|a, b| a.qualified_name.cmp(&b.qualified_name));
        let total_available = metadata.len();
        let limited = metadata.into_iter().take(limit);

        let results: Vec<Value> = limited
            .map(|tool| match detail {
                DetailLevel::NameOnly => json!({
                    "tool": tool.qualified_name,
                    "path": tool.relative_path,
                }),
                DetailLevel::Summary => json!({
                    "tool": tool.qualified_name,
                    "server": tool.server,
                    "path": tool.relative_path,
                    "summary": tool.summary,
                }),
                DetailLevel::Full => json!({
                    "tool": tool.qualified_name,
                    "server": tool.server,
                    "path": tool.relative_path,
                    "summary": tool.summary,
                    "docstring": tool.docstring,
                    "preview": tool.preview,
                }),
            })
            .collect();

        let response = json!({
            "detail_level": detail.as_str(),
            "count": results.len(),
            "total_available": total_available,
            "results": results,
        });

        Ok(response.to_string())
    }
}

fn extract_docstring(contents: &str) -> Option<String> {
    let trimmed = contents.trim_start();
    let marker = "\"\"\"";
    if let Some(start) = trimmed.find(marker) {
        let after_start = &trimmed[start + marker.len()..];
        if let Some(end) = after_start.find(marker) {
            return Some(after_start[..end].trim().to_string());
        }
    }
    None
}

fn truncate_preview(contents: &str) -> String {
    const MAX_CHARS: usize = 1200;
    let mut preview: String = contents.lines().take(40).collect::<Vec<_>>().join("\n");
    if preview.len() > MAX_CHARS {
        preview.truncate(MAX_CHARS);
        preview.push_str("\n...<truncated>...");
    }
    preview
}
