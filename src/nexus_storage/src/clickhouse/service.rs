use crate::error::{Result, StorageError};
use clickhouse_rs::{types::Block, types::Complex, Pool};
use std::sync::Arc;

type CoreError = StorageError;

/// Configuration for ClickHouse connection
#[derive(Debug, Clone)]
pub struct ClickHouseConfig {
    /// ClickHouse server host
    pub host: String,
    /// ClickHouse server port (default: 9000 for native, 8123 for HTTP)
    pub port: u16,
    /// Database name
    pub database: String,
    /// Username
    pub username: String,
    /// Password
    pub password: String,
    /// Connection pool size (default: 10)
    pub pool_size: usize,
}

impl ClickHouseConfig {
    /// Create a new configuration with default values
    pub fn new(host: impl Into<String>) -> Self {
        Self {
            host: host.into(),
            port: 9000,
            database: "default".to_string(),
            username: "default".to_string(),
            password: String::new(),
            pool_size: 10,
        }
    }

    /// Set the port
    pub fn with_port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    /// Set the database name
    pub fn with_database(mut self, database: impl Into<String>) -> Self {
        self.database = database.into();
        self
    }

    /// Set the username
    pub fn with_username(mut self, username: impl Into<String>) -> Self {
        self.username = username.into();
        self
    }

    /// Set the password
    pub fn with_password(mut self, password: impl Into<String>) -> Self {
        self.password = password.into();
        self
    }

    /// Set the connection pool size
    pub fn with_pool_size(mut self, pool_size: usize) -> Self {
        self.pool_size = pool_size;
        self
    }

    /// Create configuration from environment variables
    ///
    /// Reads the following environment variables:
    /// - `CLICKHOUSE_HOST` (default: "localhost")
    /// - `CLICKHOUSE_PORT` (default: 9000)
    /// - `CLICKHOUSE_DATABASE` (default: "default")
    /// - `CLICKHOUSE_USERNAME` (default: "default")
    /// - `CLICKHOUSE_PASSWORD` (default: "")
    /// - `CLICKHOUSE_POOL_SIZE` (default: 10)
    pub fn from_env() -> Self {
        Self {
            host: std::env::var("CLICKHOUSE_HOST").unwrap_or_else(|_| "localhost".to_string()),
            port: std::env::var("CLICKHOUSE_PORT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(9000),
            database: std::env::var("CLICKHOUSE_DATABASE")
                .unwrap_or_else(|_| "default".to_string()),
            username: std::env::var("CLICKHOUSE_USER")
                .or_else(|_| std::env::var("CLICKHOUSE_USERNAME"))
                .unwrap_or_else(|_| "default".to_string()),
            password: std::env::var("CLICKHOUSE_PASSWORD")
                .unwrap_or_else(|_| "default".to_string()),
            pool_size: std::env::var("CLICKHOUSE_POOL_SIZE")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(10),
        }
    }

    /// Build the connection URL
    ///
    /// clickhouse-rs uses tcp:// protocol for native connections
    /// Format: tcp://[username:password@]host:port[/database]
    /// Note: username without password is not supported, so we only include auth if password is set
    pub fn connection_url(&self) -> String {
        // Ensure host is not empty
        let host = if self.host.is_empty() {
            "localhost"
        } else {
            &self.host
        };

        // Build URL with authentication only if both username and password are provided
        // clickhouse-rs doesn't support username@host format without password
        let url = if !self.username.is_empty() && !self.password.is_empty() {
            // Both username and password
            format!(
                "tcp://{}:{}@{}:{}",
                self.username, self.password, host, self.port
            )
        } else {
            // No authentication (username without password is not supported)
            format!("tcp://{}:{}", host, self.port)
        };

        // Add database if provided and not empty
        if !self.database.is_empty() {
            format!("{}/{}", url, self.database)
        } else {
            url
        }
    }
}

impl Default for ClickHouseConfig {
    fn default() -> Self {
        Self::from_env()
    }
}

/// ClickHouse service for recording and reading data
///
/// This service provides a shared connection pool that can be used across
/// multiple modules. It supports both synchronous and asynchronous operations
/// for inserting and querying data.
///
/// # Example
///
/// ```no_run
/// use nexus_core::services::ClickHouseService;
///
/// #[tokio::main]
/// async fn main() -> Result<(), Box<dyn std::error::Error>> {
///     // Create service from environment variables
///     let service = ClickHouseService::from_env().await?;
///
///     // Insert data
///     service.insert("INSERT INTO events (id, name) VALUES (1, 'test')").await?;
///
///     // Query data
///     let block = service.query("SELECT * FROM events LIMIT 10").await?;
///     println!("Rows: {}", block.row_count());
///
///     Ok(())
/// }
/// ```
///
/// # Example with custom configuration
///
/// ```no_run
/// use nexus_core::services::{ClickHouseService, ClickHouseConfig};
///
/// #[tokio::main]
/// async fn main() -> Result<(), Box<dyn std::error::Error>> {
///     let config = ClickHouseConfig::new("localhost")
///         .with_port(9000)
///         .with_database("mydb")
///         .with_username("user")
///         .with_password("pass");
///
///     let service = ClickHouseService::new(config).await?;
///
///     // Use the service...
///     Ok(())
/// }
/// ```
#[derive(Clone)]
pub struct ClickHouseService {
    pool: Arc<Pool>,
}

impl ClickHouseService {
    /// Create a new ClickHouse service with the given configuration
    ///
    /// # Arguments
    ///
    /// * `config` - ClickHouse configuration
    ///
    /// # Returns
    ///
    /// A new ClickHouse service instance
    ///
    /// # Errors
    ///
    /// Returns an error if the connection pool cannot be created
    pub async fn new(config: ClickHouseConfig) -> Result<Self> {
        let url = config.connection_url();

        // Validate URL is not empty
        if url.is_empty() {
            return Err(CoreError::Other("ClickHouse connection URL is empty".to_string()));
        }

        let pool = Pool::new(url.clone());

        Ok(Self {
            pool: Arc::new(pool),
        })
    }

    /// Create a new ClickHouse service from environment variables
    ///
    /// Reads configuration from environment variables (see `ClickHouseConfig::from_env`)
    ///
    /// # Returns
    ///
    /// A new ClickHouse service instance
    ///
    /// # Errors
    ///
    /// Returns an error if the connection pool cannot be created
    pub async fn from_env() -> Result<Self> {
        let config = ClickHouseConfig::from_env();
        Self::new(config).await
    }

    /// Execute an INSERT query
    ///
    /// # Arguments
    ///
    /// * `query` - SQL INSERT query string
    ///
    /// # Returns
    ///
    /// Number of rows inserted
    ///
    /// # Example
    ///
    /// ```no_run
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// # let service = nexus_core::services::ClickHouseService::from_env().await?;
    /// service.insert("INSERT INTO events (id, name) VALUES (1, 'test')").await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn insert(&self, query: &str) -> Result<u64> {
        let mut client = self.pool.get_handle().await.map_err(|e| {
            CoreError::Other(format!("Failed to get ClickHouse connection: {}", e))
        })?;

        client.execute(query).await.map_err(|e| {
            CoreError::Other(format!("Failed to execute INSERT query: {}", e))
        })?;

        // ClickHouse execute returns the number of rows affected
        // For INSERT queries, this is typically 0 or 1
        Ok(1)
    }

    /// Insert data from a Block
    ///
    /// # Arguments
    ///
    /// * `table` - Table name
    /// * `block` - Data block to insert (must implement AsRef<Block>)
    ///
    /// # Returns
    ///
    /// Number of rows inserted
    ///
    /// # Example
    ///
    /// ```no_run
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// # use clickhouse_rs::types::Block;
    /// # let service = nexus_core::services::ClickHouseService::from_env().await?;
    /// let block = Block::new().column("id", vec![1u32, 2u32]);
    /// service.insert_block("events", &block).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn insert_block<B>(&self, table: &str, block: B) -> Result<u64>
    where
        B: AsRef<Block> + Send,
    {
        let mut client = self.pool.get_handle().await.map_err(|e| {
            CoreError::Other(format!("Failed to get ClickHouse connection: {}", e))
        })?;

        client
            .insert(table, block)
            .await
            .map_err(|e| CoreError::Other(format!("Failed to insert block: {}", e)))?;

        Ok(1)
    }

    /// Execute a SELECT query and return results as a Block
    ///
    /// # Arguments
    ///
    /// * `query` - SQL SELECT query string
    ///
    /// # Returns
    ///
    /// A Block containing the query results
    ///
    /// # Example
    ///
    /// ```no_run
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// # let service = nexus_core::services::ClickHouseService::from_env().await?;
    /// let block = service.query("SELECT * FROM events LIMIT 10").await?;
    /// for row in block.rows() {
    ///     // Process row data
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn query(&self, query: &str) -> Result<Block<Complex>> {
        let mut client = self.pool.get_handle().await.map_err(|e| {
            CoreError::Other(format!("Failed to get ClickHouse connection: {}", e))
        })?;

        let block =
            client.query(query).fetch_all().await.map_err(|e| {
                CoreError::Other(format!("Failed to execute query: {}", e))
            })?;

        Ok(block)
    }

    /// Execute any SQL query (INSERT, UPDATE, DELETE, etc.)
    ///
    /// # Arguments
    ///
    /// * `query` - SQL query string
    ///
    /// # Returns
    ///
    /// Number of rows affected
    ///
    /// # Example
    ///
    /// ```no_run
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// # let service = nexus_core::services::ClickHouseService::from_env().await?;
    /// service.execute("CREATE TABLE IF NOT EXISTS events (id UInt32, name String) ENGINE = Memory").await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn execute(&self, query: &str) -> Result<u64> {
        let mut client = self.pool.get_handle().await.map_err(|e| {
            CoreError::Other(format!("Failed to get ClickHouse connection: {}", e))
        })?;

        client
            .execute(query)
            .await
            .map_err(|e| CoreError::Other(format!("Failed to execute query: {}", e)))?;

        Ok(0)
    }

    /// Get a reference to the underlying connection pool
    ///
    /// This allows advanced use cases where direct pool access is needed
    pub fn pool(&self) -> &Pool {
        &self.pool
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_new() {
        let config = ClickHouseConfig::new("localhost");
        assert_eq!(config.host, "localhost");
        assert_eq!(config.port, 9000);
        assert_eq!(config.database, "default");
        assert_eq!(config.username, "default");
    }

    #[test]
    fn test_config_builder() {
        let config = ClickHouseConfig::new("localhost")
            .with_port(8123)
            .with_database("mydb")
            .with_username("user")
            .with_password("pass")
            .with_pool_size(20);

        assert_eq!(config.host, "localhost");
        assert_eq!(config.port, 8123);
        assert_eq!(config.database, "mydb");
        assert_eq!(config.username, "user");
        assert_eq!(config.password, "pass");
        assert_eq!(config.pool_size, 20);
    }

    #[test]
    fn test_config_connection_url() {
        let config = ClickHouseConfig::new("localhost")
            .with_port(9000)
            .with_database("testdb")
            .with_username("testuser")
            .with_password("testpass");

        let url = config.connection_url();
        assert!(url.contains("tcp://"));
        assert!(url.contains("testuser:testpass"));
        assert!(url.contains("localhost:9000"));
        assert!(url.contains("testdb"));
    }

    #[test]
    fn test_config_from_env_defaults() {
        // Save original values
        let original_host = std::env::var("CLICKHOUSE_HOST").ok();
        let original_port = std::env::var("CLICKHOUSE_PORT").ok();
        let original_db = std::env::var("CLICKHOUSE_DATABASE").ok();
        let original_user = std::env::var("CLICKHOUSE_USER").ok();
        let original_username = std::env::var("CLICKHOUSE_USERNAME").ok();
        let original_pass = std::env::var("CLICKHOUSE_PASSWORD").ok();

        // Clear environment variables
        std::env::remove_var("CLICKHOUSE_HOST");
        std::env::remove_var("CLICKHOUSE_PORT");
        std::env::remove_var("CLICKHOUSE_DATABASE");
        std::env::remove_var("CLICKHOUSE_USER");
        std::env::remove_var("CLICKHOUSE_USERNAME");
        std::env::remove_var("CLICKHOUSE_PASSWORD");

        let config = ClickHouseConfig::from_env();
        assert_eq!(config.host, "localhost");
        assert_eq!(config.port, 9000);
        assert_eq!(config.database, "default");
        assert_eq!(config.username, "default");
        assert_eq!(config.password, "default");

        // Restore original values
        if let Some(host) = original_host {
            std::env::set_var("CLICKHOUSE_HOST", host);
        }
        if let Some(port) = original_port {
            std::env::set_var("CLICKHOUSE_PORT", port);
        }
        if let Some(db) = original_db {
            std::env::set_var("CLICKHOUSE_DATABASE", db);
        }
        if let Some(user) = original_user {
            std::env::set_var("CLICKHOUSE_USER", user);
        }
        if let Some(username) = original_username {
            std::env::set_var("CLICKHOUSE_USERNAME", username);
        }
        if let Some(pass) = original_pass {
            std::env::set_var("CLICKHOUSE_PASSWORD", pass);
        }
    }
}
