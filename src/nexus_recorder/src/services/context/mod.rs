//! Context processing services.
//!
//! This module provides AI-powered context analysis for user interactions,
//! including click context and frame processing.

pub mod click_context;
pub mod context_processing;

// Re-exports for convenience
pub use click_context::{ClickContextEvent, ClickContextHandle, ClickContextService};
pub use context_processing::{ProcessingConfig, ProcessingHandle, ProcessingJob, ProcessingService};

