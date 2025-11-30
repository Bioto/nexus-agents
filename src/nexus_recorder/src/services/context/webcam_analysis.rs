//! Webcam sentiment and attention analysis service.
//!
//! This module provides real-time analysis of webcam frames to detect user sentiment,
//! attention level, and behavioral observations during recording sessions.

use crate::error::{RecorderError, Result};
use crate::services::storage::Database;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use chrono::Utc;
use log::{debug, error, info, warn};
use nexus_core::models::{ContentPart, ImageUrl};
use nexus_core::{ChatCompletionRequest, Message, MessageContent, NexusApiService};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::env;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tempfile::tempdir;
use uuid::Uuid;

/// System prompt for webcam sentiment analysis.
const WEBCAM_ANALYSIS_SYSTEM_PROMPT: &str = r#"You are an expert in analyzing human behavior and emotions from video frames. 
Analyze the webcam frame of a computer user and provide observations about their current state.
Be objective and concise. Focus on observable cues like facial expression, posture, and gaze direction.
Do not make assumptions beyond what is visually apparent."#;

/// User prompt template for webcam analysis.
const WEBCAM_ANALYSIS_USER_PROMPT: &str = r#"Analyze this webcam frame of a computer user. Describe:
1. Emotional state: happy, neutral, frustrated, confused, focused, tired, or other
2. Attention: focused on screen, distracted, looking away, engaged, multitasking
3. Energy level: high, medium, low
4. Notable observations about posture, gestures, or behavior

Be concise. Respond with a JSON object containing these fields:
{
  "sentiment": "string - primary emotional state",
  "attention": "string - attention/focus state", 
  "energy": "string - energy level",
  "confidence": "number 0-1 - how confident you are in this assessment",
  "notes": "string - brief additional observations"
}"#;

/// Configuration for webcam sentiment analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebcamAnalysisConfig {
    /// Whether webcam analysis is enabled.
    pub enabled: bool,
    /// Interval between frame analyses in seconds.
    pub interval_secs: u64,
    /// Vision model to use for analysis.
    pub model: String,
    /// Maximum tokens for analysis response.
    pub max_tokens: u32,
    /// Webcam device path (e.g., /dev/video0)
    pub device_path: String,
}

impl Default for WebcamAnalysisConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            interval_secs: 5,
            model: env::var("VISION_MODEL").unwrap_or_else(|_| "gpt-4o-mini".to_string()),
            max_tokens: 150,
            device_path: "/dev/video0".to_string(),
        }
    }
}

impl WebcamAnalysisConfig {
    /// Create a new config with analysis enabled.
    pub fn new(interval_secs: u64) -> Self {
        Self {
            enabled: true,
            interval_secs,
            ..Default::default()
        }
    }

    /// Create a new config with analysis enabled and custom device path.
    pub fn with_device(interval_secs: u64, device_path: String) -> Self {
        Self {
            enabled: true,
            interval_secs,
            device_path,
            ..Default::default()
        }
    }

    /// Create config from environment variables.
    pub fn from_env() -> Self {
        let mut config = Self::default();

        if let Ok(value) = env::var("WEBCAM_ANALYSIS_ENABLED") {
            config.enabled = matches!(
                value.to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            );
        }

        if let Ok(value) = env::var("WEBCAM_ANALYSIS_INTERVAL") {
            if let Ok(interval) = value.parse::<u64>() {
                config.interval_secs = interval.max(1);
            }
        }

        if let Ok(model) = env::var("WEBCAM_ANALYSIS_MODEL") {
            config.model = model;
        }

        if let Ok(value) = env::var("WEBCAM_ANALYSIS_MAX_TOKENS") {
            if let Ok(tokens) = value.parse::<u32>() {
                config.max_tokens = tokens.max(50).min(500);
            }
        }

        if let Ok(device) = env::var("WEBCAM_ANALYSIS_DEVICE") {
            config.device_path = device;
        }

        config
    }
}

/// Handle for controlling the webcam analysis service.
#[derive(Clone)]
pub struct WebcamAnalysisHandle {
    stop_flag: Arc<AtomicBool>,
    _task_handle: Arc<tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>>,
}

impl WebcamAnalysisHandle {
    /// Signal the analysis service to stop.
    pub fn stop(&self) {
        self.stop_flag.store(true, Ordering::SeqCst);
    }

    /// Wait for analysis to complete.
    pub async fn wait_for_completion(self) {
        self.stop_flag.store(true, Ordering::SeqCst);
        // Give time for final analysis to complete
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

/// Service for periodic webcam sentiment analysis.
pub struct WebcamAnalysisService;

impl WebcamAnalysisService {
    /// Extract a frame from a video file at a specific timestamp.
    /// This works even while the video is still being recorded.
    fn extract_frame_from_video(video_path: &PathBuf, timestamp_secs: f64) -> Result<Vec<u8>> {
        let temp_dir = tempdir().map_err(RecorderError::Io)?;
        let output_path = temp_dir.path().join("frame.jpg");

        // Use ffmpeg to extract a frame from the video file
        // Using -sseof to seek from the end (more reliable for live files)
        // or -ss with a recent timestamp
        let output = Command::new("ffmpeg")
            .arg("-ss")
            .arg(format!("{:.2}", timestamp_secs.max(0.0)))
            .arg("-i")
            .arg(video_path)
            .arg("-frames:v")
            .arg("1")
            .arg("-q:v")
            .arg("2") // High quality JPEG
            .arg("-f")
            .arg("image2")
            .arg("-y")
            .arg(&output_path)
            .stderr(Stdio::piped())
            .stdout(Stdio::piped())
            .output()
            .map_err(|e| RecorderError::Other(format!("Failed to run ffmpeg: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // Check for common "not enough data" errors
            if stderr.contains("Output file is empty") || stderr.contains("nothing was encoded") {
                return Err(RecorderError::Other(
                    "Video file doesn't have enough data yet (try again later)".to_string()
                ));
            }
            // Show the last 10 lines of stderr (where actual errors are) instead of first 5 (version header)
            let error_lines: Vec<&str> = stderr.lines().collect();
            let error_msg = if error_lines.len() > 10 {
                error_lines[error_lines.len()-10..].join("\n")
            } else {
                error_lines.join("\n")
            };
            return Err(RecorderError::Other(format!(
                "FFmpeg frame extraction failed (exit code {:?}): {}",
                output.status.code(),
                error_msg
            )));
        }

        // Check if output file exists and has content
        if !output_path.exists() {
            return Err(RecorderError::Other(
                "FFmpeg succeeded but output file not created".to_string()
            ));
        }

        let data = std::fs::read(&output_path).map_err(|e| {
            RecorderError::Other(format!("Failed to read extracted frame: {}", e))
        })?;

        if data.is_empty() {
            return Err(RecorderError::Other(
                "Extracted frame is empty (video may not have enough data yet)".to_string()
            ));
        }

        Ok(data)
    }

    /// Analyze a single frame using the vision API.
    async fn analyze_frame(
        api_service: &NexusApiService,
        frame_data: &[u8],
        model: &str,
        max_tokens: u32,
    ) -> Result<String> {
        let base64_image = BASE64.encode(frame_data);

        let content = MessageContent::Array(vec![
            ContentPart::Text {
                text: WEBCAM_ANALYSIS_USER_PROMPT.to_string(),
            },
            ContentPart::ImageUrl {
                image_url: ImageUrl {
                    url: format!("data:image/jpeg;base64,{}", base64_image),
                },
            },
        ]);

        let messages = vec![
            Message::system(WEBCAM_ANALYSIS_SYSTEM_PROMPT),
            Message::user_with_content(content),
        ];

        let mut request = ChatCompletionRequest::new(model.to_string(), messages);
        request.max_tokens = Some(max_tokens);

        let response = api_service.chat(request).await?;
        let text = response
            .content
            .as_ref()
            .map(|c| c.extract_text())
            .unwrap_or_default();

        Ok(text.trim().to_string())
    }

    /// Store analysis result in the database.
    async fn store_analysis(
        db: &Database,
        session_id: &str,
        frame_index: u64,
        video_timestamp: f64,
        analysis_result: &str,
    ) -> Result<()> {
        let timestamp = Utc::now().to_rfc3339();

        // Try to parse the analysis result as JSON
        let metadata = if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(analysis_result)
        {
            json!({
                "frame_index": frame_index,
                "video_timestamp": video_timestamp,
                "analysis": parsed,
            })
        } else {
            json!({
                "frame_index": frame_index,
                "video_timestamp": video_timestamp,
                "analysis_text": analysis_result,
            })
        };

        db.insert_event(
            session_id,
            "analysis",
            Some("webcam_sentiment"),
            None,
            None,
            None,
            None,
            None,
            &timestamp,
            Some(video_timestamp),
            Some(metadata),
            None,
        )
        .await?;

        Ok(())
    }

    /// Start the webcam analysis service.
    ///
    /// This spawns a background task that periodically extracts frames from the
    /// recording video file and analyzes them for sentiment.
    pub fn start(
        config: WebcamAnalysisConfig,
        db: Database,
        session_id: String,
        video_path: PathBuf, // Path to the video file being recorded
    ) -> Result<WebcamAnalysisHandle> {
        if !config.enabled {
            return Err(RecorderError::Other(
                "Webcam analysis is not enabled".to_string(),
            ));
        }

        info!(
            "🎭 Starting webcam sentiment analysis (interval: {}s, model: {}, video: {})",
            config.interval_secs, config.model, video_path.display()
        );

        // Create the vision API service
        let api_service = Arc::new(NexusApiService::from_env_vision()?);
        let db = Arc::new(db);
        let stop_flag = Arc::new(AtomicBool::new(false));
        let interval = Duration::from_secs(config.interval_secs);

        let task_stop_flag = Arc::clone(&stop_flag);
        let model = config.model.clone();
        let max_tokens = config.max_tokens;

        // During live recording, the video is written as MPEG-TS (for streaming access)
        // The path we receive is the final .mp4 path, so we need to use .ts extension
        let live_video_path = if video_path.extension().and_then(|e| e.to_str()) == Some("mp4") {
            video_path.with_extension("ts")
        } else {
            video_path.clone()
        };
        
        info!("🎭 Will read from live file: {}", live_video_path.display());

        // Spawn the periodic analysis task
        let task_handle = tokio::spawn(async move {
            let mut frame_index = 0u64;
            let start_time = std::time::Instant::now();

            // Wait for video to have enough data (at least 5 seconds of recording)
            info!("🎭 Waiting 5s for video to accumulate data...");
            tokio::time::sleep(Duration::from_secs(5)).await;

            loop {
                if task_stop_flag.load(Ordering::SeqCst) {
                    info!("🎭 Webcam analysis stopping...");
                    break;
                }

                let elapsed = start_time.elapsed().as_secs_f64();
                // Extract a frame from a few seconds ago (to ensure data exists)
                let frame_timestamp = (elapsed - 3.0).max(1.0);

                // Check if the video file exists
                if !live_video_path.exists() {
                    warn!("⚠️  Video file not found yet: {}", live_video_path.display());
                    tokio::time::sleep(interval).await;
                    continue;
                }
                
                info!("🎭 Extracting frame {} at {:.1}s from {} ...", frame_index, frame_timestamp, live_video_path.display());
                
                let frame_result = tokio::task::spawn_blocking({
                    let video = live_video_path.clone();
                    move || Self::extract_frame_from_video(&video, frame_timestamp)
                })
                .await;

                match frame_result {
                    Ok(Ok(frame_data)) => {
                        info!(
                            "🎭 Frame {} extracted ({} bytes), analyzing...",
                            frame_index,
                            frame_data.len()
                        );

                        // Analyze the frame
                        match Self::analyze_frame(&api_service, &frame_data, &model, max_tokens)
                            .await
                        {
                            Ok(analysis) => {
                                info!(
                                    "🎭 Frame {} analysis result: {}",
                                    frame_index,
                                    analysis.chars().take(150).collect::<String>()
                                );

                                // Store the analysis
                                if let Err(e) = Self::store_analysis(
                                    &db,
                                    &session_id,
                                    frame_index,
                                    frame_timestamp,
                                    &analysis,
                                )
                                .await
                                {
                                    warn!("⚠️  Failed to store webcam analysis: {}", e);
                                }
                            }
                            Err(e) => {
                                warn!("⚠️  Frame {} analysis API call failed: {}", frame_index, e);
                            }
                        }
                    }
                    Ok(Err(e)) => {
                        // Log at warn level so it's visible
                        warn!("⚠️  Frame {} extraction failed: {}", frame_index, e);
                    }
                    Err(e) => {
                        error!("⚠️  Frame {} extraction task panicked: {}", frame_index, e);
                    }
                }

                frame_index += 1;

                // Wait for next interval
                tokio::time::sleep(interval).await;
            }

            info!(
                "🎭 Webcam analysis completed: {} frames analyzed",
                frame_index
            );
        });

        Ok(WebcamAnalysisHandle {
            stop_flag,
            _task_handle: Arc::new(tokio::sync::Mutex::new(Some(task_handle))),
        })
    }

    /// Start analysis only if enabled in config.
    pub fn maybe_start(
        config: Option<WebcamAnalysisConfig>,
        db: Database,
        session_id: String,
        video_path: PathBuf,
    ) -> Option<WebcamAnalysisHandle> {
        let config = config?;

        if !config.enabled {
            info!("🎭 Webcam analysis disabled");
            return None;
        }

        match Self::start(config, db, session_id, video_path) {
            Ok(handle) => Some(handle),
            Err(e) => {
                warn!("⚠️  Failed to start webcam analysis: {}", e);
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = WebcamAnalysisConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.interval_secs, 5);
        assert_eq!(config.max_tokens, 150);
        assert_eq!(config.device_path, "/dev/video0");
    }

    #[test]
    fn test_new_config() {
        let config = WebcamAnalysisConfig::new(10);
        assert!(config.enabled);
        assert_eq!(config.interval_secs, 10);
    }

    #[test]
    fn test_with_device_config() {
        let config = WebcamAnalysisConfig::with_device(10, "/dev/video2".to_string());
        assert!(config.enabled);
        assert_eq!(config.interval_secs, 10);
        assert_eq!(config.device_path, "/dev/video2");
    }
}
