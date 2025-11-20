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
    /// Returns true if the event should be stored, false to skip.
    fn on_keyboard_event(
        &self,
        event: &InputEvent,
        video_timestamp: f64,
        recording_start: DateTime<Utc>,
    ) -> bool;

    /// Called when a mouse event is captured.
    /// 
    /// # Arguments
    /// * `event` - The mouse event
    /// * `video_timestamp` - Current video timestamp in seconds
    /// * `recording_start` - When recording started (for absolute time calculations)
    /// 
    /// Returns true if the event should be stored, false to skip.
    fn on_mouse_event(
        &self,
        event: &InputEvent,
        video_timestamp: f64,
        recording_start: DateTime<Utc>,
    ) -> bool;
}

/// Default callback implementation that accepts all events.
pub struct DefaultEventCallback;

impl EventCallback for DefaultEventCallback {
    fn on_keyboard_event(
        &self,
        _event: &InputEvent,
        _video_timestamp: f64,
        _recording_start: DateTime<Utc>,
    ) -> bool {
        true
    }

    fn on_mouse_event(
        &self,
        _event: &InputEvent,
        _video_timestamp: f64,
        _recording_start: DateTime<Utc>,
    ) -> bool {
        true
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
        let db = Database::new(&self.config.database_path)?;
        db.create_session(&session_id)?;

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
                let should_store = match &event {
                    InputEvent::Keyboard { .. } => {
                        callback_clone.on_keyboard_event(&event, video_timestamp, recording_start_clone)
                    }
                    InputEvent::Mouse { .. } => {
                        callback_clone.on_mouse_event(&event, video_timestamp, recording_start_clone)
                    }
                };

                if should_store {
                    // Store in database (errors are logged but don't stop recording)
                    let timestamp = Local::now().to_rfc3339();
                    match &event {
                        InputEvent::Keyboard { key, pressed, .. } => {
                            if let Err(e) = db_clone.insert_event(
                                &session_id_clone,
                                "keyboard",
                                Some(if *pressed { "press" } else { "release" }),
                                Some(key),
                                None,
                                None,
                                None,
                                Some(*pressed),
                                &timestamp,
                            ) {
                                eprintln!("⚠️  Failed to store keyboard event in database: {}", e);
                            }

                            if *pressed {
                                if let Err(e) = db_clone.update_key_frequency(&session_id_clone, key) {
                                    eprintln!("⚠️  Failed to update key frequency: {}", e);
                                }
                            }
                        }
                        InputEvent::Mouse {
                            event_type,
                            button,
                            x,
                            y,
                            timestamp: _,
                        } => {
                            if let Err(e) = db_clone.insert_event(
                                &session_id_clone,
                                "mouse",
                                Some(event_type),
                                None,
                                button.as_deref(),
                                *x,
                                *y,
                                None,
                                &timestamp,
                            ) {
                                eprintln!("⚠️  Failed to store mouse event in database: {}", e);
                            }

                            if event_type == "click" {
                                if let Some(ref btn) = button {
                                    if let Err(e) = db_clone.update_mouse_button_frequency(&session_id_clone, btn) {
                                        eprintln!("⚠️  Failed to update mouse button frequency: {}", e);
                                    }
                                }
                            }
                        }
                    }
                }
            }

            Ok::<(), LoggerError>(())
        });

        Ok(RecordingSession {
            session_id,
            recording_start,
            input_handle,
            screen_handle,
            process_handle,
            stop_signal,
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

                for (idx, &pressed) in mouse.button_pressed.iter().enumerate() {
                    let was_pressed = last_mouse_buttons.get(idx).copied().unwrap_or(false);
                    let button_name = button_names
                        .get(idx)
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

