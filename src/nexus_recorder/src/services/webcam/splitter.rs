//! Webcam splitter service for virtual camera output.
//!
//! This service captures frames from a physical V4L2 device and writes them
//! to multiple v4l2loopback virtual devices, allowing multiple applications
//! to access the same camera feed simultaneously.
//!
//! # Prerequisites
//!
//! Before using this module, you must:
//! 1. Install v4l2loopback: `sudo apt install v4l2loopback-dkms v4l2loopback-utils`
//! 2. Load the kernel module: `sudo modprobe v4l2loopback devices=2 video_nr=10,11`
//!
//! **Important**: Do NOT use `exclusive_caps=1` when loading v4l2loopback, as it causes
//! format negotiation issues with FFmpeg. If you're getting "Invalid argument" errors,
//! reload the module without exclusive_caps:
//! ```bash
//! sudo modprobe -r v4l2loopback
//! sudo modprobe v4l2loopback devices=2 video_nr=10,11
//! ```

use crate::error::{RecorderError, Result};
use log::{debug, error, info, warn};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

/// Configuration for the webcam splitter.
#[derive(Debug, Clone)]
pub struct SplitterConfig {
    /// Input device path (the real camera, e.g., "/dev/video0")
    pub input_device: String,
    /// Output device paths (v4l2loopback virtual devices)
    pub output_devices: Vec<String>,
    /// Frame rate (frames per second)
    pub framerate: u32,
    /// Input format (auto-detected if None)
    pub input_format: Option<String>,
    /// Video width (auto-detected if None)
    pub width: Option<u32>,
    /// Video height (auto-detected if None)
    pub height: Option<u32>,
}

impl Default for SplitterConfig {
    fn default() -> Self {
        Self {
            input_device: "/dev/video0".to_string(),
            output_devices: vec!["/dev/video10".to_string(), "/dev/video11".to_string()],
            framerate: 30,
            input_format: None,
            width: None,
            height: None,
        }
    }
}

/// Webcam splitter service.
///
/// Captures video from a physical camera and writes to multiple virtual cameras,
/// allowing multiple applications to access the same camera feed.
pub struct WebcamSplitter {
    config: SplitterConfig,
    stop_flag: Arc<AtomicBool>,
}

impl WebcamSplitter {
    /// Create a new webcam splitter.
    #[allow(clippy::missing_errors_doc)]
    pub fn new(config: SplitterConfig) -> Result<Self> {
        // Validate input device exists
        if !Path::new(&config.input_device).exists() {
            return Err(RecorderError::Other(format!(
                "Input device {} does not exist",
                config.input_device
            )));
        }

        // Validate we have at least one output device
        if config.output_devices.is_empty() {
            return Err(RecorderError::Other(
                "At least one output device is required".to_string(),
            ));
        }

        // Warn about missing output devices (they may be created later)
        for device in &config.output_devices {
            if !Path::new(device).exists() {
                warn!(
                    "Output device {} does not exist. Make sure v4l2loopback is loaded.",
                    device
                );
            }
        }

        info!(
            "WebcamSplitter initialized: {} -> {:?}",
            config.input_device, config.output_devices
        );

        Ok(Self {
            config,
            stop_flag: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Check if v4l2loopback devices are available.
    #[allow(clippy::missing_errors_doc)]
    pub fn check_loopback_devices(&self) -> Result<()> {
        for device in &self.config.output_devices {
            if !Path::new(device).exists() {
                let video_nrs = self.config.output_devices.iter()
                    .filter_map(|d| d.strip_prefix("/dev/video"))
                    .collect::<Vec<_>>()
                    .join(",");
                return Err(RecorderError::Other(format!(
                    "Virtual camera {} not found.\n\
                    Load v4l2loopback WITHOUT exclusive_caps:\n\
                    sudo modprobe v4l2loopback devices={} video_nr={}\n\
                    \n\
                    If already loaded with exclusive_caps=1, reload it:\n\
                    sudo modprobe -r v4l2loopback && sudo modprobe v4l2loopback devices={} video_nr={}",
                    device,
                    self.config.output_devices.len(),
                    video_nrs,
                    self.config.output_devices.len(),
                    video_nrs
                )));
            }
        }
        Ok(())
    }

    /// Query the input device format using v4l2-ctl.
    fn query_input_format(&self) -> Result<(u32, u32, String)> {
        let output = Command::new("v4l2-ctl")
            .args(["--device", &self.config.input_device, "--get-fmt-video"])
            .output()
            .map_err(|e| RecorderError::Other(format!("Failed to run v4l2-ctl: {}", e)))?;

        let stdout = String::from_utf8_lossy(&output.stdout);

        let mut width = 1920u32;
        let mut height = 1080u32;
        let mut fourcc = "MJPG".to_string();

        for line in stdout.lines() {
            let line = line.trim();
            if line.starts_with("Width/Height") {
                if let Some(dims) = line.split(':').nth(1) {
                    let parts: Vec<&str> = dims.trim().split('/').collect();
                    if parts.len() == 2 {
                        width = parts[0].parse().unwrap_or(1920);
                        height = parts[1].parse().unwrap_or(1080);
                    }
                }
            } else if line.starts_with("Pixel Format") {
                if let Some(fmt) = line.split('\'').nth(1) {
                    fourcc = fmt.to_string();
                }
            }
        }

        info!("Input format: {}x{} {}", width, height, fourcc);
        Ok((width, height, fourcc))
    }

    /// Build the ffmpeg command for splitting video to multiple outputs.
    fn build_ffmpeg_command(&self) -> Result<Command> {
        let (width, height, fourcc) =
            if let (Some(w), Some(h)) = (self.config.width, self.config.height) {
                let fmt = self.config.input_format.clone().unwrap_or_else(|| "mjpeg".to_string());
                (w, h, fmt)
            } else {
                self.query_input_format()?
            };

        let input_format = match fourcc.to_uppercase().as_str() {
            "MJPG" | "MJPEG" => "mjpeg",
            "YUYV" | "YUY2" => "yuyv422",
            "NV12" => "nv12",
            _ => "mjpeg",
        };

        let mut cmd = Command::new("ffmpeg");

        // Input configuration
        cmd.arg("-f").arg("v4l2")
            .arg("-input_format").arg(input_format)
            .arg("-video_size").arg(format!("{}x{}", width, height))
            .arg("-framerate").arg(self.config.framerate.to_string())
            .arg("-i").arg(&self.config.input_device);

        // For v4l2loopback, we need to decode once and then output to each device
        // Using format filter to convert to YUV420P which v4l2loopback handles better
        let num_outputs = self.config.output_devices.len();
        if num_outputs > 1 {
            // Build split filter with proper format conversion
            let output_labels: Vec<String> = (0..num_outputs)
                .map(|i| format!("[out{}]", i))
                .collect();
            let filter_complex = format!(
                "[0:v]format=yuv420p,split={}{}",
                num_outputs,
                output_labels.join("")
            );
            cmd.arg("-filter_complex").arg(filter_complex);

            // Map each output label to a device with rawvideo codec
            for (i, device) in self.config.output_devices.iter().enumerate() {
                cmd.arg("-map").arg(format!("[out{}]", i))
                    .arg("-f").arg("v4l2")
                    .arg("-pix_fmt").arg("yuv420p")
                    .arg(device);
            }
        } else {
            // Single output with format conversion
            cmd.arg("-vf").arg("format=yuv420p")
                .arg("-f").arg("v4l2")
                .arg("-pix_fmt").arg("yuv420p")
                .arg(&self.config.output_devices[0]);
        }

        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        Ok(cmd)
    }

    /// Start the splitter in the current thread (blocking).
    #[allow(clippy::missing_errors_doc)]
    pub fn run(&mut self) -> Result<()> {
        self.check_loopback_devices()?;

        info!("Starting webcam splitter...");
        info!("  Input: {}", self.config.input_device);
        info!("  Outputs: {:?}", self.config.output_devices);

        let mut cmd = self.build_ffmpeg_command()?;

        let mut process = cmd.spawn().map_err(|e| {
            RecorderError::Other(format!(
                "Failed to start ffmpeg: {}. Make sure ffmpeg is installed.",
                e
            ))
        })?;

        info!("Webcam splitter started");

        let stderr = process.stderr.take();
        let stop_flag_clone = Arc::clone(&self.stop_flag);

        let monitor_handle = if let Some(stderr) = stderr {
            Some(thread::spawn(move || {
                use std::io::{BufRead, BufReader};
                let reader = BufReader::new(stderr);
                for line in reader.lines() {
                    if stop_flag_clone.load(Ordering::Relaxed) {
                        break;
                    }
                    if let Ok(line) = line {
                        if line.contains("error") || line.contains("Error") {
                            error!("FFmpeg: {}", line);
                        } else {
                            debug!("FFmpeg: {}", line);
                        }
                    }
                }
            }))
        } else {
            None
        };

        while !self.stop_flag.load(Ordering::Relaxed) {
            thread::sleep(Duration::from_millis(100));

            if let Ok(Some(status)) = process.try_wait() {
                if !status.success() && !self.stop_flag.load(Ordering::Relaxed) {
                    error!("FFmpeg exited unexpectedly with code: {:?}", status.code());
                    return Err(RecorderError::Other(format!(
                        "FFmpeg exited with error: {:?}",
                        status.code()
                    )));
                }
                break;
            }
        }

        info!("Stopping webcam splitter...");
        let _ = process.kill();
        let _ = process.wait();

        if let Some(handle) = monitor_handle {
            let _ = handle.join();
        }

        info!("Webcam splitter stopped");
        Ok(())
    }

    /// Start the splitter in a background thread.
    #[allow(clippy::missing_errors_doc)]
    pub fn start(&mut self) -> Result<SplitterHandle> {
        self.check_loopback_devices()?;

        let stop_flag = Arc::clone(&self.stop_flag);
        let config = self.config.clone();

        let handle = thread::spawn(move || {
            let mut splitter = match WebcamSplitter::new(config) {
                Ok(s) => s,
                Err(e) => {
                    error!("Failed to create splitter in thread: {}", e);
                    return;
                }
            };
            splitter.stop_flag = stop_flag;
            if let Err(e) = splitter.run() {
                error!("Splitter error: {}", e);
            }
        });

        Ok(SplitterHandle {
            stop_flag: Arc::clone(&self.stop_flag),
            handle: Some(handle),
        })
    }

    /// Signal the splitter to stop.
    pub fn stop(&self) {
        self.stop_flag.store(true, Ordering::Relaxed);
    }
}

/// Handle to a running splitter instance.
pub struct SplitterHandle {
    stop_flag: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl SplitterHandle {
    /// Stop the splitter and wait for it to finish.
    pub fn stop(mut self) {
        self.stop_flag.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }

    /// Check if the splitter is still running.
    #[must_use]
    pub fn is_running(&self) -> bool {
        self.handle.as_ref().is_some_and(|h| !h.is_finished())
    }
}

impl Drop for SplitterHandle {
    fn drop(&mut self) {
        self.stop_flag.store(true, Ordering::Relaxed);
    }
}

/// Helper function to create v4l2loopback devices.
///
/// Requires root privileges.
#[allow(clippy::missing_errors_doc, clippy::missing_panics_doc)]
pub fn create_loopback_devices(
    device_count: usize,
    start_device_nr: u32,
    labels: Option<Vec<String>>,
) -> Result<Vec<String>> {
    let video_nrs: Vec<String> = (0..device_count)
        .map(|i| (start_device_nr + u32::try_from(i).unwrap()).to_string())
        .collect();

    let labels = labels.unwrap_or_else(|| {
        (0..device_count)
            .map(|i| format!("VirtualCam{}", i + 1))
            .collect()
    });

    let video_nr_arg = video_nrs.join(",");
    let labels_arg = labels.join(",");

    info!("Creating v4l2loopback devices: video_nr={}", video_nr_arg);

    let output = Command::new("sudo")
        .args([
            "modprobe",
            "v4l2loopback",
            &format!("devices={}", device_count),
            &format!("video_nr={}", video_nr_arg),
            &format!("card_label={}", labels_arg),
            // Note: NOT using exclusive_caps=1 because it causes format negotiation issues
            // with FFmpeg writing to the devices. Without exclusive_caps, the devices
            // accept whatever format the writer provides.
        ])
        .output()
        .map_err(|e| RecorderError::Other(format!("Failed to run modprobe: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(RecorderError::Other(format!(
            "Failed to create loopback devices: {}",
            stderr
        )));
    }

    let devices: Vec<String> = video_nrs
        .iter()
        .map(|nr| format!("/dev/video{}", nr))
        .collect();

    thread::sleep(Duration::from_millis(500));

    for device in &devices {
        if !Path::new(device).exists() {
            return Err(RecorderError::Other(format!(
                "Device {} was not created",
                device
            )));
        }
    }

    info!("Created loopback devices: {:?}", devices);
    Ok(devices)
}

/// Remove v4l2loopback module. Requires root privileges.
#[allow(clippy::missing_errors_doc)]
pub fn remove_loopback_devices() -> Result<()> {
    let output = Command::new("sudo")
        .args(["modprobe", "-r", "v4l2loopback"])
        .output()
        .map_err(|e| RecorderError::Other(format!("Failed to run modprobe: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(RecorderError::Other(format!(
            "Failed to remove loopback module: {}",
            stderr
        )));
    }

    info!("Removed v4l2loopback module");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_splitter_config_default() {
        let config = SplitterConfig::default();
        assert_eq!(config.input_device, "/dev/video0");
        assert_eq!(config.output_devices.len(), 2);
        assert_eq!(config.framerate, 30);
    }
}
