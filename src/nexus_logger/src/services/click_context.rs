use crate::error::{LoggerError, Result};
use crate::services::database::Database;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use chrono::{DateTime, Local, Utc};
use nexus_core::models::{ContentPart, ImageUrl};
use nexus_core::{ChatCompletionRequest, Message, MessageContent, NexusApiService};
use nexus_screen::ScreenRecorder;
use serde_json::json;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tempfile::tempdir;
use tokio::sync::mpsc;
use tokio::time::sleep;
use uuid::Uuid;

const FRAME_SYSTEM_PROMPT: &str = "You are an expert UI and user behavior analyst. Analyze the provided screenshot and, if given, use any prior frame descriptions to infer the user's likely action. In one or two clear sentences, describe what the user is doing, referencing salient UI elements, visible text, and any change or intent you can deduce from the visual context.";
const SUMMARY_SYSTEM_PROMPT: &str = "You summarize what likely happened around a click event based on prior frame descriptions. Mention the probable user intent in one concise sentence.";

#[derive(Clone)]
pub struct ClickContextHandle {
    sender: mpsc::UnboundedSender<ClickContextEvent>,
    worker: Arc<tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>>,
}

impl ClickContextHandle {
    pub fn trigger(&self, event: ClickContextEvent) {
        if let Err(err) = self.sender.send(event) {
            eprintln!("⚠️  Failed to enqueue click analysis: {}", err);
        }
    }

    /// Wait for all in-flight click analyses to complete
    pub async fn wait_for_completion(self) {
        // Drop sender to signal worker to finish after processing queued events
        drop(self.sender);

        // Wait for worker to finish
        let mut worker_guard = self.worker.lock().await;
        if let Some(handle) = worker_guard.take() {
            let _ = handle.await;
        }
    }
}

#[derive(Clone, Debug)]
pub struct ClickContextEvent {
    pub id: String,
    pub session_id: Option<String>,
    pub timestamp_utc: DateTime<Utc>,
    pub button: Option<String>,
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub video_timestamp: Option<f64>,
    pub video_path: Option<PathBuf>,
}

impl ClickContextEvent {
    pub fn new(
        session_id: Option<String>,
        timestamp_utc: DateTime<Utc>,
        button: Option<String>,
        x: Option<i32>,
        y: Option<i32>,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            session_id,
            timestamp_utc,
            button,
            x,
            y,
            video_timestamp: None,
            video_path: None,
        }
    }

    pub fn with_video_context(mut self, video_timestamp: f64, video_path: PathBuf) -> Self {
        self.video_timestamp = Some(video_timestamp);
        self.video_path = Some(video_path);
        self
    }

    fn timestamp_local(&self) -> String {
        self.timestamp_utc.with_timezone(&Local).to_rfc3339()
    }

    fn click_label(&self) -> String {
        self.button
            .clone()
            .unwrap_or_else(|| "unknown button".to_string())
    }
}

#[derive(Clone, Debug)]
pub struct ClickContextConfig {
    pub enabled: bool,
    pub frame_count: u32,
    pub frame_interval_ms: u64,
    pub per_frame_model: String,
    pub summary_model: String,
    pub monitor_index: Option<usize>,
    pub per_frame_max_tokens: u32,
    pub summary_max_tokens: u32,
    pub save_frames_dir: Option<PathBuf>,
    pub use_video_extraction: bool,
}

impl Default for ClickContextConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            frame_count: 3,
            frame_interval_ms: 1_000,
            per_frame_model: env::var("NEXUS_LOGGER_CLICK_CONTEXT_MODEL")
                .unwrap_or_else(|_| "gpt-4o-mini".to_string()),
            summary_model: env::var("NEXUS_LOGGER_CLICK_CONTEXT_SUMMARY_MODEL")
                .unwrap_or_else(|_| "gpt-4o-mini".to_string()),
            monitor_index: env::var("NEXUS_LOGGER_CLICK_CONTEXT_MONITOR")
                .ok()
                .and_then(|v| v.parse::<usize>().ok()),
            per_frame_max_tokens: env::var("NEXUS_LOGGER_CLICK_CONTEXT_FRAME_TOKENS")
                .ok()
                .and_then(|v| v.parse::<u32>().ok())
                .unwrap_or(200),
            summary_max_tokens: env::var("NEXUS_LOGGER_CLICK_CONTEXT_SUMMARY_TOKENS")
                .ok()
                .and_then(|v| v.parse::<u32>().ok())
                .unwrap_or(120),
            save_frames_dir: None,
            use_video_extraction: true, // Default to video extraction when available
        }
    }
}

impl ClickContextConfig {
    pub fn from_env() -> Self {
        let mut config = Self::default();
        if let Ok(value) = env::var("NEXUS_LOGGER_CLICK_CONTEXT_ENABLED") {
            config.enabled = matches!(
                value.to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            );
        }
        if let Ok(value) = env::var("NEXUS_LOGGER_CLICK_CONTEXT_FRAME_COUNT") {
            if let Ok(count) = value.parse::<u32>() {
                config.frame_count = count.max(1).min(6);
            }
        }
        if let Ok(value) = env::var("NEXUS_LOGGER_CLICK_CONTEXT_FRAME_INTERVAL_MS") {
            if let Ok(interval) = value.parse::<u64>() {
                config.frame_interval_ms = interval.max(250);
            }
        }
        if let Ok(path) = env::var("NEXUS_LOGGER_CLICK_CONTEXT_SAVE_FRAMES") {
            let trimmed = path.trim();
            if !trimmed.is_empty() {
                let path_buf = PathBuf::from(trimmed);
                if let Err(err) = fs::create_dir_all(&path_buf) {
                    eprintln!(
                        "⚠️  Failed to prepare click-context frame directory ({}): {}",
                        trimmed, err
                    );
                } else {
                    config.save_frames_dir = Some(path_buf);
                }
            }
        }
        config
    }
}

pub struct ClickContextService;

impl ClickContextService {
    pub fn maybe_start(db: Database) -> Option<ClickContextHandle> {
        let config = ClickContextConfig::from_env();
        if !config.enabled {
            return None;
        }

        match Self::start(config, db) {
            Ok(handle) => Some(handle),
            Err(err) => {
                eprintln!(
                    "⚠️  Click context analysis disabled (initialization failed): {}",
                    err
                );
                None
            }
        }
    }

    fn start(config: ClickContextConfig, db: Database) -> Result<ClickContextHandle> {
        let api_service = Arc::new(NexusApiService::from_env()?);
        let screen_recorder = Arc::new(ScreenRecorder::new()?);
        let db = Arc::new(db);
        let config = Arc::new(config);
        let (tx, mut rx) = mpsc::unbounded_channel::<ClickContextEvent>();

        let worker_handle = tokio::spawn({
            let api = Arc::clone(&api_service);
            let recorder = Arc::clone(&screen_recorder);
            let cfg = Arc::clone(&config);
            let database = Arc::clone(&db);
            async move {
                while let Some(event) = rx.recv().await {
                    if let Err(err) = Self::process_event(
                        Arc::clone(&api),
                        Arc::clone(&recorder),
                        Arc::clone(&cfg),
                        Arc::clone(&database),
                        event,
                    )
                    .await
                    {
                        eprintln!("⚠️  Click context analysis failed: {}", err);
                    }
                }
                println!("🔍 Click context worker shutting down");
            }
        });

        Ok(ClickContextHandle {
            sender: tx,
            worker: Arc::new(tokio::sync::Mutex::new(Some(worker_handle))),
        })
    }

    async fn process_event(
        api_service: Arc<NexusApiService>,
        _screen_recorder: Arc<ScreenRecorder>,
        config: Arc<ClickContextConfig>,
        db: Arc<Database>,
        event: ClickContextEvent,
    ) -> Result<()> {
        // Only process events that have video context (post-recording batch mode)
        if event.video_path.is_none() || event.video_timestamp.is_none() {
            return Err(LoggerError::Other(
                "Click event missing video context (skipping real-time analysis)".to_string(),
            ));
        }

        println!("🎬 Extracting frames from recorded video...");
        let frames = Self::extract_frames_from_video(&config, &event).await?;

        println!(
            "✅ Captured {} frames, starting parallel analysis...",
            frames.len()
        );

        // Analyze frames in parallel instead of sequentially
        let descriptions =
            Self::describe_frames_parallel(api_service.clone(), &config, &event, &frames).await?;

        if descriptions.is_empty() {
            eprintln!("⚠️  No frame descriptions generated (all frames failed to analyze)");
            return Err(LoggerError::Other(
                "No frames captured for click context".to_string(),
            ));
        }

        println!(
            "✅ Analyzed {} / {} frames, generating summary...",
            descriptions.len(),
            frames.len()
        );
        let summary =
            Self::summarize_click(api_service.clone(), &config, &event, &descriptions).await?;
        Self::print_result(&event, &descriptions, &summary);
        Self::store_summary(db, &event, &descriptions, &summary).await?;
        Ok(())
    }

    fn print_result(event: &ClickContextEvent, frames: &[FrameDescription], summary: &str) {
        println!(
            "\n🖱️  Click analysis @ {} ({} at {:?}):",
            event.timestamp_local(),
            event.click_label(),
            event.x.zip(event.y)
        );
        for frame in frames {
            if let Some(path) = &frame.file_path {
                println!(
                    "   • +{}s: {}  [{}]",
                    frame.offset_secs,
                    frame.description,
                    path.display()
                );
            } else {
                println!("   • +{}s: {}", frame.offset_secs, frame.description);
            }
        }
        println!("   → Summary: {}", summary.trim());
    }

    /// Analyze all frames in parallel and combine results
    async fn describe_frames_parallel(
        api_service: Arc<NexusApiService>,
        config: &ClickContextConfig,
        event: &ClickContextEvent,
        frames: &[CapturedFrame],
    ) -> Result<Vec<FrameDescription>> {
        println!("🤖  Analyzing {} frames in parallel...", frames.len());

        // Launch all frame analyses in parallel
        let mut analysis_tasks = Vec::new();

        for (idx, frame) in frames.iter().enumerate() {
            let api = Arc::clone(&api_service);
            let cfg = config.clone();
            let evt = event.clone();
            let frm = frame.clone();

            let task =
                tokio::spawn(
                    async move { Self::describe_frame_simple(api, &cfg, &evt, &frm).await },
                );

            analysis_tasks.push((idx, frame.offset_secs, frame.file_path.clone(), task));
        }

        // Collect results as they complete
        let mut descriptions = Vec::new();
        for (idx, offset, file_path, task) in analysis_tasks {
            match task.await {
                Ok(Ok(text)) => {
                    println!("   ✓ Frame {} (+{}s) analyzed", idx + 1, offset);
                    descriptions.push(FrameDescription {
                        offset_secs: offset,
                        description: text,
                        file_path,
                    });
                }
                Ok(Err(err)) => {
                    eprintln!("   ✗ Frame {} (+{}s) failed: {}", idx + 1, offset, err);
                }
                Err(err) => {
                    eprintln!("   ✗ Frame {} (+{}s) task failed: {}", idx + 1, offset, err);
                }
            }
        }

        // Sort by offset to maintain temporal order
        descriptions.sort_by_key(|d| d.offset_secs);
        Ok(descriptions)
    }

    /// Get video duration in seconds using FFprobe
    fn get_video_duration(video_path: &Path) -> Result<f64> {
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

    /// Extract frames from recorded video file using FFmpeg
    async fn extract_frames_from_video(
        config: &ClickContextConfig,
        event: &ClickContextEvent,
    ) -> Result<Vec<CapturedFrame>> {
        let video_path = event.video_path.as_ref().ok_or_else(|| {
            LoggerError::Other("Video path not provided for frame extraction".to_string())
        })?;
        let base_timestamp = event.video_timestamp.ok_or_else(|| {
            LoggerError::Other("Video timestamp not provided for frame extraction".to_string())
        })?;

        // Check if video file exists
        if !video_path.exists() {
            return Err(LoggerError::Other(format!(
                "Video file not found: {}",
                video_path.display()
            )));
        }

        // Get video duration and adjust frame count if needed
        let video_duration = Self::get_video_duration(video_path).map_err(|e| {
            LoggerError::Other(format!(
                "Cannot determine video duration (required for frame extraction): {}",
                e
            ))
        })?;

        // Check if click timestamp is within video duration
        if base_timestamp > video_duration {
            return Err(LoggerError::Other(format!(
                "Click at {:.2}s is after video ended at {:.2}s (timing issue during recording stop)",
                base_timestamp, video_duration
            )));
        }

        // Calculate how many frames we can actually extract
        let frame_interval_secs = config.frame_interval_ms as f64 / 1000.0;
        let available_duration = video_duration - base_timestamp;
        let max_possible_frames =
            ((available_duration / frame_interval_secs).floor() as u32 + 1).min(config.frame_count);

        if max_possible_frames == 0 {
            return Err(LoggerError::Other(format!(
                "Click at {:.2}s too close to video end ({:.2}s) to extract any frames",
                base_timestamp, video_duration
            )));
        }

        let actual_frame_count = max_possible_frames;
        if actual_frame_count < config.frame_count {
            println!(
                "   ℹ️  Extracting {} frames (reduced from {}) due to video duration ({:.2}s)",
                actual_frame_count, config.frame_count, video_duration
            );
        }

        let mut frames = Vec::new();
        let output_dir = if let Some(root) = &config.save_frames_dir {
            root.clone()
        } else {
            let temp = tempdir().map_err(LoggerError::Io)?;
            let path = temp.path().to_path_buf();
            // Keep the tempdir alive by not dropping it
            std::mem::forget(temp);
            path
        };

        // Ensure output directory exists
        if !output_dir.exists() {
            fs::create_dir_all(&output_dir).map_err(|e| {
                LoggerError::Other(format!("Failed to create output directory: {}", e))
            })?;
        }

        println!(
            "🎬 Extracting {} frames from video at timestamp {:.2}s",
            actual_frame_count, base_timestamp
        );

        // Extract all frames in parallel using FFmpeg
        let mut extract_tasks = Vec::new();

        for idx in 0..actual_frame_count {
            let timestamp =
                base_timestamp + (idx as f64 * config.frame_interval_ms as f64 / 1000.0);
            let output_path = output_dir.join(Self::frame_filename(event, idx));
            let video_path_clone = video_path.clone();

            let task = tokio::task::spawn_blocking(move || -> Result<PathBuf> {
                Self::extract_single_frame(&video_path_clone, timestamp, &output_path)?;
                Ok(output_path)
            });

            extract_tasks.push((idx, task));
        }

        // Wait for all extractions to complete
        for (idx, task) in extract_tasks {
            match task.await {
                Ok(Ok(path)) => {
                    let data = tokio::fs::read(&path).await?;
                    let base64 = BASE64.encode(data);
                    frames.push(CapturedFrame {
                        offset_secs: idx as u64,
                        base64_image: base64,
                        file_path: config.save_frames_dir.as_ref().map(|_| path),
                    });
                }
                Ok(Err(e)) => {
                    eprintln!("⚠️  Failed to extract frame {}: {}", idx, e);
                }
                Err(e) => {
                    eprintln!("⚠️  Frame extraction task {} failed: {}", idx, e);
                }
            }
        }

        Ok(frames)
    }

    /// Extract a single frame from video at the given timestamp
    fn extract_single_frame(video_path: &Path, timestamp: f64, output_path: &Path) -> Result<()> {
        use std::process::Command;

        // Verify video file exists before attempting extraction
        if !video_path.exists() {
            return Err(LoggerError::Other(format!(
                "Video file not found: {}",
                video_path.display()
            )));
        }

        // Ensure output directory exists
        if let Some(parent) = output_path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent).map_err(|e| {
                    LoggerError::Other(format!(
                        "Failed to create output directory {}: {}",
                        parent.display(),
                        e
                    ))
                })?;
            }
        }

        let output = Command::new("ffmpeg")
            .arg("-ss")
            .arg(format!("{:.3}", timestamp))
            .arg("-i")
            .arg(video_path)
            .arg("-frames:v")
            .arg("1")
            .arg("-q:v")
            .arg("2") // High quality
            .arg("-y") // Overwrite
            .arg(output_path)
            .stderr(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .output()
            .map_err(|e| {
                LoggerError::Other(format!(
                    "Failed to run FFmpeg for {} at {:.2}s: {}",
                    video_path.display(),
                    timestamp,
                    e
                ))
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(LoggerError::Other(format!(
                "FFmpeg frame extraction failed at {:.2}s from {}: {}",
                timestamp,
                video_path.display(),
                stderr.lines().take(5).collect::<Vec<_>>().join(" | ")
            )));
        }

        // Verify output file was created
        if !output_path.exists() {
            return Err(LoggerError::Other(format!(
                "FFmpeg succeeded but output file not found: {}",
                output_path.display()
            )));
        }

        Ok(())
    }

    /// Capture frames live from screen (fallback when video not available)
    #[allow(dead_code)]
    async fn capture_frames_live(
        screen_recorder: Arc<ScreenRecorder>,
        config: &ClickContextConfig,
        event: &ClickContextEvent,
    ) -> Result<Vec<CapturedFrame>> {
        let mut frames = Vec::new();
        let temp_dir = if config.save_frames_dir.is_none() {
            Some(tempdir().map_err(LoggerError::Io)?)
        } else {
            None
        };

        println!(
            "🖼️  Capturing {} contextual frames for click at ({}, {})",
            config.frame_count,
            event.x.unwrap_or_default(),
            event.y.unwrap_or_default()
        );

        for idx in 0..config.frame_count {
            if idx > 0 {
                sleep(Duration::from_millis(config.frame_interval_ms)).await;
            }
            println!(
                "   • Capturing frame {} / {} (delay {} ms)",
                idx + 1,
                config.frame_count,
                if idx == 0 {
                    0
                } else {
                    config.frame_interval_ms
                }
            );

            let capture_path = if let Some(root) = &config.save_frames_dir {
                let path = Self::saved_frame_path(root, event, idx);
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent).map_err(LoggerError::Io)?;
                }
                path
            } else {
                temp_dir
                    .as_ref()
                    .expect("temporary directory should exist")
                    .path()
                    .join(Self::frame_filename(event, idx))
            };

            Self::capture_single_frame(
                Arc::clone(&screen_recorder),
                config.monitor_index,
                &capture_path,
            )
            .await?;

            let data = tokio::fs::read(&capture_path).await?;
            let base64 = BASE64.encode(data);
            frames.push(CapturedFrame {
                offset_secs: idx as u64,
                base64_image: base64,
                file_path: config
                    .save_frames_dir
                    .as_ref()
                    .map(|_| capture_path.clone()),
            });
        }

        Ok(frames)
    }

    #[allow(dead_code)]
    async fn capture_single_frame(
        screen_recorder: Arc<ScreenRecorder>,
        monitor_index: Option<usize>,
        target_path: &Path,
    ) -> Result<()> {
        let path_string = target_path
            .to_str()
            .ok_or_else(|| LoggerError::Other("Invalid temp file path".to_string()))?
            .to_string();

        tokio::task::spawn_blocking(move || -> Result<()> {
            if let Some(index) = monitor_index {
                screen_recorder
                    .capture_screenshot_to_file_with_monitor(&path_string, Some(index))?;
            } else {
                screen_recorder.capture_screenshot_to_file(&path_string)?;
            }
            Ok(())
        })
        .await
        .map_err(|e| LoggerError::Other(format!("Screenshot task failed: {}", e)))??;

        Ok(())
    }

    /// Describe a single frame without prior context (for parallel processing)
    async fn describe_frame_simple(
        api_service: Arc<NexusApiService>,
        config: &ClickContextConfig,
        event: &ClickContextEvent,
        frame: &CapturedFrame,
    ) -> Result<String> {
        let instruction = format!(
            "Frame captured +{}s from click at ({}, {}) using {}. Describe what is visible in this frame in one or two sentences, focusing on UI elements, text, and user actions.",
            frame.offset_secs,
            event.x.unwrap_or_default(),
            event.y.unwrap_or_default(),
            event.click_label()
        );

        let content = MessageContent::Array(vec![
            ContentPart::Text { text: instruction },
            ContentPart::ImageUrl {
                image_url: ImageUrl {
                    url: format!("data:image/png;base64,{}", frame.base64_image),
                },
            },
        ]);

        let messages = vec![
            Message::system(FRAME_SYSTEM_PROMPT),
            Message::user_with_content(content),
        ];

        let request = ChatCompletionRequest::new(config.per_frame_model.clone(), messages);

        let response = api_service.chat(request).await?;
        let text = response
            .content
            .as_ref()
            .map(|c| c.extract_text())
            .unwrap_or_default();
        Ok(text.trim().to_string())
    }

    async fn summarize_click(
        api_service: Arc<NexusApiService>,
        config: &ClickContextConfig,
        event: &ClickContextEvent,
        frames: &[FrameDescription],
    ) -> Result<String> {
        let mut user_prompt = format!(
            "Click at ({}, {}) using {}.\n",
            event.x.unwrap_or_default(),
            event.y.unwrap_or_default(),
            event.click_label()
        );

        for frame in frames {
            user_prompt.push_str(&format!(
                "Frame +{}s: {}\n",
                frame.offset_secs, frame.description
            ));
        }
        user_prompt.push_str(
            "Summarize the combined evidence in one sentence focused on what the user likely did.",
        );

        let messages = vec![
            Message::system(SUMMARY_SYSTEM_PROMPT),
            Message::user(user_prompt),
        ];
        let request = ChatCompletionRequest::new(config.summary_model.clone(), messages);

        let response = api_service.chat(request).await?;
        let text = response
            .content
            .as_ref()
            .map(|c| c.extract_text())
            .unwrap_or_default();
        Ok(text.trim().to_string())
    }

    async fn store_summary(
        db: Arc<Database>,
        event: &ClickContextEvent,
        frames: &[FrameDescription],
        summary: &str,
    ) -> Result<()> {
        let session_id = match &event.session_id {
            Some(id) => id,
            None => return Ok(()),
        };

        let metadata = json!({
            "summary": summary,
            "frames": frames.iter().map(|frame| {
                json!({
                    "offset_secs": frame.offset_secs,
                    "description": frame.description,
                    "file_path": frame.file_path.as_ref().map(|p| p.display().to_string()),
                })
            }).collect::<Vec<_>>(),
            "click": {
                "timestamp": event.timestamp_utc.to_rfc3339(),
                "button": event.button,
                "x": event.x,
                "y": event.y,
            }
        });

        let timestamp = event.timestamp_utc.to_rfc3339();

        db.insert_event(
            session_id,
            "analysis",
            Some("click_context"),
            None,
            event.button.as_deref(),
            event.x,
            event.y,
            None,
            &timestamp,
            None,
            Some(metadata),
            None,
        )
        .await?;

        Ok(())
    }
}

#[derive(Clone)]
struct CapturedFrame {
    offset_secs: u64,
    base64_image: String,
    file_path: Option<PathBuf>,
}

struct FrameDescription {
    offset_secs: u64,
    description: String,
    file_path: Option<PathBuf>,
}

impl ClickContextService {
    fn frame_filename(event: &ClickContextEvent, idx: u32) -> String {
        let session = event.session_id.as_deref().unwrap_or("session");
        format!("{}_{}_frame_{}.png", session, event.id, idx)
    }

    #[allow(dead_code)]
    fn saved_frame_path(root: &Path, event: &ClickContextEvent, idx: u32) -> PathBuf {
        root.join(Self::frame_filename(event, idx))
    }
}
