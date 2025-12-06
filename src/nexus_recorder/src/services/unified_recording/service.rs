//! Core service implementation for unified recording.
//!
//! This module contains the `UnifiedRecordingService` and `RecordingSession` structs
//! that orchestrate screen, audio, and input recording.

use crate::error::{RecorderError, Result};
use crate::services::audio::{AudioRecorder, AudioRecordingConfig as AudioRecordingConfigInternal};
use crate::services::storage::{BatchEvent, BatchEventInserter, Database, RotatingEventWriter, RotatingEventWriterHandle};
use crate::services::input::InputEvent;
use crate::services::context::{ClickContextHandle, ClickContextService};
use crate::services::context::context_processing::{ProcessingHandle, ProcessingJob, ProcessingService};
use chrono::{DateTime, Local, Utc};
use log::{error, info, warn};
use serde_json::json;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use uuid::Uuid;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use super::config::{
    AudioRecordingConfig, DefaultEventCallback, EventCallback, InputCaptureConfig, OverlayLabel,
    ScreenRecordingConfig, UnifiedRecordingConfig, AUDIO_BUFFER_DURATION_MS, INPUT_POLL_INTERVAL_MS,
};
use super::timeline::print_timeline;

/// Helper enum to support both bounded and unbounded event senders
enum EventSender {
    Bounded(mpsc::Sender<(InputEvent, Instant)>),
    Unbounded(mpsc::UnboundedSender<(InputEvent, Instant)>),
}

impl EventSender {
    /// Try to send an event (non-blocking)
    fn try_send(&self, event: (InputEvent, Instant)) -> bool {
        match self {
            EventSender::Bounded(tx) => tx.try_send(event).is_ok(),
            EventSender::Unbounded(tx) => tx.send(event).is_ok(),
        }
    }
}

/// Helper enum to support both bounded and unbounded event receivers
enum EventReceiver {
    Bounded(mpsc::Receiver<(InputEvent, Instant)>),
    Unbounded(mpsc::UnboundedReceiver<(InputEvent, Instant)>),
}

impl EventReceiver {
    /// Receive an event asynchronously
    async fn recv(&mut self) -> Option<(InputEvent, Instant)> {
        match self {
            EventReceiver::Bounded(rx) => rx.recv().await,
            EventReceiver::Unbounded(rx) => rx.recv().await,
        }
    }
}

/// Shared context for spawned tasks that need access to recording state.
///
/// This struct wraps frequently-needed `Arc`s to reduce repetitive cloning
/// in the event processing loop. A single `.clone()` of `RecordingContext`
/// provides all the shared state a spawned task needs.
#[derive(Clone)]
struct RecordingContext {
    /// Database connection (Arc for cheap cloning)
    db: Arc<Database>,
    /// Session ID (Arc<str> to avoid String allocation on each clone)
    session_id: Arc<str>,
}

impl RecordingContext {
    fn new(db: Arc<Database>, session_id: Arc<str>) -> Self {
        Self { db, session_id }
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
            .map_err(|e| RecorderError::Other(format!("Failed to run ffprobe: {}", e)))?;

        if !output.status.success() {
            return Err(RecorderError::Other(
                "FFprobe failed to get video duration".to_string(),
            ));
        }

        let duration_str = String::from_utf8_lossy(&output.stdout);
        duration_str
            .trim()
            .parse::<f64>()
            .map_err(|e| RecorderError::Other(format!("Failed to parse video duration: {}", e)))
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
    ///
    /// # Errors
    ///
    /// Returns [`RecorderError`] if recording initialization fails (database, audio, screen capture, or input capture).
    /// Starts a unified recording session (screen, audio, and input capture).
    ///
    /// # Errors
    ///
    /// Returns [`RecorderError::Database`] if database initialization fails.
    /// Returns [`RecorderError::ScreenCapture`] if screen recording initialization fails.
    /// Returns [`RecorderError::Audio`] if audio recording initialization fails.
    /// Returns [`RecorderError::InputCapture`] if input capture initialization fails.
    /// Returns [`RecorderError::Io`] if file operations fail.
    #[must_use = "Recording may fail and the Result should be handled"]
    pub async fn start_recording(&self, stop_signal: Arc<AtomicBool>) -> Result<RecordingSession> {
        // Use Arc<str> for session_id to avoid cloning String in hot paths
        let session_id: Arc<str> = Arc::from(Uuid::new_v4().to_string());
        let recording_start = Utc::now();
        let recording_start_instant = Instant::now();

        // Initialize database
        let db = Database::new().await?;
        db.create_session(&session_id).await?;
        let click_context = ClickContextService::maybe_start(db.clone());
        let db = Arc::new(db);

        // Channel for events from input capture
        // Use bounded channel if batch inserter is configured for backpressure
        const EVENT_CHANNEL_SIZE: usize = 10_000;
        let (event_tx, event_rx) = if self.config.batch_inserter_config.is_some() {
            let (tx, rx) = mpsc::channel::<(InputEvent, Instant)>(EVENT_CHANNEL_SIZE);
            (EventSender::Bounded(tx), EventReceiver::Bounded(rx))
        } else {
            let (tx, rx) = mpsc::unbounded_channel::<(InputEvent, Instant)>();
            (EventSender::Unbounded(tx), EventReceiver::Unbounded(rx))
        };

        // Start batch inserter if configured
        let batch_inserter = if let Some(ref batch_config) = self.config.batch_inserter_config {
            let (handle, task) =
                BatchEventInserter::spawn(batch_config.clone(), stop_signal.clone())?;
            Some((handle, task))
        } else {
            None
        };

        // Start rotating event writer if configured
        let rotating_writer = if let Some(ref writer_config) = self.config.event_writer_config {
            let (handle, task) =
                RotatingEventWriter::spawn(writer_config.clone(), stop_signal.clone())?;
            Some((handle, task))
        } else {
            None
        };
        let rotating_writer_handle = rotating_writer.as_ref().map(|(h, _)| h.clone());

        // Start input capture in background
        let input_config = self.config.input_config.clone();
        let capture_keyboard = self.config.capture_keyboard;
        let capture_mouse = self.config.capture_mouse;
        let capture_mouse_moves = self.config.capture_mouse_moves;
        let callback_clone = Arc::clone(&self.callback);
        
        // Create shared context for spawned tasks (single struct clone vs multiple Arc clones)
        let ctx = RecordingContext::new(Arc::clone(&db), Arc::clone(&session_id));
        let ctx_for_process = ctx.clone();
        
        let recording_start_clone = recording_start;
        let stop_signal_input = stop_signal.clone();

        let click_context_for_input = click_context.clone();
        let session_id_for_input = Arc::clone(&session_id);
        // Use screen output path for input events (clicks/keyboard are associated with desktop)
        // Webcam video won't have input events, only desktop recording will
        let video_path_for_input = self.config.screen_config.as_ref()
            .map(|c| c.output_path.clone())
            .or_else(|| self.config.webcam_config.as_ref().map(|c| c.output_path.clone()))
            .unwrap_or_else(|| PathBuf::from("output/recording.mp4"));
        let rotating_writer_for_input = rotating_writer_handle.clone();
        let input_handle = tokio::task::spawn_blocking(move || {
            Self::run_input_capture_blocking(
                capture_keyboard,
                capture_mouse,
                capture_mouse_moves,
                input_config,
                event_tx,
                stop_signal_input,
                rotating_writer_for_input,
                click_context_for_input,
                session_id_for_input,
                video_path_for_input,
            )
        });

        // Start screen recording in background (if enabled)
        let screen_handle = if let Some(screen_config) = self.config.screen_config.clone() {
            let stop_signal_screen = stop_signal.clone();
            Some(tokio::task::spawn_blocking(move || {
                Self::run_screen_recording_blocking(screen_config, stop_signal_screen)
            }))
        } else {
            None
        };

        // Start webcam recording in background (if enabled)
        let webcam_handle = if let Some(webcam_config) = self.config.webcam_config.clone() {
            let stop_signal_webcam = stop_signal.clone();
            Some(tokio::task::spawn_blocking(move || {
                Self::run_webcam_recording_blocking(webcam_config, stop_signal_webcam)
            }))
        } else {
            None
        };

        // Start webcam sentiment analysis (if enabled and webcam is recording)
        // Use the same ProcessingService that handles click context
        let webcam_analysis_handle = if self.config.webcam_config.is_some() {
            if let Some(analysis_config) = self.config.webcam_analysis_config.clone() {
                let video_path = self.config.webcam_config.as_ref()
                    .map(|c| c.output_path.clone())
                    .unwrap_or_else(|| PathBuf::from("output/recording.mp4"));
                let video_path = if video_path.is_absolute() {
                    video_path
                } else {
                    std::env::current_dir()
                        .ok()
                        .map(|cwd| cwd.join(&video_path))
                        .unwrap_or(video_path)
                };
                
                // Start or reuse ProcessingService for webcam analysis
                match Database::new().await {
                    Ok(db) => {
                        // Use ProcessingService - start if not already started
                        let processing_config = crate::services::context::context_processing::ProcessingConfig::from_env();
                        match ProcessingService::start(processing_config, db) {
                            Ok(handle) => {
                                // Start periodic webcam analysis jobs
                                if let Err(e) = ProcessingService::start_webcam_analysis(
                                    handle.clone(),
                                    analysis_config.interval_secs,
                                    session_id.to_string(),
                                    video_path,
                                    stop_signal.clone(),
                                ) {
                                    warn!("⚠️  Failed to start webcam analysis: {}", e);
                                    None
                                } else {
                                    Some(handle)
                                }
                            }
                            Err(e) => {
                                warn!("⚠️  Failed to start processing service for webcam analysis: {}", e);
                                None
                            }
                        }
                    }
                    Err(e) => {
                        warn!("⚠️  Failed to create database for webcam analysis: {}", e);
                        None
                    }
                }
            } else {
                None
            }
        } else {
            None
        };

        // Start periodic context processing (if enabled)
        let periodic_context_handle = if self.config.periodic_context_enabled {
            if let Some(interval_secs) = self.config.periodic_context_interval_secs {
                // Get or create ProcessingService handle
                let processing_handle = if let Some(ref ctx) = click_context {
                    // Reuse click context handle if available
                    ctx.inner().clone()
                } else {
                    // Create new ProcessingService if click context is disabled
                    let processing_config = crate::services::context::context_processing::ProcessingConfig::from_env();
                    match ProcessingService::start(processing_config, (*db).clone()) {
                        Ok(handle) => handle,
                        Err(e) => {
                            warn!("⚠️  Failed to start processing service for periodic context: {}", e);
                            return Err(RecorderError::Other(format!(
                                "Failed to start processing service: {}", e
                            )));
                        }
                    }
                };

                // Use screen video for periodic context (input events are associated with desktop)
                let video_path = self.config.screen_config.as_ref()
                    .map(|c| c.output_path.clone())
                    .or_else(|| self.config.webcam_config.as_ref().map(|c| c.output_path.clone()))
                    .unwrap_or_else(|| PathBuf::from("output/recording.mp4"));
                
                let video_path = if video_path.is_absolute() {
                    video_path
                } else {
                    std::env::current_dir()
                        .ok()
                        .map(|cwd| cwd.join(&video_path))
                        .unwrap_or(video_path)
                };

                let session_id_str = session_id.to_string();
                let frames_per_interval = self.config.periodic_context_frames_per_interval;
                let stop_signal_periodic = stop_signal.clone();
                let recording_start_instant_clone = recording_start_instant;

                Some(tokio::spawn(async move {
                    // Wait for initial delay (1 minute to accumulate data)
                    info!("🔄 Periodic context processing: waiting 60s for initial data accumulation...");
                    tokio::time::sleep(Duration::from_secs(60)).await;

                    let mut last_processed_timestamp = 0.0f64;
                    let mut interval_count = 0u64;

                    loop {
                        if stop_signal_periodic.load(Ordering::SeqCst) {
                            info!("🔄 Periodic context processing stopping...");
                            break;
                        }

                        // Calculate current video timestamp from elapsed time
                        let elapsed = recording_start_instant_clone.elapsed();
                        let current_timestamp = elapsed.as_secs_f64();

                        // Check if we have enough time elapsed since last processing
                        if current_timestamp - last_processed_timestamp >= interval_secs as f64 {
                            let interval_start = last_processed_timestamp;
                            let interval_end = current_timestamp;

                            info!(
                                "🔄 Processing periodic context interval {}: {:.1}s - {:.1}s",
                                interval_count + 1,
                                interval_start,
                                interval_end
                            );

                            // Create processing job for this interval
                            let job = ProcessingJob::new(
                                "periodic_context",
                                format!("interval_{}", interval_count),
                                Utc::now(),
                            )
                            .with_session_id(Some(session_id_str.clone()))
                            .with_video_context(Some(interval_start), Some(video_path.clone()))
                            .with_metadata(json!({
                                "interval_start": interval_start,
                                "interval_end": interval_end,
                                "interval_duration": interval_end - interval_start,
                                "interval_count": interval_count,
                                "frames_per_interval": frames_per_interval,
                            }));

                            // Trigger job (non-blocking)
                            processing_handle.trigger(job);

                            last_processed_timestamp = current_timestamp;
                            interval_count += 1;
                        }

                        // Sleep for a short time before checking again
                        tokio::time::sleep(Duration::from_secs(interval_secs)).await;
                    }

                    info!("🔄 Periodic context processing completed: {} intervals processed", interval_count);
                }))
            } else {
                None
            }
        } else {
            None
        };

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
                let mut loopback_created = false;
                #[cfg(target_os = "linux")]
                {
                    use crate::services::audio::AudioRecorder;
                    info!("📺 Setting up desktop audio monitoring...");

                    // Capture current default source
                    if let Ok(current_source) = AudioRecorder::get_default_source() {
                        info!("📝 Current default source: {}", current_source);
                        previous_default_source = Some(current_source);
                    }

                    // Create loopback sink (this also sets combine-sink as default output)
                    match AudioRecorder::create_loopback_sink(None) {
                        Ok((monitor_name, module_ids, prev_sink)) => {
                            info!("✅ Created loopback sink: {}", monitor_name);
                            loopback_module_ids.extend(module_ids);
                            previous_default_sink = Some(prev_sink);
                            loopback_created = true;

                            // DON'T change the default source - we'll use the monitor source name directly
                            // This allows the microphone to continue using the default source
                            info!("ℹ️  Using monitor source '{}' directly (not changing default source)", monitor_name);
                            info!(
                                "   This allows microphone to use default source simultaneously"
                            );
                        }
                        Err(e) => {
                            warn!("⚠️  Failed to create loopback sink: {}", e);
                            warn!("   Desktop audio monitoring will be disabled, but recording will continue");
                            // Don't create audio handle for this config, but continue with other configs
                            loopback_created = false;
                        }
                    }

                    // Give a moment for the loopback sink to be ready
                    if loopback_created {
                        tokio::time::sleep(Duration::from_millis(200)).await;
                    }
                }

                // Only create audio handle if loopback was successfully created (or on non-Linux)
                #[cfg(not(target_os = "linux"))]
                let loopback_created = true; // On non-Linux, always proceed

                if loopback_created {
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

        // Extract batch inserter handle if available
        let batch_inserter_handle = batch_inserter.as_ref().map(|(h, _)| h.clone());

        // Process events and call callbacks
        let stop_signal_process = stop_signal.clone();
        let video_start_time = recording_start_instant;
        let mut event_rx = event_rx;
        let process_handle = tokio::spawn(async move {
            loop {
                // Check stop signal first
                if stop_signal_process.load(Ordering::SeqCst) {
                    break;
                }

                // Try to receive event with timeout to allow periodic stop signal checks
                let event_result =
                    tokio::time::timeout(Duration::from_millis(AUDIO_BUFFER_DURATION_MS), event_rx.recv()).await;

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
                    if let Ok(mut guard) = overlay_labels_clone.lock() {
                        guard.push(label.clone());
                    } else {
                        warn!("Overlay labels mutex poisoned, label not stored");
                    }

                    // Log overlay label to database (always use direct insert for overlay labels)
                    let ctx = ctx_for_process.clone();
                    let label_text = label.text.clone();
                    let label_timestamp = label.timestamp;
                    let label_duration = label.duration;
                    let label_x = label.x;
                    let label_y = label.y;
                    let timestamp_for_label = Local::now().to_rfc3339();
                    tokio::spawn(async move {
                        let metadata = json!({
                            "text": label_text,
                            "duration": label_duration,
                            "x": label_x,
                            "y": label_y,
                        });
                        if let Err(e) = ctx.db
                            .insert_event(
                                &ctx.session_id,
                                "overlay",
                                Some("label"),
                                None,
                                None,
                                label_x.map(|x| x as i32),
                                label_y.map(|y| y as i32),
                                None,
                                &timestamp_for_label,
                                Some(label_timestamp), // timecode - video timestamp when label appears
                                Some(metadata),
                                None, // screenshot_id
                            )
                            .await
                        {
                            warn!("⚠️  Failed to store overlay label in database: {}", e);
                        }
                    });
                }

                if should_store {
                    // Store in database (errors are logged but don't stop recording)
                    let timestamp = Local::now().to_rfc3339();
                    let video_timestamp_for_db = video_timestamp;

                    // Use batch inserter if available, otherwise fall back to direct inserts
                    if let Some(ref inserter) = batch_inserter_handle {
                        match &event {
                            InputEvent::Keyboard { key, pressed, .. } => {
                                let batch_event = BatchEvent {
                                    session_id: Arc::clone(&ctx_for_process.session_id),
                                    event_type: "keyboard".to_string(),
                                    event_subtype: Some(
                                        if *pressed { "press" } else { "release" }.to_string(),
                                    ),
                                    key: Some(key.clone()),
                                    button: None,
                                    x: None,
                                    y: None,
                                    pressed: Some(*pressed),
                                    timestamp: timestamp.clone(),
                                    timecode: Some(video_timestamp_for_db),
                                    metadata: None,
                                    screenshot_id: None,
                                };
                                if !inserter.try_insert(batch_event) {
                                    warn!("⚠️  Batch inserter buffer full, event dropped");
                                }

                                // Update key frequency (still direct since it's aggregate data)
                                if *pressed {
                                    let ctx = ctx_for_process.clone();
                                    let key_for_freq = key.clone();
                                    tokio::spawn(async move {
                                        if let Err(e) = ctx.db
                                            .update_key_frequency(
                                                &ctx.session_id,
                                                &key_for_freq,
                                            )
                                            .await
                                        {
                                            warn!("⚠️  Failed to update key frequency: {}", e);
                                        }
                                    });
                                }
                            }
                            InputEvent::Mouse {
                                event_type,
                                button,
                                x,
                                y,
                                timestamp: _,
                            } => {
                                let batch_event = BatchEvent {
                                    session_id: Arc::clone(&ctx_for_process.session_id),
                                    event_type: "mouse".to_string(),
                                    event_subtype: Some(event_type.clone()),
                                    key: None,
                                    button: button.clone(),
                                    x: *x,
                                    y: *y,
                                    pressed: None,
                                    timestamp: timestamp.clone(),
                                    timecode: Some(video_timestamp_for_db),
                                    metadata: None,
                                    screenshot_id: None,
                                };
                                if !inserter.try_insert(batch_event) {
                                    warn!("⚠️  Batch inserter buffer full, event dropped");
                                }

                                // Update button frequency (still direct since it's aggregate data)
                                if event_type == "click" {
                                    if let Some(ref btn) = button {
                                        let ctx = ctx_for_process.clone();
                                        let btn_for_freq = btn.clone();
                                        tokio::spawn(async move {
                                            if let Err(e) = ctx.db
                                                .update_mouse_button_frequency(
                                                    &ctx.session_id,
                                                    &btn_for_freq,
                                                )
                                                .await
                                            {
                                                warn!("⚠️  Failed to update mouse button frequency: {}", e);
                                            }
                                        });
                                    }
                                }
                            }
                        }
                    } else {
                        // Legacy direct insert path
                        match &event {
                            InputEvent::Keyboard { key, pressed, .. } => {
                                let ctx = ctx_for_process.clone();
                                let key_for_event = key.clone();
                                let timestamp_for_event = timestamp.clone();
                                let pressed_for_event = *pressed;
                                tokio::spawn(async move {
                                    if let Err(e) = ctx.db
                                        .insert_event(
                                            &ctx.session_id,
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
                                            Some(video_timestamp_for_db),
                                            None,
                                            None,
                                        )
                                        .await
                                    {
                                        warn!(
                                            "⚠️  Failed to store keyboard event in database: {}",
                                            e
                                        );
                                    }

                                    if pressed_for_event {
                                        if let Err(e) = ctx.db
                                            .update_key_frequency(
                                                &ctx.session_id,
                                                &key_for_event,
                                            )
                                            .await
                                        {
                                            warn!("⚠️  Failed to update key frequency: {}", e);
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
                                let ctx = ctx_for_process.clone();
                                let event_type_for_event = event_type.clone();
                                let button_for_event = button.clone();
                                let x_for_event = *x;
                                let y_for_event = *y;
                                let timestamp_for_event = timestamp.clone();
                                tokio::spawn(async move {
                                    if let Err(e) = ctx.db
                                        .insert_event(
                                            &ctx.session_id,
                                            "mouse",
                                            Some(&event_type_for_event),
                                            None,
                                            button_for_event.as_deref(),
                                            x_for_event,
                                            y_for_event,
                                            None,
                                            &timestamp_for_event,
                                            Some(video_timestamp_for_db),
                                            None,
                                            None,
                                        )
                                        .await
                                    {
                                        warn!(
                                            "⚠️  Failed to store mouse event in database: {}",
                                            e
                                        );
                                    }

                                    if event_type_for_event == "click" {
                                        if let Some(ref btn) = button_for_event {
                                            if let Err(e) = ctx.db
                                                .update_mouse_button_frequency(
                                                    &ctx.session_id,
                                                    btn,
                                                )
                                                .await
                                            {
                                                warn!(
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
            }

            // Flush batch inserter if used
            if let Some(ref inserter) = batch_inserter_handle {
                if let Err(e) = inserter.flush().await {
                    warn!("⚠️  Failed to flush batch inserter: {}", e);
                }
            }

            // Labels are stored in the shared Arc<Mutex<Vec<OverlayLabel>>>
            // They will be applied after screen recording completes

            Ok::<(), RecorderError>(())
        });

        Ok(RecordingSession {
            session_id,
            recording_start,
            input_handle,
            screen_handle,
            webcam_handle,
            webcam_analysis_handle,
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
            periodic_context_handle,
        })
    }

    fn run_input_capture_blocking(
        capture_keyboard: bool,
        capture_mouse: bool,
        capture_mouse_moves: bool,
        input_config: InputCaptureConfig,
        event_tx: EventSender,
        stop_signal: Arc<AtomicBool>,
        rotating_writer: Option<RotatingEventWriterHandle>,
        _click_context: Option<ClickContextHandle>,
        _session_id: Arc<str>,
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

        // Open output file if specified (only if not using rotating writer)
        let mut file_handle: Option<std::fs::File> = if rotating_writer.is_none() {
            if let Some(ref path) = input_config.output_file {
                Some(
                    OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(path)
                        .map_err(|e| {
                            RecorderError::Io(std::io::Error::new(
                                std::io::ErrorKind::Other,
                                format!("Failed to open output file: {}", e),
                            ))
                        })?,
                )
            } else {
                None
            }
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
                        Self::write_event_output_with_rotation(
                            &event,
                            &input_config.format,
                            &mut file_handle,
                            &rotating_writer,
                        )?;
                        let _ = event_tx.try_send((event, event_time));
                    }
                }

                for key in &last_keys {
                    if !keys_set.contains(key) {
                        let event = InputEvent::Keyboard {
                            key: format!("{:?}", key),
                            pressed: false,
                            timestamp: timestamp.clone(),
                        };
                        Self::write_event_output_with_rotation(
                            &event,
                            &input_config.format,
                            &mut file_handle,
                            &rotating_writer,
                        )?;
                        let _ = event_tx.try_send((event, event_time));
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
                        Self::write_event_output_with_rotation(
                            &event,
                            &input_config.format,
                            &mut file_handle,
                            &rotating_writer,
                        )?;
                        let _ = event_tx.try_send((event, event_time));
                        // Don't process clicks during recording; batch-process after video is complete
                    } else if !pressed && was_pressed {
                        let event = InputEvent::Mouse {
                            event_type: "release".to_string(),
                            button: Some(button_name.clone()),
                            x: Some(mouse.coords.0),
                            y: Some(mouse.coords.1),
                            timestamp: timestamp.clone(),
                        };
                        Self::write_event_output_with_rotation(
                            &event,
                            &input_config.format,
                            &mut file_handle,
                            &rotating_writer,
                        )?;
                        let _ = event_tx.try_send((event, event_time));
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
                    Self::write_event_output_with_rotation(
                        &event,
                        &input_config.format,
                        &mut file_handle,
                        &rotating_writer,
                    )?;
                    let _ = event_tx.try_send((event, event_time));
                }

                last_mouse_buttons = mouse.button_pressed.clone();
            }

            std::thread::sleep(Duration::from_millis(INPUT_POLL_INTERVAL_MS));
        }

        Ok(())
    }

    fn run_screen_recording_blocking(
        config: ScreenRecordingConfig,
        stop_signal: Arc<AtomicBool>,
    ) -> Result<()> {
        use crate::services::screen::{ScreenRecordingConfig as RecordingConfig, ScreenRecorder};

        // Convert our config to nexus_screen's RecordingConfig
        let recording_config = RecordingConfig {
            framerate: config.framerate,
            duration_secs: config.duration_secs,
            output_path: config.output_path.clone(),
            monitor_index: config.monitor_index,
            window_id: None,
            window_title: None,
            include_audio: config.include_audio,
            fast: false,
            segment_duration_secs: config.segment_duration_secs,
        };

        // Use segmented recording if configured, otherwise use standard recording
        if config.segment_duration_secs.is_some() {
            info!("📹 Using FFmpeg CLI for segmented video recording");
            ScreenRecorder::record_with_segmentation(recording_config, stop_signal).map_err(
                |e| RecorderError::Other(format!("Segmented screen recording failed: {}", e)),
            )?;
        } else {
            // Create recorder
            let recorder =
                ScreenRecorder::new_with_config(recording_config.clone()).map_err(|e| {
                    RecorderError::Other(format!("Failed to initialize screen recorder: {}", e))
                })?;

            // Start recording (this is blocking)
            recorder
                .record(recording_config, stop_signal)
                .map_err(|e| RecorderError::Other(format!("Screen recording failed: {}", e)))?;
        }

        Ok(())
    }

    fn run_webcam_recording_blocking(
        config: crate::services::webcam::WebcamRecordingConfig,
        stop_signal: Arc<AtomicBool>,
    ) -> Result<()> {
        use crate::services::webcam::WebcamRecorder;
        use std::sync::Arc as StdArc;

        info!("📹 Starting webcam recording: {:?}", config.output_path);

        // Try to create recorder using v4l2 crate
        // This may fail for v4l2loopback virtual camera devices
        match WebcamRecorder::new(config.clone()) {
            Ok(recorder) => {
                info!("📹 Using v4l2 format detection");
                
                // Wrap recorder in Arc so we can share it with the monitor thread
                let recorder_arc = StdArc::new(std::sync::Mutex::new(recorder));
                let recorder_for_monitor = StdArc::clone(&recorder_arc);
                let stop_signal_clone = stop_signal.clone();

                // Spawn a thread to monitor stop_signal and stop the recorder
                let monitor_handle = std::thread::spawn(move || {
                    while !stop_signal_clone.load(Ordering::Relaxed) {
                        std::thread::sleep(std::time::Duration::from_millis(100));
                    }
                    // Stop the recorder when stop_signal is set
                    if let Ok(rec) = recorder_for_monitor.lock() {
                        rec.stop();
                    }
                });

                // Record (blocking call - will check stop_flag internally)
                let result = {
                    let mut rec = recorder_arc.lock().map_err(|e| {
                        RecorderError::Other(format!("Failed to lock recorder: {}", e))
                    })?;
                    rec.record()
                };

                // Signal monitor to stop
                stop_signal.store(true, Ordering::Relaxed);
                let _ = monitor_handle.join();

                if let Err(e) = result {
                    error!("Webcam recording error: {}", e);
                    return Err(RecorderError::Other(format!("Webcam recording failed: {}", e)));
                }
            }
            Err(e) => {
                // v4l2 format detection failed - use direct FFmpeg recording
                // This is common for v4l2loopback virtual camera devices
                warn!("📹 v4l2 format detection failed: {}. Using direct FFmpeg recording.", e);
                
                WebcamRecorder::record_direct(&config, stop_signal)?;
            }
        }

        info!("✅ Webcam recording complete");
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

        info!(
            "🚀 {} recording function started, output: {}",
            device_type,
            config.output_path.display()
        );

        // Create audio recorder
        let recorder = AudioRecorder::new()?;

        // Handle desktop audio monitoring
        // NOTE: Loopback sink is created in start_recording() before this function is called
        // We just need to verify it exists and get the monitor name
        #[cfg(target_os = "linux")]
        let (_module_ids, _monitor_source_name, _previous_default_source) =
            if config.monitor_desktop_audio {
                // Loopback sink should already exist (created in start_recording)
                // Just verify and get the monitor name - don't create again
                let monitor_name = "nexus_audio_monitor.monitor".to_string();
                info!("📺 Using existing loopback sink: {}", monitor_name);
                // Module IDs will be cleaned up in wait() method, not here
                (Vec::<u32>::new(), Some(monitor_name), None::<String>)
            } else {
                (Vec::new(), None, None)
            };
        #[cfg(not(target_os = "linux"))]
        let (module_ids, _monitor_source_name, _previous_default_source) =
            if config.monitor_desktop_audio {
                log::warn!("Desktop audio monitoring is only supported on Linux");
                return Err(RecorderError::Other(
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
            info!("📺 Using monitor source name directly: '{}'", monitor_name);
            AudioRecordingConfigInternal {
                sample_rate: config.sample_rate,
                channels: 2, // Desktop audio is typically stereo
                duration: None,
                device_name: Some(monitor_name), // Use monitor source name directly
            }
        } else {
            AudioRecordingConfigInternal {
                sample_rate: config.sample_rate,
                channels: config.channels,
                duration: None,
                device_name: mic_device_name,
            }
        };

        // Log device selection for debugging
        let device_name_display = recording_config
            .device_name
            .as_ref()
            .map(|d| d.as_str())
            .unwrap_or("system default");

        info!(
            "{} starting: device='{}' (requested {} Hz, {} channels)",
            device_type,
            device_name_display,
            recording_config.sample_rate,
            recording_config.channels
        );

        // Convert output path to absolute FIRST to ensure consistent file location
        let output_path = if config.output_path.is_absolute() {
            config.output_path.clone()
        } else {
            std::env::current_dir()
                .map_err(|e| {
                    RecorderError::Io(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        format!("Failed to get current directory: {}", e),
                    ))
                })?
                .join(&config.output_path)
        };

        info!(
            "📁 {} output path resolved to: {}",
            device_type,
            output_path.display()
        );

        // Use streaming API to have control over stop signal
        // This will return the actual sample rate and channels from the device
        info!("🎙️  {} attempting to create audio stream...", device_type);
        let (mut stream, rx, actual_sample_rate, actual_channels) =
            match recorder.stream_audio_chunks(recording_config.clone()) {
                Ok(result) => {
                    info!("✅ {} stream created successfully", device_type);
                    result
                }
                Err(e) => {
                    error!("❌ {} failed to create stream: {}", device_type, e);
                    return Err(RecorderError::Other(format!(
                        "Failed to create audio stream for {}: {}",
                        device_type, e
                    )));
                }
            };

        info!(
            "{} opened: device='{}' (actual {} Hz, {} channels) -> {}",
            device_type,
            device_name_display,
            actual_sample_rate,
            actual_channels,
            output_path.display()
        );

        // Create WAV file BEFORE starting the stream to ensure it exists
        // This way, even if the stream fails, we have a record that recording was attempted
        let spec = WavSpec {
            channels: actual_channels,
            sample_rate: actual_sample_rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };

        let writer = File::create(&output_path).map_err(|e| {
            RecorderError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!(
                    "Failed to create audio file at {}: {}",
                    output_path.display(),
                    e
                ),
            ))
        })?;
        let mut wav_writer = WavWriter::new(BufWriter::new(writer), spec)
            .map_err(|e| RecorderError::Other(format!("Failed to create WAV writer: {}", e)))?;

        // Verify file was created
        if !output_path.exists() {
            return Err(RecorderError::Other(format!(
                "File was not created at {} even though File::create() succeeded",
                output_path.display()
            )));
        }

        // Start the stream
        stream
            .play()
            .map_err(|e| RecorderError::Other(format!("Failed to start audio stream: {}", e)))?;

        // Record audio chunks until stop signal
        // For stereo, samples come interleaved: [L, R, L, R, ...]
        // For mono, samples come as: [M, M, M, ...]
        let mut samples_written = 0u64;
        let mut write_error_occurred = false;

        while !stop_signal.load(Ordering::SeqCst) {
            // Try to receive audio chunk with timeout to allow periodic stop signal checks
            match rx.recv_timeout(Duration::from_millis(AUDIO_BUFFER_DURATION_MS)) {
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
                                    error!(
                                        "❌ Error writing audio sample to {}: {}",
                                        output_path.display(),
                                        e
                                    );
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
                                    error!(
                                        "❌ Error writing audio sample to {}: {}",
                                        output_path.display(),
                                        e
                                    );
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
                    warn!(
                        "⚠️  Audio stream channel disconnected for {}",
                        output_path.display()
                    );
                    break;
                }
            }
        }

        info!(
            "📊 {} recording: wrote {} samples before finalization",
            device_type, samples_written
        );

        // Stop the stream
        stream
            .pause()
            .map_err(|e| RecorderError::Other(format!("Failed to pause audio stream: {}", e)))?;

        // Finalize WAV file - this is critical, even if no audio was recorded
        drop(stream);

        info!("💾 Finalizing WAV file at {}...", output_path.display());
        wav_writer.finalize().map_err(|e| {
            RecorderError::Other(format!(
                "Failed to finalize WAV file at {}: {}",
                output_path.display(),
                e
            ))
        })?;
        info!("✅ WAV file finalized successfully");

        // Give filesystem a moment to sync
        std::thread::sleep(Duration::from_millis(AUDIO_BUFFER_DURATION_MS));

        // Verify file exists after finalization
        if !output_path.exists() {
            return Err(RecorderError::Other(format!(
                "WAV file does not exist after finalization at {} (samples written: {})",
                output_path.display(),
                samples_written
            )));
        }

        // Log file size for debugging
        match std::fs::metadata(&output_path) {
            Ok(metadata) => {
                info!(
                    "✅ {} recording file created: {} ({} bytes, {} samples)",
                    device_type,
                    output_path.display(),
                    metadata.len(),
                    samples_written
                );
            }
            Err(e) => {
                return Err(RecorderError::Other(format!(
                    "Failed to get file metadata for {} after creation: {}",
                    output_path.display(),
                    e
                )));
            }
        }

        // NOTE: Desktop audio loopback sink cleanup is handled in RecordingSession::wait()
        // to ensure proper ordering (restore default source before removing modules)

        info!(
            "✅ {} recording function completed successfully, returning Ok(())",
            device_type
        );
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
        let mut file = std::fs::File::open(wav_path).map_err(|e| {
            RecorderError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Failed to open WAV file: {}", e),
            ))
        })?;

        let mut wav_data = Vec::new();
        file.read_to_end(&mut wav_data).map_err(|e| {
            RecorderError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Failed to read WAV file: {}", e),
            ))
        })?;

        // Decode WAV to f32 samples
        let mut reader = hound::WavReader::new(std::io::Cursor::new(wav_data))
            .map_err(|e| RecorderError::Other(format!("Failed to read WAV: {}", e)))?;

        let spec = reader.spec();
        let sample_rate = spec.sample_rate;

        // Convert samples to f32
        let samples: Vec<f32> = match spec.bits_per_sample {
            16 => reader
                .samples::<i16>()
                .map(|s| {
                    s.map(|sample| sample as f32 / 32768.0)
                        .map_err(|e| RecorderError::Other(format!("Failed to read sample: {}", e)))
                })
                .collect::<std::result::Result<Vec<_>, _>>()?,
            24 => reader
                .samples::<i32>()
                .map(|s| {
                    s.map(|sample| (sample >> 8) as f32 / 8388608.0)
                        .map_err(|e| RecorderError::Other(format!("Failed to read sample: {}", e)))
                })
                .collect::<std::result::Result<Vec<_>, _>>()?,
            32 => reader
                .samples::<i32>()
                .map(|s| {
                    s.map(|sample| sample as f32 / 2147483648.0)
                        .map_err(|e| RecorderError::Other(format!("Failed to read sample: {}", e)))
                })
                .collect::<std::result::Result<Vec<_>, _>>()?,
            _ => {
                return Err(RecorderError::Other(format!(
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
        .map_err(|e| RecorderError::Other(format!("Failed to create Whisper context: {}", e)))?;

        let mut state = ctx
            .create_state()
            .map_err(|e| RecorderError::Other(format!("Failed to create Whisper state: {}", e)))?;

        // Configure transcription parameters
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_language(Some("en"));
        params.set_translate(false);
        params.set_print_progress(false);
        params.set_print_special(false);

        // Run transcription
        state
            .full(params, &whisper_samples)
            .map_err(|e| RecorderError::Other(format!("Transcription failed: {}", e)))?;

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
                        warn!("⚠️  Failed to store transcription segment: {}", e);
                    }
                }
            }
        }

        Ok(())
    }

    /// Check if FFmpeg is available in the system PATH
    fn check_ffmpeg_available() -> Result<()> {
        use std::process::Command;

        let output = Command::new("ffmpeg").arg("-version").output();

        match output {
            Ok(result) if result.status.success() => Ok(()),
            Ok(_) => Err(RecorderError::Other(
                "FFmpeg is installed but returned an error. Please check your FFmpeg installation."
                    .to_string(),
            )),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(RecorderError::Other(format!(
                "FFmpeg is not installed or not found in PATH. \
                    Please install FFmpeg to enable video overlays:\n\
                    - Linux (Ubuntu/Debian): sudo apt-get install ffmpeg\n\
                    - macOS: brew install ffmpeg\n\
                    - Or download from: https://ffmpeg.org/download.html\n\
                    Error: {}",
                e
            ))),
            Err(e) => Err(RecorderError::Other(format!(
                "Failed to check FFmpeg availability: {}",
                e
            ))),
        }
    }

    /// Apply overlays to video using FFmpeg
    pub fn apply_video_overlays(
        video_path: &PathBuf,
        labels: &[OverlayLabel],
        show_timestamp: bool,
        show_labels: bool,
    ) -> Result<()> {
        use std::process::Command;

        // Check if FFmpeg is available before attempting to use it
        Self::check_ffmpeg_available()?;

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
            .map_err(|e| RecorderError::Other(format!("Failed to execute FFmpeg: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            info!("FFmpeg stderr: {}", stderr);
            info!("FFmpeg stdout: {}", stdout);
            info!("FFmpeg filter used: {}", filter_complex);
            return Err(RecorderError::Other(format!(
                "FFmpeg failed to apply overlays. Exit code: {}",
                output.status.code().unwrap_or(-1)
            )));
        } else {
            println!("✅ FFmpeg overlay processing completed");
        }

        // Replace original file with processed version
        std::fs::rename(&temp_output, video_path).map_err(|e| {
            RecorderError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Failed to replace video file: {}", e),
            ))
        })?;

        Ok(())
    }

    /// Write event output, using rotating writer if available, otherwise falling back to legacy file handle.
    fn write_event_output_with_rotation(
        event: &InputEvent,
        format: &str,
        file_handle: &mut Option<std::fs::File>,
        rotating_writer: &Option<RotatingEventWriterHandle>,
    ) -> Result<()> {
        let output = match format {
            "json" => serde_json::to_string(event)
                .map_err(|e| RecorderError::Other(format!("Failed to serialize event: {}", e)))?,
            "text" => event.to_text(),
            "both" => {
                format!(
                    "{} | {}",
                    event.to_text(),
                    serde_json::to_string(event).map_err(|e| {
                        RecorderError::Other(format!("Failed to serialize event: {}", e))
                    })?
                )
            }
            _ => return Err(RecorderError::Configuration("Invalid format".to_string())),
        };

        // Use rotating writer if available
        if let Some(writer) = rotating_writer {
            // Use non-blocking try_write since we're in a blocking context
            if let Err(e) = writer.try_write(output.clone()) {
                warn!("⚠️  Failed to write event to rotating writer: {}", e);
                // Fall through to print to stdout as fallback
                println!("{}", output);
            }
        } else if let Some(ref mut file) = file_handle {
            use std::io::Write;
            writeln!(file, "{}", output).map_err(|e| {
                RecorderError::Io(std::io::Error::new(
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
    /// Session ID (Arc<str> for cheap cloning across spawned tasks)
    session_id: Arc<str>,
    recording_start: DateTime<Utc>,
    input_handle: tokio::task::JoinHandle<Result<()>>,
    screen_handle: Option<tokio::task::JoinHandle<Result<()>>>,
    webcam_handle: Option<tokio::task::JoinHandle<Result<()>>>,
    webcam_analysis_handle: Option<ProcessingHandle>,
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
    periodic_context_handle: Option<tokio::task::JoinHandle<()>>,
}

impl RecordingSession {
    /// Stop the recording session.
    pub fn stop(&self) {
        self.stop_signal.store(true, Ordering::SeqCst);
    }

    /// Wait for the recording session to complete.
    /// This will wait until the stop signal is set (via stop() or duration expires).
    pub async fn wait(mut self) -> Result<()> {
        // Store audio recording start events if enabled
        let db = Database::new().await?;

        // Store video recording path (prefer screen, fallback to webcam)
        // Input events are associated with desktop/screen recording
        let video_path = self.config.screen_config.as_ref()
            .map(|c| c.output_path.clone())
            .or_else(|| self.config.webcam_config.as_ref().map(|c| c.output_path.clone()))
            .unwrap_or_else(|| PathBuf::from("output/recording.mp4"));
        
        let video_path = if video_path.is_absolute() {
            video_path
        } else {
            std::env::current_dir()
                .ok()
                .map(|cwd| cwd.join(&video_path))
                .unwrap_or_else(|| video_path)
        };

        let timestamp = Local::now().to_rfc3339();
        let framerate = self.config.webcam_config.as_ref()
            .map(|c| c.framerate)
            .or_else(|| self.config.screen_config.as_ref().map(|c| c.framerate))
            .unwrap_or(30);
        let include_audio = self.config.screen_config.as_ref()
            .map(|c| c.include_audio)
            .unwrap_or(false);
        let video_metadata = json!({
            "output_path": video_path.to_string_lossy(),
            "framerate": framerate,
            "include_audio": include_audio,
        });
        if let Err(e) = db
            .insert_event(
                &self.session_id,
                "recording",
                Some("video_start"),
                None,
                None,
                None,
                None,
                None,
                &timestamp,
                None, // timecode
                Some(video_metadata),
                None, // screenshot_id
            )
            .await
        {
            warn!("⚠️  Failed to store video recording start event: {}", e);
        }

        for audio_config in &self.config.audio_configs {
            if audio_config.enabled {
                let timestamp = Local::now().to_rfc3339();
                // Convert to absolute path for reliable querying
                let audio_path = if audio_config.output_path.is_absolute() {
                    audio_config.output_path.clone()
                } else {
                    std::env::current_dir()
                        .ok()
                        .map(|cwd| cwd.join(&audio_config.output_path))
                        .unwrap_or_else(|| audio_config.output_path.clone())
                };

                let metadata = json!({
                    "output_path": audio_path.to_string_lossy(),
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
                    warn!("⚠️  Failed to store audio recording start event: {}", e);
                }
            }
        }

        // Wait for all tasks to complete
        // They will exit when stop_signal is set
        let input_result = self.input_handle.await;
        let process_result = self.process_handle.await;

        input_result
            .map_err(|e| RecorderError::Other(format!("Input capture task failed: {}", e)))??;
        process_result
            .map_err(|e| RecorderError::Other(format!("Event processing task failed: {}", e)))??;

        // Wait for screen recording if enabled
        if let Some(screen_handle) = self.screen_handle {
            let screen_result = screen_handle.await;
            screen_result
                .map_err(|e| RecorderError::Other(format!("Screen recording task failed: {}", e)))??;
        }

        // Wait for webcam recording if enabled
        if let Some(webcam_handle) = self.webcam_handle {
            let webcam_result = webcam_handle.await;
            webcam_result
                .map_err(|e| RecorderError::Other(format!("Webcam recording task failed: {}", e)))??;
        }

        // Stop and wait for webcam analysis if enabled
        // Note: ProcessingHandle doesn't need explicit stop - it stops when sender is dropped
        // The stop_signal already handles stopping the periodic job scheduler
        if let Some(handle) = self.webcam_analysis_handle.take() {
            info!("🎭 Webcam sentiment analysis will complete when processing queue is empty...");
            handle.wait_for_completion().await;
            info!("🎭 Webcam sentiment analysis completed");
        }

        // Wait for periodic context processing to complete
        if let Some(handle) = self.periodic_context_handle.take() {
            info!("🔄 Waiting for periodic context processing to complete...");
            let _ = handle.await;
            info!("🔄 Periodic context processing completed");
        }

        // Wait for all audio recordings to complete
        for (handle_idx, audio_handle) in self.audio_handles.into_iter().enumerate() {
            let audio_result = audio_handle.await;
            // Get the config index for this handle
            let config_idx = self
                .audio_config_indices
                .get(handle_idx)
                .copied()
                .unwrap_or(handle_idx); // Fallback to handle_idx if mapping is missing
            info!(
                "🔍 Audio recording handle {} (config {}) result: {:?}",
                handle_idx,
                config_idx,
                audio_result
                    .as_ref()
                    .map(|r| r
                        .as_ref()
                        .map(|_| "Ok(())")
                        .map_err(|e| format!("Err({})", e)))
                    .map_err(|e| format!("JoinError({:?})", e))
            );
            match audio_result {
                Ok(Ok(())) => {
                    // Get the corresponding audio config using the mapped index
                    if let Some(audio_config) = self.config.audio_configs.get(config_idx) {
                        let timestamp = Local::now().to_rfc3339();
                        // Convert to absolute path for reliable querying
                        let audio_path = if audio_config.output_path.is_absolute() {
                            audio_config.output_path.clone()
                        } else {
                            std::env::current_dir()
                                .ok()
                                .map(|cwd| cwd.join(&audio_config.output_path))
                                .unwrap_or_else(|| audio_config.output_path.clone())
                        };

                        let metadata = json!({
                            "output_path": audio_path.to_string_lossy(),
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
                            warn!("⚠️  Failed to store audio recording stop event: {}", e);
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
                                println!(
                                    "✅ {} recording completed: {}",
                                    audio_type,
                                    wav_path.display()
                                );
                            } else {
                                warn!(
                                    "⚠️  {} recording reported success but file not found at: {}",
                                    audio_type,
                                    wav_path.display()
                                );
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
                                        .map_err(|e| {
                                            RecorderError::Io(std::io::Error::new(
                                                std::io::ErrorKind::Other,
                                                format!("Failed to get current directory: {}", e),
                                            ))
                                        })?
                                        .join(&audio_config.output_path)
                                };

                                // Verify file exists before attempting transcription
                                if !wav_path.exists() {
                                    warn!(
                                        "⚠️  {} transcription skipped: WAV file not found at {}",
                                        audio_type,
                                        wav_path.display()
                                    );
                                } else {
                                    println!(
                                        "🎤 Transcribing {} with model: {}...",
                                        audio_type,
                                        model_path.display()
                                    );
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
                                            warn!(
                                                "⚠️  {} transcription failed: {}",
                                                audio_type, e
                                            );
                                        }
                                    }
                                }
                            } else {
                                warn!("⚠️  Transcription enabled but no model path specified");
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
                    warn!("⚠️  Audio recording failed: {}", e);
                }
                Err(e) => {
                    warn!("⚠️  Audio recording task failed: {}", e);
                }
            }
        }

        // Restore previous default source/sink and clean up loopback modules
        #[cfg(target_os = "linux")]
        {
            use crate::services::AudioRecorder;

            // Restore default sink FIRST (before removing modules)
            if let Some(ref prev_sink) = self.previous_default_sink {
                info!("🔄 Restoring previous default sink...");
                if let Err(e) = AudioRecorder::set_default_sink(prev_sink) {
                    warn!("⚠️  Failed to restore previous default sink: {}", e);
                } else {
                    info!("✅ Restored previous default sink: {}", prev_sink);
                }
            }

            // Restore default source (only if we actually changed it - which we don't anymore with Option 2)
            // Keeping this for safety, but it should be a no-op since we don't change the default source
            if let Some(ref prev_source) = self.previous_default_source {
                // Check if current default is different (meaning something else changed it)
                if let Ok(current) = AudioRecorder::get_default_source() {
                    if current != *prev_source {
                        info!("🔄 Restoring previous default source (was changed by something else)...");
                        if let Err(e) = AudioRecorder::set_default_source(prev_source) {
                            warn!("⚠️  Failed to restore previous default source: {}", e);
                        } else {
                            info!("✅ Restored previous default source: {}", prev_source);
                        }
                    } else {
                        info!(
                            "ℹ️  Default source unchanged (still '{}'), no restoration needed",
                            prev_source
                        );
                    }
                }
            }

            // Clean up loopback modules (after restoring defaults)
            if !self.loopback_module_ids.is_empty() {
                info!("🧹 Cleaning up PulseAudio loopback sink...");
                for module_id in &self.loopback_module_ids {
                    if *module_id > 0 {
                        if let Err(e) = AudioRecorder::remove_pulseaudio_module(*module_id) {
                            warn!(
                                "⚠️  Failed to remove PulseAudio module {}: {}",
                                module_id, e
                            );
                        } else {
                            info!("✅ Removed PulseAudio module {}", module_id);
                        }
                    }
                }
            }
        }

        // Now that screen recording is complete, apply overlays
        let labels = match self.overlay_labels.lock() {
            Ok(guard) => guard.clone(),
            Err(poisoned) => {
                warn!("Overlay labels mutex was poisoned, recovering data");
                poisoned.into_inner().clone()
            }
        };
        println!("📝 Applying overlays: {} labels collected", labels.len());

        if self.config.show_timestamp || self.config.show_labels {
            // Wait a moment to ensure file is fully written
            tokio::time::sleep(Duration::from_millis(500)).await;

            if let Err(e) = UnifiedRecordingService::apply_video_overlays(
                self.config.webcam_config.as_ref()
                    .map(|c| &c.output_path)
                    .or_else(|| self.config.screen_config.as_ref().map(|c| &c.output_path))
                    .unwrap_or(&PathBuf::from("output/recording.mp4")),
                &labels,
                self.config.show_timestamp,
                self.config.show_labels,
            ) {
                warn!("⚠️  Failed to apply video overlays: {}", e);
            } else {
                println!("✅ Overlays applied successfully");
            }
        }

        // Store video recording stop event with final path
        // Prefer screen video (where input events are associated)
        let video_path = self.config.screen_config.as_ref()
            .map(|c| c.output_path.clone())
            .or_else(|| self.config.webcam_config.as_ref().map(|c| c.output_path.clone()))
            .unwrap_or_else(|| PathBuf::from("output/recording.mp4"));
        
        let video_path = if video_path.is_absolute() {
            video_path
        } else {
            std::env::current_dir()
                .ok()
                .map(|cwd| cwd.join(&video_path))
                .unwrap_or_else(|| video_path)
        };

        let timestamp = Local::now().to_rfc3339();
        let video_duration = UnifiedRecordingService::get_video_duration(&video_path).ok();
        let framerate = self.config.webcam_config.as_ref()
            .map(|c| c.framerate)
            .or_else(|| self.config.screen_config.as_ref().map(|c| c.framerate))
            .unwrap_or(30);
        let include_audio = self.config.screen_config.as_ref()
            .map(|c| c.include_audio)
            .unwrap_or(false);
        let video_stop_metadata = json!({
            "output_path": video_path.to_string_lossy(),
            "framerate": framerate,
            "include_audio": include_audio,
            "duration_seconds": video_duration,
        });
        if let Err(e) = db
            .insert_event(
                &self.session_id,
                "recording",
                Some("video_stop"),
                None,
                None,
                None,
                None,
                None,
                &timestamp,
                None, // timecode
                Some(video_stop_metadata),
                None, // screenshot_id
            )
            .await
        {
            warn!("⚠️  Failed to store video recording stop event: {}", e);
        }

        if let (Some(ctx), Some(fps)) = (self.click_context, self.config.context_fps) {
            if fps > 0.0 {
                println!(
                    "\n🧠 Processing full recording context at {:.2} fps...",
                    fps
                );
                tokio::time::sleep(Duration::from_millis(1000)).await;
                // Use screen video for context analysis (input events are associated with desktop)
                let video_path = self.config.screen_config.as_ref()
                    .map(|c| c.output_path.clone())
                    .or_else(|| self.config.webcam_config.as_ref().map(|c| c.output_path.clone()))
                    .unwrap_or_else(|| PathBuf::from("output/recording.mp4"));
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
                .with_session_id(Some(self.session_id.to_string()))
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
        let session_start = db
            .get_session_start_time(&self.session_id)
            .await?
            .unwrap_or(self.recording_start);

        print_timeline(&self.session_id, &events, session_start)
    }
}
