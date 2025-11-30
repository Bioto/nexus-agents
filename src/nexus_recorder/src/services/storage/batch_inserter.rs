//! Batch event inserter for ClickHouse.
//!
//! This module provides batched database inserts to reduce per-event overhead.
//! Events are buffered in memory and flushed either:
//! - When the buffer reaches a threshold (default: 100 events)
//! - After a time interval (default: 1 second)
//! - On explicit flush or shutdown

use crate::error::{RecorderError, Result};
use super::database::Database;
use serde_json::Value;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

/// Configuration for the batch inserter.
#[derive(Clone, Debug)]
pub struct BatchInserterConfig {
    /// Maximum number of events to buffer before flushing
    pub batch_size: usize,
    /// Maximum time to wait before flushing
    pub flush_interval: Duration,
    /// Channel buffer size for backpressure
    pub channel_size: usize,
}

impl Default for BatchInserterConfig {
    fn default() -> Self {
        Self {
            batch_size: 100,
            flush_interval: Duration::from_secs(1),
            channel_size: 10_000,
        }
    }
}

/// Event data for batch insertion.
#[derive(Debug, Clone)]
pub struct BatchEvent {
    /// Session ID (Arc<str> for cheap cloning in hot paths)
    pub session_id: Arc<str>,
    pub event_type: String,
    pub event_subtype: Option<String>,
    pub key: Option<String>,
    pub button: Option<String>,
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub pressed: Option<bool>,
    pub timestamp: String,
    pub timecode: Option<f64>,
    pub metadata: Option<Value>,
    pub screenshot_id: Option<String>,
}

/// Message sent to the inserter task
enum InserterMessage {
    /// Insert an event
    Insert(BatchEvent),
    /// Flush the buffer immediately
    Flush,
    /// Shutdown the inserter
    Shutdown,
}

/// Handle to send events to the batch inserter
#[derive(Clone)]
pub struct BatchInserterHandle {
    tx: mpsc::Sender<InserterMessage>,
    events_inserted: Arc<AtomicU64>,
    batches_flushed: Arc<AtomicU64>,
    events_dropped: Arc<AtomicU64>,
}

impl BatchInserterHandle {
    /// Queue an event for batch insertion
    pub async fn insert(&self, event: BatchEvent) -> Result<()> {
        self.tx
            .send(InserterMessage::Insert(event))
            .await
            .map_err(|e| RecorderError::Other(format!("Failed to send insert message: {}", e)))?;
        Ok(())
    }

    /// Queue an event synchronously (non-blocking, may drop if buffer full)
    pub fn try_insert(&self, event: BatchEvent) -> bool {
        match self.tx.try_send(InserterMessage::Insert(event)) {
            Ok(()) => true,
            Err(mpsc::error::TrySendError::Full(_)) => {
                self.events_dropped.fetch_add(1, Ordering::Relaxed);
                false
            }
            Err(mpsc::error::TrySendError::Closed(_)) => false,
        }
    }

    /// Force flush the buffer
    pub async fn flush(&self) -> Result<()> {
        self.tx
            .send(InserterMessage::Flush)
            .await
            .map_err(|e| RecorderError::Other(format!("Failed to send flush message: {}", e)))?;
        Ok(())
    }

    /// Shutdown the inserter gracefully
    pub async fn shutdown(&self) -> Result<()> {
        self.tx
            .send(InserterMessage::Shutdown)
            .await
            .map_err(|e| RecorderError::Other(format!("Failed to send shutdown message: {}", e)))?;
        Ok(())
    }

    /// Get the number of events inserted
    pub fn events_inserted(&self) -> u64 {
        self.events_inserted.load(Ordering::Relaxed)
    }

    /// Get the number of batches flushed
    pub fn batches_flushed(&self) -> u64 {
        self.batches_flushed.load(Ordering::Relaxed)
    }

    /// Get the number of events dropped due to backpressure
    pub fn events_dropped(&self) -> u64 {
        self.events_dropped.load(Ordering::Relaxed)
    }
}

/// Batch event inserter that buffers events and performs batch inserts.
pub struct BatchEventInserter {
    config: BatchInserterConfig,
    buffer: Vec<BatchEvent>,
    events_inserted: Arc<AtomicU64>,
    batches_flushed: Arc<AtomicU64>,
    #[allow(dead_code)] // Tracked but only read from BatchInserterHandle
    events_dropped: Arc<AtomicU64>,
}

impl BatchEventInserter {
    /// Create a new batch inserter and spawn the background task.
    ///
    /// Returns a handle that can be used to send events to the inserter.
    pub fn spawn(
        config: BatchInserterConfig,
        stop_signal: Arc<AtomicBool>,
    ) -> Result<(BatchInserterHandle, tokio::task::JoinHandle<Result<()>>)> {
        let (tx, rx) = mpsc::channel(config.channel_size);
        let events_inserted = Arc::new(AtomicU64::new(0));
        let batches_flushed = Arc::new(AtomicU64::new(0));
        let events_dropped = Arc::new(AtomicU64::new(0));

        let handle = BatchInserterHandle {
            tx,
            events_inserted: events_inserted.clone(),
            batches_flushed: batches_flushed.clone(),
            events_dropped: events_dropped.clone(),
        };

        let inserter = Self {
            config,
            buffer: Vec::with_capacity(100),
            events_inserted,
            batches_flushed,
            events_dropped,
        };

        let task_handle = tokio::spawn(inserter.run(rx, stop_signal));

        Ok((handle, task_handle))
    }

    /// Main inserter loop
    async fn run(
        mut self,
        mut rx: mpsc::Receiver<InserterMessage>,
        stop_signal: Arc<AtomicBool>,
    ) -> Result<()> {
        let mut last_flush = Instant::now();

        // Initialize database connection
        let db = match Database::new().await {
            Ok(db) => db,
            Err(e) => {
                eprintln!(
                    "⚠️  Failed to initialize database for batch inserter: {}",
                    e
                );
                return Err(e);
            }
        };

        loop {
            // Check stop signal
            if stop_signal.load(Ordering::SeqCst) {
                // Final flush before shutdown
                if !self.buffer.is_empty() {
                    self.flush_buffer(&db).await?;
                }
                break;
            }

            // Try to receive with timeout for periodic flush
            let timeout = self
                .config
                .flush_interval
                .saturating_sub(last_flush.elapsed());
            let msg = tokio::time::timeout(timeout, rx.recv()).await;

            match msg {
                Ok(Some(InserterMessage::Insert(event))) => {
                    self.buffer.push(event);

                    // Flush if buffer is full
                    if self.buffer.len() >= self.config.batch_size {
                        self.flush_buffer(&db).await?;
                        last_flush = Instant::now();
                    }
                }
                Ok(Some(InserterMessage::Flush)) => {
                    if !self.buffer.is_empty() {
                        self.flush_buffer(&db).await?;
                        last_flush = Instant::now();
                    }
                }
                Ok(Some(InserterMessage::Shutdown)) | Ok(None) => {
                    // Channel closed or shutdown requested
                    if !self.buffer.is_empty() {
                        self.flush_buffer(&db).await?;
                    }
                    break;
                }
                Err(_) => {
                    // Timeout - periodic flush
                    if !self.buffer.is_empty() {
                        self.flush_buffer(&db).await?;
                        last_flush = Instant::now();
                    }
                }
            }
        }

        Ok(())
    }

    /// Flush the buffer to the database
    async fn flush_buffer(&mut self, db: &Database) -> Result<()> {
        if self.buffer.is_empty() {
            return Ok(());
        }

        let events = std::mem::take(&mut self.buffer);
        let count = events.len();

        match db.batch_insert_events(&events).await {
            Ok(()) => {
                self.events_inserted
                    .fetch_add(count as u64, Ordering::Relaxed);
                self.batches_flushed.fetch_add(1, Ordering::Relaxed);
            }
            Err(e) => {
                eprintln!("⚠️  Failed to batch insert {} events: {}", count, e);
                // Put events back in buffer for retry? For now, just log and continue
                // This prevents blocking the capture loop
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_batch_inserter_handle_creation() {
        let config = BatchInserterConfig::default();
        let stop_signal = Arc::new(AtomicBool::new(false));

        // Note: This test will fail without a ClickHouse connection,
        // but it tests the handle creation logic
        let result = BatchEventInserter::spawn(config, stop_signal.clone());

        // The spawn itself should succeed even without DB
        assert!(result.is_ok());

        let (handle, task) = result.unwrap();
        assert_eq!(handle.events_inserted(), 0);
        assert_eq!(handle.batches_flushed(), 0);
        assert_eq!(handle.events_dropped(), 0);

        // Shutdown gracefully
        stop_signal.store(true, Ordering::SeqCst);
        let _ = task.await;
    }

    #[test]
    fn test_batch_event_creation() {
        let event = BatchEvent {
            session_id: Arc::from("test-session"),
            event_type: "keyboard".to_string(),
            event_subtype: Some("press".to_string()),
            key: Some("A".to_string()),
            button: None,
            x: None,
            y: None,
            pressed: Some(true),
            timestamp: "2024-01-01T00:00:00Z".to_string(),
            timecode: Some(1.5),
            metadata: None,
            screenshot_id: None,
        };

        assert_eq!(&*event.session_id, "test-session");
        assert_eq!(event.event_type, "keyboard");
    }
}
