//! ClickHouse support is provided by the shared `nexus_storage` crate.
//!
//! Re-export the storage definitions to preserve the previous API surface for
//! consumers that referenced `nexus_core::services::ClickHouseService`.

pub use nexus_storage::clickhouse::{ClickHouseConfig, ClickHouseService};
