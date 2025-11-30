//! Webcam video recording service.
//!
//! This service handles video recording from V4L2 devices independently
//! from PTZ controls. Multiple recorders can be active simultaneously,
//! and they can work alongside PTZ controllers.

use crate::error::{RecorderError, Result};
use crate::services::webcam::device::WebcamDevice;
use crate::services::webcam::format::{mjpeg_to_rgb, yuyv_to_rgb};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use v4l::buffer::Type;
use v4l::io::mmap::Stream;
use v4l::io::traits::CaptureStream;
use v4l::video::Capture;
use v4l::FourCC;
use log::{debug, error, info, warn};

/// Configuration for webcam recording.
#[derive(Debug, Clone)]
pub struct WebcamRecordingConfig {
    /// Device path (e.g., "/dev/video0")
    pub device_path: String,
    /// Output file path
    pub output_path: PathBuf,
    /// Frame rate (frames per second)
    pub framerate: u32,
    /// Maximum duration in seconds (None = record until stopped)
    pub max_duration_secs: Option<u64>,
    /// Enable preview window (requires minifb)
    pub enable_preview: bool,
    /// Preview window title
    pub preview_title: Option<String>,
}

impl Default for WebcamRecordingConfig {
    fn default() -> Self {
        Self {
            device_path: "/dev/video0".to_string(),
            output_path: PathBuf::from("output/webcam_recording.mp4"),
            framerate: 30,
            max_duration_secs: None,
            enable_preview: false,
            preview_title: Some("Webcam Recording".to_string()),
        }
    }
}

/// Webcam video recorder service.
///
/// This service records video from a V4L2 device. It can run independently
/// from PTZ controls, allowing concurrent recording and control operations.
pub struct WebcamRecorder {
    device: WebcamDevice,
    config: WebcamRecordingConfig,
    stop_flag: Arc<AtomicBool>,
}

impl WebcamRecorder {
    /// Create a new webcam recorder.
    pub fn new(config: WebcamRecordingConfig) -> Result<Self> {
        let mut device = WebcamDevice::open(&config.device_path)?;
        
        // Get and verify format
        let format = device.device().format()
            .map_err(|e| RecorderError::Other(format!("Failed to get format: {}", e)))?;
        
        let width = format.width as usize;
        let height = format.height as usize;
        let fourcc = format.fourcc;
        
        info!("Webcam recorder initialized: {}x{} {:?}", width, height, fourcc);
        
        // Try to set MJPEG if not already set
        let is_mjpeg = fourcc == FourCC::new(b"MJPG");
        let is_yuyv = fourcc == FourCC::new(b"YUYV");
        
        if !is_mjpeg && !is_yuyv {
            warn!("Unsupported format: {:?}. Trying to set MJPEG...", fourcc);
            let mut new_format = format.clone();
            new_format.fourcc = FourCC::new(b"MJPG");
            if let Err(e) = device.device_mut().set_format(&new_format) {
                return Err(RecorderError::Other(format!("Failed to set format: {}", e)));
            }
            info!("Changed format to MJPEG");
        }

        Ok(Self {
            device,
            config,
            stop_flag: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Start recording in a blocking manner.
    ///
    /// This will record video until `stop()` is called or the maximum duration is reached.
    pub fn record(&mut self) -> Result<()> {
        let format = self.device.device().format()
            .map_err(|e| RecorderError::Other(format!("Failed to get format: {}", e)))?;
        
        let width = format.width as usize;
        let height = format.height as usize;
        let fourcc = format.fourcc;
        let is_mjpeg = fourcc == FourCC::new(b"MJPG");
        
        info!("Starting recording: {}x{} (MJPEG={})", width, height, is_mjpeg);
        info!("Output: {:?}", self.config.output_path);

        // Create output directory if needed
        if let Some(parent) = self.config.output_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| RecorderError::Other(format!("Failed to create output directory: {}", e)))?;
        }

        // Use ffmpeg to handle both capture and encoding
        // We can't read from v4l2 device in two places simultaneously
        let output_path = self.config.output_path.clone();
        let framerate = self.config.framerate;
        let device_path = self.device.path().to_string();
        let start_time = std::time::Instant::now();
        
        info!("Starting ffmpeg to capture and encode webcam video...");
        let mut ffmpeg_process = match Command::new("ffmpeg")
            .arg("-y") // Overwrite output file
            .arg("-f").arg("v4l2")
            .arg("-input_format").arg(if is_mjpeg { "mjpeg" } else { "yuyv422" })
            .arg("-video_size").arg(format!("{}x{}", width, height))
            .arg("-framerate").arg(framerate.to_string())
            .arg("-i").arg(&device_path)
            .arg("-c:v").arg("libx264")
            .arg("-preset").arg("medium")
            .arg("-crf").arg("23")
            .arg("-pix_fmt").arg("yuv420p")
            .arg(&output_path)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(p) => {
                info!("✅ FFmpeg encoder started");
                p
            }
            Err(e) => {
                error!("Failed to start ffmpeg: {}", e);
                return Err(RecorderError::Other(format!(
                    "Failed to start ffmpeg encoder: {}. Make sure ffmpeg is installed.",
                    e
                )));
            }
        };

        // Monitor ffmpeg stderr in a separate thread
        let ffmpeg_stderr = ffmpeg_process.stderr.take();
        let stop_flag_clone = Arc::clone(&self.stop_flag);
        let ffmpeg_monitor = if let Some(stderr) = ffmpeg_stderr {
            Some(thread::spawn(move || {
                use std::io::{BufRead, BufReader};
                let reader = BufReader::new(stderr);
                for line in reader.lines() {
                    if let Ok(line) = line {
                        if line.contains("error") || line.contains("Error") || line.contains("failed") {
                            error!("FFmpeg: {}", line);
                        } else if line.contains("frame=") {
                            debug!("FFmpeg: {}", line);
                        }
                    }
                    if stop_flag_clone.load(Ordering::Relaxed) {
                        break;
                    }
                }
            }))
        } else {
            None
        };

        // Give ffmpeg time to start recording, then reset PTZ controls
        info!("Waiting for recording to start before resetting PTZ...");
        thread::sleep(Duration::from_millis(2000));
        
        // Reset PTZ controls to default after recording has started
        info!("Resetting PTZ controls to 50% positions (center)...");
        if let Err(e) = self.reset_ptz_to_default() {
            warn!("Failed to reset PTZ controls: {}", e);
            // Don't fail recording if PTZ reset fails
        }
        
        // Give device time to settle after PTZ reset
        thread::sleep(Duration::from_millis(1000));

        // Wait for stop signal or duration limit
        while !self.stop_flag.load(Ordering::Relaxed) {
            // Check duration limit
            if let Some(max_duration) = self.config.max_duration_secs {
                if start_time.elapsed().as_secs() >= max_duration {
                    info!("Maximum duration reached, stopping recording");
                    self.stop_flag.store(true, Ordering::Relaxed);
                    break;
                }
            }
            
            thread::sleep(Duration::from_millis(100));
            
            // Check if ffmpeg process has exited unexpectedly
            if let Ok(Some(status)) = ffmpeg_process.try_wait() {
                if !status.success() {
                    error!("FFmpeg process exited unexpectedly with code: {:?}", status.code());
                    return Err(RecorderError::Other(format!(
                        "FFmpeg process exited with error code: {:?}",
                        status.code()
                    )));
                }
            }
        }

        info!("Stopping webcam recording...");
        
        // Send SIGTERM to ffmpeg to gracefully stop
        if let Err(e) = ffmpeg_process.kill() {
            warn!("Failed to send kill signal to ffmpeg: {}", e);
        }
        
        // Wait for ffmpeg to finish
        info!("Waiting for ffmpeg encoder to finish...");
        if let Some(monitor) = ffmpeg_monitor {
            let _ = monitor.join();
        }
        
        match ffmpeg_process.wait() {
            Ok(status) => {
                if status.success() || status.code().is_none() {
                    info!("✅ FFmpeg encoding completed");
                } else {
                    warn!("FFmpeg exited with code: {:?} (may be normal if interrupted)", status.code());
                }
            }
            Err(e) => {
                error!("Failed to wait for ffmpeg: {}", e);
                return Err(RecorderError::Other(format!(
                    "Failed to wait for ffmpeg process: {}",
                    e
                )));
            }
        }
        
        info!("✅ Webcam recording saved to: {:?}", output_path);
        Ok(())
    }

    /// Start recording in a separate thread.
    ///
    /// Returns a handle that can be used to stop the recording.
    pub fn record_async(&mut self) -> Result<WebcamRecorderHandle> {
        let stop_flag = Arc::clone(&self.stop_flag);
        let config = self.config.clone();
        let device_path = self.device.path().to_string();
        
        // We need to clone the device or reopen it in the thread
        // For simplicity, we'll reopen it in the thread
        let handle = thread::spawn(move || {
            let mut config = config;
            config.device_path = device_path;
            let mut recorder = match Self::new(config) {
                Ok(r) => r,
                Err(e) => {
                    error!("Failed to create recorder in thread: {}", e);
                    return;
                }
            };
            recorder.stop_flag = stop_flag;
            if let Err(e) = recorder.record() {
                error!("Recording error: {}", e);
            }
        });

        Ok(WebcamRecorderHandle {
            stop_flag: Arc::clone(&self.stop_flag),
            handle: Some(handle),
        })
    }

    /// Stop the recording.
    pub fn stop(&self) {
        self.stop_flag.store(true, Ordering::Relaxed);
    }

    /// Reset PTZ controls to default positions.
    /// Pan and tilt are set to center, zoom is set to minimum (widest view).
    fn reset_ptz_to_default(&mut self) -> Result<()> {
        use v4l::control::{Control, Value};

        // V4L2 Camera Class Control IDs
        const V4L2_CTRL_CLASS_CAMERA: u32 = 0x009a0000;
        const V4L2_CID_CAMERA_CLASS_BASE: u32 = V4L2_CTRL_CLASS_CAMERA | 0x900;
        const V4L2_CID_PAN_ABSOLUTE: u32 = V4L2_CID_CAMERA_CLASS_BASE + 8;
        const V4L2_CID_TILT_ABSOLUTE: u32 = V4L2_CID_CAMERA_CLASS_BASE + 9;
        const V4L2_CID_ZOOM_ABSOLUTE: u32 = V4L2_CID_CAMERA_CLASS_BASE + 13;

        // Default ranges
        const DEFAULT_PAN_MIN: i64 = -648000;
        const DEFAULT_PAN_MAX: i64 = 648000;
        const DEFAULT_TILT_MIN: i64 = -324000;
        const DEFAULT_TILT_MAX: i64 = 324000;
        const DEFAULT_ZOOM_MIN: i64 = 100;
        const DEFAULT_ZOOM_MAX: i64 = 500;

        let controls: Vec<_> = self.device.device().query_controls().unwrap_or_default();

        // Reset pan to 50% of range (center)
        match self.device.device().control(V4L2_CID_PAN_ABSOLUTE) {
            Ok(_) => {
                let (min, max) = controls.iter()
                    .find(|c| c.id == V4L2_CID_PAN_ABSOLUTE)
                    .map(|c| (c.minimum, c.maximum))
                    .unwrap_or((DEFAULT_PAN_MIN, DEFAULT_PAN_MAX));
                // 50% of the way from min to max (center)
                let value_50pct = min + ((max - min) * 50) / 100;
                if let Err(e) = self.device.device_mut().set_control(Control {
                    id: V4L2_CID_PAN_ABSOLUTE,
                    value: Value::Integer(value_50pct),
                }) {
                    warn!("Failed to reset pan to 50%: {}", e);
                } else {
                    info!("Reset pan to 50% of range (center): {}", value_50pct);
                }
            }
            Err(_) => {
                debug!("Pan control not available, skipping reset");
            }
        }

        // Reset tilt to 50% of range (center)
        match self.device.device().control(V4L2_CID_TILT_ABSOLUTE) {
            Ok(_) => {
                let (min, max) = controls.iter()
                    .find(|c| c.id == V4L2_CID_TILT_ABSOLUTE)
                    .map(|c| (c.minimum, c.maximum))
                    .unwrap_or((DEFAULT_TILT_MIN, DEFAULT_TILT_MAX));
                // 50% of the way from min to max (center)
                let value_25pct = min + ((max - min) * 25) / 100;
                if let Err(e) = self.device.device_mut().set_control(Control {
                    id: V4L2_CID_TILT_ABSOLUTE,
                    value: Value::Integer(value_25pct),
                }) {
                    warn!("Failed to reset tilt to 25%: {}", e);
                } else {
                    info!("Reset tilt to 25% of range (center): {}", value_25pct);
                }
            }
            Err(_) => {
                debug!("Tilt control not available, skipping reset");
            }
        }

        // Reset zoom to 50% of range (mid zoom)
        match self.device.device().control(V4L2_CID_ZOOM_ABSOLUTE) {
            Ok(_) => {
                let (min, max) = controls.iter()
                    .find(|c| c.id == V4L2_CID_ZOOM_ABSOLUTE)
                    .map(|c| (c.minimum, c.maximum))
                    .unwrap_or((DEFAULT_ZOOM_MIN, DEFAULT_ZOOM_MAX));
                // 50% of the way from min to max (mid zoom)
                let value_50pct = min + ((max - min) * 50) / 100;
                if let Err(e) = self.device.device_mut().set_control(Control {
                    id: V4L2_CID_ZOOM_ABSOLUTE,
                    value: Value::Integer(value_50pct),
                }) {
                    warn!("Failed to reset zoom to 50%: {}", e);
                } else {
                    info!("Reset zoom to 50% of range (mid zoom): {}", value_50pct);
                }
            }
            Err(_) => {
                debug!("Zoom control not available, skipping reset");
            }
        }

        Ok(())
    }
}

/// Handle to an async recording session.
pub struct WebcamRecorderHandle {
    stop_flag: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

impl WebcamRecorderHandle {
    /// Stop the recording and wait for the thread to finish.
    pub fn stop(mut self) {
        self.stop_flag.store(true, Ordering::Relaxed);
        // Wait for thread to finish
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for WebcamRecorderHandle {
    fn drop(&mut self) {
        self.stop_flag.store(true, Ordering::Relaxed);
    }
}

