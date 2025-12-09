//! Postgres storage primitives shared across Nexus services.

pub mod config;
pub mod database;

pub use config::PostgresConfig;
pub use database::Database;
