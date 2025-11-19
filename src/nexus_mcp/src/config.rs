use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::path::Path;

/// Configuration for a single MCP server instance
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    /// Server name/identifier
    pub name: String,
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
    pub fn parse_bind_addr(&self) -> Result<SocketAddr, String> {
        self.bind
            .parse()
            .map_err(|e| format!("Invalid bind address '{}': {}", self.bind, e))
    }

    /// Validate the configuration
    pub fn validate(&self) -> Result<(), String> {
        match self.transport.as_str() {
            "stdio" => Ok(()),
            "http" => {
                // For HTTP, either bind (local server) or url (external server) must be specified
                if self.url.is_some() && !self.bind.is_empty() && self.bind != default_bind() {
                    return Err(format!(
                        "Server '{}': Cannot specify both 'url' and 'bind' - use 'url' for external servers, 'bind' for local servers",
                        self.name
                    ));
                }
                
                if self.url.is_none() {
                    // Local server - validate bind address
                    self.parse_bind_addr()?;
                    if self.path.is_empty() {
                        return Err("Path cannot be empty for HTTP transport".to_string());
                    }
                } else {
                    // External server - validate URL
                    let url_str = self.url.as_ref().unwrap();
                    if !url_str.starts_with("http://") && !url_str.starts_with("https://") {
                        return Err(format!(
                            "Server '{}': URL must start with 'http://' or 'https://'",
                            self.name
                        ));
                    }
                }
                Ok(())
            }
            _ => Err(format!(
                "Invalid transport type: {}. Must be 'stdio' or 'http'",
                self.transport
            )),
        }
    }
    
    /// Check if this is an external server (has URL)
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
    let mut result = String::new();
    let mut chars = value.chars().peekable();
    
    while let Some(ch) = chars.next() {
        if ch == '$' {
            // Check for ${VAR_NAME} syntax
            if chars.peek() == Some(&'{') {
                chars.next(); // consume '{'
                let mut var_name = String::new();
                while let Some(ch) = chars.next() {
                    if ch == '}' {
                        break;
                    }
                    var_name.push(ch);
                }
                // Get environment variable value
                let env_value = std::env::var(&var_name)
                    .unwrap_or_else(|_| {
                        eprintln!("Warning: Environment variable '{}' not found, using empty string", var_name);
                        String::new()
                    });
                result.push_str(&env_value);
            } else {
                // Check for $VAR_NAME syntax (simple form)
                let mut var_name = String::new();
                let mut found_var = false;
                while let Some(&ch) = chars.peek() {
                    if ch.is_alphanumeric() || ch == '_' {
                        var_name.push(ch);
                        chars.next();
                        found_var = true;
                    } else {
                        break;
                    }
                }
                if found_var {
                    let env_value = std::env::var(&var_name)
                        .unwrap_or_else(|_| {
                            eprintln!("Warning: Environment variable '{}' not found, using empty string", var_name);
                            String::new()
                        });
                    result.push_str(&env_value);
                } else {
                    // Not a variable, just a literal $
                    result.push('$');
                }
            }
        } else {
            result.push(ch);
        }
    }
    
    result
}

impl MultiServerConfig {
    /// Load configuration from a TOML file
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, Box<dyn std::error::Error>> {
        let contents = std::fs::read_to_string(path.as_ref())
            .map_err(|e| format!("Failed to read config file: {}", e))?;
        let mut config: MultiServerConfig = toml::from_str(&contents)
            .map_err(|e| format!("Failed to parse config file: {}", e))?;
        
        // Expand environment variables in headers
        for server in &mut config.servers {
            server.expand_env_vars();
        }
        
        config.validate()?;
        Ok(config)
    }

    /// Validate all server configurations
    pub fn validate(&self) -> Result<(), String> {
        if self.servers.is_empty() {
            return Err("At least one server configuration is required".to_string());
        }

        // Check for duplicate names
        let mut names = std::collections::HashSet::new();
        for server in &self.servers {
            if names.contains(&server.name) {
                return Err(format!("Duplicate server name: {}", server.name));
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
                    return Err(format!(
                        "Duplicate bind address {} for server '{}'",
                        addr, server.name
                    ));
                }
                bind_addrs.insert(addr);
            }
        }

        Ok(())
    }
}

