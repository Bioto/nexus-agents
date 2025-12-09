//! Configuration for the fitness module.
//!
//! The fitness module shares database configuration with the nutrition module,
//! as both use the same PostgreSQL database instance.

use std::env;

/// Get the database URL from environment variables.
/// Falls back to the same defaults as the nutrition module.
pub fn get_database_url() -> String {
    if let Ok(url) = env::var("DATABASE_URL") {
        return url;
    }

    let host = env::var("POSTGRES_HOST").unwrap_or_else(|_| "localhost".to_string());
    let port = env::var("POSTGRES_PORT").unwrap_or_else(|_| "5432".to_string());
    let database = env::var("POSTGRES_DATABASE").unwrap_or_else(|_| "nutrition".to_string());
    let user = env::var("POSTGRES_USER").unwrap_or_else(|_| "postgres".to_string());
    let password = env::var("POSTGRES_PASSWORD").unwrap_or_else(|_| "postgres".to_string());

    format!(
        "postgresql://{}:{}@{}:{}/{}",
        user, password, host, port, database
    )
}

/// Default MCP server port for the fitness module
pub const DEFAULT_MCP_PORT: u16 = 8082;

/// Default HTTP server port for the fitness REST API
pub const DEFAULT_HTTP_PORT: u16 = 8081;
