use crate::error::{LoggerError, Result};
use crate::services::capture::InputEvent;
use crate::services::database::Database;
use chrono::{DateTime, Local, Utc};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use uuid::Uuid;

/// Overlay label information
#[derive(Debug, Clone)]
pub struct OverlayLabel {
    /// Label text to display
    pub text: String,
    /// Video timestamp when this label should appear (seconds)
    pub timestamp: f64,
    /// Duration to show the label (seconds, None = show until next label)
    pub duration: Option<f64>,
    /// X position (None = auto)
    pub x: Option<u32>,
    /// Y position (None = auto)
    pub y: Option<u32>,
}

/// Callback trait for processing events during recording.
/// Implement this to receive events and timestamp video accordingly.
pub trait EventCallback: Send + Sync {
    /// Called when a keyboard event is captured.
    /// 
    /// # Arguments
    /// * `event` - The keyboard event
    /// * `video_timestamp` - Current video timestamp in seconds
    /// * `recording_start` - When recording started (for absolute time calculations)
    /// 
    /// Returns (should_store, optional_label) where:
    /// - should_store: true if the event should be stored, false to skip
    /// - optional_label: Some(label) to add overlay text, None for no overlay
    fn on_keyboard_event(
        &self,
        event: &InputEvent,
        video_timestamp: f64,
        recording_start: DateTime<Utc>,
    ) -> (bool, Option<OverlayLabel>);

    /// Called when a mouse event is captured.
    /// 
    /// # Arguments
    /// * `event` - The mouse event
    /// * `video_timestamp` - Current video timestamp in seconds
    /// * `recording_start` - When recording started (for absolute time calculations)
    /// 
    /// Returns (should_store, optional_label) where:
    /// - should_store: true if the event should be stored, false to skip
    /// - optional_label: Some(label) to add overlay text, None for no overlay
    fn on_mouse_event(
        &self,
        event: &InputEvent,
        video_timestamp: f64,
        recording_start: DateTime<Utc>,
    ) -> (bool, Option<OverlayLabel>);
}

/// Default callback implementation that accepts all events.
pub struct DefaultEventCallback;

impl EventCallback for DefaultEventCallback {
    fn on_keyboard_event(
        &self,
        event: &InputEvent,
        video_timestamp: f64,
        _recording_start: DateTime<Utc>,
    ) -> (bool, Option<OverlayLabel>) {
        // Generate labels for important keys
        if let InputEvent::Keyboard { key, pressed, .. } = event {
            if *pressed && (key == "Enter" || key == "Escape" || key == "Space" || key == "Tab") {
                return (true, Some(OverlayLabel {
                    text: format!("Key: {}", key),
                    timestamp: video_timestamp,
                    duration: Some(2.0),
                    x: None,
                    y: None,
                }));
            }
        }
        (true, None)
    }

    fn on_mouse_event(
        &self,
        event: &InputEvent,
        video_timestamp: f64,
        _recording_start: DateTime<Utc>,
    ) -> (bool, Option<OverlayLabel>) {
        // Generate labels for mouse clicks
        if let InputEvent::Mouse { event_type, button, x, y, .. } = event {
            if event_type == "click" {
                let btn_name = button.as_deref().unwrap_or("unknown");
                return (true, Some(OverlayLabel {
                    text: format!("Click: {}", btn_name),
                    timestamp: video_timestamp,
                    duration: Some(1.5),
                    x: x.map(|x| x as u32),
                    y: y.map(|y| y as u32),
                }));
            }
        }
        (true, None)
    }
}

/// Configuration for unified recording (screen + input).
#[derive(Clone, Debug)]
pub struct UnifiedRecordingConfig {
    /// Screen recording configuration
    pub screen_config: ScreenRecordingConfig,
    /// Input capture configuration
    pub input_config: InputCaptureConfig,
    /// Database path for storing events
    pub database_path: PathBuf,
    /// Whether to capture keyboard events
    pub capture_keyboard: bool,
    /// Whether to capture mouse events
    pub capture_mouse: bool,
    /// Whether to capture mouse moves
    pub capture_mouse_moves: bool,
    /// Whether to add timestamp overlay to video
    pub show_timestamp: bool,
    /// Whether to add event labels to video
    pub show_labels: bool,
}

/// Screen recording configuration.
#[derive(Clone, Debug)]
pub struct ScreenRecordingConfig {
    /// Output video file path
    pub output_path: PathBuf,
    /// Frame rate (FPS)
    pub framerate: u32,
    /// Duration in seconds (None = until stopped)
    pub duration_secs: Option<u64>,
    /// Monitor index (None = primary)
    pub monitor_index: Option<usize>,
    /// Include audio
    pub include_audio: bool,
}

impl Default for ScreenRecordingConfig {
    fn default() -> Self {
        Self {
            output_path: PathBuf::from("recording.mp4"),
            framerate: 30,
            duration_secs: None,
            monitor_index: None,
            include_audio: true,
        }
    }
}

/// Input capture configuration.
#[derive(Clone, Debug)]
pub struct InputCaptureConfig {
    /// Output file for events (None = stdout)
    pub output_file: Option<PathBuf>,
    /// Output format: "json", "text", or "both"
    pub format: String,
}

impl Default for InputCaptureConfig {
    fn default() -> Self {
        Self {
            output_file: None,
            format: "text".to_string(),
        }
    }
}

impl Default for UnifiedRecordingConfig {
    fn default() -> Self {
        Self {
            screen_config: ScreenRecordingConfig::default(),
            input_config: InputCaptureConfig::default(),
            database_path: PathBuf::from("events.db"),
            capture_keyboard: true,
            capture_mouse: true,
            capture_mouse_moves: false,
            show_timestamp: true,
            show_labels: true,
        }
    }
}

/// Unified recording service that coordinates screen recording and input capture.
pub struct UnifiedRecordingService {
    config: UnifiedRecordingConfig,
    callback: Arc<dyn EventCallback>,
}

impl UnifiedRecordingService {
    /// Create a new unified recording service with default callback.
    pub fn new(config: UnifiedRecordingConfig) -> Self {
        Self {
            config,
            callback: Arc::new(DefaultEventCallback),
        }
    }

    /// Create a new unified recording service with a custom callback.
    pub fn with_callback<C: EventCallback + 'static>(
        config: UnifiedRecordingConfig,
        callback: C,
    ) -> Self {
        Self {
            config,
            callback: Arc::new(callback),
        }
    }

    /// Start unified recording (screen + input).
    /// 
    /// This method:
    /// 1. Starts screen recording in a background task
    /// 2. Starts input capture in a background task
    /// 3. Synchronizes timestamps between video and events
    /// 4. Calls the callback for each event
    /// 5. Stores events in the database
    /// 
    /// Returns when recording is stopped (via stop_signal or duration limit).
    pub async fn start_recording(
        &self,
        stop_signal: Arc<AtomicBool>,
    ) -> Result<RecordingSession> {
        let session_id = Uuid::new_v4().to_string();
        let recording_start = Utc::now();
        
        // Initialize database
        let db = Database::new().await?;
        db.create_session(&session_id).await?;

        // Channel for events from input capture
        let (event_tx, mut event_rx) = mpsc::unbounded_channel::<(InputEvent, Instant)>();

        // Start input capture in background
        let input_config = self.config.input_config.clone();
        let capture_keyboard = self.config.capture_keyboard;
        let capture_mouse = self.config.capture_mouse;
        let capture_mouse_moves = self.config.capture_mouse_moves;
        let callback_clone = Arc::clone(&self.callback);
        let db_clone = Arc::new(db);
        let session_id_clone = session_id.clone();
        let recording_start_clone = recording_start;
        let stop_signal_input = stop_signal.clone();

        let input_handle = tokio::task::spawn_blocking(move || {
            Self::run_input_capture_blocking(
                capture_keyboard,
                capture_mouse,
                capture_mouse_moves,
                input_config,
                event_tx,
                stop_signal_input,
            )
        });

        // Start screen recording in background
        // Note: This requires nexus_screen to be available
        // For now, we'll create a placeholder that can be implemented
        let screen_config = self.config.screen_config.clone();
        let stop_signal_screen = stop_signal.clone();
        let screen_handle = tokio::task::spawn_blocking(move || {
            Self::run_screen_recording_blocking(screen_config, stop_signal_screen)
        });

        // Shared storage for overlay labels
        let overlay_labels = Arc::new(std::sync::Mutex::new(Vec::<OverlayLabel>::new()));
        let overlay_labels_clone = overlay_labels.clone();
        
        // Process events and call callbacks
        let stop_signal_process = stop_signal.clone();
        let process_handle = tokio::spawn(async move {
            let mut video_start_time = None;

            loop {
                // Check stop signal first
                if stop_signal_process.load(Ordering::SeqCst) {
                    break;
                }

                // Try to receive event with timeout to allow periodic stop signal checks
                let event_result = tokio::time::timeout(
                    Duration::from_millis(100),
                    event_rx.recv(),
                ).await;

                let (event, event_time) = match event_result {
                    Ok(Some(event)) => event,
                    Ok(None) => {
                        // Channel closed, input capture stopped
                        break;
                    }
                    Err(_) => {
                        // Timeout - continue loop to check stop signal
                        continue;
                    }
                };

                // Calculate video timestamp
                // For now, we estimate based on elapsed time
                // In a real implementation, you'd get this from the video encoder
                if video_start_time.is_none() {
                    video_start_time = Some(event_time);
                }
                let elapsed = event_time.duration_since(video_start_time.unwrap());
                let video_timestamp = elapsed.as_secs_f64();

                // Call appropriate callback
                let (should_store, overlay_label) = match &event {
                    InputEvent::Keyboard { .. } => {
                        callback_clone.on_keyboard_event(&event, video_timestamp, recording_start_clone)
                    }
                    InputEvent::Mouse { .. } => {
                        callback_clone.on_mouse_event(&event, video_timestamp, recording_start_clone)
                    }
                };

                // Collect overlay labels if provided
                if let Some(label) = overlay_label {
                    overlay_labels_clone.lock().unwrap().push(label);
                }

                if should_store {
                    // Store in database (errors are logged but don't stop recording)
                    let timestamp = Local::now().to_rfc3339();
                    match &event {
                        InputEvent::Keyboard { key, pressed, .. } => {
                            let db_for_event = db_clone.clone();
                            let session_id_for_event = session_id_clone.clone();
                            let key_for_event = key.clone();
                            let timestamp_for_event = timestamp.clone();
                            let pressed_for_event = *pressed;
                            tokio::spawn(async move {
                                if let Err(e) = db_for_event.insert_event(
                                    &session_id_for_event,
                                    "keyboard",
                                    Some(if pressed_for_event { "press" } else { "release" }),
                                    Some(&key_for_event),
                                    None,
                                    None,
                                    None,
                                    Some(pressed_for_event),
                                    &timestamp_for_event,
                                    None, // timecode
                                    None, // metadata
                                    None, // screenshot_id
                                ).await {
                                    eprintln!("⚠️  Failed to store keyboard event in database: {}", e);
                                }

                                if pressed_for_event {
                                    if let Err(e) = db_for_event.update_key_frequency(&session_id_for_event, &key_for_event).await {
                                        eprintln!("⚠️  Failed to update key frequency: {}", e);
                                    }
                                }
                            });
                        }
                        InputEvent::Mouse {
                            event_type,
                            button,
                            x,
                            y,
                            timestamp: _,
                        } => {
                            let db_for_event = db_clone.clone();
                            let session_id_for_event = session_id_clone.clone();
                            let event_type_for_event = event_type.clone();
                            let button_for_event = button.clone();
                            let x_for_event = *x;
                            let y_for_event = *y;
                            let timestamp_for_event = timestamp.clone();
                            tokio::spawn(async move {
                                if let Err(e) = db_for_event.insert_event(
                                    &session_id_for_event,
                                    "mouse",
                                    Some(&event_type_for_event),
                                    None,
                                    button_for_event.as_deref(),
                                    x_for_event,
                                    y_for_event,
                                    None,
                                    &timestamp_for_event,
                                    None, // timecode
                                    None, // metadata
                                    None, // screenshot_id
                                ).await {
                                    eprintln!("⚠️  Failed to store mouse event in database: {}", e);
                                }

                                if event_type_for_event == "click" {
                                    if let Some(ref btn) = button_for_event {
                                        if let Err(e) = db_for_event.update_mouse_button_frequency(&session_id_for_event, btn).await {
                                            eprintln!("⚠️  Failed to update mouse button frequency: {}", e);
                                        }
                                    }
                                }
                            });
                        }
                    }
                }
            }

            // Labels are stored in the shared Arc<Mutex<Vec<OverlayLabel>>>
            // They will be applied after screen recording completes

            Ok::<(), LoggerError>(())
        });

        Ok(RecordingSession {
            session_id,
            recording_start,
            input_handle,
            screen_handle,
            process_handle,
            stop_signal,
            overlay_labels,
            config: self.config.clone(),
        })
    }

    fn run_input_capture_blocking(
        capture_keyboard: bool,
        capture_mouse: bool,
        capture_mouse_moves: bool,
        input_config: InputCaptureConfig,
        event_tx: mpsc::UnboundedSender<(InputEvent, Instant)>,
        stop_signal: Arc<AtomicBool>,
    ) -> Result<()> {
        use device_query::{DeviceQuery, DeviceState, Keycode};
        use std::collections::HashSet;
        use std::fs::OpenOptions;

        let device_state = DeviceState::new();
        let mut last_keys: Vec<Keycode> = vec![];
        let mut last_mouse_buttons: Vec<bool> = vec![];
        let mut last_mouse_pos: Option<(i32, i32)> = None;

        // Open output file if specified
        let mut file_handle: Option<std::fs::File> = if let Some(ref path) = input_config.output_file {
            Some(
                OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)
                    .map_err(|e| {
                        LoggerError::Io(std::io::Error::new(
                            std::io::ErrorKind::Other,
                            format!("Failed to open output file: {}", e),
                        ))
                    })?,
            )
        } else {
            None
        };

        while !stop_signal.load(Ordering::SeqCst) {
            let timestamp = chrono::Local::now().to_rfc3339();
            let event_time = Instant::now();

            // Capture keyboard events
            if capture_keyboard {
                let keys = device_state.get_keys();
                let keys_set: HashSet<Keycode> = keys.iter().cloned().collect();
                let last_keys_set: HashSet<Keycode> = last_keys.iter().cloned().collect();

                for key in &keys {
                    if !last_keys_set.contains(key) {
                        let event = InputEvent::Keyboard {
                            key: format!("{:?}", key),
                            pressed: true,
                            timestamp: timestamp.clone(),
                        };
                        Self::write_event_output(&event, &input_config.format, &mut file_handle)?;
                        let _ = event_tx.send((event, event_time));
                    }
                }

                for key in &last_keys {
                    if !keys_set.contains(key) {
                        let event = InputEvent::Keyboard {
                            key: format!("{:?}", key),
                            pressed: false,
                            timestamp: timestamp.clone(),
                        };
                        Self::write_event_output(&event, &input_config.format, &mut file_handle)?;
                        let _ = event_tx.send((event, event_time));
                    }
                }

                last_keys = keys;
            }

            // Capture mouse events
            if capture_mouse {
                let mouse = device_state.get_mouse();
                let current_pos = (mouse.coords.0, mouse.coords.1);
                let pos_changed = last_mouse_pos.map(|last| last != current_pos).unwrap_or(true);
                last_mouse_pos = Some(current_pos);

                let button_names = ["Left", "Right", "Middle", "X1", "X2"];

                for (idx, &pressed) in mouse.button_pressed.iter().enumerate().skip(1) {
                    let was_pressed = last_mouse_buttons.get(idx).copied().unwrap_or(false);
                    let button_name = button_names
                        .get(idx - 1)
                        .copied()
                        .map(String::from)
                        .unwrap_or_else(|| format!("Button{}", idx));

                    if pressed && !was_pressed {
                        let event = InputEvent::Mouse {
                            event_type: "click".to_string(),
                            button: Some(button_name.clone()),
                            x: Some(mouse.coords.0),
                            y: Some(mouse.coords.1),
                            timestamp: timestamp.clone(),
                        };
                        Self::write_event_output(&event, &input_config.format, &mut file_handle)?;
                        let _ = event_tx.send((event, event_time));
                    } else if !pressed && was_pressed {
                        let event = InputEvent::Mouse {
                            event_type: "release".to_string(),
                            button: Some(button_name.clone()),
                            x: Some(mouse.coords.0),
                            y: Some(mouse.coords.1),
                            timestamp: timestamp.clone(),
                        };
                        Self::write_event_output(&event, &input_config.format, &mut file_handle)?;
                        let _ = event_tx.send((event, event_time));
                    }
                }

                if capture_mouse_moves && pos_changed {
                    let event = InputEvent::Mouse {
                        event_type: "move".to_string(),
                        button: None,
                        x: Some(mouse.coords.0),
                        y: Some(mouse.coords.1),
                        timestamp: timestamp.clone(),
                    };
                    Self::write_event_output(&event, &input_config.format, &mut file_handle)?;
                    let _ = event_tx.send((event, event_time));
                }

                last_mouse_buttons = mouse.button_pressed.clone();
            }

            std::thread::sleep(Duration::from_millis(10));
        }

        Ok(())
    }

    fn run_screen_recording_blocking(
        config: ScreenRecordingConfig,
        stop_signal: Arc<AtomicBool>,
    ) -> Result<()> {
        use nexus_screen::{RecordingConfig, ScreenRecorder};

        // Convert our config to nexus_screen's RecordingConfig
        let recording_config = RecordingConfig {
            framerate: config.framerate,
            duration_secs: config.duration_secs,
            output_path: config.output_path,
            monitor_index: config.monitor_index,
            window_id: None,
            window_title: None,
            include_audio: config.include_audio,
            fast: false,
        };

        // Create recorder
        let recorder = ScreenRecorder::new_with_config(recording_config.clone())
            .map_err(|e| {
                LoggerError::Other(format!("Failed to initialize screen recorder: {}", e))
            })?;

        // Start recording (this is blocking)
        recorder.record(recording_config, stop_signal).map_err(|e| {
            LoggerError::Other(format!("Screen recording failed: {}", e))
        })?;

        Ok(())
    }

    /// Apply overlays to video using FFmpeg
    pub fn apply_video_overlays(
        video_path: &PathBuf,
        labels: &[OverlayLabel],
        show_timestamp: bool,
        show_labels: bool,
    ) -> Result<()> {
        use std::process::Command;

        // Create temporary output file
        let temp_output = video_path.with_extension("tmp.mp4");
        
        // Build FFmpeg filter complex for overlays
        let mut filter_parts: Vec<String> = Vec::new();
        
        // Add timestamp overlay if enabled
        if show_timestamp {
            // Draw timestamp in top-left corner
            // Use pts:hms format - escape colon for FFmpeg filter syntax
            // Format: HH:MM:SS.mmm
            filter_parts.push(
                "drawtext=text='%{pts\\:hms}':fontcolor=white:fontsize=24:x=10:y=10:box=1:boxcolor=black@0.5:boxborderw=2".to_string()
            );
        }
        
        // Add event labels if enabled
        if show_labels && !labels.is_empty() {
            // For each label, create a drawtext filter
            // Note: FFmpeg filters can be complex, so we'll add labels as they occur
            // For simplicity, we'll add a single label at a time using enable/disable
            for (idx, label) in labels.iter().enumerate() {
                let start_time = label.timestamp;
                let end_time = label.duration
                    .map(|d| start_time + d)
                    .unwrap_or_else(|| {
                        // Default to 2 seconds if no duration specified
                        start_time + 2.0
                    });
                
                // Escape text for FFmpeg
                // FFmpeg drawtext needs text escaped - replace single quotes and colons
                let escaped_text = label.text
                    .replace('\\', "\\\\")
                    .replace('\'', "\\'")
                    .replace(':', "\\:");
                
                // Use proper FFmpeg filter syntax
                // For multiple drawtext filters, we chain them with commas
                // Use simpler positioning - bottom center for labels
                let x_pos = label.x.map(|x| x.to_string())
                    .unwrap_or_else(|| "(w-tw)/2".to_string()); // Center horizontally
                let y_pos = label.y.map(|y| y.to_string())
                    .unwrap_or_else(|| format!("h-th-{}", 30 + (idx * 30))); // Stack from bottom
                
                // Build filter string
                let filter_str = format!(
                    "drawtext=text='{}':fontcolor=yellow:fontsize=24:x={}:y={}:box=1:boxcolor=black@0.8:boxborderw=3:enable='between(t,{},{})'",
                    escaped_text, x_pos, y_pos, start_time, end_time
                );
                filter_parts.push(filter_str);
            }
        }
        
        // If no overlays, just return (no processing needed)
        if filter_parts.is_empty() {
            return Ok(());
        }
        
        // Combine all filters - chain them properly for multiple drawtext filters
        // FFmpeg requires chaining with commas for multiple filters on same input
        let filter_complex = filter_parts.join(",");
        
        println!("🎬 Applying FFmpeg overlays...");
        println!("   Timestamp overlay: {}", if show_timestamp { "✓" } else { "✗" });
        println!("   Event labels: {} labels", labels.len());
        if !labels.is_empty() {
            for (idx, label) in labels.iter().take(5).enumerate() {
                println!("     {}. '{}' at {:.2}s", idx + 1, label.text, label.timestamp);
            }
            if labels.len() > 5 {
                println!("     ... and {} more", labels.len() - 5);
            }
        }
        
        // Build FFmpeg command
        let mut cmd = Command::new("ffmpeg");
        cmd.arg("-i")
            .arg(video_path)
            .arg("-vf")
            .arg(&filter_complex)
            .arg("-c:a")
            .arg("copy") // Copy audio without re-encoding
            .arg("-y") // Overwrite output
            .arg(&temp_output)
            .stderr(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped());
        
        // Execute FFmpeg
        let output = cmd.output().map_err(|e| {
            LoggerError::Other(format!("Failed to execute FFmpeg: {}", e))
        })?;
        
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            eprintln!("FFmpeg stderr: {}", stderr);
            eprintln!("FFmpeg stdout: {}", stdout);
            eprintln!("FFmpeg filter used: {}", filter_complex);
            return Err(LoggerError::Other(format!(
                "FFmpeg failed to apply overlays. Exit code: {}",
                output.status.code().unwrap_or(-1)
            )));
        } else {
            println!("✅ FFmpeg overlay processing completed");
        }
        
        // Replace original file with processed version
        std::fs::rename(&temp_output, video_path).map_err(|e| {
            LoggerError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Failed to replace video file: {}", e),
            ))
        })?;
        
        Ok(())
    }

    fn write_event_output(
        event: &InputEvent,
        format: &str,
        file_handle: &mut Option<std::fs::File>,
    ) -> Result<()> {
        use std::io::Write;

        let output = match format {
            "json" => serde_json::to_string(event).map_err(|e| {
                LoggerError::Other(format!("Failed to serialize event: {}", e))
            })?,
            "text" => event.to_text(),
            "both" => {
                format!(
                    "{} | {}",
                    event.to_text(),
                    serde_json::to_string(event).map_err(|e| {
                        LoggerError::Other(format!("Failed to serialize event: {}", e))
                    })?
                )
            }
            _ => return Err(LoggerError::Configuration("Invalid format".to_string())),
        };

        if let Some(ref mut file) = file_handle {
            writeln!(file, "{}", output).map_err(|e| {
                LoggerError::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Failed to write to file: {}", e),
                ))
            })?;
        } else {
            println!("{}", output);
        }

        Ok(())
    }
}

/// Active recording session.
pub struct RecordingSession {
    session_id: String,
    recording_start: DateTime<Utc>,
    input_handle: tokio::task::JoinHandle<Result<()>>,
    screen_handle: tokio::task::JoinHandle<Result<()>>,
    process_handle: tokio::task::JoinHandle<Result<()>>,
    stop_signal: Arc<AtomicBool>,
    overlay_labels: Arc<std::sync::Mutex<Vec<OverlayLabel>>>,
    config: UnifiedRecordingConfig,
}

impl RecordingSession {
    /// Stop the recording session.
    pub fn stop(&self) {
        self.stop_signal.store(true, Ordering::SeqCst);
    }

    /// Wait for the recording session to complete.
    /// This will wait until the stop signal is set (via stop() or duration expires).
    pub async fn wait(self) -> Result<()> {
        // Wait for all tasks to complete
        // They will exit when stop_signal is set
        let input_result = self.input_handle.await;
        let screen_result = self.screen_handle.await;
        let process_result = self.process_handle.await;

        input_result.map_err(|e| LoggerError::Other(format!("Input capture task failed: {}", e)))??;
        screen_result.map_err(|e| LoggerError::Other(format!("Screen recording task failed: {}", e)))??;
        process_result.map_err(|e| LoggerError::Other(format!("Event processing task failed: {}", e)))??;

        // Now that screen recording is complete, apply overlays
        let labels = self.overlay_labels.lock().unwrap().clone();
        println!("📝 Applying overlays: {} labels collected", labels.len());
        
        if self.config.show_timestamp || self.config.show_labels {
            // Wait a moment to ensure file is fully written
            tokio::time::sleep(Duration::from_millis(500)).await;
            
            if let Err(e) = UnifiedRecordingService::apply_video_overlays(
                &self.config.screen_config.output_path,
                &labels,
                self.config.show_timestamp,
                self.config.show_labels,
            ) {
                eprintln!("⚠️  Failed to apply video overlays: {}", e);
            } else {
                println!("✅ Overlays applied successfully");
            }
        }

        Ok(())
    }

    /// Get the session ID.
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Get when recording started.
    pub fn recording_start(&self) -> DateTime<Utc> {
        self.recording_start
    }
}

