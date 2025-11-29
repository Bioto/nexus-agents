//! End-to-end test for optimized recording with file rotation and batch inserts.
//!
//! This example runs unified recording with:
//! - Rotating event writer (rotates every 10 seconds for testing)
//! - Batch database inserter (flushes every 1 second)
//! - Bounded event channels for backpressure
//!
//! Run with: cargo run --example test_optimized_recording -p nexus_logger

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
            event_type, button, x, y, ..
        } = event
        {
            if event_type == "click" {
                println!(
                    "🖱️  [{:.1}s] Click: {} at ({}, {})",
                    video_timestamp,
                    button.as_deref().unwrap_or("?"),
                    x.unwrap_or(0),
                    y.unwrap_or(0)
                );
            }
        }
        (true, None)
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create output directory
    let output_dir = PathBuf::from("/tmp/optimized_recording_test");
    std::fs::create_dir_all(&output_dir)?;

    println!("🧪 Optimized Recording Test");
    println!("═══════════════════════════════════════════════════════════════");
    println!("Output directory: {}", output_dir.display());
    println!();
    println!("This test runs unified recording with:");
    println!("  • Rotating event writer (rotates every 10 seconds)");
    println!("  • Batch database inserter (flushes every 1 second)");
    println!("  • Bounded event channels (10,000 capacity)");
    println!();
    println!("Watch the output directory for rotating files:");
    println!("  watch -n 1 'ls -la {}'", output_dir.display());
    println!();
    println!("Press Ctrl+C to stop recording");
    println!("═══════════════════════════════════════════════════════════════");
    println!();

    // Create configuration with optimizations enabled
    let config = UnifiedRecordingConfig {
        screen_config: ScreenRecordingConfig {
            output_path: output_dir.join("recording.mp4"),
            framerate: 30,
            duration_secs: None, // Record until stopped
            monitor_index: None, // Primary monitor
            include_audio: false, // Disable audio for simpler test
            segment_duration_secs: None, // No video segmentation (test event rotation only)
        },
        input_config: InputCaptureConfig {
            output_file: Some(output_dir.join("events.json")), // Legacy path (ignored when rotation enabled)
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

        // === OPTIMIZATIONS ENABLED ===
        
        // Rotating event writer - rotates every 10 seconds for testing
        event_writer_config: Some(EventWriterConfig {
            base_path: output_dir.join("events"),
            extension: "json".to_string(),
            rotation_interval: Duration::from_secs(10), // Rotate every 10 seconds!
            buffer_size: 4 * 1024, // 4KB buffer (smaller for faster flushes)
            flush_interval: Duration::from_millis(500), // Flush every 500ms
            compress_rotated: true, // Compress old files with gzip
            retention_days: None, // Keep all files
        }),

        // Batch database inserter - flushes every 1 second or 50 events
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

    println!("🎬 Starting optimized recording...");
    println!();

    // Start recording
    let session = service.start_recording(stop_signal.clone()).await?;
    let session_id = session.session_id().to_string();
    let recording_start = session.recording_start();

    println!("✅ Recording started!");
    println!("   Session ID: {}", session_id);
    println!("   Started at: {}", recording_start);
    println!();
    println!("📁 Files will rotate every 10 seconds.");
    println!("   Look for: events_YYYYMMDD_HHMMSS.json and .json.gz files");
    println!();

    // Wait for recording to complete
    session.wait().await?;

    println!();
    println!("✅ Recording complete!");
    println!();
    println!("📊 Check output files:");
    println!("   ls -la {}", output_dir.display());

    // List files in output directory
    println!();
    println!("Generated files:");
    for entry in std::fs::read_dir(&output_dir)? {
        let entry = entry?;
        let metadata = entry.metadata()?;
        let size = metadata.len();
        let name = entry.file_name();
        println!("   {:>10} bytes  {}", size, name.to_string_lossy());
    }

    Ok(())
}

