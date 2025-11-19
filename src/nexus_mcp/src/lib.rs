pub mod cli;
pub mod codegen;
pub mod config;
pub mod external_servers;
pub mod mcp_client;
pub mod schema;
pub mod server;

pub use cli::{run_generate_code, run_generate_external, run_shell, run_start_servers, Cli, Commands};
pub use codegen::CodeGenerator;
pub use config::{MultiServerConfig, ServerConfig};
pub use external_servers::generate_external_server_tools;
pub use server::NexusMcpServer;
