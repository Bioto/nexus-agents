use crate::error::{Result, ToolboxError};
use sqlx::{postgres::PgPoolOptions, PgPool};
use std::sync::Arc;

use super::config::PostgresConfig;

/// Database connection pool wrapper
#[derive(Clone)]
pub struct Database {
    pool: Arc<PgPool>,
}

impl Database {
    /// Create a new database instance with Postgres service from environment
    pub async fn new() -> Result<Self> {
        let config = PostgresConfig::from_env();
        Self::with_config(config).await
    }

    /// Create a new database instance with custom Postgres configuration
    pub async fn with_config(config: PostgresConfig) -> Result<Self> {
        let connection_url = config.connection_url();
        eprintln!("Connecting to Postgres at: {}", connection_url);

        let pool = PgPoolOptions::new()
            .max_connections(config.pool_size)
            .connect(&connection_url)
            .await
            .map_err(|e| {
                ToolboxError::Configuration(format!("Failed to create Postgres pool: {}", e))
            })?;

        // Run migrations
        // Note: Migration path is relative to the crate root (src/x_toolbox/)
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .map_err(|e| {
                ToolboxError::Configuration(format!("Failed to run migrations: {}", e))
            })?;

        Ok(Self {
            pool: Arc::new(pool),
        })
    }

    /// Get a reference to the connection pool
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }
}

