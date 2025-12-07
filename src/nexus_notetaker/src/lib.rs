//! Nexus Notetaker - processes recorded sessions and produces structured notes.
//!
//! This crate intentionally does not start recordings; it consumes session data
//! produced by the unified recorder and summarizes it via `nexus_core`.

pub mod cli;
pub mod services;

pub use cli::{run_process, Cli, Commands};
pub use services::{NotetakerService, SessionData};

/// Common result type for the notetaker crate.
pub type Result<T> = anyhow::Result<T>;
