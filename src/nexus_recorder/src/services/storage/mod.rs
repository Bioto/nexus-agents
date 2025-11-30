//! Storage and database services.
//!
//! This module provides database operations, batch insertion, and event writing.

pub mod batch_inserter;
pub mod database;
pub mod rotating_writer;

// Re-exports for convenience
pub use batch_inserter::{BatchEvent, BatchEventInserter, BatchInserterConfig, BatchInserterHandle};
pub use database::{Database, TimelineEvent};
pub use rotating_writer::{EventWriterConfig, RotatingEventWriter, RotatingEventWriterHandle};

