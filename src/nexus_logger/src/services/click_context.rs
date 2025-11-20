use crate::error::{LoggerError, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use chrono::{DateTime, Local, Utc};
use nexus_core::{ChatCompletionRequest, Message, MessageContent, NexusApiService};
use nexus_screen::ScreenRecorder;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tempfile::tempdir;
use tokio::sync::mpsc;
use tokio::time::sleep;
use uuid::Uuid;

const FRAME_SYSTEM_PROMPT: &str = "You are a perceptive UI analyst. Describe each screenshot in one or two short sentences that call out salient UI elements, readable text, and any obvious actions by the user.";
const SUMMARY_SYSTEM_PROMPT: &str = "You summarize what likely happened around a click event based on prior frame descriptions. Mention the probable user intent in one concise sentence.";

#[derive(Clone)]
pub struct ClickContextHandle {
    sender: mpsc::UnboundedSender<ClickContextEvent>,
}

impl ClickContextHandle {
    pub fn trigger(&self, event: ClickContextEvent) {
        if let Err(err) = self.sender.send(event) {
            eprintln!("⚠️  Failed to enqueue click analysis: {}", err);
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
        }
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
    pub fn maybe_start() -> Option<ClickContextHandle> {
        let config = ClickContextConfig::from_env();
        if !config.enabled {
            return None;
        }

        match Self::start(config) {
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

    fn start(config: ClickContextConfig) -> Result<ClickContextHandle> {
        let api_service = Arc::new(NexusApiService::from_env()?);
        let screen_recorder = Arc::new(ScreenRecorder::new()?);
        let config = Arc::new(config);
        let (tx, mut rx) = mpsc::unbounded_channel::<ClickContextEvent>();

        tokio::spawn({
            let api = Arc::clone(&api_service);
            let recorder = Arc::clone(&screen_recorder);
            let cfg = Arc::clone(&config);
            async move {
                while let Some(event) = rx.recv().await {
                    if let Err(err) = Self::process_event(
                        Arc::clone(&api),
                        Arc::clone(&recorder),
                        Arc::clone(&cfg),
                        event,
                    )
                    .await
                    {
                        eprintln!("⚠️  Click context analysis failed: {}", err);
                    }
                }
            }
        });

        Ok(ClickContextHandle { sender: tx })
    }

    async fn process_event(
        api_service: Arc<NexusApiService>,
        screen_recorder: Arc<ScreenRecorder>,
        config: Arc<ClickContextConfig>,
        event: ClickContextEvent,
    ) -> Result<()> {
        let frames = Self::capture_frames(screen_recorder, &config, &event).await?;
        let mut descriptions = Vec::new();

        for frame in &frames {
            let text = Self::describe_frame(api_service.clone(), &config, &event, frame).await?;
            descriptions.push(FrameDescription {
                offset_secs: frame.offset_secs,
                description: text,
                file_path: frame.file_path.clone(),
            });
        }

        if descriptions.is_empty() {
            return Err(LoggerError::Other(
                "No frames captured for click context".to_string(),
            ));
        }

        let summary = Self::summarize_click(api_service, &config, &event, &descriptions).await?;
        Self::print_result(&event, &descriptions, &summary);
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

    async fn capture_frames(
        screen_recorder: Arc<ScreenRecorder>,
        config: &ClickContextConfig,
        event: &ClickContextEvent,
    ) -> Result<Vec<CapturedFrame>> {
        let mut frames = Vec::new();
        let temp_dir = if config.save_frames_dir.is_none() {
            Some(tempdir()?)
        } else {
            None
        };

        for idx in 0..config.frame_count {
            if idx > 0 {
                sleep(Duration::from_millis(config.frame_interval_ms)).await;
            }

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

    async fn describe_frame(
        api_service: Arc<NexusApiService>,
        config: &ClickContextConfig,
        event: &ClickContextEvent,
        frame: &CapturedFrame,
    ) -> Result<String> {
        let description_prompt = format!(
            "Frame captured +{}s from click at ({}, {}). List the most important on-screen elements and any visible actions succinctly.",
            frame.offset_secs,
            event.x.unwrap_or_default(),
            event.y.unwrap_or_default()
        );
        let content = MessageContent::with_image(description_prompt, frame.base64_image.clone());
        let messages = vec![
            Message::system(FRAME_SYSTEM_PROMPT),
            Message::user_with_content(content),
        ];

        let request = ChatCompletionRequest::new(config.per_frame_model.clone(), messages)
            .with_max_tokens(config.per_frame_max_tokens);

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
        let request = ChatCompletionRequest::new(config.summary_model.clone(), messages)
            .with_max_tokens(config.summary_max_tokens);

        let response = api_service.chat(request).await?;
        let text = response
            .content
            .as_ref()
            .map(|c| c.extract_text())
            .unwrap_or_default();
        Ok(text.trim().to_string())
    }
}

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

    fn saved_frame_path(root: &Path, event: &ClickContextEvent, idx: u32) -> PathBuf {
        root.join(Self::frame_filename(event, idx))
    }
}
