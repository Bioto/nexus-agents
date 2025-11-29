use crate::error::{LoggerError, Result};
use crate::services::database::Database;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use chrono::{DateTime, Local, Utc};
use image::ImageReader;
use nexus_core::models::{ContentPart, ImageUrl};
use nexus_core::{ChatCompletionRequest, Message, MessageContent, NexusApiService};
use nexus_screen::ScreenRecorder;
use serde_json::{json, Value};
use std::env;
use std::fs;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tempfile::tempdir;
use tokio::sync::mpsc;
use tokio::time::sleep;
use uuid::Uuid;

const FRAME_SYSTEM_PROMPT: &str = "You are an expert UI and user behavior analyst. Analyze the provided screenshot and, if given, use any prior frame descriptions to infer the user's likely action. In one or two clear sentences, describe what the user is doing, referencing salient UI elements, visible text, and any change or intent you can deduce from the visual context.";
const SUMMARY_SYSTEM_PROMPT: &str = "\
You are an expert in interpreting user behavior from UI activity logs. Given a sequence of frame descriptions around a click event, provide a detailed summary of what likely happened, focusing on both the immediate action and surrounding context. Consider user intent, what the user might already know about the navigation target or item, and any visible clues about task progression or discovery. Explain not just what was clicked, but also what the user may have been seeking (e.g., navigating to a new item, reviewing existing information, taking action on a new element, etc.), and how the interface state or prior actions contribute to your reasoning. Write a clear, multi-sentence summary describing both the user’s action and their probable understanding or goal in this context.";
const FRAME_BATCH_SIZE: usize = 3;
const CONTEXT_WINDOW_BEFORE: f64 = 2.0; // seconds before frame
const CONTEXT_WINDOW_AFTER: f64 = 2.0; // seconds after frame
const KEY_GAP_THRESHOLD_MS: u64 = 500; // milliseconds between keys to detect word boundary

#[derive(Clone)]
pub struct ProcessingHandle {
    sender: mpsc::UnboundedSender<ProcessingJob>,
    worker: Arc<tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>>,
}

impl ProcessingHandle {
    pub fn trigger(&self, job: ProcessingJob) {
        if let Err(err) = self.sender.send(job) {
            eprintln!("⚠️  Failed to enqueue context processing: {}", err);
        }
    }

    pub async fn wait_for_completion(self) {
        drop(self.sender);
        let mut worker_guard = self.worker.lock().await;
        if let Some(handle) = worker_guard.take() {
            let _ = handle.await;
        }
    }
}

#[derive(Clone, Debug)]
pub struct ProcessingJob {
    pub id: String,
    pub session_id: Option<String>,
    pub timestamp_utc: DateTime<Utc>,
    pub kind: String,
    pub label: String,
    pub button: Option<String>,
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub video_timestamp: Option<f64>,
    pub video_path: Option<PathBuf>,
    pub metadata: Value,
    pub frames_per_second: Option<f64>,
}

impl ProcessingJob {
    pub fn new(
        kind: impl Into<String>,
        label: impl Into<String>,
        timestamp_utc: DateTime<Utc>,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            session_id: None,
            timestamp_utc,
            kind: kind.into(),
            label: label.into(),
            button: None,
            x: None,
            y: None,
            video_timestamp: None,
            video_path: None,
            metadata: Value::Null,
            frames_per_second: None,
        }
    }

    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = id.into();
        self
    }

    pub fn with_session_id(mut self, session_id: Option<String>) -> Self {
        self.session_id = session_id;
        self
    }

    pub fn with_button(mut self, button: Option<String>) -> Self {
        self.button = button;
        self
    }

    pub fn with_coordinates(mut self, x: Option<i32>, y: Option<i32>) -> Self {
        self.x = x;
        self.y = y;
        self
    }

    pub fn with_video_context(
        mut self,
        video_timestamp: Option<f64>,
        video_path: Option<PathBuf>,
    ) -> Self {
        self.video_timestamp = video_timestamp;
        self.video_path = video_path;
        self.frames_per_second = None;
        self
    }

    pub fn with_full_video_sampling(mut self, video_path: PathBuf, frames_per_second: f64) -> Self {
        self.video_timestamp = None;
        self.video_path = Some(video_path);
        self.frames_per_second = Some(frames_per_second.max(0.1));
        self
    }

    pub fn with_metadata(mut self, metadata: Value) -> Self {
        self.metadata = metadata;
        self
    }

    pub fn timestamp_local(&self) -> String {
        self.timestamp_utc.with_timezone(&Local).to_rfc3339()
    }

    pub fn coordinates(&self) -> Option<(i32, i32)> {
        self.x.zip(self.y)
    }
}

#[derive(Clone, Debug)]
pub struct ProcessingConfig {
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

impl Default for ProcessingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            frame_count: 3,
            frame_interval_ms: 1_000,
            // Try VISION_MODEL first, then NEXUS_LOGGER_CLICK_CONTEXT_MODEL for backward compatibility
            per_frame_model: env::var("VISION_MODEL")
                .or_else(|_| env::var("NEXUS_LOGGER_CLICK_CONTEXT_MODEL"))
                .unwrap_or_else(|_| "gpt-4o-mini".to_string()),
            // Try VISION_MODEL first, then NEXUS_LOGGER_CLICK_CONTEXT_SUMMARY_MODEL for backward compatibility
            summary_model: env::var("VISION_MODEL")
                .or_else(|_| env::var("NEXUS_LOGGER_CLICK_CONTEXT_SUMMARY_MODEL"))
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
            use_video_extraction: true,
        }
    }
}

impl ProcessingConfig {
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
                        "⚠️  Failed to prepare context frame directory ({}): {}",
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

pub struct ProcessingService;

impl ProcessingService {
    pub fn start(config: ProcessingConfig, db: Database) -> Result<ProcessingHandle> {
        // Use vision-specific API service for image processing
        let api_service = Arc::new(NexusApiService::from_env_vision()?);
        let screen_recorder = Arc::new(ScreenRecorder::new()?);
        let db = Arc::new(db);
        let config = Arc::new(config);
        let (tx, mut rx) = mpsc::unbounded_channel::<ProcessingJob>();

        let worker_handle = tokio::spawn({
            let api = Arc::clone(&api_service);
            let recorder = Arc::clone(&screen_recorder);
            let cfg = Arc::clone(&config);
            let database = Arc::clone(&db);
            async move {
                while let Some(job) = rx.recv().await {
                    if let Err(err) = Self::process_job(
                        Arc::clone(&api),
                        Arc::clone(&recorder),
                        Arc::clone(&cfg),
                        Arc::clone(&database),
                        job,
                    )
                    .await
                    {
                        eprintln!("⚠️  Context processing failed: {}", err);
                    }
                }
                println!("🧠 Context processing worker shutting down");
            }
        });

        Ok(ProcessingHandle {
            sender: tx,
            worker: Arc::new(tokio::sync::Mutex::new(Some(worker_handle))),
        })
    }

    async fn process_job(
        api_service: Arc<NexusApiService>,
        _screen_recorder: Arc<ScreenRecorder>,
        config: Arc<ProcessingConfig>,
        db: Arc<Database>,
        job: ProcessingJob,
    ) -> Result<()> {
        if job.video_path.is_none() {
            return Err(LoggerError::Other(
                "Processing job missing video path (skipping)".to_string(),
            ));
        }

        let frames = if let Some(fps) = job.frames_per_second {
            println!(
                "🎬 Extracting frames for '{}' ({}) at {:.2} fps across full video...",
                job.kind, job.label, fps
            );
            Self::extract_frames_full_video(&config, &job, fps, Arc::clone(&db)).await?
        } else {
            if job.video_timestamp.is_none() {
                return Err(LoggerError::Other(
                    "Processing job missing video timestamp (skipping)".to_string(),
                ));
            }
            println!("🎬 Extracting frames for '{}' ({})...", job.kind, job.label);
            Self::extract_frames_from_video(&config, &job, Arc::clone(&db)).await?
        };

        println!(
            "✅ Captured {} frames, starting parallel analysis...",
            frames.len()
        );

        let descriptions = Self::describe_frames_in_batches(
            Arc::clone(&api_service),
            Arc::clone(&config),
            &job,
            &frames,
            Arc::clone(&db),
        )
        .await?;

        if descriptions.is_empty() {
            eprintln!("⚠️  No frame descriptions generated (all frames failed to analyze)");
            return Err(LoggerError::Other(
                "No frames captured for context processing".to_string(),
            ));
        }

        println!(
            "✅ Analyzed {} / {} frames, generating summary...",
            descriptions.len(),
            frames.len()
        );
        let summary =
            Self::summarize_job(api_service.clone(), &config, &job, &descriptions).await?;
        Self::print_result(&job, &descriptions, &summary);
        Self::store_summary(db, &job, &descriptions, &summary).await?;
        Ok(())
    }

    fn print_result(job: &ProcessingJob, frames: &[FrameDescription], summary: &str) {
        println!(
            "\n🧠 Context analysis @ {} ({} {:?}):",
            job.timestamp_local(),
            job.kind,
            job.coordinates()
        );
        for frame in frames {
            if let Some(path) = &frame.file_path {
                println!(
                    "   • {:+.2}s: {}  [{}]",
                    frame.offset_secs,
                    frame.description,
                    path.display()
                );
            } else {
                println!("   • {:+.2}s: {}", frame.offset_secs, frame.description);
            }
        }
        println!("   → Summary: {}", summary.trim());
    }

    async fn describe_frames_in_batches(
        api_service: Arc<NexusApiService>,
        config: Arc<ProcessingConfig>,
        job: &ProcessingJob,
        frames: &[CapturedFrame],
        db: Arc<Database>,
    ) -> Result<Vec<FrameDescription>> {
        println!(
            "🤖  Analyzing {} frames in batches of {} (batches processed in parallel)...",
            frames.len(),
            FRAME_BATCH_SIZE
        );

        let mut handles = Vec::new();
        for (batch_idx, chunk) in frames.chunks(FRAME_BATCH_SIZE).enumerate() {
            let api = Arc::clone(&api_service);
            let cfg = Arc::clone(&config);
            let job_clone = job.clone();
            let batch_frames: Vec<CapturedFrame> = chunk.to_vec();
            let db_clone = Arc::clone(&db);

            let handle = tokio::spawn(async move {
                Self::process_batch(api, cfg, job_clone, batch_idx, batch_frames, db_clone).await
            });
            handles.push(handle);
        }

        let mut batches = Vec::new();
        for handle in handles {
            match handle.await {
                Ok(Ok(batch)) => batches.push(batch),
                Ok(Err(err)) => {
                    eprintln!("⚠️  Batch processing failed: {}", err);
                }
                Err(err) => {
                    eprintln!("⚠️  Batch task panicked: {}", err);
                }
            }
        }

        batches.sort_by_key(|batch| batch.index);

        let mut all_descriptions = Vec::new();
        for batch in batches {
            if let Some(summary) = &batch.summary {
                println!(
                    "🧾 Batch {} summary ({} frames): {}",
                    batch.index + 1,
                    batch.descriptions.len(),
                    summary
                );
            }
            all_descriptions.extend(batch.descriptions);
        }

        Ok(all_descriptions)
    }

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

    async fn extract_frames_from_video(
        config: &ProcessingConfig,
        job: &ProcessingJob,
        db: Arc<Database>,
    ) -> Result<Vec<CapturedFrame>> {
        let video_path = job.video_path.as_ref().ok_or_else(|| {
            LoggerError::Other("Video path not provided for frame extraction".to_string())
        })?;
        let base_timestamp = job.video_timestamp.ok_or_else(|| {
            LoggerError::Other("Video timestamp not provided for frame extraction".to_string())
        })?;

        if !video_path.exists() {
            return Err(LoggerError::Other(format!(
                "Video file not found: {}",
                video_path.display()
            )));
        }

        let video_duration = Self::get_video_duration(video_path).map_err(|e| {
            LoggerError::Other(format!(
                "Cannot determine video duration (required for frame extraction): {}",
                e
            ))
        })?;

        if base_timestamp > video_duration {
            return Err(LoggerError::Other(format!(
                "Context timestamp {:.2}s is after video ended at {:.2}s",
                base_timestamp, video_duration
            )));
        }

        let frame_interval_secs = config.frame_interval_ms as f64 / 1000.0;
        let available_duration = video_duration - base_timestamp;
        let max_possible_frames =
            ((available_duration / frame_interval_secs).floor() as u32 + 1).min(config.frame_count);

        if max_possible_frames == 0 {
            return Err(LoggerError::Other(format!(
                "Context timestamp {:.2}s too close to video end ({:.2}s)",
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
            std::mem::forget(temp);
            path
        };

        if !output_dir.exists() {
            fs::create_dir_all(&output_dir).map_err(|e| {
                LoggerError::Other(format!("Failed to create output directory: {}", e))
            })?;
        }

        println!(
            "🎬 Extracting {} frames from video at timestamp {:.2}s",
            actual_frame_count, base_timestamp
        );

        let mut extract_tasks = Vec::new();

        for idx in 0..actual_frame_count {
            let timestamp =
                base_timestamp + (idx as f64 * config.frame_interval_ms as f64 / 1000.0);
            let output_path = output_dir.join(Self::frame_filename(job, idx));
            let video_path_clone = video_path.clone();

            let task = tokio::task::spawn_blocking(move || -> Result<PathBuf> {
                Self::extract_single_frame(&video_path_clone, timestamp, &output_path)?;
                Ok(output_path)
            });

            extract_tasks.push((timestamp - base_timestamp, task));
        }

        for (offset_secs, task) in extract_tasks {
            match task.await {
                Ok(Ok(path)) => {
                    let data = tokio::fs::read(&path).await?;
                    let base64 = BASE64.encode(&data);
                    
                    // Get image dimensions and store screenshot
                    if let Some(session_id) = &job.session_id {
                        let frame_timestamp = base_timestamp + offset_secs;
                        let frame_number = frames.len() as u64;
                        let db_clone = Arc::clone(&db);
                        let path_str = path.to_string_lossy().to_string();
                        let session_id_clone = session_id.clone();
                        
                        // Get image dimensions
                        let (width, height) = tokio::task::spawn_blocking({
                            let path_clone = path.clone();
                            move || -> Result<(u32, u32)> {
                                // Check file exists and has content
                                let metadata = std::fs::metadata(&path_clone)
                                    .map_err(|e| LoggerError::Other(format!(
                                        "Failed to get metadata for {}: {}", 
                                        path_clone.display(), e
                                    )))?;
                                
                                if metadata.len() == 0 {
                                    return Err(LoggerError::Other(format!(
                                        "Image file is empty: {}", 
                                        path_clone.display()
                                    )));
                                }
                                
                                // Read first few bytes to check format
                                let mut file = std::fs::File::open(&path_clone)
                                    .map_err(|e| LoggerError::Io(std::io::Error::new(
                                        std::io::ErrorKind::Other,
                                        format!("Failed to open {}: {}", path_clone.display(), e)
                                    )))?;
                                
                                let mut header = [0u8; 8];
                                use std::io::Read;
                                file.read_exact(&mut header).map_err(|e| {
                                    LoggerError::Other(format!(
                                        "Failed to read header from {}: {}", 
                                        path_clone.display(), e
                                    ))
                                })?;
                                
                                // PNG signature: 89 50 4E 47 0D 0A 1A 0A
                                let png_signature = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
                                if header != png_signature {
                                    return Err(LoggerError::Other(format!(
                                        "File {} does not have PNG signature. \
                                        First 8 bytes: {:02X?}. File size: {} bytes. \
                                        Expected PNG signature: 89 50 4E 47 0D 0A 1A 0A",
                                        path_clone.display(),
                                        header,
                                        metadata.len()
                                    )));
                                }
                                
                                // Reset file and decode with explicit format
                                let reader = ImageReader::new(BufReader::new(
                                    std::fs::File::open(&path_clone).map_err(LoggerError::Io)?
                                ));
                                let img = reader.with_guessed_format()
                                    .map_err(|e| LoggerError::Other(format!(
                                        "Failed to create ImageReader for {}: {}", 
                                        path_clone.display(), e
                                    )))?
                                    .decode()
                                    .map_err(|e| LoggerError::Other(format!(
                                        "Failed to decode PNG image from {} (size: {} bytes): {}", 
                                        path_clone.display(), metadata.len(), e
                                    )))?;
                                Ok((img.width(), img.height()))
                            }
                        })
                        .await
                        .map_err(|e| LoggerError::Other(format!("Image decode task failed: {}", e)))??;
                        
                        // Store screenshot in database
                        let timestamp_utc = Utc::now();
                        let click_x = job.x;
                        let click_y = job.y;
                        let job_id = job.id.clone();
                        let job_kind = job.kind.clone();
                        tokio::spawn(async move {
                            if let Err(e) = db_clone
                                .insert_screenshot(
                                    &session_id_clone,
                                    timestamp_utc,
                                    frame_number,
                                    &path_str,
                                    width,
                                    height,
                                    click_x,
                                    click_y,
                                    Some(json!({
                                        "offset_secs": offset_secs,
                                        "video_timestamp": frame_timestamp,
                                        "job_id": job_id,
                                        "job_kind": job_kind,
                                    })),
                                )
                                .await
                            {
                                eprintln!("⚠️  Failed to store screenshot in database: {}", e);
                            }
                        });
                    }
                    
                    frames.push(CapturedFrame {
                        offset_secs: offset_secs,
                        base64_image: base64,
                        file_path: config.save_frames_dir.as_ref().map(|_| path),
                    });
                }
                Ok(Err(e)) => {
                    eprintln!("⚠️  Failed to extract frame at +{:.2}s: {}", offset_secs, e);
                }
                Err(e) => {
                    eprintln!(
                        "⚠️  Frame extraction task at +{:.2}s failed: {}",
                        offset_secs, e
                    );
                }
            }
        }

        Ok(frames)
    }

    async fn extract_frames_full_video(
        config: &ProcessingConfig,
        job: &ProcessingJob,
        frames_per_second: f64,
        db: Arc<Database>,
    ) -> Result<Vec<CapturedFrame>> {
        let video_path = job.video_path.as_ref().ok_or_else(|| {
            LoggerError::Other("Video path not provided for frame extraction".to_string())
        })?;

        if !video_path.exists() {
            return Err(LoggerError::Other(format!(
                "Video file not found: {}",
                video_path.display()
            )));
        }

        if frames_per_second <= 0.0 {
            return Err(LoggerError::Other(
                "Frames per second must be greater than zero".to_string(),
            ));
        }

        let video_duration = Self::get_video_duration(video_path)?;
        let frame_interval = 1.0 / frames_per_second;
        let mut timestamps = Vec::new();
        let mut current = 0.0;
        while current < video_duration {
            timestamps.push(current);
            current += frame_interval;
        }
        if timestamps.is_empty() {
            timestamps.push(0.0);
        }

        let output_dir = if let Some(root) = &config.save_frames_dir {
            root.clone()
        } else {
            let temp = tempdir().map_err(LoggerError::Io)?;
            let path = temp.path().to_path_buf();
            std::mem::forget(temp);
            path
        };

        if !output_dir.exists() {
            fs::create_dir_all(&output_dir).map_err(|e| {
                LoggerError::Other(format!("Failed to create output directory: {}", e))
            })?;
        }

        println!(
            "🎬 Extracting {} frames from full video ({:.2}s @ {:.2} fps)",
            timestamps.len(),
            video_duration,
            frames_per_second
        );

        let mut extract_tasks = Vec::new();
        for (idx, timestamp) in timestamps.iter().enumerate() {
            let output_path = output_dir.join(Self::frame_filename(job, idx as u32));
            let video_path_clone = video_path.clone();
            let ts = *timestamp;

            let task = tokio::task::spawn_blocking(move || -> Result<PathBuf> {
                Self::extract_single_frame(&video_path_clone, ts, &output_path)?;
                Ok(output_path)
            });
            extract_tasks.push((ts, idx, task));
        }

        let mut frames = Vec::new();
        for (timestamp, frame_idx, task) in extract_tasks {
            match task.await {
                Ok(Ok(path)) => {
                    let data = tokio::fs::read(&path).await?;
                    let base64 = BASE64.encode(&data);
                    
                    // Get image dimensions and store screenshot
                    if let Some(session_id) = &job.session_id {
                        let frame_number = frame_idx as u64;
                        let db_clone = Arc::clone(&db);
                        let path_str = path.to_string_lossy().to_string();
                        let session_id_clone = session_id.clone();
                        
                        // Get image dimensions
                        let (width, height) = tokio::task::spawn_blocking({
                            let path_clone = path.clone();
                            move || -> Result<(u32, u32)> {
                                // Check file exists and has content
                                let metadata = std::fs::metadata(&path_clone)
                                    .map_err(|e| LoggerError::Other(format!(
                                        "Failed to get metadata for {}: {}", 
                                        path_clone.display(), e
                                    )))?;
                                
                                if metadata.len() == 0 {
                                    return Err(LoggerError::Other(format!(
                                        "Image file is empty: {}", 
                                        path_clone.display()
                                    )));
                                }
                                
                                // Read first few bytes to check format
                                let mut file = std::fs::File::open(&path_clone)
                                    .map_err(|e| LoggerError::Io(std::io::Error::new(
                                        std::io::ErrorKind::Other,
                                        format!("Failed to open {}: {}", path_clone.display(), e)
                                    )))?;
                                
                                let mut header = [0u8; 8];
                                use std::io::Read;
                                file.read_exact(&mut header).map_err(|e| {
                                    LoggerError::Other(format!(
                                        "Failed to read header from {}: {}", 
                                        path_clone.display(), e
                                    ))
                                })?;
                                
                                // PNG signature: 89 50 4E 47 0D 0A 1A 0A
                                let png_signature = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
                                if header != png_signature {
                                    return Err(LoggerError::Other(format!(
                                        "File {} does not have PNG signature. \
                                        First 8 bytes: {:02X?}. File size: {} bytes. \
                                        Expected PNG signature: 89 50 4E 47 0D 0A 1A 0A",
                                        path_clone.display(),
                                        header,
                                        metadata.len()
                                    )));
                                }
                                
                                // Reset file and decode with explicit format
                                let reader = ImageReader::new(BufReader::new(
                                    std::fs::File::open(&path_clone).map_err(LoggerError::Io)?
                                ));
                                let img = reader.with_guessed_format()
                                    .map_err(|e| LoggerError::Other(format!(
                                        "Failed to create ImageReader for {}: {}", 
                                        path_clone.display(), e
                                    )))?
                                    .decode()
                                    .map_err(|e| LoggerError::Other(format!(
                                        "Failed to decode PNG image from {} (size: {} bytes): {}", 
                                        path_clone.display(), metadata.len(), e
                                    )))?;
                                Ok((img.width(), img.height()))
                            }
                        })
                        .await
                        .map_err(|e| LoggerError::Other(format!("Image decode task failed: {}", e)))??;
                        
                        // Store screenshot in database
                        let timestamp_utc = Utc::now();
                        let click_x = job.x;
                        let click_y = job.y;
                        let job_id = job.id.clone();
                        let job_kind = job.kind.clone();
                        tokio::spawn(async move {
                            if let Err(e) = db_clone
                                .insert_screenshot(
                                    &session_id_clone,
                                    timestamp_utc,
                                    frame_number,
                                    &path_str,
                                    width,
                                    height,
                                    click_x,
                                    click_y,
                                    Some(json!({
                                        "offset_secs": timestamp,
                                        "video_timestamp": timestamp,
                                        "job_id": job_id,
                                        "job_kind": job_kind,
                                        "full_video_sampling": true,
                                    })),
                                )
                                .await
                            {
                                eprintln!("⚠️  Failed to store screenshot in database: {}", e);
                            }
                        });
                    }
                    
                    frames.push(CapturedFrame {
                        offset_secs: timestamp,
                        base64_image: base64,
                        file_path: config.save_frames_dir.as_ref().map(|_| path),
                    });
                }
                Ok(Err(e)) => {
                    eprintln!("⚠️  Failed to extract frame at {:.2}s: {}", timestamp, e);
                }
                Err(e) => {
                    eprintln!(
                        "⚠️  Frame extraction task at {:.2}s failed: {}",
                        timestamp, e
                    );
                }
            }
        }

        Ok(frames)
    }

    fn extract_single_frame(video_path: &Path, timestamp: f64, output_path: &Path) -> Result<()> {
        use std::process::Command;

        if !video_path.exists() {
            return Err(LoggerError::Other(format!(
                "Video file not found: {}",
                video_path.display()
            )));
        }

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
            .arg("-vcodec")
            .arg("png")
            .arg("-pix_fmt")
            .arg("rgb24")
            .arg("-y")
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
            let stdout = String::from_utf8_lossy(&output.stdout);
            eprintln!("⚠️  FFmpeg extraction failed for {} at {:.2}s", output_path.display(), timestamp);
            eprintln!("   Command: ffmpeg -ss {:.3} -i {} -frames:v 1 -vcodec png -pix_fmt rgb24 -y {}", 
                timestamp, video_path.display(), output_path.display());
            eprintln!("   Exit code: {}", output.status.code().unwrap_or(-1));
            if !stderr.is_empty() {
                eprintln!("   Stderr:\n{}", stderr.lines().take(20).collect::<Vec<_>>().join("\n"));
            }
            if !stdout.is_empty() {
                eprintln!("   Stdout:\n{}", stdout.lines().take(20).collect::<Vec<_>>().join("\n"));
            }
            return Err(LoggerError::Other(format!(
                "FFmpeg frame extraction failed at {:.2}s from {}:\n  Exit code: {}\n  Stderr: {}\n  Stdout: {}",
                timestamp,
                video_path.display(),
                output.status.code().unwrap_or(-1),
                stderr.lines().take(10).collect::<Vec<_>>().join("\n    "),
                stdout.lines().take(10).collect::<Vec<_>>().join("\n    ")
            )));
        }

        if !output_path.exists() {
            return Err(LoggerError::Other(format!(
                "FFmpeg succeeded but output file not found: {}",
                output_path.display()
            )));
        }

        // Verify the file has content (not empty)
        let metadata = std::fs::metadata(&output_path).map_err(|e| {
            LoggerError::Other(format!(
                "Failed to get metadata for extracted frame {}: {}",
                output_path.display(),
                e
            ))
        })?;
        
        if metadata.len() == 0 {
            return Err(LoggerError::Other(format!(
                "FFmpeg extracted empty file at {}: {}",
                timestamp,
                output_path.display()
            )));
        }

        // Log successful extraction for debugging
        eprintln!(
            "✅ Extracted frame at {:.2}s: {} ({} bytes)",
            timestamp,
            output_path.display(),
            metadata.len()
        );

        Ok(())
    }

    #[allow(dead_code)]
    async fn capture_frames_live(
        screen_recorder: Arc<ScreenRecorder>,
        config: &ProcessingConfig,
        job: &ProcessingJob,
    ) -> Result<Vec<CapturedFrame>> {
        let mut frames = Vec::new();
        let temp_dir = if config.save_frames_dir.is_none() {
            Some(tempdir().map_err(LoggerError::Io)?)
        } else {
            None
        };

        println!(
            "🖼️  Capturing {} contextual frames for '{}' at {:?}",
            config.frame_count,
            job.label,
            job.coordinates()
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
                let path = Self::saved_frame_path(root, job, idx);
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent).map_err(LoggerError::Io)?;
                }
                path
            } else {
                temp_dir
                    .as_ref()
                    .expect("temporary directory should exist")
                    .path()
                    .join(Self::frame_filename(job, idx))
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
                offset_secs: idx as f64,
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

    async fn process_batch(
        api_service: Arc<NexusApiService>,
        config: Arc<ProcessingConfig>,
        job: ProcessingJob,
        batch_index: usize,
        frames: Vec<CapturedFrame>,
        db: Arc<Database>,
    ) -> Result<BatchResult> {
        println!(
            "\n🧩 Processing batch {} ({} frame{})",
            batch_index + 1,
            frames.len(),
            if frames.len() == 1 { "" } else { "s" }
        );

        let mut descriptions = Vec::new();
        for (frame_idx, frame) in frames.iter().enumerate() {
            match Self::describe_frame_with_context(
                Arc::clone(&api_service),
                config.as_ref(),
                &job,
                &descriptions,
                frame,
                Some(Arc::clone(&db)),
            )
            .await
            {
                Ok(text) => {
                    println!(
                        "   ✓ Batch {} Frame {} (+{:.2}s) analyzed",
                        batch_index + 1,
                        frame_idx + 1,
                        frame.offset_secs
                    );
                    descriptions.push(FrameDescription {
                        offset_secs: frame.offset_secs,
                        description: text,
                        file_path: frame.file_path.clone(),
                    });
                }
                Err(err) => {
                    eprintln!(
                        "   ✗ Batch {} Frame {} (+{:.2}s) failed: {}",
                        batch_index + 1,
                        frame_idx + 1,
                        frame.offset_secs,
                        err
                    );
                }
            }
        }

        let summary = if !descriptions.is_empty() {
            match Self::summarize_batch(
                Arc::clone(&api_service),
                config.as_ref(),
                &job,
                batch_index,
                &descriptions,
            )
            .await
            {
                Ok(text) if !text.is_empty() => Some(text),
                Ok(_) => None,
                Err(err) => {
                    eprintln!("⚠️  Failed to summarize batch {}: {}", batch_index + 1, err);
                    None
                }
            }
        } else {
            None
        };

        Ok(BatchResult {
            index: batch_index,
            descriptions,
            summary,
        })
    }

    /// Reconstruct text from keyboard events, handling backspace and special keys
    fn reconstruct_text_from_keys(events: &[crate::services::database::TimelineEvent]) -> String {
        use std::collections::HashMap;
        
        let mut text = String::new();
        let mut modifier_keys: HashMap<String, bool> = HashMap::new();
        let mut last_key_time: Option<DateTime<Utc>> = None;
        
        // Filter to only keyboard press events, sorted by time
        let mut key_events: Vec<_> = events.iter()
            .filter(|e| e.event_type == "keyboard" && e.pressed == Some(true))
            .collect();
        
        key_events.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
        
        for event in key_events {
            let key = event.key.as_ref().map(|k| k.as_str()).unwrap_or("");
            
            // Track modifier keys
            if key == "LControl" || key == "RControl" || key == "Control" {
                modifier_keys.insert("Ctrl".to_string(), true);
                continue;
            }
            if key == "LShift" || key == "RShift" || key == "Shift" {
                modifier_keys.insert("Shift".to_string(), true);
                continue;
            }
            if key == "LAlt" || key == "RAlt" || key == "Alt" {
                modifier_keys.insert("Alt".to_string(), true);
                continue;
            }
            
            // Handle special keys
            match key {
                "Backspace" => {
                    text.pop();
                    modifier_keys.clear();
                    continue;
                }
                "Delete" => {
                    // Delete removes next char, but we're building forward, so just skip
                    modifier_keys.clear();
                    continue;
                }
                "Enter" => {
                    if modifier_keys.contains_key("Ctrl") {
                        text.push_str("[Ctrl+Enter]");
                    } else {
                        text.push_str("\n");
                    }
                    modifier_keys.clear();
                    continue;
                }
                "Tab" => {
                    text.push_str("    "); // 4 spaces for tab
                    modifier_keys.clear();
                    continue;
                }
                "Space" => {
                    text.push(' ');
                    modifier_keys.clear();
                    last_key_time = Some(event.timestamp);
                    continue;
                }
                "Escape" | "Return" => {
                    modifier_keys.clear();
                    continue;
                }
                _ => {}
            }
            
            // Handle modifier combinations
            if modifier_keys.contains_key("Ctrl") {
                // Common shortcuts
                match key {
                    "C" => {
                        text.push_str("[Ctrl+C]");
                        modifier_keys.clear();
                        continue;
                    }
                    "V" => {
                        text.push_str("[Ctrl+V]");
                        modifier_keys.clear();
                        continue;
                    }
                    "X" => {
                        text.push_str("[Ctrl+X]");
                        modifier_keys.clear();
                        continue;
                    }
                    "Z" => {
                        text.push_str("[Ctrl+Z]");
                        modifier_keys.clear();
                        continue;
                    }
                    "A" => {
                        text.push_str("[Ctrl+A]");
                        modifier_keys.clear();
                        continue;
                    }
                    "S" => {
                        text.push_str("[Ctrl+S]");
                        modifier_keys.clear();
                        continue;
                    }
                    _ => {
                        // Other Ctrl+key combinations
                        text.push_str(&format!("[Ctrl+{}]", key));
                        modifier_keys.clear();
                        continue;
                    }
                }
            }
            
            // Regular character - check for word boundary
            if let Some(last_time) = last_key_time {
                let gap = event.timestamp.signed_duration_since(last_time);
                if gap.num_milliseconds() > KEY_GAP_THRESHOLD_MS as i64 {
                    text.push(' '); // Word boundary
                }
            }
            
            // Convert key to character (simplified - handles common cases)
            let ch_opt: Option<char> = if modifier_keys.contains_key("Shift") {
                // Uppercase or shifted characters
                match key {
                    "1" => Some('!'),
                    "2" => Some('@'),
                    "3" => Some('#'),
                    "4" => Some('$'),
                    "5" => Some('%'),
                    "6" => Some('^'),
                    "7" => Some('&'),
                    "8" => Some('*'),
                    "9" => Some('('),
                    "0" => Some(')'),
                    "-" => Some('_'),
                    "=" => Some('+'),
                    "[" => Some('{'),
                    "]" => Some('}'),
                    "\\" => Some('|'),
                    ";" => Some(':'),
                    "'" => Some('"'),
                    "," => Some('<'),
                    "." => Some('>'),
                    "/" => Some('?'),
                    _ => {
                        // Try to get uppercase version
                        if key.len() == 1 {
                            key.chars().next().map(|c| c.to_ascii_uppercase())
                        } else {
                            None
                        }
                    }
                }
            } else {
                // Regular character
                if key.len() == 1 {
                    key.chars().next()
                } else {
                    None
                }
            };
            
            // Only add single character keys (filter out multi-char key names)
            if let Some(ch) = ch_opt {
                text.push(ch);
            }
            
            modifier_keys.clear();
            last_key_time = Some(event.timestamp);
        }
        
        text.trim().to_string()
    }
    
    /// Gather context (clicks and keyboard events) for a frame
    async fn gather_frame_context(
        db: &Database,
        session_id: &str,
        video_timestamp: f64,
    ) -> Result<(Vec<String>, String)> {
        // Get events in time window
        let events = db.get_events_in_window(
            session_id,
            video_timestamp,
            CONTEXT_WINDOW_BEFORE,
            CONTEXT_WINDOW_AFTER,
        ).await?;
        
        let mut clicks = Vec::new();
        let mut keyboard_events = Vec::new();
        
        for event in &events {
            match event.event_type.as_str() {
                "mouse" => {
                    if event.event_subtype.as_deref() == Some("click") {
                        let button = event.button.as_deref().unwrap_or("unknown");
                        let coords = if let (Some(x), Some(y)) = (event.x, event.y) {
                            format!("({}, {})", x, y)
                        } else {
                            String::new()
                        };
                        let time_str = if let Some(tc) = event.timecode {
                            format!("{:.2}s", tc)
                        } else {
                            "?".to_string()
                        };
                        clicks.push(format!("{} click at {} {}", button, coords, time_str));
                    }
                }
                "keyboard" => {
                    keyboard_events.push(event.clone());
                }
                _ => {}
            }
        }
        
        // Reconstruct text from keyboard events
        let reconstructed_text = Self::reconstruct_text_from_keys(&keyboard_events);
        
        Ok((clicks, reconstructed_text))
    }

    async fn describe_frame_with_context(
        api_service: Arc<NexusApiService>,
        config: &ProcessingConfig,
        job: &ProcessingJob,
        prior_descriptions: &[FrameDescription],
        frame: &CapturedFrame,
        db: Option<Arc<Database>>,
    ) -> Result<String> {
        let coordinates = job.coordinates().unwrap_or((0, 0));
        
        // Calculate absolute video timestamp for this frame
        let frame_video_timestamp = if let Some(base) = job.video_timestamp {
            base + frame.offset_secs
        } else {
            // For full video sampling, offset_secs is already absolute
            frame.offset_secs
        };
        
        // Gather context (clicks and keyboard events) if database is available
        let mut context_info = String::new();
        if let Some(db_ref) = db.as_ref() {
            if let Some(session_id) = &job.session_id {
                match Self::gather_frame_context(db_ref, session_id, frame_video_timestamp).await {
                    Ok((clicks, reconstructed_text)) => {
                        if !clicks.is_empty() || !reconstructed_text.is_empty() {
                            context_info.push_str("\n\nUser activity in this time window (±2s):");
                            if !clicks.is_empty() {
                                context_info.push_str("\n• Clicks: ");
                                context_info.push_str(&clicks.join(", "));
                            }
                            if !reconstructed_text.is_empty() {
                                context_info.push_str(&format!("\n• Text entered: \"{}\"", reconstructed_text));
                            }
                        }
                    }
                    Err(e) => {
                        // Log but don't fail - context is optional
                        eprintln!("⚠️  Failed to gather frame context: {}", e);
                    }
                }
            }
        }
        
        let mut prompt = format!(
            "Frame captured +{:.2}s from '{}' at ({}, {}).{}",
            frame.offset_secs, job.label, coordinates.0, coordinates.1, context_info
        );

        if prior_descriptions.is_empty() {
            prompt.push_str(
                " This is the first frame in this batch. Describe the visible UI in one or two sentences and infer the user's likely intent.",
            );
        } else {
            prompt.push_str(
                " Continue the story by referencing the prior observations below. Highlight what changed, what stayed the same, and what the user is probably doing now.",
            );
            prompt.push_str("\n\nPrior frames:");
            for desc in prior_descriptions {
                prompt.push_str(&format!(
                    "\n• +{:.2}s: {}",
                    desc.offset_secs, desc.description
                ));
            }
            prompt.push_str("\n\nDescribe the current frame:");
        }

        let content = MessageContent::Array(vec![
            ContentPart::Text { text: prompt },
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

    async fn summarize_job(
        api_service: Arc<NexusApiService>,
        config: &ProcessingConfig,
        job: &ProcessingJob,
        frames: &[FrameDescription],
    ) -> Result<String> {
        let mut user_prompt = format!(
            "Context '{}' ({}) at {:?}.\n",
            job.label,
            job.kind,
            job.coordinates()
        );

        for frame in frames {
            user_prompt.push_str(&format!(
                "Frame +{:.2}s: {}\n",
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

    async fn summarize_batch(
        api_service: Arc<NexusApiService>,
        config: &ProcessingConfig,
        job: &ProcessingJob,
        batch_idx: usize,
        frames: &[FrameDescription],
    ) -> Result<String> {
        if frames.is_empty() {
            return Ok(String::new());
        }

        let mut prompt = format!(
            "Batch {} of context '{}' ({} frames).\n",
            batch_idx + 1,
            job.label,
            frames.len()
        );
        for frame in frames {
            prompt.push_str(&format!(
                "Frame +{:.2}s: {}\n",
                frame.offset_secs, frame.description
            ));
        }
        prompt.push_str(
            "Summarize this batch in one or two sentences, focusing on how the user's behavior evolved during these frames.",
        );

        let messages = vec![
            Message::system(SUMMARY_SYSTEM_PROMPT),
            Message::user(prompt),
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
        job: &ProcessingJob,
        frames: &[FrameDescription],
        summary: &str,
    ) -> Result<()> {
        let session_id = match &job.session_id {
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
            "job": {
                "timestamp": job.timestamp_utc.to_rfc3339(),
                "kind": job.kind,
                "label": job.label,
                "button": job.button,
                "x": job.x,
                "y": job.y,
                "video_timestamp": job.video_timestamp,
            },
            "metadata": job.metadata
        });

        let timestamp = job.timestamp_utc.to_rfc3339();

        db.insert_event(
            session_id,
            "analysis",
            Some(&job.kind),
            None,
            job.button.as_deref(),
            job.x,
            job.y,
            None,
            &timestamp,
            None,
            Some(metadata),
            None,
        )
        .await?;

        Ok(())
    }

    fn frame_filename(job: &ProcessingJob, idx: u32) -> String {
        let session = job.session_id.as_deref().unwrap_or("session");
        format!("{}_{}_frame_{}.png", session, job.id, idx)
    }

    #[allow(dead_code)]
    fn saved_frame_path(root: &Path, job: &ProcessingJob, idx: u32) -> PathBuf {
        root.join(Self::frame_filename(job, idx))
    }
}

#[derive(Clone)]
struct CapturedFrame {
    offset_secs: f64,
    base64_image: String,
    file_path: Option<PathBuf>,
}

struct FrameDescription {
    offset_secs: f64,
    description: String,
    file_path: Option<PathBuf>,
}

struct BatchResult {
    index: usize,
    descriptions: Vec<FrameDescription>,
    summary: Option<String>,
}
