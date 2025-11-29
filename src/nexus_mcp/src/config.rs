use crate::error::NexusError;
use regex::{Captures, Regex};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::path::Path;
use std::sync::OnceLock;

/// Configuration for a single MCP server instance
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    /// Server name/identifier
    pub name: String,
    /// Human-readable description of the server's purpose and capabilities
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Server type: "nexus" or "nutrition" (default: "nexus")
    /// Determines which server implementation to use
    #[serde(default = "default_server_type")]
    pub server_type: String,
    /// Transport type: "stdio" or "http"
    #[serde(default = "default_transport")]
    pub transport: String,
    /// Bind address for HTTP transport (e.g., "127.0.0.1:8000")
    /// Only used when transport is "http" and starting a local server
    /// Mutually exclusive with `url`
    #[serde(default = "default_bind")]
    pub bind: String,
    /// External server URL (e.g., "https://mcp.context7.com/mcp")
    /// Only used when transport is "http" and connecting to an external server
    /// Mutually exclusive with `bind`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// HTTP path endpoint (default: "/mcp")
    /// Only used when transport is "http" and using `bind` (local server)
    #[serde(default = "default_path")]
    pub path: String,
    /// Custom HTTP headers to include in requests
    /// Only used when connecting to external servers via `url`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headers: Option<std::collections::HashMap<String, String>>,
}

fn default_server_type() -> String {
    "nexus".to_string()
}

fn default_transport() -> String {
    "stdio".to_string()
}

fn default_bind() -> String {
    "0.0.0.0:8000".to_string()
}

fn default_path() -> String {
    "/mcp".to_string()
}

/// Root configuration structure for multiple servers
#[derive(Debug, Serialize, Deserialize)]
pub struct MultiServerConfig {
    /// List of server configurations
    pub servers: Vec<ServerConfig>,
}

impl ServerConfig {
    /// Parse bind address, returning an error if invalid
    #[must_use = "this returns the parsed address, it doesn't modify self"]
    pub fn parse_bind_addr(&self) -> Result<SocketAddr, NexusError> {
        self.bind
            .parse()
            .map_err(|e| NexusError::Config(format!("Invalid bind address '{}': {}", self.bind, e)))
    }

    /// Validate the configuration
    #[must_use = "validation result must be checked"]
    pub fn validate(&self) -> Result<(), NexusError> {
        match self.transport.as_str() {
            "stdio" => Ok(()),
            "http" => {
                // For HTTP, either bind (local server) or url (external server) must be specified
                if self.url.is_some() && !self.bind.is_empty() && self.bind != default_bind() {
                    return Err(NexusError::Config(format!(
                        "Server '{}': Cannot specify both 'url' and 'bind' - use 'url' for external servers, 'bind' for local servers",
                        self.name
                    )));
                }

                if self.url.is_none() {
                    // Local server - validate bind address
                    self.parse_bind_addr()?;
                    if self.path.is_empty() {
                        return Err(NexusError::Config(
                            "Path cannot be empty for HTTP transport".to_string(),
                        ));
                    }
                } else {
                    // External server - validate URL
                    let url_str = self.url.as_ref().unwrap();
                    if !url_str.starts_with("http://") && !url_str.starts_with("https://") {
                        return Err(NexusError::Config(format!(
                            "Server '{}': URL must start with 'http://' or 'https://'",
                            self.name
                        )));
                    }
                }
                Ok(())
            }
            _ => Err(NexusError::Config(format!(
                "Invalid transport type: {}. Must be 'stdio' or 'http'",
                self.transport
            ))),
        }
    }

    /// Check if this is an external server (has URL)
    #[must_use]
    pub fn is_external(&self) -> bool {
        self.url.is_some()
    }
}

impl ServerConfig {
    /// Expand environment variables in header values
    /// Supports both ${VAR_NAME} and $VAR_NAME syntax
    pub fn expand_env_vars(&mut self) {
        if let Some(ref mut headers) = self.headers {
            let mut expanded_headers = std::collections::HashMap::new();
            for (key, value) in headers.iter() {
                let expanded_value = expand_env_var(value);
                expanded_headers.insert(key.clone(), expanded_value);
            }
            *headers = expanded_headers;
        }
    }
}

/// Expand environment variable references in a string
/// Supports ${VAR_NAME} and $VAR_NAME syntax
fn expand_env_var(value: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"\$\{?([a-zA-Z_][a-zA-Z0-9_]*)\}?").unwrap());

    re.replace_all(value, |caps: &Captures| {
        let var_name = &caps[1];
        std::env::var(var_name).unwrap_or_else(|_| {
            eprintln!(
                "Warning: Environment variable '{}' not found, using empty string",
                var_name
            );
            String::new()
        })
    })
    .to_string()
}

impl MultiServerConfig {
    /// Load configuration from a TOML file
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, NexusError> {
        let contents = std::fs::read_to_string(path.as_ref())
            .map_err(|e| NexusError::Config(format!("Failed to read config file: {}", e)))?;
        let mut config: MultiServerConfig = toml::from_str(&contents)?;

        // Expand environment variables in headers
        for server in &mut config.servers {
            server.expand_env_vars();
        }

        config.validate()?;
        Ok(config)
    }

    /// Validate all server configurations
    pub fn validate(&self) -> Result<(), NexusError> {
        if self.servers.is_empty() {
            return Err(NexusError::Config(
                "At least one server configuration is required".to_string(),
            ));
        }

        // Check for duplicate names
        let mut names = std::collections::HashSet::new();
        for server in &self.servers {
            if names.contains(&server.name) {
                return Err(NexusError::Config(format!(
                    "Duplicate server name: {}",
                    server.name
                )));
            }
            names.insert(&server.name);
            server.validate()?;
        }

        // Check for duplicate bind addresses (for local HTTP servers)
        let mut bind_addrs = std::collections::HashSet::new();
        for server in &self.servers {
            if server.transport == "http" && !server.is_external() {
                let addr = server.parse_bind_addr()?;
                if bind_addrs.contains(&addr) {
                    return Err(NexusError::Config(format!(
                        "Duplicate bind address {} for server '{}'",
                        addr, server.name
                    )));
                }
                bind_addrs.insert(addr);
            }
        }

        Ok(())
    }
}
