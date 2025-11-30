//! Context processing services.
//!
//! This module provides AI-powered context analysis for user interactions,
//! including click context, frame processing, and webcam sentiment analysis.

pub mod click_context;
pub mod context_processing;
pub mod webcam_analysis;

// Re-exports for convenience
pub use click_context::{ClickContextEvent, ClickContextHandle, ClickContextService};
pub use context_processing::{ProcessingConfig, ProcessingHandle, ProcessingJob, ProcessingService};
pub use webcam_analysis::{WebcamAnalysisConfig, WebcamAnalysisHandle, WebcamAnalysisService};

