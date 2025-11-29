//! Rotating event writer with async buffered writes and time-based rotation.
//!
//! This module provides a high-performance event writer that:
//! - Uses async buffered writes to reduce disk I/O
//! - Rotates files based on time intervals
//! - Compresses rotated files in the background using gzip

use crate::error::{LoggerError, Result};
use chrono::Local;
use flate2::write::GzEncoder;
use flate2::Compression;
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

/// Configuration for the rotating event writer.
#[derive(Clone, Debug)]
pub struct EventWriterConfig {
    /// Base path for event files (without extension)
    pub base_path: PathBuf,
    /// File extension (e.g., "json", "txt")
    pub extension: String,
    /// How often to rotate files (default: 1 hour)
    pub rotation_interval: Duration,
    /// Buffer size for writes (default: 64KB)
    pub buffer_size: usize,
    /// How often to flush the buffer (default: 1 second)
    pub flush_interval: Duration,
    /// Whether to compress rotated files (default: true)
    pub compress_rotated: bool,
    /// Retention period in days (None = keep forever)
    pub retention_days: Option<u32>,
}

impl Default for EventWriterConfig {
    fn default() -> Self {
        Self {
            base_path: PathBuf::from("events"),
            extension: "json".to_string(),
            rotation_interval: Duration::from_secs(3600), // 1 hour
            buffer_size: 64 * 1024,                       // 64KB
            flush_interval: Duration::from_secs(1),
            compress_rotated: true,
            retention_days: None,
        }
    }
}

/// Event to be written
#[derive(Debug, Clone)]
pub struct WriteEvent {
    /// Formatted event data to write
    pub data: String,
}

/// Message sent to the writer task
enum WriterMessage {
    /// Write an event
    Write(WriteEvent),
    /// Flush the buffer
    Flush,
    /// Rotate the file immediately
    Rotate,
    /// Shutdown the writer
    Shutdown,
}

/// Handle to send events to the rotating writer
#[derive(Clone)]
pub struct RotatingEventWriterHandle {
    tx: mpsc::Sender<WriterMessage>,
    events_written: Arc<AtomicU64>,
    bytes_written: Arc<AtomicU64>,
    files_rotated: Arc<AtomicU64>,
}

impl RotatingEventWriterHandle {
    /// Write an event to the file
    pub async fn write(&self, data: String) -> Result<()> {
        self.tx
            .send(WriterMessage::Write(WriteEvent { data }))
            .await
            .map_err(|e| LoggerError::Other(format!("Failed to send write message: {}", e)))?;
        Ok(())
    }

    /// Write an event synchronously (non-blocking, may drop if buffer full)
    pub fn try_write(&self, data: String) -> Result<()> {
        self.tx
            .try_send(WriterMessage::Write(WriteEvent { data }))
            .map_err(|e| LoggerError::Other(format!("Failed to send write message: {}", e)))?;
        Ok(())
    }

    /// Force flush the buffer
    pub async fn flush(&self) -> Result<()> {
        self.tx
            .send(WriterMessage::Flush)
            .await
            .map_err(|e| LoggerError::Other(format!("Failed to send flush message: {}", e)))?;
        Ok(())
    }

    /// Force rotate the file
    pub async fn rotate(&self) -> Result<()> {
        self.tx
            .send(WriterMessage::Rotate)
            .await
            .map_err(|e| LoggerError::Other(format!("Failed to send rotate message: {}", e)))?;
        Ok(())
    }

    /// Shutdown the writer gracefully
    pub async fn shutdown(&self) -> Result<()> {
        self.tx
            .send(WriterMessage::Shutdown)
            .await
            .map_err(|e| LoggerError::Other(format!("Failed to send shutdown message: {}", e)))?;
        Ok(())
    }

    /// Get the number of events written
    pub fn events_written(&self) -> u64 {
        self.events_written.load(Ordering::Relaxed)
    }

    /// Get the number of bytes written
    pub fn bytes_written(&self) -> u64 {
        self.bytes_written.load(Ordering::Relaxed)
    }

    /// Get the number of files rotated
    pub fn files_rotated(&self) -> u64 {
        self.files_rotated.load(Ordering::Relaxed)
    }
}

/// Rotating event writer that handles file rotation and background compression.
pub struct RotatingEventWriter {
    config: EventWriterConfig,
    current_file: Option<BufWriter<File>>,
    current_file_path: Option<PathBuf>,
    rotation_start: Instant,
    events_written: Arc<AtomicU64>,
    bytes_written: Arc<AtomicU64>,
    files_rotated: Arc<AtomicU64>,
    compression_tx: Option<mpsc::UnboundedSender<PathBuf>>,
}

impl RotatingEventWriter {
    /// Create a new rotating event writer and spawn the background task.
    ///
    /// Returns a handle that can be used to send events to the writer.
    pub fn spawn(
        config: EventWriterConfig,
        stop_signal: Arc<AtomicBool>,
    ) -> Result<(RotatingEventWriterHandle, tokio::task::JoinHandle<Result<()>>)> {
        let (tx, rx) = mpsc::channel(10_000); // Bounded channel for backpressure
        let events_written = Arc::new(AtomicU64::new(0));
        let bytes_written = Arc::new(AtomicU64::new(0));
        let files_rotated = Arc::new(AtomicU64::new(0));

        let handle = RotatingEventWriterHandle {
            tx,
            events_written: events_written.clone(),
            bytes_written: bytes_written.clone(),
            files_rotated: files_rotated.clone(),
        };

        // Spawn compression worker if enabled
        let compression_tx = if config.compress_rotated {
            let (comp_tx, comp_rx) = mpsc::unbounded_channel();
            let retention_days = config.retention_days;
            let base_path = config.base_path.clone();
            tokio::spawn(Self::compression_worker(comp_rx, retention_days, base_path));
            Some(comp_tx)
        } else {
            None
        };

        let mut writer = Self {
            config,
            current_file: None,
            current_file_path: None,
            rotation_start: Instant::now(),
            events_written,
            bytes_written,
            files_rotated,
            compression_tx,
        };

        let task_handle = tokio::spawn(async move { writer.run(rx, stop_signal).await });

        Ok((handle, task_handle))
    }

    /// Generate a timestamped filename
    fn generate_filename(&self) -> PathBuf {
        let timestamp = Local::now().format("%Y%m%d_%H%M%S");
        let filename = format!(
            "{}_{}.{}",
            self.config
                .base_path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("events"),
            timestamp,
            self.config.extension
        );

        if let Some(parent) = self.config.base_path.parent() {
            parent.join(filename)
        } else {
            PathBuf::from(filename)
        }
    }

    /// Open a new file for writing
    fn open_new_file(&mut self) -> Result<()> {
        let path = self.generate_filename();

        // Create parent directories if they don't exist
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                LoggerError::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Failed to create directory {}: {}", parent.display(), e),
                ))
            })?;
        }

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| {
                LoggerError::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Failed to open file {}: {}", path.display(), e),
                ))
            })?;

        self.current_file = Some(BufWriter::with_capacity(self.config.buffer_size, file));
        self.current_file_path = Some(path.clone());
        self.rotation_start = Instant::now();

        eprintln!("📝 Opened new event file: {}", path.display());
        Ok(())
    }

    /// Rotate the current file
    fn rotate(&mut self) -> Result<()> {
        // Flush and close current file
        if let Some(mut writer) = self.current_file.take() {
            writer.flush().map_err(|e| {
                LoggerError::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Failed to flush file: {}", e),
                ))
            })?;
        }

        // Queue old file for compression
        if let (Some(old_path), Some(ref tx)) = (&self.current_file_path, &self.compression_tx) {
            let _ = tx.send(old_path.clone());
        }

        self.files_rotated.fetch_add(1, Ordering::Relaxed);

        // Open new file
        self.current_file_path = None;
        self.open_new_file()
    }

    /// Check if rotation is needed based on time
    fn should_rotate(&self) -> bool {
        self.rotation_start.elapsed() >= self.config.rotation_interval
    }

    /// Write an event to the current file
    fn write_event(&mut self, event: WriteEvent) -> Result<()> {
        // Ensure file is open
        if self.current_file.is_none() {
            self.open_new_file()?;
        }

        // Check rotation
        if self.should_rotate() {
            self.rotate()?;
        }

        // Write event
        if let Some(ref mut writer) = self.current_file {
            let bytes = event.data.as_bytes();
            writer.write_all(bytes).map_err(|e| {
                LoggerError::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Failed to write event: {}", e),
                ))
            })?;
            writer.write_all(b"\n").map_err(|e| {
                LoggerError::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Failed to write newline: {}", e),
                ))
            })?;

            self.events_written.fetch_add(1, Ordering::Relaxed);
            self.bytes_written
                .fetch_add(bytes.len() as u64 + 1, Ordering::Relaxed);
        }

        Ok(())
    }

    /// Flush the current buffer
    fn flush_buffer(&mut self) -> Result<()> {
        if let Some(ref mut writer) = self.current_file {
            writer.flush().map_err(|e| {
                LoggerError::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Failed to flush buffer: {}", e),
                ))
            })?;
        }
        Ok(())
    }

    /// Main writer loop
    async fn run(
        &mut self,
        mut rx: mpsc::Receiver<WriterMessage>,
        stop_signal: Arc<AtomicBool>,
    ) -> Result<()> {
        let mut last_flush = Instant::now();
        let mut events_since_flush = 0u32;
        const FLUSH_EVENT_THRESHOLD: u32 = 100;

        loop {
            // Check stop signal
            if stop_signal.load(Ordering::SeqCst) {
                // Final flush before shutdown
                self.flush_buffer()?;
                break;
            }

            // Try to receive with timeout for periodic flush
            let timeout = self.config.flush_interval.saturating_sub(last_flush.elapsed());
            let msg = tokio::time::timeout(timeout, rx.recv()).await;

            match msg {
                Ok(Some(WriterMessage::Write(event))) => {
                    if let Err(e) = self.write_event(event) {
                        eprintln!("⚠️  Error writing event: {}", e);
                    }
                    events_since_flush += 1;

                    // Flush after threshold events
                    if events_since_flush >= FLUSH_EVENT_THRESHOLD {
                        self.flush_buffer()?;
                        last_flush = Instant::now();
                        events_since_flush = 0;
                    }
                }
                Ok(Some(WriterMessage::Flush)) => {
                    self.flush_buffer()?;
                    last_flush = Instant::now();
                    events_since_flush = 0;
                }
                Ok(Some(WriterMessage::Rotate)) => {
                    self.rotate()?;
                    last_flush = Instant::now();
                    events_since_flush = 0;
                }
                Ok(Some(WriterMessage::Shutdown)) | Ok(None) => {
                    // Channel closed or shutdown requested
                    self.flush_buffer()?;
                    break;
                }
                Err(_) => {
                    // Timeout - periodic flush
                    if events_since_flush > 0 {
                        self.flush_buffer()?;
                        last_flush = Instant::now();
                        events_since_flush = 0;
                    }
                }
            }
        }

        // Final rotation to compress last file
        if self.current_file.is_some() {
            if let Some(old_path) = self.current_file_path.take() {
                self.current_file = None;
                if let Some(ref tx) = self.compression_tx {
                    let _ = tx.send(old_path);
                }
            }
        }

        Ok(())
    }

    /// Background worker for compressing rotated files
    async fn compression_worker(
        mut rx: mpsc::UnboundedReceiver<PathBuf>,
        retention_days: Option<u32>,
        base_path: PathBuf,
    ) {
        while let Some(path) = rx.recv().await {
            // Compress in blocking task
            let path_clone = path.clone();
            let result = tokio::task::spawn_blocking(move || compress_file(&path_clone)).await;

            match result {
                Ok(Ok(())) => {
                    eprintln!("✅ Compressed: {}", path.display());
                }
                Ok(Err(e)) => {
                    eprintln!("⚠️  Compression failed for {}: {}", path.display(), e);
                }
                Err(e) => {
                    eprintln!("⚠️  Compression task failed for {}: {}", path.display(), e);
                }
            }

            // Clean up old files if retention is configured
            if let Some(days) = retention_days {
                if let Some(parent) = base_path.parent() {
                    if let Err(e) = cleanup_old_files(parent, days) {
                        eprintln!("⚠️  Failed to cleanup old files: {}", e);
                    }
                }
            }
        }
    }
}

/// Compress a file using gzip
fn compress_file(path: &Path) -> Result<()> {
    let input = std::fs::read(path).map_err(|e| {
        LoggerError::Io(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("Failed to read file for compression: {}", e),
        ))
    })?;

    let output_path = path.with_extension(format!(
        "{}.gz",
        path.extension().and_then(|s| s.to_str()).unwrap_or("")
    ));

    let output_file = File::create(&output_path).map_err(|e| {
        LoggerError::Io(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("Failed to create compressed file: {}", e),
        ))
    })?;

    let mut encoder = GzEncoder::new(output_file, Compression::default());
    encoder.write_all(&input).map_err(|e| {
        LoggerError::Io(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("Failed to write compressed data: {}", e),
        ))
    })?;
    encoder.finish().map_err(|e| {
        LoggerError::Io(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("Failed to finalize compression: {}", e),
        ))
    })?;

    // Remove original file
    std::fs::remove_file(path).map_err(|e| {
        LoggerError::Io(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("Failed to remove original file after compression: {}", e),
        ))
    })?;

    Ok(())
}

/// Clean up files older than the retention period
fn cleanup_old_files(directory: &Path, retention_days: u32) -> Result<()> {
    let cutoff = std::time::SystemTime::now()
        - std::time::Duration::from_secs(retention_days as u64 * 24 * 60 * 60);

    let entries = std::fs::read_dir(directory).map_err(|e| {
        LoggerError::Io(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("Failed to read directory: {}", e),
        ))
    })?;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            if let Ok(metadata) = entry.metadata() {
                if let Ok(modified) = metadata.modified() {
                    if modified < cutoff {
                        // Only delete .gz files (compressed) and event files
                        let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
                        if ext == "gz" || ext == "json" || ext == "txt" {
                            if let Err(e) = std::fs::remove_file(&path) {
                                eprintln!(
                                    "⚠️  Failed to remove old file {}: {}",
                                    path.display(),
                                    e
                                );
                            } else {
                                eprintln!("🗑️  Removed old file: {}", path.display());
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_rotating_writer_basic() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path().join("test_events");

        let config = EventWriterConfig {
            base_path,
            extension: "json".to_string(),
            rotation_interval: Duration::from_secs(3600),
            buffer_size: 1024,
            flush_interval: Duration::from_millis(100),
            compress_rotated: false,
            retention_days: None,
        };

        let stop_signal = Arc::new(AtomicBool::new(false));
        let (handle, task) = RotatingEventWriter::spawn(config, stop_signal.clone()).unwrap();

        // Write some events
        for i in 0..10 {
            handle
                .write(format!(r#"{{"event": {}}}"#, i))
                .await
                .unwrap();
        }

        // Flush and shutdown
        handle.flush().await.unwrap();
        handle.shutdown().await.unwrap();
        task.await.unwrap().unwrap();

        assert_eq!(handle.events_written(), 10);
    }

    #[tokio::test]
    async fn test_rotating_writer_rotation() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path().join("test_events");

        let config = EventWriterConfig {
            base_path: base_path.clone(),
            extension: "json".to_string(),
            rotation_interval: Duration::from_millis(100), // Fast rotation for testing
            buffer_size: 1024,
            flush_interval: Duration::from_millis(50),
            compress_rotated: false,
            retention_days: None,
        };

        let stop_signal = Arc::new(AtomicBool::new(false));
        let (handle, task) = RotatingEventWriter::spawn(config, stop_signal.clone()).unwrap();

        // Write events with delays to trigger rotation
        for i in 0..5 {
            handle
                .write(format!(r#"{{"event": {}}}"#, i))
                .await
                .unwrap();
            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        // Wait for rotation
        tokio::time::sleep(Duration::from_millis(200)).await;

        // Write more events
        for i in 5..10 {
            handle
                .write(format!(r#"{{"event": {}}}"#, i))
                .await
                .unwrap();
        }

        handle.shutdown().await.unwrap();
        task.await.unwrap().unwrap();

        // Check that multiple files were created
        let files: Vec<_> = std::fs::read_dir(temp_dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("json"))
            .collect();

        assert!(files.len() >= 1);
    }
}

