//! End-to-end test for video segmentation using FFmpeg CLI.
//!
//! This example runs unified recording with:
//! - Video segmentation (creates new MP4 file every 30 seconds)
//! - Rotating event writer (rotates every 30 seconds)
//! - Both files rotate at the same interval for easy correlation
//!
//! Run with: cargo run --example test_video_segmentation -p nexus_logger

use nexus_logger::{
    BatchInserterConfig, EventCallback, EventWriterConfig, InputCaptureConfig, InputEvent,
    OverlayLabel, ScreenRecordingConfig, UnifiedRecordingConfig, UnifiedRecordingService,
};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Simple callback that logs events
struct TestCallback;

impl EventCallback for TestCallback {
    fn on_keyboard_event(
        &self,
        event: &InputEvent,
        video_timestamp: f64,
        _recording_start: chrono::DateTime<chrono::Utc>,
    ) -> (bool, Option<OverlayLabel>) {
        if let InputEvent::Keyboard { key, pressed, .. } = event {
            if *pressed {
                println!("⌨️  [{:.1}s] Key: {}", video_timestamp, key);
            }
        }
        (true, None)
    }

    fn on_mouse_event(
        &self,
        event: &InputEvent,
        video_timestamp: f64,
        _recording_start: chrono::DateTime<chrono::Utc>,
    ) -> (bool, Option<OverlayLabel>) {
        if let InputEvent::Mouse {
            event_type, button, ..
        } = event
        {
            if event_type == "click" {
                println!(
                    "🖱️  [{:.1}s] Click: {}",
                    video_timestamp,
                    button.as_deref().unwrap_or("?"),
                );
            }
        }
        (true, None)
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create output directory
    let output_dir = PathBuf::from("/tmp/video_segmentation_test");
    std::fs::create_dir_all(&output_dir)?;

    println!("🎬 Video Segmentation Test");
    println!("═══════════════════════════════════════════════════════════════");
    println!("Output directory: {}", output_dir.display());
    println!();
    println!("This test runs unified recording with:");
    println!("  • VIDEO SEGMENTATION: New MP4 every 30 seconds");
    println!("  • Event file rotation: New JSON every 30 seconds");
    println!("  • Batch database inserts");
    println!();
    println!("Watch the output directory for rotating files:");
    println!("  watch -n 1 'ls -la {}'", output_dir.display());
    println!();
    println!("Expected output after 2 minutes:");
    println!("  recording_000.mp4, recording_001.mp4, recording_002.mp4, ...");
    println!("  events_*.json.gz (compressed old files)");
    println!();
    println!("Press Ctrl+C to stop recording");
    println!("═══════════════════════════════════════════════════════════════");
    println!();

    // Short segment duration for testing (30 seconds)
    let segment_duration_secs = 30;

    // Create configuration with video segmentation enabled
    let config = UnifiedRecordingConfig {
        screen_config: ScreenRecordingConfig {
            output_path: output_dir.join("recording.mp4"),
            framerate: 30,
            duration_secs: None,  // Record until stopped
            monitor_index: None,  // Primary monitor
            include_audio: false, // Disable audio for simpler test
            segment_duration_secs: Some(segment_duration_secs), // VIDEO SEGMENTATION ENABLED!
        },
        input_config: InputCaptureConfig {
            output_file: Some(output_dir.join("events.json")),
            format: "json".to_string(),
        },
        audio_configs: Vec::new(),
        database_path: output_dir.join("events.db"),
        capture_keyboard: true,
        capture_mouse: true,
        capture_mouse_moves: false,
        show_timestamp: false,
        show_labels: false,
        context_fps: None,

        // Rotating event writer - same interval as video segments
        event_writer_config: Some(EventWriterConfig {
            base_path: output_dir.join("events"),
            extension: "json".to_string(),
            rotation_interval: Duration::from_secs(segment_duration_secs),
            buffer_size: 4 * 1024,
            flush_interval: Duration::from_millis(500),
            compress_rotated: true,
            retention_days: None,
        }),

        // Batch database inserter
        batch_inserter_config: Some(BatchInserterConfig {
            batch_size: 50,
            flush_interval: Duration::from_secs(1),
            channel_size: 10_000,
        }),
    };

    // Create callback
    let callback = TestCallback;

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

    println!("🎬 Starting segmented recording...");
    println!("   Video segments every {} seconds", segment_duration_secs);
    println!();

    // Start recording
    let session = service.start_recording(stop_signal.clone()).await?;
    let session_id = session.session_id().to_string();
    let recording_start = session.recording_start();

    println!("✅ Recording started!");
    println!("   Session ID: {}", session_id);
    println!("   Started at: {}", recording_start);
    println!();

    // Wait for recording to complete
    session.wait().await?;

    println!();
    println!("✅ Recording complete!");
    println!();
    println!("📊 Generated files:");

    // List files in output directory
    let mut files: Vec<_> = std::fs::read_dir(&output_dir)?
        .filter_map(|e| e.ok())
        .collect();
    files.sort_by_key(|e| e.file_name());

    for entry in files {
        let metadata = entry.metadata()?;
        let size = metadata.len();
        let name = entry.file_name();
        let size_str = if size > 1024 * 1024 {
            format!("{:.1} MB", size as f64 / 1024.0 / 1024.0)
        } else if size > 1024 {
            format!("{:.1} KB", size as f64 / 1024.0)
        } else {
            format!("{} bytes", size)
        };
        println!("   {:>10}  {}", size_str, name.to_string_lossy());
    }

    Ok(())
}
