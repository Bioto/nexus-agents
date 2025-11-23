use std::env;

/// Configuration for Postgres connection
#[derive(Debug, Clone)]
pub struct PostgresConfig {
    /// Postgres server host
    pub host: String,
    /// Postgres server port (default: 5432)
    pub port: u16,
    /// Database name
    pub database: String,
    /// Username
    pub username: String,
    /// Password
    pub password: String,
    /// Connection pool size (default: 10)
    pub pool_size: u32,
}

impl PostgresConfig {
    /// Create a new configuration with default values
    pub fn new(host: impl Into<String>) -> Self {
        Self {
            host: host.into(),
            port: 5432,
            database: "postgres".to_string(),
            username: "postgres".to_string(),
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
    pub fn with_pool_size(mut self, pool_size: u32) -> Self {
        self.pool_size = pool_size;
        self
    }

    /// Create configuration from environment variables
    ///
    /// Reads the following environment variables:
    /// - `POSTGRES_HOST` (default: "localhost")
    /// - `POSTGRES_PORT` (default: 5432)
    /// - `POSTGRES_DATABASE` (default: "postgres")
    /// - `POSTGRES_USER` or `POSTGRES_USERNAME` (default: "postgres")
    /// - `POSTGRES_PASSWORD` (default: "")
    /// - `POSTGRES_POOL_SIZE` (default: 10)
    pub fn from_env() -> Self {
        Self {
            host: env::var("POSTGRES_HOST").unwrap_or_else(|_| "localhost".to_string()),
            port: env::var("POSTGRES_PORT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(5432),
            database: env::var("POSTGRES_DATABASE")
                .unwrap_or_else(|_| "postgres".to_string()),
            username: env::var("POSTGRES_USER")
                .or_else(|_| env::var("POSTGRES_USERNAME"))
                .unwrap_or_else(|_| "postgres".to_string()),
            password: env::var("POSTGRES_PASSWORD").unwrap_or_default(),
            pool_size: env::var("POSTGRES_POOL_SIZE")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(10),
        }
    }

    /// Build the connection URL
    ///
    /// Format: postgresql://[username:password@]host:port/database
    pub fn connection_url(&self) -> String {
        if self.password.is_empty() {
            format!(
                "postgresql://{}@{}:{}/{}",
                self.username, self.host, self.port, self.database
            )
        } else {
            format!(
                "postgresql://{}:{}@{}:{}/{}",
                self.username, self.password, self.host, self.port, self.database
            )
        }
    }
}

