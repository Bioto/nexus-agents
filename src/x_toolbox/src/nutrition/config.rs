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
    /// Original DATABASE_URL if provided (preserves query params like sslmode)
    raw_url: Option<String>,
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
            raw_url: None,
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
    /// If `DATABASE_URL` is set, it takes precedence and is used directly.
    /// Otherwise, reads the following environment variables:
    /// - `POSTGRES_HOST` (default: "localhost")
    /// - `POSTGRES_PORT` (default: 5432)
    /// - `POSTGRES_DATABASE` (default: "postgres")
    /// - `POSTGRES_USER` or `POSTGRES_USERNAME` (default: "postgres")
    /// - `POSTGRES_PASSWORD` (default: "")
    /// - `POSTGRES_POOL_SIZE` (default: 10)
    pub fn from_env() -> Self {
        // Check for DATABASE_URL first - if set, parse it
        if let Ok(url) = env::var("DATABASE_URL") {
            if let Some(config) = Self::from_url(&url) {
                return config;
            }
            eprintln!("[WARN] Failed to parse DATABASE_URL, falling back to individual env vars");
        }

        Self {
            host: env::var("POSTGRES_HOST").unwrap_or_else(|_| "localhost".to_string()),
            port: env::var("POSTGRES_PORT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(5432),
            database: env::var("POSTGRES_DATABASE").unwrap_or_else(|_| "postgres".to_string()),
            username: env::var("POSTGRES_USER")
                .or_else(|_| env::var("POSTGRES_USERNAME"))
                .unwrap_or_else(|_| "postgres".to_string()),
            password: env::var("POSTGRES_PASSWORD").unwrap_or_default(),
            pool_size: env::var("POSTGRES_POOL_SIZE")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(10),
            raw_url: None,
        }
    }

    /// Parse a DATABASE_URL into configuration
    ///
    /// Supports format: postgresql://[user[:password]@]host[:port]/database[?params]
    pub fn from_url(original_url: &str) -> Option<Self> {
        // Store the original URL for later use (preserves query params)
        let raw_url = original_url.to_string();

        // Remove the scheme prefix for parsing
        let url = original_url
            .strip_prefix("postgresql://")
            .or_else(|| original_url.strip_prefix("postgres://"))?;

        // Split off query params (we'll preserve them via raw_url)
        let (main_part, _query) = url.split_once('?').unwrap_or((url, ""));

        // Parse user:password@host:port/database
        let (auth_host, database) = main_part.rsplit_once('/')?;
        let database = database.to_string();

        let (auth, host_port) = if auth_host.contains('@') {
            let (auth, hp) = auth_host.rsplit_once('@')?;
            (Some(auth), hp)
        } else {
            (None, auth_host)
        };

        // Parse host:port
        let (host, port) = if host_port.contains(':') {
            let (h, p) = host_port.rsplit_once(':')?;
            (h.to_string(), p.parse().ok()?)
        } else {
            (host_port.to_string(), 5432)
        };

        // Parse user:password
        let (username, password) = if let Some(auth) = auth {
            if auth.contains(':') {
                let (u, p) = auth.split_once(':')?;
                (u.to_string(), p.to_string())
            } else {
                (auth.to_string(), String::new())
            }
        } else {
            ("postgres".to_string(), String::new())
        };

        let pool_size = env::var("POSTGRES_POOL_SIZE")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(10);

        Some(Self {
            host,
            port,
            database,
            username,
            password,
            pool_size,
            raw_url: Some(raw_url),
        })
    }

    /// Parse a DATABASE_URL string directly (for external callers)
    pub fn from_database_url(url: &str) -> Option<Self> {
        // Prepend the scheme if needed
        let full_url = if url.starts_with("postgresql://") || url.starts_with("postgres://") {
            url.to_string()
        } else {
            format!("postgresql://{}", url)
        };
        Self::from_url(&full_url)
    }

    /// Build the connection URL
    ///
    /// If created from DATABASE_URL, returns the original URL (preserving query params).
    /// Otherwise builds: postgresql://[username:password@]host:port/database
    pub fn connection_url(&self) -> String {
        // Use the original URL if available (preserves query params like sslmode)
        if let Some(ref url) = self.raw_url {
            return url.clone();
        }

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
