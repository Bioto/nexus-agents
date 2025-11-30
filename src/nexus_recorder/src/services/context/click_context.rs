use super::context_processing::{
    ProcessingConfig, ProcessingHandle, ProcessingJob, ProcessingService,
};
use crate::services::storage::Database;
use chrono::{DateTime, Utc};
use serde_json::json;
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Clone)]
pub struct ClickContextHandle {
    inner: ProcessingHandle,
}

impl From<ProcessingHandle> for ClickContextHandle {
    fn from(inner: ProcessingHandle) -> Self {
        Self { inner }
    }
}

impl ClickContextHandle {
    pub fn trigger(&self, event: ClickContextEvent) {
        self.inner.trigger(event.into());
    }

    pub fn trigger_job(&self, job: ProcessingJob) {
        self.inner.trigger(job);
    }

    pub async fn wait_for_completion(self) {
        self.inner.wait_for_completion().await;
    }

    /// Get the inner ProcessingHandle for direct access
    pub fn inner(&self) -> &ProcessingHandle {
        &self.inner
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

    fn click_label(&self) -> String {
        self.button
            .clone()
            .unwrap_or_else(|| "unknown button".to_string())
    }
}

pub struct ClickContextService;

impl ClickContextService {
    pub fn maybe_start(db: Database) -> Option<ClickContextHandle> {
        let config = ProcessingConfig::from_env();
        if !config.enabled {
            return None;
        }

        match ProcessingService::start(config, db) {
            Ok(handle) => Some(handle.into()),
            Err(err) => {
                eprintln!(
                    "⚠️  Click context analysis disabled (initialization failed): {}",
                    err
                );
                None
            }
        }
    }
}

impl From<ClickContextEvent> for ProcessingJob {
    fn from(event: ClickContextEvent) -> Self {
        ProcessingJob::new("click_context", event.click_label(), event.timestamp_utc)
            .with_id(event.id)
            .with_session_id(event.session_id)
            .with_button(event.button.clone())
            .with_coordinates(event.x, event.y)
            .with_video_context(event.video_timestamp, event.video_path)
            .with_metadata(json!({
                "click": {
                    "button": event.button,
                    "x": event.x,
                    "y": event.y,
                }
            }))
    }
}
