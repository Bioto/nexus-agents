//! Webcam video recording service.
//!
//! This service handles video recording from V4L2 devices independently
//! from PTZ controls. Multiple recorders can be active simultaneously,
//! and they can work alongside PTZ controllers.

use crate::error::{RecorderError, Result};
use crate::services::webcam::device::WebcamDevice;
use log::{debug, error, info, warn};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use v4l::video::Capture;
use v4l::FourCC;

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
    /// Skip PTZ reset on startup (preserves AI tracking mode on smart cameras)
    pub skip_ptz_reset: bool,
    /// Reset PTZ then reconnect to reinitialize AI tracking
    pub ai_reinit: bool,
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
            skip_ptz_reset: true, // Default true to preserve AI tracking mode on smart cameras
            ai_reinit: false,
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
        // If AI reinit is requested, we need to reset PTZ, disconnect, then reconnect
        let device = if config.ai_reinit {
            info!("🤖 AI reinit requested: resetting PTZ and reconnecting...");

            // First, open device and reset PTZ
            let mut temp_device = WebcamDevice::open(&config.device_path)?;
            Self::reset_ptz_on_device(&mut temp_device)?;

            // Drop the device to close the connection
            info!("🔌 Disconnecting from camera...");
            drop(temp_device);

            // Wait for camera to process the disconnect
            info!("⏳ Waiting for camera AI to reinitialize...");
            thread::sleep(Duration::from_millis(2000));

            // Reconnect - camera's AI should now reinitialize
            info!("🔌 Reconnecting to camera...");
            WebcamDevice::open(&config.device_path)?
        } else {
            WebcamDevice::open(&config.device_path)?
        };

        let mut device = device;

        // Get and verify format
        let format = device
            .device()
            .format()
            .map_err(|e| RecorderError::Other(format!("Failed to get format: {}", e)))?;

        let width = format.width as usize;
        let height = format.height as usize;
        let fourcc = format.fourcc;

        info!(
            "Webcam recorder initialized: {}x{} {:?}",
            width, height, fourcc
        );

        // Check for supported formats:
        // - MJPG: Compressed MJPEG (most webcams)
        // - YUYV: Packed YUV 4:2:2 (some webcams)
        // - YU12/I420: Planar YUV 4:2:0 (v4l2loopback devices)
        // - NV12: Semi-planar YUV 4:2:0
        let is_mjpeg = fourcc == FourCC::new(b"MJPG");
        let is_yuyv = fourcc == FourCC::new(b"YUYV");
        let is_yuv420p = fourcc == FourCC::new(b"YU12")
            || fourcc == FourCC::new(b"I420")
            || fourcc == FourCC::new(b"NV12");

        let is_supported = is_mjpeg || is_yuyv || is_yuv420p;

        if !is_supported {
            warn!("Unsupported format: {:?}. Trying to set MJPEG...", fourcc);
            let mut new_format = format.clone();
            new_format.fourcc = FourCC::new(b"MJPG");
            if let Err(e) = device.device_mut().set_format(&new_format) {
                // v4l2loopback devices may not support format changes - that's OK
                // They output whatever format the writer provides
                warn!(
                    "Could not change format to MJPEG: {}. Will try rawvideo.",
                    e
                );
            } else {
                info!("Changed format to MJPEG");
            }
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
        let format = self
            .device
            .device()
            .format()
            .map_err(|e| RecorderError::Other(format!("Failed to get format: {}", e)))?;

        let width = format.width as usize;
        let height = format.height as usize;
        let fourcc = format.fourcc;

        // Determine the FFmpeg input format based on device fourcc
        let (input_format, format_name) = if fourcc == FourCC::new(b"MJPG") {
            ("mjpeg", "MJPEG")
        } else if fourcc == FourCC::new(b"YUYV") {
            ("yuyv422", "YUYV")
        } else if fourcc == FourCC::new(b"YU12") || fourcc == FourCC::new(b"I420") {
            ("yuv420p", "YUV420P")
        } else if fourcc == FourCC::new(b"NV12") {
            ("nv12", "NV12")
        } else {
            // For v4l2loopback or unknown formats, try rawvideo with yuv420p
            // This is the most common format for virtual cameras
            warn!("Unknown fourcc {:?}, assuming rawvideo yuv420p", fourcc);
            ("rawvideo", "rawvideo")
        };

        info!(
            "Starting recording: {}x{} (format={})",
            width, height, format_name
        );
        info!("Output: {:?}", self.config.output_path);

        // Create output directory if needed
        if let Some(parent) = self.config.output_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                RecorderError::Other(format!("Failed to create output directory: {}", e))
            })?;
        }

        // Use ffmpeg to handle both capture and encoding
        // We can't read from v4l2 device in two places simultaneously
        let output_path = self.config.output_path.clone();
        let framerate = self.config.framerate;
        let device_path = self.device.path().to_string();
        let start_time = std::time::Instant::now();

        // Determine output format based on extension
        // Use MPEG-TS for live analysis (streaming format, can be read while writing)
        let output_ext = output_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("mp4");
        let use_ts_for_live = output_ext == "mp4"; // We'll transcode to MP4 at the end if needed

        // For live webcam analysis, we use MPEG-TS format (designed for streaming)
        // TS can be read while being written, unlike MP4/MKV
        let live_output_path = if use_ts_for_live {
            output_path.with_extension("ts")
        } else {
            output_path.clone()
        };

        info!("Starting ffmpeg to capture and encode webcam video...");
        info!(
            "Live recording to: {:?} (will convert to {:?} when done)",
            live_output_path, output_path
        );

        // Build FFmpeg command with appropriate input format
        // For rawvideo (v4l2loopback), we need to specify pixel format explicitly
        let mut cmd = Command::new("ffmpeg");
        cmd.arg("-y"); // Overwrite output file
        cmd.arg("-f").arg("v4l2");

        if input_format == "rawvideo" {
            // For v4l2loopback devices outputting raw frames
            cmd.arg("-pix_fmt").arg("yuv420p");
        } else {
            cmd.arg("-input_format").arg(input_format);
        }

        let mut ffmpeg_process = match cmd
            .arg("-video_size")
            .arg(format!("{}x{}", width, height))
            .arg("-framerate")
            .arg(framerate.to_string())
            .arg("-i")
            .arg(&device_path)
            .arg("-c:v")
            .arg("libx264")
            .arg("-preset")
            .arg("ultrafast") // Faster encoding = quicker data availability
            .arg("-tune")
            .arg("zerolatency") // Minimize latency for live streaming
            .arg("-crf")
            .arg("23")
            .arg("-pix_fmt")
            .arg("yuv420p")
            .arg("-g")
            .arg("30") // Keyframe every 30 frames (1 second at 30fps)
            .arg("-flush_packets")
            .arg("1") // Flush packets immediately to disk
            .arg("-fflags")
            .arg("+flush_packets+genpts") // Additional flush flags
            .arg(&live_output_path)
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
                        if line.contains("error")
                            || line.contains("Error")
                            || line.contains("failed")
                        {
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

        // Reset PTZ controls to default after recording has started (unless skipped)
        if !self.config.skip_ptz_reset {
            info!("Resetting PTZ controls to 50% positions (center)...");
            if let Err(e) = self.reset_ptz_to_default() {
                warn!("Failed to reset PTZ controls: {}", e);
                // Don't fail recording if PTZ reset fails
            }
        } else {
            info!("Skipping PTZ reset (preserving AI tracking mode)");
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
            // Note: Exit code 255 is normal when FFmpeg is interrupted (Ctrl+C/SIGINT)
            if let Ok(Some(status)) = ffmpeg_process.try_wait() {
                if !status.success() {
                    let exit_code = status.code();
                    // Exit code 255 means FFmpeg was interrupted by a signal (Ctrl+C)
                    // This is expected and not an error - FFmpeg receives SIGINT before our stop_flag is set
                    if exit_code == Some(255) || self.stop_flag.load(Ordering::Relaxed) {
                        info!(
                            "FFmpeg exited with code {:?} (interrupted, this is normal)",
                            exit_code
                        );
                        break; // Exit the loop gracefully
                    } else {
                        error!(
                            "FFmpeg process exited unexpectedly with code: {:?}",
                            exit_code
                        );
                        return Err(RecorderError::Other(format!(
                            "FFmpeg process exited with error code: {:?}",
                            exit_code
                        )));
                    }
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
                    // Exit code 255 is normal when FFmpeg is interrupted (Ctrl+C)
                    // Other non-zero codes might indicate an error, but if we're stopping, it's likely fine
                    let exit_code = status.code();
                    if self.stop_flag.load(Ordering::Relaxed) {
                        info!("✅ FFmpeg stopped (exit code: {:?})", exit_code);
                    } else {
                        warn!(
                            "FFmpeg exited with code: {:?} (may indicate an error)",
                            exit_code
                        );
                    }
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

        // If we recorded to TS, convert to MP4
        if use_ts_for_live && live_output_path != output_path {
            info!("Converting TS to MP4...");
            let convert_result = Command::new("ffmpeg")
                .arg("-y")
                .arg("-i")
                .arg(&live_output_path)
                .arg("-c")
                .arg("copy")
                .arg(&output_path)
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .output();

            match convert_result {
                Ok(output) if output.status.success() => {
                    info!("✅ Converted to MP4: {:?}", output_path);
                    // Remove the temporary TS file
                    if let Err(e) = std::fs::remove_file(&live_output_path) {
                        warn!("Failed to remove temporary TS file: {}", e);
                    }
                }
                Ok(output) => {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    warn!("TS to MP4 conversion failed: {}", stderr);
                    info!("Keeping TS file: {:?}", live_output_path);
                }
                Err(e) => {
                    warn!("Failed to run conversion: {}", e);
                    info!("Keeping TS file: {:?}", live_output_path);
                }
            }
        }

        info!("✅ Webcam recording saved to: {:?}", output_path);
        Ok(())
    }

    /// Record directly using FFmpeg without v4l2 crate format detection.
    ///
    /// This is useful for v4l2loopback virtual cameras where the v4l2 crate
    /// may fail to query format information. FFmpeg handles these devices better.
    pub fn record_direct(config: &WebcamRecordingConfig, stop_flag: Arc<AtomicBool>) -> Result<()> {
        let device_path = &config.device_path;
        let output_path = &config.output_path;
        let framerate = config.framerate;

        info!(
            "📹 Direct FFmpeg recording from {} to {:?}",
            device_path, output_path
        );

        // Create output directory if needed
        if let Some(parent) = output_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                RecorderError::Other(format!("Failed to create output directory: {}", e))
            })?;
        }

        // Use MPEG-TS for live analysis (can be read while writing)
        let output_ext = output_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("mp4");
        let use_ts_for_live = output_ext == "mp4";

        let live_output_path = if use_ts_for_live {
            output_path.with_extension("ts")
        } else {
            output_path.clone()
        };

        info!(
            "Live recording to: {:?} (will convert to {:?} when done)",
            live_output_path, output_path
        );

        // For v4l2loopback devices, we need to explicitly specify the input format
        // because the device doesn't properly advertise its format until data is flowing.
        // The splitter outputs raw YUV420P frames at the camera's native resolution.
        // We use rawvideo format with explicit pixel format for v4l2loopback compatibility.
        // Detect v4l2loopback: typically /dev/video10, /dev/video11, etc. (video1X where X is a digit)
        let is_loopback = device_path.starts_with("/dev/video1")
            && device_path.len() > "/dev/video1".len()
            && device_path
                .chars()
                .last()
                .map(|c| c.is_ascii_digit())
                .unwrap_or(false);

        info!(
            "📹 Device {} detected as v4l2loopback: {}",
            device_path, is_loopback
        );

        // Build FFmpeg command
        // For v4l2loopback: we need to specify the input format because the device
        // doesn't properly advertise its capabilities until data is flowing
        let mut cmd = Command::new("ffmpeg");
        cmd.arg("-y"); // Overwrite output
        cmd.arg("-f").arg("v4l2"); // Input format is v4l2 for both real and loopback devices

        if is_loopback {
            // v4l2loopback: specify the pixel format the splitter is writing
            // The splitter outputs YUV420P frames
            cmd.arg("-input_format")
                .arg("yuv420p")
                .arg("-video_size")
                .arg("1280x720"); // Match splitter output (TODO: make configurable)
        }

        cmd.arg("-framerate")
            .arg(framerate.to_string())
            .arg("-i")
            .arg(device_path);

        // Output encoding settings
        let mut ffmpeg_process = cmd
            .arg("-c:v")
            .arg("libx264")
            .arg("-preset")
            .arg("ultrafast")
            .arg("-tune")
            .arg("zerolatency")
            .arg("-crf")
            .arg("23")
            .arg("-pix_fmt")
            .arg("yuv420p")
            .arg("-g")
            .arg("30")
            .arg("-flush_packets")
            .arg("1")
            .arg("-fflags")
            .arg("+flush_packets+genpts")
            .arg(&live_output_path)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                RecorderError::Other(format!(
                    "Failed to start ffmpeg: {}. Make sure ffmpeg is installed.",
                    e
                ))
            })?;

        info!("✅ FFmpeg direct recording started");

        // Monitor stderr in background
        let ffmpeg_stderr = ffmpeg_process.stderr.take();
        let stop_flag_clone = Arc::clone(&stop_flag);
        let _monitor_handle = if let Some(stderr) = ffmpeg_stderr {
            Some(thread::spawn(move || {
                use std::io::{BufRead, BufReader};
                let reader = BufReader::new(stderr);
                for line in reader.lines() {
                    if stop_flag_clone.load(Ordering::Relaxed) {
                        break;
                    }
                    if let Ok(line) = line {
                        if line.contains("error")
                            || line.contains("Error")
                            || line.contains("failed")
                        {
                            error!("FFmpeg: {}", line);
                        } else if line.contains("frame=") {
                            debug!("FFmpeg: {}", line);
                        }
                    }
                }
            }))
        } else {
            None
        };

        // Wait for stop signal or process exit
        while !stop_flag.load(Ordering::Relaxed) {
            thread::sleep(Duration::from_millis(100));

            if let Ok(Some(status)) = ffmpeg_process.try_wait() {
                if !status.success() && !stop_flag.load(Ordering::Relaxed) {
                    return Err(RecorderError::Other(format!(
                        "FFmpeg exited unexpectedly with code: {:?}",
                        status.code()
                    )));
                }
                break;
            }
        }

        // Stop ffmpeg gracefully
        info!("Stopping FFmpeg recording...");
        let _ = ffmpeg_process.kill();
        let _ = ffmpeg_process.wait();

        // Convert TS to MP4 if needed
        if use_ts_for_live && live_output_path != *output_path {
            info!("Converting TS to MP4...");
            let convert_result = Command::new("ffmpeg")
                .arg("-y")
                .arg("-i")
                .arg(&live_output_path)
                .arg("-c")
                .arg("copy")
                .arg(output_path)
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .output();

            match convert_result {
                Ok(output) if output.status.success() => {
                    info!("✅ Converted to MP4: {:?}", output_path);
                    if let Err(e) = std::fs::remove_file(&live_output_path) {
                        warn!("Failed to remove temporary TS file: {}", e);
                    }
                }
                Ok(output) => {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    warn!("TS to MP4 conversion failed: {}", stderr);
                }
                Err(e) => {
                    warn!("Failed to run conversion: {}", e);
                }
            }
        }

        info!("✅ Direct webcam recording complete: {:?}", output_path);
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

    /// Reset PTZ controls on a device (static helper for AI reinit).
    /// Centers pan/tilt and sets zoom to minimum.
    fn reset_ptz_on_device(device: &mut WebcamDevice) -> Result<()> {
        use v4l::control::{Control, Value};

        // V4L2 Camera Class Control IDs
        const V4L2_CTRL_CLASS_CAMERA: u32 = 0x009a0000;
        const V4L2_CID_CAMERA_CLASS_BASE: u32 = V4L2_CTRL_CLASS_CAMERA | 0x900;
        const V4L2_CID_PAN_ABSOLUTE: u32 = V4L2_CID_CAMERA_CLASS_BASE + 8;
        const V4L2_CID_TILT_ABSOLUTE: u32 = V4L2_CID_CAMERA_CLASS_BASE + 9;
        const V4L2_CID_ZOOM_ABSOLUTE: u32 = V4L2_CID_CAMERA_CLASS_BASE + 13;

        let controls: Vec<_> = device.device().query_controls().unwrap_or_default();

        // Reset pan to center (0)
        if device.device().control(V4L2_CID_PAN_ABSOLUTE).is_ok() {
            let (min, max) = controls
                .iter()
                .find(|c| c.id == V4L2_CID_PAN_ABSOLUTE)
                .map(|c| (c.minimum, c.maximum))
                .unwrap_or((-648000, 648000));
            let center = min + (max - min) / 2;
            let _ = device.device_mut().set_control(Control {
                id: V4L2_CID_PAN_ABSOLUTE,
                value: Value::Integer(center),
            });
            info!("Reset pan to center: {}", center);
        }

        // Reset tilt to center
        if device.device().control(V4L2_CID_TILT_ABSOLUTE).is_ok() {
            let (min, max) = controls
                .iter()
                .find(|c| c.id == V4L2_CID_TILT_ABSOLUTE)
                .map(|c| (c.minimum, c.maximum))
                .unwrap_or((-324000, 324000));
            let center = min + (max - min) / 2;
            let _ = device.device_mut().set_control(Control {
                id: V4L2_CID_TILT_ABSOLUTE,
                value: Value::Integer(center),
            });
            info!("Reset tilt to center: {}", center);
        }

        // Reset zoom to minimum (widest view)
        if device.device().control(V4L2_CID_ZOOM_ABSOLUTE).is_ok() {
            let min = controls
                .iter()
                .find(|c| c.id == V4L2_CID_ZOOM_ABSOLUTE)
                .map(|c| c.minimum)
                .unwrap_or(100);
            let _ = device.device_mut().set_control(Control {
                id: V4L2_CID_ZOOM_ABSOLUTE,
                value: Value::Integer(min),
            });
            info!("Reset zoom to minimum: {}", min);
        }

        Ok(())
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
                let (min, max) = controls
                    .iter()
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
                let (min, max) = controls
                    .iter()
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
                let (min, max) = controls
                    .iter()
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
