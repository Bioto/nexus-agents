use crate::error::{LoggerError, Result};
use crate::services::capture::InputEvent;
use crate::services::click_context::{ClickContextHandle, ClickContextService};
use crate::services::context_processing::ProcessingJob;
use crate::services::database::Database;
use chrono::{DateTime, Local, Utc};
use nexus_audio::{AudioRecorder, RecordingConfig as NexusAudioRecordingConfig};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};
use serde_json::json;
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
                return (
                    true,
                    Some(OverlayLabel {
                        text: format!("Key: {}", key),
                        timestamp: video_timestamp,
                        duration: Some(2.0),
                        x: None,
                        y: None,
                    }),
                );
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
        if let InputEvent::Mouse {
            event_type,
            button,
            x,
            y,
            ..
        } = event
        {
            if event_type == "click" {
                let btn_name = button.as_deref().unwrap_or("unknown");
                return (
                    true,
                    Some(OverlayLabel {
                        text: format!("Click: {}", btn_name),
                        timestamp: video_timestamp,
                        duration: Some(1.5),
                        x: x.map(|x| x as u32),
                        y: y.map(|y| y as u32),
                    }),
                );
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
    /// Audio recording configurations (can have multiple for mic + monitor)
    pub audio_configs: Vec<AudioRecordingConfig>,
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
    /// Frames per second for post-recording context analysis (None = disabled)
    pub context_fps: Option<f64>,
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

/// Audio recording configuration.
#[derive(Clone, Debug)]
pub struct AudioRecordingConfig {
    /// Whether to record audio
    pub enabled: bool,
    /// Output path for WAV file
    pub output_path: PathBuf,
    /// Sample rate in Hz (default 48000)
    pub sample_rate: u32,
    /// Number of channels (1 = mono, 2 = stereo, default 1 for mic, 2 for desktop)
    pub channels: u16,
    /// Specific microphone device name (None = default device)
    pub device_name: Option<String>,
    /// Whether to monitor desktop audio output (creates virtual loopback sink)
    /// When true, records desktop audio instead of microphone input
    pub monitor_desktop_audio: bool,
    /// Whether to transcribe audio (future feature)
    pub transcribe: bool,
    /// Path to Whisper model if transcribing (None = disabled)
    pub transcription_model_path: Option<PathBuf>,
}

impl Default for AudioRecordingConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            output_path: PathBuf::from("recording.wav"),
            sample_rate: 48000,
            channels: 1,
            device_name: None,
            monitor_desktop_audio: false,
            transcribe: false,
            transcription_model_path: None,
        }
    }
}

impl Default for UnifiedRecordingConfig {
    fn default() -> Self {
        Self {
            screen_config: ScreenRecordingConfig::default(),
            input_config: InputCaptureConfig::default(),
            audio_configs: Vec::new(),
            database_path: PathBuf::from("events.db"),
            capture_keyboard: true,
            capture_mouse: true,
            capture_mouse_moves: false,
            show_timestamp: true,
            show_labels: true,
            context_fps: None,
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

    /// Get video duration in seconds using FFprobe
    fn get_video_duration(video_path: &PathBuf) -> Result<f64> {
        use std::process::Command;

        let output = Command::new("ffprobe")
            .arg("-v")
            .arg("error")
            .arg("-show_entries")
            .arg("format=duration")
            .arg("-of")
            .arg("default=noprint_wrappers=1:nokey=1")
            .arg(video_path)
            .output()
            .map_err(|e| LoggerError::Other(format!("Failed to run ffprobe: {}", e)))?;

        if !output.status.success() {
            return Err(LoggerError::Other(
                "FFprobe failed to get video duration".to_string(),
            ));
        }

        let duration_str = String::from_utf8_lossy(&output.stdout);
        duration_str
            .trim()
            .parse::<f64>()
            .map_err(|e| LoggerError::Other(format!("Failed to parse video duration: {}", e)))
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
    pub async fn start_recording(&self, stop_signal: Arc<AtomicBool>) -> Result<RecordingSession> {
        let session_id = Uuid::new_v4().to_string();
        let recording_start = Utc::now();
        let recording_start_instant = Instant::now();

        // Initialize database
        let db = Database::new().await?;
        db.create_session(&session_id).await?;
        let click_context = ClickContextService::maybe_start(db.clone());
        let db = Arc::new(db);

        // Channel for events from input capture
        let (event_tx, mut event_rx) = mpsc::unbounded_channel::<(InputEvent, Instant)>();

        // Start input capture in background
        let input_config = self.config.input_config.clone();
        let capture_keyboard = self.config.capture_keyboard;
        let capture_mouse = self.config.capture_mouse;
        let capture_mouse_moves = self.config.capture_mouse_moves;
        let callback_clone = Arc::clone(&self.callback);
        let db_clone = Arc::clone(&db);
        let session_id_clone = session_id.clone();
        let recording_start_clone = recording_start;
        let stop_signal_input = stop_signal.clone();

        let click_context_for_input = click_context.clone();
        let session_id_for_input = session_id.clone();
        let video_path_for_input = self.config.screen_config.output_path.clone();
        let input_handle = tokio::task::spawn_blocking(move || {
            Self::run_input_capture_blocking(
                capture_keyboard,
                capture_mouse,
                capture_mouse_moves,
                input_config,
                event_tx,
                stop_signal_input,
                click_context_for_input,
                session_id_for_input,
                video_path_for_input,
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

        // Start audio recording tasks for all enabled audio configs
        // CRITICAL: Desktop audio MUST start first to set up loopback sink and default source
        // before microphone recording tries to connect to its device
        let mut audio_handles = Vec::new();
        let mut audio_config_indices = Vec::new(); // Track which config index each handle corresponds to
        
        // Start desktop audio monitoring tasks first (they set up the loopback and default source)
        // CRITICAL: We need to set the monitor as default source BEFORE starting the recording
        // so that pipewire/pulse devices route to it
        #[cfg(target_os = "linux")]
        let mut previous_default_source: Option<String> = None;
        #[cfg(target_os = "linux")]
        let mut previous_default_sink: Option<String> = None;
        #[cfg(target_os = "linux")]
        let mut loopback_module_ids: Vec<u32> = Vec::new();
        #[cfg(not(target_os = "linux"))]
        let previous_default_source: Option<String> = None;
        #[cfg(not(target_os = "linux"))]
        let previous_default_sink: Option<String> = None;
        #[cfg(not(target_os = "linux"))]
        let loopback_module_ids: Vec<u32> = Vec::new();
        
        for (config_idx, audio_config) in self.config.audio_configs.iter().enumerate() {
            if audio_config.enabled && audio_config.monitor_desktop_audio {
                // Set up loopback sink and default source BEFORE starting recording
                #[cfg(target_os = "linux")]
                {
                    use nexus_audio::AudioRecorder;
                    eprintln!("📺 Setting up desktop audio monitoring...");
                    
                    // Capture current default source
                    if let Ok(current_source) = AudioRecorder::get_default_source() {
                        eprintln!("📝 Current default source: {}", current_source);
                        previous_default_source = Some(current_source);
                    }
                    
                    // Create loopback sink (this also sets combine-sink as default output)
                    match AudioRecorder::create_loopback_sink(None) {
                        Ok((monitor_name, module_ids, prev_sink)) => {
                            eprintln!("✅ Created loopback sink: {}", monitor_name);
                            loopback_module_ids.extend(module_ids);
                            previous_default_sink = Some(prev_sink);
                            
                            // DON'T change the default source - we'll use the monitor source name directly
                            // This allows the microphone to continue using the default source
                            eprintln!("ℹ️  Using monitor source '{}' directly (not changing default source)", monitor_name);
                            eprintln!("   This allows microphone to use default source simultaneously");
                        }
                        Err(e) => {
                            eprintln!("❌ Failed to create loopback sink: {}", e);
                            return Err(LoggerError::Other(format!(
                                "Failed to create loopback sink: {}", e
                            )));
                        }
                    }
                    
                    // Give a moment for the loopback sink to be ready
                    tokio::time::sleep(Duration::from_millis(200)).await;
                }
                
                let audio_config_clone = audio_config.clone();
                let stop_signal_audio = stop_signal.clone();
                
                audio_config_indices.push(config_idx);
                audio_handles.push(tokio::task::spawn_blocking(move || {
                    Self::run_audio_recording_blocking(audio_config_clone, stop_signal_audio)
                }));
                
                // Give desktop audio 500ms to start recording before microphone tries to connect
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        }
        
        // Then start microphone tasks (they use explicit device names)
        // Since we're not changing the default source for desktop audio, the microphone can use 'pulse'/'default'
        // and it will route to the original default source (the microphone)
        for (config_idx, audio_config) in self.config.audio_configs.iter().enumerate() {
            if audio_config.enabled && !audio_config.monitor_desktop_audio {
                let audio_config_clone = audio_config.clone();
                let stop_signal_audio = stop_signal.clone();
                
                audio_config_indices.push(config_idx);
                audio_handles.push(tokio::task::spawn_blocking(move || {
                    Self::run_audio_recording_blocking(audio_config_clone, stop_signal_audio)
                }));
            }
        }

        // Shared storage for overlay labels
        let overlay_labels = Arc::new(std::sync::Mutex::new(Vec::<OverlayLabel>::new()));
        let overlay_labels_clone = overlay_labels.clone();

        // Process events and call callbacks
        let stop_signal_process = stop_signal.clone();
        let video_start_time = recording_start_instant;
        let process_handle = tokio::spawn(async move {
            loop {
                // Check stop signal first
                if stop_signal_process.load(Ordering::SeqCst) {
                    break;
                }

                // Try to receive event with timeout to allow periodic stop signal checks
                let event_result =
                    tokio::time::timeout(Duration::from_millis(100), event_rx.recv()).await;

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

                // Calculate video timestamp from recording start time
                // This ensures timestamps are accurate even if the user doesn't interact immediately
                let elapsed = event_time.duration_since(video_start_time);
                let video_timestamp = elapsed.as_secs_f64();

                // Call appropriate callback
                let (should_store, overlay_label) = match &event {
                    InputEvent::Keyboard { .. } => callback_clone.on_keyboard_event(
                        &event,
                        video_timestamp,
                        recording_start_clone,
                    ),
                    InputEvent::Mouse { .. } => callback_clone.on_mouse_event(
                        &event,
                        video_timestamp,
                        recording_start_clone,
                    ),
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
                                if let Err(e) = db_for_event
                                    .insert_event(
                                        &session_id_for_event,
                                        "keyboard",
                                        Some(if pressed_for_event {
                                            "press"
                                        } else {
                                            "release"
                                        }),
                                        Some(&key_for_event),
                                        None,
                                        None,
                                        None,
                                        Some(pressed_for_event),
                                        &timestamp_for_event,
                                        None, // timecode
                                        None, // metadata
                                        None, // screenshot_id
                                    )
                                    .await
                                {
                                    eprintln!(
                                        "⚠️  Failed to store keyboard event in database: {}",
                                        e
                                    );
                                }

                                if pressed_for_event {
                                    if let Err(e) = db_for_event
                                        .update_key_frequency(&session_id_for_event, &key_for_event)
                                        .await
                                    {
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
                                if let Err(e) = db_for_event
                                    .insert_event(
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
                                    )
                                    .await
                                {
                                    eprintln!("⚠️  Failed to store mouse event in database: {}", e);
                                }

                                if event_type_for_event == "click" {
                                    if let Some(ref btn) = button_for_event {
                                        if let Err(e) = db_for_event
                                            .update_mouse_button_frequency(
                                                &session_id_for_event,
                                                btn,
                                            )
                                            .await
                                        {
                                            eprintln!(
                                                "⚠️  Failed to update mouse button frequency: {}",
                                                e
                                            );
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
            audio_handles,
            process_handle,
            stop_signal,
            overlay_labels,
            config: self.config.clone(),
            click_context,
            previous_default_source,
            previous_default_sink,
            loopback_module_ids,
            audio_config_indices,
        })
    }

    fn run_input_capture_blocking(
        capture_keyboard: bool,
        capture_mouse: bool,
        capture_mouse_moves: bool,
        input_config: InputCaptureConfig,
        event_tx: mpsc::UnboundedSender<(InputEvent, Instant)>,
        stop_signal: Arc<AtomicBool>,
        _click_context: Option<ClickContextHandle>,
        _session_id: String,
        _video_path: PathBuf,
    ) -> Result<()> {
        use device_query::{DeviceQuery, DeviceState, Keycode};
        use std::collections::HashSet;
        use std::fs::OpenOptions;

        let device_state = DeviceState::new();
        let mut last_keys: Vec<Keycode> = vec![];
        let mut last_mouse_buttons: Vec<bool> = vec![];
        let mut last_mouse_pos: Option<(i32, i32)> = None;
        let _recording_start_instant = Instant::now();

        // Open output file if specified
        let mut file_handle: Option<std::fs::File> =
            if let Some(ref path) = input_config.output_file {
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
            let timestamp_utc = chrono::Utc::now();
            let timestamp = timestamp_utc.with_timezone(&chrono::Local).to_rfc3339();
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
                let pos_changed = last_mouse_pos
                    .map(|last| last != current_pos)
                    .unwrap_or(true);
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
                        // Don't process clicks during recording; batch-process after video is complete
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
        let recorder = ScreenRecorder::new_with_config(recording_config.clone()).map_err(|e| {
            LoggerError::Other(format!("Failed to initialize screen recorder: {}", e))
        })?;

        // Start recording (this is blocking)
        recorder
            .record(recording_config, stop_signal)
            .map_err(|e| LoggerError::Other(format!("Screen recording failed: {}", e)))?;

        Ok(())
    }

    fn run_audio_recording_blocking(
        config: AudioRecordingConfig,
        stop_signal: Arc<AtomicBool>,
    ) -> Result<()> {
        use hound::{WavSpec, WavWriter};
        use std::fs::File;
        use std::io::BufWriter;
        use std::sync::mpsc;

        let device_type = if config.monitor_desktop_audio {
            "📺 Desktop audio monitor"
        } else {
            "🎤 Microphone"
        };
        
        eprintln!("🚀 {} recording function started, output: {}", 
            device_type, config.output_path.display());

        // Create audio recorder
        let recorder = AudioRecorder::new()?;

        // Handle desktop audio monitoring
        // NOTE: Loopback sink is created in start_recording() before this function is called
        // We just need to verify it exists and get the monitor name
        #[cfg(target_os = "linux")]
        let (_module_ids, _monitor_source_name, _previous_default_source) = if config.monitor_desktop_audio {
            // Loopback sink should already exist (created in start_recording)
            // Just verify and get the monitor name - don't create again
            let monitor_name = "nexus_audio_monitor.monitor".to_string();
            eprintln!("📺 Using existing loopback sink: {}", monitor_name);
            // Module IDs will be cleaned up in wait() method, not here
            (Vec::<u32>::new(), Some(monitor_name), None::<String>)
        } else {
            (Vec::new(), None, None)
        };
        #[cfg(not(target_os = "linux"))]
        let (module_ids, _monitor_source_name, _previous_default_source) = if config.monitor_desktop_audio {
            log::warn!("Desktop audio monitoring is only supported on Linux");
            return Err(LoggerError::Other(
                "Desktop audio monitoring is only supported on Linux".to_string(),
            ));
        } else {
            (Vec::new(), None, None)
        };

        // For microphone: use the device_name that was pre-selected (if any)
        // The device_name should already be set in the config before this function is called
        // This avoids interference from the loopback sink created by desktop audio
        let mic_device_name = if !config.monitor_desktop_audio {
            if let Some(ref device_name) = config.device_name {
                log::info!("🎤 Using pre-selected microphone device: {}", device_name);
            } else {
                log::warn!("⚠️  No microphone device specified, using system default (may pick wrong device!)");
            }
            config.device_name.clone()
        } else {
            config.device_name.clone()
        };

        // Convert our config to nexus_audio's RecordingConfig
        // For desktop audio: use the monitor source name directly
        // For microphone: use the device name we determined above
        let recording_config = if config.monitor_desktop_audio {
            // Use the monitor source name directly instead of changing default source
            // The monitor name should be "nexus_audio_monitor.monitor"
            let monitor_name = "nexus_audio_monitor.monitor".to_string();
            eprintln!("📺 Using monitor source name directly: '{}'", monitor_name);
            NexusAudioRecordingConfig {
                sample_rate: config.sample_rate,
                channels: 2, // Desktop audio is typically stereo
                duration: None,
                device_name: Some(monitor_name), // Use monitor source name directly
            }
        } else {
            NexusAudioRecordingConfig {
                sample_rate: config.sample_rate,
                channels: config.channels,
                duration: None,
                device_name: mic_device_name,
            }
        };

        // Log device selection for debugging
        let device_name_display = recording_config.device_name.as_ref()
            .map(|d| d.as_str())
            .unwrap_or("system default");
        
        eprintln!("{} starting: device='{}' (requested {} Hz, {} channels)", 
            device_type, device_name_display, 
            recording_config.sample_rate, recording_config.channels);

        // Convert output path to absolute FIRST to ensure consistent file location
        let output_path = if config.output_path.is_absolute() {
            config.output_path.clone()
        } else {
            std::env::current_dir()
                .map_err(|e| LoggerError::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Failed to get current directory: {}", e),
                )))?
                .join(&config.output_path)
        };

        eprintln!("📁 {} output path resolved to: {}", device_type, output_path.display());

        // Use streaming API to have control over stop signal
        // This will return the actual sample rate and channels from the device
        eprintln!("🎙️  {} attempting to create audio stream...", device_type);
        let (mut stream, rx, actual_sample_rate, actual_channels) = match recorder.stream_audio_chunks(recording_config.clone()) {
            Ok(result) => {
                eprintln!("✅ {} stream created successfully", device_type);
                result
            }
            Err(e) => {
                eprintln!("❌ {} failed to create stream: {}", device_type, e);
                return Err(LoggerError::Other(format!(
                    "Failed to create audio stream for {}: {}",
                    device_type, e
                )));
            }
        };

        eprintln!("{} opened: device='{}' (actual {} Hz, {} channels) -> {}", 
            device_type, device_name_display,
            actual_sample_rate, actual_channels,
            output_path.display());

        // Create WAV file BEFORE starting the stream to ensure it exists
        // This way, even if the stream fails, we have a record that recording was attempted
        let spec = WavSpec {
            channels: actual_channels,
            sample_rate: actual_sample_rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };

        let writer = File::create(&output_path)
            .map_err(|e| LoggerError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Failed to create audio file at {}: {}", output_path.display(), e),
            )))?;
        let mut wav_writer = WavWriter::new(BufWriter::new(writer), spec)
            .map_err(|e| LoggerError::Other(format!("Failed to create WAV writer: {}", e)))?;

        // Verify file was created
        if !output_path.exists() {
            return Err(LoggerError::Other(format!(
                "File was not created at {} even though File::create() succeeded",
                output_path.display()
            )));
        }

        // Start the stream
        stream.play().map_err(|e| {
            LoggerError::Other(format!("Failed to start audio stream: {}", e))
        })?;

        // Record audio chunks until stop signal
        // For stereo, samples come interleaved: [L, R, L, R, ...]
        // For mono, samples come as: [M, M, M, ...]
        let mut samples_written = 0u64;
        let mut write_error_occurred = false;
        
        while !stop_signal.load(Ordering::SeqCst) {
            // Try to receive audio chunk with timeout to allow periodic stop signal checks
            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok(samples) => {
                    // Write samples to WAV file (convert f32 to i16)
                    // For stereo, write interleaved samples correctly
                    // For mono, write samples directly
                    if actual_channels == 2 {
                        // Stereo: samples are already interleaved [L, R, L, R, ...]
                        for sample in samples {
                            let clamped = sample.clamp(-1.0, 1.0);
                            let sample_i16 = (clamped * 32767.0).round() as i16;
                            match wav_writer.write_sample(sample_i16) {
                                Ok(()) => {
                                    samples_written += 1;
                                }
                                Err(e) => {
                                    eprintln!("❌ Error writing audio sample to {}: {}", output_path.display(), e);
                                    write_error_occurred = true;
                                    break;
                                }
                            }
                        }
                    } else {
                        // Mono: write samples directly
                        for sample in samples {
                            let clamped = sample.clamp(-1.0, 1.0);
                            let sample_i16 = (clamped * 32767.0).round() as i16;
                            match wav_writer.write_sample(sample_i16) {
                                Ok(()) => {
                                    samples_written += 1;
                                }
                                Err(e) => {
                                    eprintln!("❌ Error writing audio sample to {}: {}", output_path.display(), e);
                                    write_error_occurred = true;
                                    break;
                                }
                            }
                        }
                    }
                    
                    if write_error_occurred {
                        break;
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    // Timeout - continue loop to check stop signal
                    continue;
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    // Channel disconnected, stop recording
                    eprintln!("⚠️  Audio stream channel disconnected for {}", output_path.display());
                    break;
                }
            }
        }
        
        eprintln!("📊 {} recording: wrote {} samples before finalization", device_type, samples_written);

        // Stop the stream
        stream.pause().map_err(|e| {
            LoggerError::Other(format!("Failed to pause audio stream: {}", e))
        })?;

        // Finalize WAV file - this is critical, even if no audio was recorded
        drop(stream);
        
        eprintln!("💾 Finalizing WAV file at {}...", output_path.display());
        wav_writer.finalize().map_err(|e| {
            LoggerError::Other(format!("Failed to finalize WAV file at {}: {}", output_path.display(), e))
        })?;
        eprintln!("✅ WAV file finalized successfully");

        // Give filesystem a moment to sync
        std::thread::sleep(Duration::from_millis(100));

        // Verify file exists after finalization
        if !output_path.exists() {
            return Err(LoggerError::Other(format!(
                "WAV file does not exist after finalization at {} (samples written: {})",
                output_path.display(), samples_written
            )));
        }

        // Log file size for debugging
        match std::fs::metadata(&output_path) {
            Ok(metadata) => {
                eprintln!("✅ {} recording file created: {} ({} bytes, {} samples)", 
                    device_type, output_path.display(), metadata.len(), samples_written);
            }
            Err(e) => {
                return Err(LoggerError::Other(format!(
                    "Failed to get file metadata for {} after creation: {}",
                    output_path.display(), e
                )));
            }
        }

        // NOTE: Desktop audio loopback sink cleanup is handled in RecordingSession::wait()
        // to ensure proper ordering (restore default source before removing modules)

        eprintln!("✅ {} recording function completed successfully, returning Ok(())", device_type);
        Ok(())
    }

    /// Transcribe a WAV file using Whisper and store segments in database
    async fn transcribe_wav_file(
        wav_path: &PathBuf,
        model_path: &PathBuf,
        session_id: &str,
        db: &Database,
        monitor_desktop_audio: bool,
    ) -> Result<()> {
        use std::io::Read;

        // Read WAV file
        let mut file = std::fs::File::open(wav_path)
            .map_err(|e| LoggerError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Failed to open WAV file: {}", e),
            )))?;

        let mut wav_data = Vec::new();
        file.read_to_end(&mut wav_data)
            .map_err(|e| LoggerError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Failed to read WAV file: {}", e),
            )))?;

        // Decode WAV to f32 samples
        let mut reader = hound::WavReader::new(std::io::Cursor::new(wav_data))
            .map_err(|e| LoggerError::Other(format!("Failed to read WAV: {}", e)))?;

        let spec = reader.spec();
        let sample_rate = spec.sample_rate;

        // Convert samples to f32
        let samples: Vec<f32> = match spec.bits_per_sample {
            16 => reader
                .samples::<i16>()
                .map(|s| {
                    s.map(|sample| sample as f32 / 32768.0)
                        .map_err(|e| LoggerError::Other(format!("Failed to read sample: {}", e)))
                })
                .collect::<std::result::Result<Vec<_>, _>>()?,
            24 => reader
                .samples::<i32>()
                .map(|s| {
                    s.map(|sample| (sample >> 8) as f32 / 8388608.0)
                        .map_err(|e| LoggerError::Other(format!("Failed to read sample: {}", e)))
                })
                .collect::<std::result::Result<Vec<_>, _>>()?,
            32 => reader
                .samples::<i32>()
                .map(|s| {
                    s.map(|sample| sample as f32 / 2147483648.0)
                        .map_err(|e| LoggerError::Other(format!("Failed to read sample: {}", e)))
                })
                .collect::<std::result::Result<Vec<_>, _>>()?,
            _ => {
                return Err(LoggerError::Other(format!(
                    "Unsupported bit depth: {} bits",
                    spec.bits_per_sample
                )));
            }
        };

        // Convert to mono if stereo (take left channel)
        let mono_samples = if spec.channels == 2 {
            samples.chunks(2).map(|chunk| chunk[0]).collect()
        } else {
            samples
        };

        // Resample to 16kHz if needed (Whisper expects 16kHz)
        let whisper_samples = if sample_rate != 16000 {
            // Simple linear resampling (for production, use a proper resampler)
            let ratio = 16000.0 / sample_rate as f32;
            let new_len = (mono_samples.len() as f32 * ratio) as usize;
            let mut resampled = Vec::with_capacity(new_len);
            for i in 0..new_len {
                let src_idx = (i as f32 / ratio) as usize;
                if src_idx < mono_samples.len() {
                    resampled.push(mono_samples[src_idx]);
                }
            }
            resampled
        } else {
            mono_samples
        };

        // Create Whisper context
        let ctx = WhisperContext::new_with_params(
            &model_path.to_string_lossy(),
            WhisperContextParameters::default(),
        )
        .map_err(|e| LoggerError::Other(format!("Failed to create Whisper context: {}", e)))?;

        let mut state = ctx
            .create_state()
            .map_err(|e| LoggerError::Other(format!("Failed to create Whisper state: {}", e)))?;

        // Configure transcription parameters
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_language(Some("en"));
        params.set_translate(false);
        params.set_print_progress(false);
        params.set_print_special(false);

        // Run transcription
        state
            .full(params, &whisper_samples)
            .map_err(|e| LoggerError::Other(format!("Transcription failed: {}", e)))?;

        // Get segments and store in database
        let num_segments = state.full_n_segments();
        println!("   Transcribed {} segments", num_segments);

        for i in 0..num_segments {
            if let Some(segment) = state.get_segment(i) {
                // Format segment to get text (segment implements Display)
                let segment_str = format!("{}", segment);
                // Extract time from segment string format: "[start_time -> end_time] text"
                // Or just use the text part
                let text = segment_str.trim().to_string();

                if !text.is_empty() {
                    // Estimate time based on segment index and average segment duration
                    // Whisper segments are typically 1-3 seconds, use index as approximation
                    let estimated_start = i as f64 * 2.0; // Rough estimate: 2 seconds per segment
                    let estimated_end = estimated_start + 2.0;
                    let timestamp = Local::now().to_rfc3339();
                    let metadata = json!({
                        "start_time": estimated_start,
                        "end_time": estimated_end,
                        "duration": estimated_end - estimated_start,
                        "segment_index": i,
                        "text": text.clone(),
                        "segment_string": segment_str,
                        "monitor_desktop_audio": monitor_desktop_audio,
                        "source": if monitor_desktop_audio { "monitor_output" } else { "microphone" },
                    });

                    if let Err(e) = db
                        .insert_event(
                            session_id,
                            "transcription",
                            Some("segment"),
                            Some(&text), // Store text in key field for easy access
                            None,
                            None,
                            None,
                            None,
                            &timestamp,
                            Some(estimated_start), // Use estimated_start as timecode
                            Some(metadata),
                            None,
                        )
                        .await
                    {
                        eprintln!("⚠️  Failed to store transcription segment: {}", e);
                    }
                }
            }
        }

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
                let end_time = label.duration.map(|d| start_time + d).unwrap_or_else(|| {
                    // Default to 2 seconds if no duration specified
                    start_time + 2.0
                });

                // Escape text for FFmpeg
                // FFmpeg drawtext needs text escaped - replace single quotes and colons
                let escaped_text = label
                    .text
                    .replace('\\', "\\\\")
                    .replace('\'', "\\'")
                    .replace(':', "\\:");

                // Use proper FFmpeg filter syntax
                // For multiple drawtext filters, we chain them with commas
                // Use simpler positioning - bottom center for labels
                let x_pos = label
                    .x
                    .map(|x| x.to_string())
                    .unwrap_or_else(|| "(w-tw)/2".to_string()); // Center horizontally
                let y_pos = label
                    .y
                    .map(|y| y.to_string())
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
        println!(
            "   Timestamp overlay: {}",
            if show_timestamp { "✓" } else { "✗" }
        );
        println!("   Event labels: {} labels", labels.len());
        if !labels.is_empty() {
            for (idx, label) in labels.iter().take(5).enumerate() {
                println!(
                    "     {}. '{}' at {:.2}s",
                    idx + 1,
                    label.text,
                    label.timestamp
                );
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
        let output = cmd
            .output()
            .map_err(|e| LoggerError::Other(format!("Failed to execute FFmpeg: {}", e)))?;

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
            "json" => serde_json::to_string(event)
                .map_err(|e| LoggerError::Other(format!("Failed to serialize event: {}", e)))?,
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
    audio_handles: Vec<tokio::task::JoinHandle<Result<()>>>,
    audio_config_indices: Vec<usize>, // Maps handle index to config index
    process_handle: tokio::task::JoinHandle<Result<()>>,
    stop_signal: Arc<AtomicBool>,
    overlay_labels: Arc<std::sync::Mutex<Vec<OverlayLabel>>>,
    config: UnifiedRecordingConfig,
    click_context: Option<ClickContextHandle>,
    previous_default_source: Option<String>,
    previous_default_sink: Option<String>,
    loopback_module_ids: Vec<u32>,
}

impl RecordingSession {
    /// Stop the recording session.
    pub fn stop(&self) {
        self.stop_signal.store(true, Ordering::SeqCst);
    }

    /// Wait for the recording session to complete.
    /// This will wait until the stop signal is set (via stop() or duration expires).
    pub async fn wait(self) -> Result<()> {
        // Store audio recording start events if enabled
        let db = Database::new().await?;
        for audio_config in &self.config.audio_configs {
            if audio_config.enabled {
                let timestamp = Local::now().to_rfc3339();
                let metadata = json!({
                    "output_path": audio_config.output_path.to_string_lossy(),
                    "sample_rate": audio_config.sample_rate,
                    "channels": audio_config.channels,
                    "device_name": audio_config.device_name,
                    "monitor_desktop_audio": audio_config.monitor_desktop_audio,
                });
                if let Err(e) = db
                    .insert_event(
                        &self.session_id,
                        "audio",
                        Some("recording_start"),
                        None,
                        None,
                        None,
                        None,
                        None,
                        &timestamp,
                        None, // timecode
                        Some(metadata),
                        None, // screenshot_id
                    )
                    .await
                {
                    eprintln!("⚠️  Failed to store audio recording start event: {}", e);
                }
            }
        }

        // Wait for all tasks to complete
        // They will exit when stop_signal is set
        let input_result = self.input_handle.await;
        let screen_result = self.screen_handle.await;
        let process_result = self.process_handle.await;

        input_result
            .map_err(|e| LoggerError::Other(format!("Input capture task failed: {}", e)))??;
        screen_result
            .map_err(|e| LoggerError::Other(format!("Screen recording task failed: {}", e)))??;
        process_result
            .map_err(|e| LoggerError::Other(format!("Event processing task failed: {}", e)))??;

        // Wait for all audio recordings to complete
        for (handle_idx, audio_handle) in self.audio_handles.into_iter().enumerate() {
            let audio_result = audio_handle.await;
            // Get the config index for this handle
            let config_idx = self.audio_config_indices.get(handle_idx)
                .copied()
                .unwrap_or(handle_idx); // Fallback to handle_idx if mapping is missing
            eprintln!("🔍 Audio recording handle {} (config {}) result: {:?}", handle_idx, config_idx,
                audio_result.as_ref().map(|r| r.as_ref().map(|_| "Ok(())").map_err(|e| format!("Err({})", e))).map_err(|e| format!("JoinError({:?})", e)));
            match audio_result {
                Ok(Ok(())) => {
                    // Get the corresponding audio config using the mapped index
                    if let Some(audio_config) = self.config.audio_configs.get(config_idx) {
                        let timestamp = Local::now().to_rfc3339();
                        let metadata = json!({
                            "output_path": audio_config.output_path.to_string_lossy(),
                            "sample_rate": audio_config.sample_rate,
                            "channels": audio_config.channels,
                            "monitor_desktop_audio": audio_config.monitor_desktop_audio,
                        });
                        if let Err(e) = db
                            .insert_event(
                                &self.session_id,
                                "audio",
                                Some("recording_stop"),
                                None,
                                None,
                                None,
                                None,
                                None,
                                &timestamp,
                                None, // timecode
                                Some(metadata),
                                None, // screenshot_id
                            )
                            .await
                        {
                            eprintln!("⚠️  Failed to store audio recording stop event: {}", e);
                        } else {
                            let audio_type = if audio_config.monitor_desktop_audio {
                                "Desktop audio"
                            } else {
                                "Microphone audio"
                            };
                            
                            // Verify file actually exists before reporting success
                            let wav_path = if audio_config.output_path.is_absolute() {
                                audio_config.output_path.clone()
                            } else {
                                std::env::current_dir()
                                    .ok()
                                    .map(|cwd| cwd.join(&audio_config.output_path))
                                    .unwrap_or_else(|| audio_config.output_path.clone())
                            };
                            
                            if wav_path.exists() {
                                println!("✅ {} recording completed: {}", audio_type, wav_path.display());
                            } else {
                                eprintln!("⚠️  {} recording reported success but file not found at: {}", 
                                    audio_type, wav_path.display());
                            }
                        }

                        // Transcribe audio if enabled
                        if audio_config.transcribe {
                            if let Some(ref model_path) = audio_config.transcription_model_path {
                                let audio_type = if audio_config.monitor_desktop_audio {
                                    "desktop audio"
                                } else {
                                    "microphone audio"
                                };
                                
                                // Convert to absolute path and verify file exists
                                let wav_path = if audio_config.output_path.is_absolute() {
                                    audio_config.output_path.clone()
                                } else {
                                    // Convert relative path to absolute using current working directory
                                    std::env::current_dir()
                                        .map_err(|e| LoggerError::Io(std::io::Error::new(
                                            std::io::ErrorKind::Other,
                                            format!("Failed to get current directory: {}", e),
                                        )))?
                                        .join(&audio_config.output_path)
                                };
                                
                                // Verify file exists before attempting transcription
                                if !wav_path.exists() {
                                    eprintln!("⚠️  {} transcription skipped: WAV file not found at {}", 
                                        audio_type, wav_path.display());
                                } else {
                                    println!("🎤 Transcribing {} with model: {}...", audio_type, model_path.display());
                                    match UnifiedRecordingService::transcribe_wav_file(
                                        &wav_path,
                                        model_path,
                                        &self.session_id,
                                        &db,
                                        audio_config.monitor_desktop_audio,
                                    )
                                    .await
                                    {
                                        Ok(()) => {
                                            println!("✅ {} transcription completed", audio_type);
                                        }
                                        Err(e) => {
                                            eprintln!("⚠️  {} transcription failed: {}", audio_type, e);
                                        }
                                    }
                                }
                            } else {
                                eprintln!("⚠️  Transcription enabled but no model path specified");
                            }
                        } else {
                            let audio_type = if audio_config.monitor_desktop_audio {
                                "desktop audio"
                            } else {
                                "microphone audio"
                            };
                            println!("ℹ️  {} transcription disabled (no Whisper model found or specified)", audio_type);
                        }
                    }
                }
                Ok(Err(e)) => {
                    eprintln!("⚠️  Audio recording failed: {}", e);
                }
                Err(e) => {
                    eprintln!("⚠️  Audio recording task failed: {}", e);
                }
            }
        }

        // Restore previous default source/sink and clean up loopback modules
        #[cfg(target_os = "linux")]
        {
            use nexus_audio::AudioRecorder;
            
            // Restore default sink FIRST (before removing modules)
            if let Some(ref prev_sink) = self.previous_default_sink {
                eprintln!("🔄 Restoring previous default sink...");
                if let Err(e) = AudioRecorder::set_default_sink(prev_sink) {
                    eprintln!("⚠️  Failed to restore previous default sink: {}", e);
                } else {
                    eprintln!("✅ Restored previous default sink: {}", prev_sink);
                }
            }
            
            // Restore default source (only if we actually changed it - which we don't anymore with Option 2)
            // Keeping this for safety, but it should be a no-op since we don't change the default source
            if let Some(ref prev_source) = self.previous_default_source {
                // Check if current default is different (meaning something else changed it)
                if let Ok(current) = AudioRecorder::get_default_source() {
                    if current != *prev_source {
                        eprintln!("🔄 Restoring previous default source (was changed by something else)...");
                        if let Err(e) = AudioRecorder::set_default_source(prev_source) {
                            eprintln!("⚠️  Failed to restore previous default source: {}", e);
                        } else {
                            eprintln!("✅ Restored previous default source: {}", prev_source);
                        }
                    } else {
                        eprintln!("ℹ️  Default source unchanged (still '{}'), no restoration needed", prev_source);
                    }
                }
            }
            
            // Clean up loopback modules (after restoring defaults)
            if !self.loopback_module_ids.is_empty() {
                eprintln!("🧹 Cleaning up PulseAudio loopback sink...");
                for module_id in &self.loopback_module_ids {
                    if *module_id > 0 {
                        if let Err(e) = AudioRecorder::remove_pulseaudio_module(*module_id) {
                            eprintln!("⚠️  Failed to remove PulseAudio module {}: {}", module_id, e);
                        } else {
                            eprintln!("✅ Removed PulseAudio module {}", module_id);
                        }
                    }
                }
            }
        }

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

        if let (Some(ctx), Some(fps)) = (self.click_context, self.config.context_fps) {
            if fps > 0.0 {
                println!(
                    "\n🧠 Processing full recording context at {:.2} fps...",
                    fps
                );
                tokio::time::sleep(Duration::from_millis(1000)).await;
                let video_path = self.config.screen_config.output_path.clone();
                let video_duration = UnifiedRecordingService::get_video_duration(&video_path).ok();

                let metadata = json!({
                    "mode": "full_video",
                    "frames_per_second": fps,
                    "video_duration_seconds": video_duration,
                });

                let job = ProcessingJob::new(
                    "video_context",
                    format!("Full recording sweep ({:.2} fps)", fps),
                    Utc::now(),
                )
                .with_session_id(Some(self.session_id.clone()))
                .with_metadata(metadata)
                .with_full_video_sampling(video_path, fps);

                ctx.trigger_job(job);
                ctx.wait_for_completion().await;
                println!("✅ Full recording analysis complete");
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

    /// Generate and display a timeline of events for this session
    pub async fn generate_timeline(&self) -> Result<()> {
        let db = Database::new().await?;
        let events = db.get_session_events(&self.session_id).await?;

        if events.is_empty() {
            println!("📋 No events found for session {}", self.session_id);
            return Ok(());
        }

        // Get session start time from database, fallback to recording_start
        let session_start = db.get_session_start_time(&self.session_id).await?
            .unwrap_or(self.recording_start);

        print_timeline(&self.session_id, &events, session_start)
    }
}

/// Print timeline for a session
pub fn print_timeline(
    session_id: &str,
    events: &[crate::services::database::TimelineEvent],
    session_start: DateTime<Utc>,
) -> Result<()> {
    if events.is_empty() {
        println!("📋 No events found for session {}", session_id);
        return Ok(());
    }

    println!("\n╔══════════════════════════════════════════════════════════════════════════════╗");
    println!("║                          📋 Recording Timeline                                 ║");
    println!("╠══════════════════════════════════════════════════════════════════════════════╣");
    println!("║ Session ID: {:<64} ║", session_id);
    println!("║ Started:    {:<64} ║", session_start.with_timezone(&Local).format("%Y-%m-%d %H:%M:%S%.3f"));
    println!("╠══════════════════════════════════════════════════════════════════════════════╣");

    // Store events with their timecodes for proper timeline display
    #[derive(Clone)]
    struct TimedEvent {
        timecode: f64, // Always has a value now
        event_type: String,
        data: String,
    }

    let mut timed_events: Vec<TimedEvent> = Vec::new();

    // Find the earliest event timestamp to use as baseline
    // This ensures all events have positive timecodes relative to the first event
    let earliest_timestamp = events.iter()
        .map(|e| e.timestamp)
        .min()
        .unwrap_or(session_start);
    
    // Use the earlier of session_start or earliest_timestamp as baseline
    // This handles cases where events might be recorded slightly before session_start
    let baseline = if earliest_timestamp < session_start {
        earliest_timestamp
    } else {
        session_start
    };

    for event in events {
        // Use timecode if available, otherwise calculate from timestamp relative to baseline
        let event_timecode = event.timecode.or_else(|| {
            let elapsed = event.timestamp.signed_duration_since(baseline);
            let seconds = elapsed.num_milliseconds() as f64 / 1000.0;
            // Always return a timecode (clamp negative to 0.0 for safety)
            Some(seconds.max(0.0))
        });
        
        // Ensure we always have a timecode (should never be None after this point)
        let event_timecode = event_timecode.unwrap_or(0.0);
        
        match event.event_type.as_str() {
            "analysis" => {
                // Extract frame descriptions from metadata
                if let Some(metadata) = event.metadata.as_object() {
                    // Check if this is full video sampling (frames have absolute timestamps)
                    // The mode is stored in metadata.metadata (from job.metadata)
                    let is_full_video = metadata.get("metadata")
                        .and_then(|m| m.get("mode"))
                        .and_then(|v| v.as_str())
                        .map(|v| v == "full_video")
                        .unwrap_or(false);
                    
                    // Get base video timestamp from job metadata if available
                    let base_video_timestamp = metadata.get("job")
                        .and_then(|job| job.get("video_timestamp"))
                        .and_then(|v| v.as_f64());
                    
                    if let Some(frames) = metadata.get("frames") {
                        if let Some(frames_array) = frames.as_array() {
                            for frame in frames_array {
                                if let Some(desc) = frame.get("description").and_then(|v| v.as_str()) {
                                    let frame_offset = frame.get("offset_secs")
                                        .and_then(|v| v.as_f64())
                                        .unwrap_or(0.0);
                                    
                                    // Calculate absolute video timestamp for this frame
                                    // For full video, offset_secs is the absolute video timestamp (0.0, 0.2, 0.4, etc.)
                                    // For click context, offset is relative to click time (+0.0s, +0.2s, etc.)
                                    let frame_timecode = if is_full_video {
                                        // For full video, offset_secs is already the absolute video timestamp
                                        frame_offset
                                    } else if let Some(base) = base_video_timestamp {
                                        // For click context, offset is relative to click time
                                        base + frame_offset
                                    } else {
                                        // Heuristic: if offset is small (< 100s) and first frame is near 0, 
                                        // it's likely an absolute timestamp from full video sampling
                                        // Otherwise, if offset is very small (< 5s), assume it's relative to some base
                                        // But we don't have the base, so use offset directly as a best guess
                                        if frame_offset < 100.0 && frame_offset >= 0.0 {
                                            // Likely absolute timestamp from full video
                                            frame_offset
                                        } else {
                                            // Can't determine - this shouldn't happen, but use offset as fallback
                                            frame_offset
                                        }
                                    };
                                    
                                    let offset_str = if is_full_video {
                                        format!("{:.2}s", frame_offset)
                                    } else {
                                        format!("+{:.2}s", frame_offset)
                                    };
                                    
                                    timed_events.push(TimedEvent {
                                        timecode: frame_timecode,
                                        event_type: "frame".to_string(),
                                        data: format!("{} {}", offset_str, desc),
                                    });
                                }
                            }
                        }
                    }
                    if let Some(summary) = metadata.get("summary").and_then(|v| v.as_str()) {
                        // For summary, use the timecode of the last frame or event timecode
                        let summary_timecode = timed_events.iter()
                            .filter(|e| e.event_type == "frame")
                            .last()
                            .map(|e| e.timecode)
                            .unwrap_or(event_timecode);
                        
                        timed_events.push(TimedEvent {
                            timecode: summary_timecode,
                            event_type: "summary".to_string(),
                            data: format!("Summary: {}", summary),
                        });
                    }
                }
            }
            "keyboard" => {
                if let Some(key) = &event.key {
                    if event.pressed.unwrap_or(false) {
                        timed_events.push(TimedEvent {
                            timecode: event_timecode,
                            event_type: "key".to_string(),
                            data: key.clone(),
                        });
                    }
                }
            }
            "mouse" => {
                if event.event_subtype.as_deref() == Some("click") {
                    let button = event.button.as_deref().unwrap_or("unknown");
                    let coords = if let (Some(x), Some(y)) = (event.x, event.y) {
                        format!("({}, {})", x, y)
                    } else {
                        String::new()
                    };
                    timed_events.push(TimedEvent {
                        timecode: event_timecode,
                        event_type: "click".to_string(),
                        data: format!("{} {}", button, coords),
                    });
                }
            }
            "transcription" => {
                if event.event_subtype.as_deref() == Some("segment") {
                    // Extract text from metadata
                    if let Some(metadata) = event.metadata.as_object() {
                        if let Some(text) = metadata.get("text").and_then(|v| v.as_str()) {
                            // Extract source information (monitor_output or microphone)
                            let source = metadata.get("source")
                                .and_then(|v| v.as_str())
                                .unwrap_or_else(|| {
                                    // Fallback: check monitor_desktop_audio flag
                                    if metadata.get("monitor_desktop_audio")
                                        .and_then(|v| v.as_bool())
                                        .unwrap_or(false)
                                    {
                                        "monitor_output"
                                    } else {
                                        "microphone"
                                    }
                                });
                            timed_events.push(TimedEvent {
                                timecode: event_timecode,
                                event_type: "transcription".to_string(),
                                data: format!("[{}] {}", source, text),
                            });
                        } else if let Some(key) = event.key.as_ref() {
                            // Fallback: use key field if metadata doesn't have text
                            let source = metadata.get("source")
                                .and_then(|v| v.as_str())
                                .unwrap_or_else(|| {
                                    if metadata.get("monitor_desktop_audio")
                                        .and_then(|v| v.as_bool())
                                        .unwrap_or(false)
                                    {
                                        "monitor_output"
                                    } else {
                                        "microphone"
                                    }
                                });
                            timed_events.push(TimedEvent {
                                timecode: event_timecode,
                                event_type: "transcription".to_string(),
                                data: format!("[{}] {}", source, key),
                            });
                        }
                    } else if let Some(key) = event.key.as_ref() {
                        // Fallback: use key field (no metadata available, assume microphone)
                        timed_events.push(TimedEvent {
                            timecode: event_timecode,
                            event_type: "transcription".to_string(),
                            data: format!("[microphone] {}", key),
                        });
                    }
                }
            }
            _ => {}
        }
    }

    // Sort by timecode
    timed_events.sort_by(|a, b| {
        a.timecode.partial_cmp(&b.timecode).unwrap_or(std::cmp::Ordering::Equal)
    });

    // Group events by timecode for display (with small tolerance for grouping)
    let mut current_timecode: Option<f64> = None;
    let mut frame_descriptions: Vec<String> = Vec::new();
    let mut keys_entered: Vec<String> = Vec::new();
    let mut clicks: Vec<(f64, String)> = Vec::new(); // Store timecode with each click
    let mut transcriptions: Vec<(f64, String)> = Vec::new(); // Store timecode with each transcription

    for timed_event in timed_events {
        // If timecode changed significantly (more than 0.1s difference), print previous group
        let timecode_changed = if let Some(current) = current_timecode {
            (current - timed_event.timecode).abs() > 0.1
        } else {
            true
        };
        
        if current_timecode.is_some() && timecode_changed {
            print_timeline_entry(
                current_timecode,
                &frame_descriptions,
                &keys_entered,
                &clicks,
                &transcriptions,
            );
            frame_descriptions.clear();
            keys_entered.clear();
            clicks.clear();
            transcriptions.clear();
        }

        current_timecode = Some(timed_event.timecode);

        match timed_event.event_type.as_str() {
            "frame" | "summary" => {
                frame_descriptions.push(timed_event.data);
            }
            "key" => {
                keys_entered.push(timed_event.data);
            }
            "click" => {
                clicks.push((timed_event.timecode, timed_event.data));
            }
            "transcription" => {
                transcriptions.push((timed_event.timecode, timed_event.data));
            }
            _ => {}
        }
    }

    // Print final group
    if !frame_descriptions.is_empty() || !keys_entered.is_empty() || !clicks.is_empty() || !transcriptions.is_empty() {
        print_timeline_entry(
            current_timecode,
            &frame_descriptions,
            &keys_entered,
            &clicks,
            &transcriptions,
        );
    }

    println!("╚══════════════════════════════════════════════════════════════════════════════╝\n");

    Ok(())
}

fn print_timeline_entry(
    timecode: Option<f64>,
    frame_descriptions: &[String],
    keys_entered: &[String],
    clicks: &[(f64, String)],
    transcriptions: &[(f64, String)],
) {
    let time_str = if let Some(tc) = timecode {
        format!("{:>8.2}s", tc)
    } else {
        "        ".to_string()
    };

    println!("║ Time: {}                                                                    ║", time_str);

    if !frame_descriptions.is_empty() {
        println!("║ 🧠 Frame Analysis:                                                          ║");
        for desc in frame_descriptions {
            // Wrap long descriptions
            let wrapped = wrap_text(desc, 75);
            for line in wrapped {
                println!("║    {}", pad_right(&line, 75));
            }
        }
    }

    if !keys_entered.is_empty() {
        let keys_str = keys_entered.join(", ");
        println!("║ ⌨️  Keys: {}", pad_right(&keys_str, 70));
    }

    if !clicks.is_empty() {
        for (click_timecode, click_data) in clicks {
            // Show timecode for each click if different from group timecode
            let click_display = if let Some(group_tc) = timecode {
                if (group_tc - click_timecode).abs() > 0.1 {
                    format!("@ {:.2}s: {}", click_timecode, click_data)
                } else {
                    click_data.clone()
                }
            } else {
                format!("@ {:.2}s: {}", click_timecode, click_data)
            };
            println!("║ 🖱️  Click: {}", pad_right(&click_display, 70));
        }
    }

    if !transcriptions.is_empty() {
        for (trans_timecode, trans_text) in transcriptions {
            // Parse source from text (format: "[source] text")
            let (source, text) = if trans_text.starts_with('[') {
                if let Some(end_bracket) = trans_text.find(']') {
                    let source_str = &trans_text[1..end_bracket];
                    let text_part = trans_text[end_bracket + 1..].trim_start();
                    (source_str, text_part)
                } else {
                    ("unknown", trans_text.as_str())
                }
            } else {
                ("microphone", trans_text.as_str())
            };
            
            // Format source display
            let source_display = match source {
                "monitor_output" => "📺 Monitor Output",
                "microphone" => "🎙️  Microphone",
                _ => "🎤 Unknown",
            };
            
            // Show timecode for each transcription if different from group timecode
            let timecode_prefix = if let Some(group_tc) = timecode {
                if (group_tc - trans_timecode).abs() > 0.1 {
                    format!("@ {:.2}s: ", trans_timecode)
                } else {
                    String::new()
                }
            } else {
                format!("@ {:.2}s: ", trans_timecode)
            };
            
            // Calculate available space for text (box is 78 chars wide, minus borders and prefix)
            // Box format: "║ " (2) + prefix + text + " ║" (2) = 78
            // Available space = 78 - 2 - prefix_len - 2 = 74 - prefix_len
            let prefix = format!("{} Transcription: ", source_display);
            let prefix_len = prefix.chars().count(); // Use char count for proper emoji handling
            let available_width = 74 - prefix_len; // 74 = 78 - 2 (left border) - 2 (right border)
            
            // Wrap long transcriptions to fit available width
            let full_text = format!("{}{}", timecode_prefix, text);
            let wrapped = wrap_text(&full_text, available_width);
            for (idx, line) in wrapped.iter().enumerate() {
                if idx == 0 {
                    println!("║ {}{}", prefix, pad_right(&line, available_width));
                } else {
                    // Continuation lines: "║    " (5 chars) + text + " ║" (2) = 78
                    let continuation_width = 74 - 4; // 4 chars for "    " indent
                    println!("║    {}", pad_right(&line, continuation_width));
                }
            }
        }
    }

    println!("╠══════════════════════════════════════════════════════════════════════════════╣");
}

fn wrap_text(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut current_line = String::new();

    for word in words {
        if current_line.is_empty() {
            current_line = word.to_string();
        } else if current_line.len() + word.len() + 1 <= width {
            current_line.push(' ');
            current_line.push_str(word);
        } else {
            lines.push(current_line);
            current_line = word.to_string();
        }
    }
    if !current_line.is_empty() {
        lines.push(current_line);
    }
    lines
}

fn pad_right(s: &str, width: usize) -> String {
    if s.len() >= width {
        s.chars().take(width).collect()
    } else {
        format!("{:<width$}", s, width = width)
    }
}
