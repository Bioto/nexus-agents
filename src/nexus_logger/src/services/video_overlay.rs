use chrono::{DateTime, Utc};
use std::sync::mpsc;

/// Overlay information to be drawn on video frames
#[derive(Debug, Clone)]
pub struct VideoOverlay {
    /// Timestamp to display (in seconds from recording start)
    pub timestamp: f64,
    /// Optional label/annotation text
    pub label: Option<String>,
    /// X position for label (None = auto-position)
    pub label_x: Option<u32>,
    /// Y position for label (None = auto-position)
    pub label_y: Option<u32>,
}

/// Channel for sending overlay information to the video encoder
pub type OverlaySender = mpsc::Sender<VideoOverlay>;
pub type OverlayReceiver = mpsc::Receiver<VideoOverlay>;

/// Creates a channel for overlay communication
pub fn create_overlay_channel() -> (OverlaySender, OverlayReceiver) {
    mpsc::channel()
}

/// Helper to format timestamp as HH:MM:SS.mmm
pub fn format_timestamp(seconds: f64) -> String {
    let total_seconds = seconds as u64;
    let hours = total_seconds / 3600;
    let minutes = (total_seconds % 3600) / 60;
    let secs = total_seconds % 60;
    let millis = ((seconds - total_seconds as f64) * 1000.0) as u32;
    
    format!("{:02}:{:02}:{:02}.{:03}", hours, minutes, secs, millis)
}

/// Get current overlay information for a given video timestamp
pub fn get_current_overlay(
    receiver: &OverlayReceiver,
    video_timestamp: f64,
    _recording_start: DateTime<Utc>,
) -> VideoOverlay {
    // Try to receive any pending overlay updates (non-blocking)
    let mut latest_overlay = VideoOverlay {
        timestamp: video_timestamp,
        label: None,
        label_x: None,
        label_y: None,
    };

    // Collect all pending overlays and use the latest one
    while let Ok(overlay) = receiver.try_recv() {
        if overlay.timestamp <= video_timestamp {
            latest_overlay = overlay;
        }
    }

    // Always update the timestamp to current video time
    latest_overlay.timestamp = video_timestamp;
    latest_overlay
}

