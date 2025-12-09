//! Video overlay services.
//!
//! This module provides video overlay functionality for annotations and timestamps.

pub mod video_overlay;

// Re-exports for convenience
pub use video_overlay::{
    create_overlay_channel, format_timestamp, get_current_overlay, OverlayReceiver, OverlaySender,
    VideoOverlay,
};
