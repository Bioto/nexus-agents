//! Shared storage module for Nexus projects.
//!
//! This crate centralizes storage primitives for both ClickHouse and Postgres
//! backends so they can be reused across binaries (recorder, notetaker,
//! nutrition tooling).

#[cfg(feature = "clickhouse")]
pub mod clickhouse;
pub mod error;
#[cfg(feature = "postgres")]
pub mod postgres;

pub use error::{Result, StorageError};
