//! Example: Using UnifiedRecordingService with custom event callbacks
//!
//! This example demonstrates how to:
//! 1. Create a unified recording service
//! 2. Implement custom event callbacks
//! 3. Timestamp video based on events
//! 4. Start and stop recording

use nexus_logger::{
    EventCallback, InputCaptureConfig, InputEvent, ScreenRecordingConfig, UnifiedRecordingConfig,
    UnifiedRecordingService,
};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Custom callback that timestamps video based on keyboard events
struct VideoTimestampCallback {
    /// Track video timestamps for important events
    important_events: Vec<(String, f64)>,
}

impl EventCallback for VideoTimestampCallback {
    fn on_keyboard_event(
        &self,
        event: &InputEvent,
        video_timestamp: f64,
        _recording_start: chrono::DateTime<chrono::Utc>,
    ) -> bool {
        if let InputEvent::Keyboard { key, pressed, .. } = event {
            if *pressed {
                // Example: Mark important keys (like Enter, Escape, etc.)
                if key == "Enter" || key == "Escape" || key == "Space" {
                    println!(
                        "🎬 Video timestamp {}s: Key '{}' pressed",
                        video_timestamp, key
                    );
                    // In a real implementation, you would:
                    // 1. Store this timestamp in your video metadata
                    // 2. Create chapter markers
                    // 3. Add annotations to the video
                }
            }
        }
        true // Store all events
    }

    fn on_mouse_event(
        &self,
        event: &InputEvent,
        video_timestamp: f64,
        _recording_start: chrono::DateTime<chrono::Utc>,
    ) -> bool {
        if let InputEvent::Mouse {
            event_type, button, ..
        } = event
        {
            if event_type == "click" {
                println!(
                    "🎬 Video timestamp {}s: Mouse {} clicked",
                    video_timestamp,
                    button.as_ref().unwrap_or(&"unknown".to_string())
                );
            }
        }
        true // Store all events
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create configuration
    let config = UnifiedRecordingConfig {
        screen_config: ScreenRecordingConfig {
            output_path: PathBuf::from("example_recording.mp4"),
            framerate: 30,
            duration_secs: None, // Record until stopped
            monitor_index: None, // Primary monitor
            include_audio: true,
        },
        input_config: InputCaptureConfig {
            output_file: Some(PathBuf::from("example_events.json")),
            format: "json".to_string(),
        },
        database_path: PathBuf::from("example_events.db"),
        capture_keyboard: true,
        capture_mouse: true,
        capture_mouse_moves: false, // Disable to reduce noise
        show_timestamp: true,
        show_labels: true,
        context_fps: Some(1.0),
    };

    // Create callback
    let callback = VideoTimestampCallback {
        important_events: Vec::new(),
    };

    // Create service with custom callback
    let service = UnifiedRecordingService::with_callback(config, callback);

    // Create stop signal
    let stop_signal = Arc::new(AtomicBool::new(false));
    let stop_signal_clone = stop_signal.clone();

    // Handle Ctrl+C
    ctrlc::set_handler(move || {
        println!("\n🛑 Stopping recording...");
        stop_signal_clone.store(true, Ordering::SeqCst);
    })?;

    println!("🎬 Starting unified recording...");
    println!("   Video: example_recording.mp4");
    println!("   Events: example_events.json");
    println!("   Database: example_events.db");
    println!("\nPress Ctrl+C to stop\n");

    // Start recording
    let session = service.start_recording(stop_signal.clone()).await?;
    let session_id = session.session_id().to_string();
    let recording_start = session.recording_start();

    // Wait for recording to complete
    session.wait().await?;

    println!("\n✅ Recording complete!");
    println!("   Session ID: {}", session_id);
    println!("   Started at: {}", recording_start);

    Ok(())
}
