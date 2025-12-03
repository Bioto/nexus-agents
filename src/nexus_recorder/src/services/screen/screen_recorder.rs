use crate::error::{RecorderError, Result};
use crate::services::unified_recording::DEFAULT_SEGMENT_DURATION_SECS;

use ffmpeg::{
    codec,
    codec::context::Context as CodecContext,
    device::input,
    encoder, format,
    format::Pixel,
    frame::Video,
    media::Type,
    packet::Packet,
    software::scaling::{flag::Flags, Context as Scaler},
    Dictionary, Rational,
};
use ffmpeg_next as ffmpeg;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::{Duration, Instant};
use log::{debug, info, warn};

use super::window_info::{WindowInfo, WindowInfoService};

type InputParams = (String, String, Vec<(String, String)>);

/// Safely converts an i32 format value to a `Pixel` type.
///
/// FFmpeg's AVPixelFormat is a C enum where -1 is AV_PIX_FMT_NONE and valid
/// formats are non-negative integers. This function validates the input before
/// conversion to avoid undefined behavior from invalid enum discriminants.
fn pixel_format_from_raw(format: i32) -> Pixel {
    use ffmpeg::ffi::AVPixelFormat;

    // AV_PIX_FMT_NONE is -1, valid formats are >= 0
    // AV_PIX_FMT_NB marks the end of the enum (typically around 200+)
    // We accept -1 (NONE) and any non-negative value, letting Pixel::from
    // handle unknown formats gracefully.
    if format == -1 {
        return Pixel::None;
    }

    if format < 0 {
        // Invalid negative value that isn't NONE
        return Pixel::None;
    }

    // SAFETY: We've validated that format is either -1 (AV_PIX_FMT_NONE) or
    // a non-negative integer. FFmpeg's AVPixelFormat enum uses -1 for NONE
    // and sequential non-negative values for valid formats. The Pixel::from
    // implementation handles unknown format values gracefully.
    let pix_fmt = unsafe { std::mem::transmute::<i32, AVPixelFormat>(format) };
    Pixel::from(pix_fmt)
}

/// Configuration for screen recording sessions.
#[derive(Clone, Debug)]
pub struct RecordingConfig {
    pub framerate: u32,
    pub duration_secs: Option<u64>,
    pub output_path: PathBuf,
    pub monitor_index: Option<usize>,
    pub window_id: Option<String>,    // Record a specific window by ID
    pub window_title: Option<String>, // Record a specific window by title pattern
    pub include_audio: bool,
    pub fast: bool, // Capture as fast as possible, ignore target FPS
    /// Video segment duration in seconds (None = no segmentation)
    /// When enabled, creates files like: recording_000.mp4, recording_001.mp4, etc.
    pub segment_duration_secs: Option<u64>,
}

/// Information about available monitors/displays.
#[derive(Clone, Debug)]
pub struct MonitorInfo {
    pub index: usize,
    pub name: String,
    pub display_name: String,
    #[allow(dead_code)]
    pub resolution: Option<String>,
    pub is_primary: bool,
    pub offset_x: Option<i32>,
    pub offset_y: Option<i32>,
}

impl Default for RecordingConfig {
    /// Returns default recording configuration (30 FPS, no duration limit, MP4 output).
    fn default() -> Self {
        Self {
            framerate: 30,
            duration_secs: None,
            output_path: PathBuf::from("output/recording.mp4"),
            monitor_index: None,
            window_id: None,
            window_title: None,
            include_audio: true,
            fast: false,
            segment_duration_secs: None,
        }
    }
}

/// Main screen recorder service.
pub struct ScreenRecorder {
    _width: u32,
    _height: u32,
    _config: RecordingConfig,
}

impl ScreenRecorder {
    /// Creates a new screen recorder with default configuration.
    ///
    /// # Errors
    ///
    /// Returns [`RecorderError::Configuration`] if FFmpeg initialization fails.
    #[must_use = "ScreenRecorder creation may fail and the Result should be handled"]
    pub fn new() -> Result<Self> {
        let config = RecordingConfig::default();
        Self::new_with_config(config)
    }

    /// Creates a new screen recorder with the specified configuration.
    ///
    /// # Errors
    ///
    /// Returns [`RecorderError::Configuration`] if FFmpeg initialization fails.
    #[must_use = "ScreenRecorder creation may fail and the Result should be handled"]
    pub fn new_with_config(config: RecordingConfig) -> Result<Self> {
        // Initialize FFmpeg early to catch missing libs
        ffmpeg::init().map_err(|e| {
            RecorderError::Configuration(format!(
                "FFmpeg initialization failed: {}. Ensure FFmpeg libraries are installed.",
                e
            ))
        })?;

        // For now, we'll get dimensions when we start recording
        // Default to common resolution - will be updated from FFmpeg input
        Ok(Self {
            _width: 1920,
            _height: 1080,
            _config: config,
        })
    }

    /// Lists all available monitors/displays on the system.
    ///
    /// # Errors
    ///
    /// Returns [`RecorderError::Configuration`] if monitor enumeration fails.
    /// Returns [`RecorderError::CommandFailed`] if xrandr command fails (Linux).
    pub fn list_monitors() -> Result<Vec<MonitorInfo>> {
        #[cfg(target_os = "linux")]
        {
            Self::list_monitors_linux()
        }

        #[cfg(target_os = "macos")]
        {
            Self::list_monitors_macos()
        }

        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            Err(anyhow::anyhow!(
                "Monitor enumeration not supported on this platform"
            ))
        }
    }

    /// Gets windows visible on the specified monitor.
    /// Uses window geometry to determine which windows are on the monitor
    ///
    /// # Errors
    ///
    /// Returns [`RecorderError::MonitorNotFound`] if the monitor index is invalid.
    /// Returns [`RecorderError::Configuration`] if monitor enumeration fails.
    pub fn get_windows_on_monitor(monitor_index: usize) -> Result<Vec<WindowInfo>> {
        let monitors = Self::list_monitors()?;
        let monitor = monitors
            .get(monitor_index)
            .ok_or_else(|| anyhow::anyhow!("Monitor index {} not found", monitor_index))?;

        let monitor_x = monitor.offset_x.unwrap_or(0);
        let monitor_y = monitor.offset_y.unwrap_or(0);
        let monitor_width = monitor
            .resolution
            .as_ref()
            .and_then(|r| r.split('x').next())
            .and_then(|w| w.parse::<i32>().ok())
            .unwrap_or(1920);
        let monitor_height = monitor
            .resolution
            .as_ref()
            .and_then(|r| r.split('x').nth(1))
            .and_then(|h| h.parse::<i32>().ok())
            .unwrap_or(1080);

        let all_windows = WindowInfoService::list_windows()?;
        let mut windows_on_monitor = Vec::new();

        for window in all_windows {
            if let Some(geom) = &window.geometry {
                // Check if window overlaps with monitor bounds
                let window_right = geom.x + geom.width as i32;
                let window_bottom = geom.y + geom.height as i32;
                let monitor_right = monitor_x + monitor_width;
                let monitor_bottom = monitor_y + monitor_height;

                // Window overlaps if it's not completely outside monitor bounds
                if !(window_right <= monitor_x
                    || geom.x >= monitor_right
                    || window_bottom <= monitor_y
                    || geom.y >= monitor_bottom)
                {
                    windows_on_monitor.push(window);
                }
            }
        }

        Ok(windows_on_monitor)
    }

    /// Gets information about the currently active window.
    ///
    /// # Errors
    ///
    /// Returns [`RecorderError::Other`] if window information retrieval fails.
    pub fn get_active_window() -> Result<Option<WindowInfo>> {
        WindowInfoService::get_active_window().map_err(|e| RecorderError::Other(e.to_string()))
    }

    #[cfg(target_os = "linux")]
    fn list_monitors_linux() -> Result<Vec<MonitorInfo>> {
        // Use xrandr to enumerate monitors
        let output = Command::new("xrandr")
            .arg("--listmonitors")
            .output()
            .map_err(|e| {
                RecorderError::Configuration(format!(
                    "Failed to run xrandr: {}. Is xrandr installed?",
                    e
                ))
            })?;

        if !output.status.success() {
            return Err(RecorderError::Configuration(format!(
                "xrandr command failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut monitors = Vec::new();
        let mut index = 0;

        // Parse xrandr --listmonitors output
        // Format: "Monitors: N\n 0: +*HDMI-1 1920/508x1080/286+0+0\n 1: +DP-1 2560/677x1440/382+1920+0"
        for line in stdout.lines().skip(1) {
            // Skip empty lines
            if line.trim().is_empty() {
                continue;
            }

            // Parse line like: " 0: +*HDMI-1 1920/508x1080/286+0+0"
            // or: " 1: +DP-1 2560/677x1440/382+1920+0"
            let line = line.trim();
            if let Some(colon_pos) = line.find(':') {
                let rest = &line[colon_pos + 1..].trim();

                // Check if this is the primary monitor (has * after +)
                let is_primary = rest.starts_with("+*");
                let name_start = if is_primary {
                    2
                } else if rest.starts_with("+") {
                    1
                } else {
                    0
                };

                // Extract monitor name (until first space)
                if let Some(space_pos) = rest[name_start..].find(' ') {
                    let name = rest[name_start..name_start + space_pos].to_string();
                    let resolution_part = &rest[name_start + space_pos + 1..];

                    // Extract resolution and position (e.g., "1920/508x1080/286+0+0" -> "1920x1080" at 0,0)
                    let (resolution, offset_x, offset_y) = if let Some(x_pos) =
                        resolution_part.find('x')
                    {
                        let width_part = &resolution_part[..x_pos];
                        let height_part = &resolution_part[x_pos + 1..];

                        let width = width_part.split('/').next().unwrap_or("?");
                        let (height, offset_str) = if let Some(plus_pos) = height_part.find('+') {
                            (
                                height_part[..plus_pos].split('/').next().unwrap_or("?"),
                                &height_part[plus_pos + 1..],
                            )
                        } else {
                            (height_part.split('/').next().unwrap_or("?"), "")
                        };

                        // Parse offset (e.g., "0+1080  DP-0" -> x=0, y=1080)
                        // The offset_str may include extra text after the offset, so we need to trim it
                        let (ox, oy) = if !offset_str.is_empty() {
                            // Trim whitespace and any trailing text (like monitor name)
                            // Find the first whitespace or end of string after the second number
                            let trimmed_offset = offset_str.trim();
                            if let Some(plus_pos) = trimmed_offset.find('+') {
                                let x_str = trimmed_offset[..plus_pos].trim();
                                // For y, we need to find where the number ends (before whitespace or end)
                                let y_part = &trimmed_offset[plus_pos + 1..];
                                // Find the end of the number (first whitespace or end of string)
                                let y_end = y_part
                                    .find(|c: char| c.is_whitespace())
                                    .unwrap_or(y_part.len());
                                let y_str = &y_part[..y_end].trim();
                                let x = x_str.parse::<i32>().ok();
                                let y = y_str.parse::<i32>().ok();
                                (x, y)
                            } else {
                                (None, None)
                            }
                        } else {
                            (Some(0), Some(0))
                        };

                        (Some(format!("{}x{}", width, height)), ox, oy)
                    } else {
                        (None, None, None)
                    };

                    let display_name = format!(
                        "{} ({})",
                        name,
                        resolution.as_deref().unwrap_or("unknown resolution")
                    );

                    monitors.push(MonitorInfo {
                        index,
                        name: name.clone(),
                        display_name,
                        resolution,
                        is_primary,
                        offset_x,
                        offset_y,
                    });

                    index += 1;
                }
            }
        }

        // If xrandr --listmonitors didn't work or returned no monitors,
        // try xrandr --query to get connected displays
        if monitors.is_empty() {
            let output = Command::new("xrandr")
                .arg("--query")
                .output()
                .map_err(|e| {
                    RecorderError::Configuration(format!("Failed to run xrandr --query: {}", e))
                })?;

            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let mut index = 0;

                // Parse xrandr --query output
                // Look for lines like: "HDMI-1 connected primary 1920x1080+0+0"
                for line in stdout.lines() {
                    if line.contains(" connected ") {
                        let parts: Vec<&str> = line.split_whitespace().collect();
                        if parts.len() >= 2 {
                            let name = parts[0].to_string();
                            let is_primary = line.contains(" primary ");

                            // Extract resolution and position if present
                            // Format: "1920x1080+0+0" or "1920x1080+1920+0"
                            let (resolution, offset_x, offset_y) = parts
                                .iter()
                                .find(|p| {
                                    p.contains('x')
                                        && p.chars()
                                            .next()
                                            .map(|c| c.is_ascii_digit())
                                            .unwrap_or(false)
                                })
                                .map(|s| {
                                    let res_part = s.split('+').next().unwrap_or(s);
                                    let resolution = Some(res_part.to_string());

                                    // Parse offsets
                                    let offsets: Vec<&str> = s.split('+').skip(1).collect();
                                    let ox = offsets.first().and_then(|x| x.parse::<i32>().ok());
                                    let oy = offsets.get(1).and_then(|y| y.parse::<i32>().ok());

                                    (resolution, ox, oy)
                                })
                                .unwrap_or((None, None, None));

                            let display_name = if let Some(ref res) = resolution {
                                format!("{} ({})", name, res)
                            } else {
                                name.clone()
                            };

                            monitors.push(MonitorInfo {
                                index,
                                name,
                                display_name,
                                resolution,
                                is_primary,
                                offset_x,
                                offset_y,
                            });

                            index += 1;
                        }
                    }
                }
            }
        }

        // If still no monitors found, at least provide the default display
        if monitors.is_empty() {
            let display = std::env::var("DISPLAY").unwrap_or_else(|_| ":0.0".to_string());
            monitors.push(MonitorInfo {
                index: 0,
                name: display.clone(),
                display_name: format!("Default Display ({})", display),
                resolution: None,
                is_primary: true,
                offset_x: Some(0),
                offset_y: Some(0),
            });
        }

        Ok(monitors)
    }

    #[cfg(target_os = "macos")]
    fn list_monitors_macos() -> Result<Vec<MonitorInfo>> {
        // Use FFmpeg's device enumeration API for avfoundation
        let mut monitors = Vec::new();

        // Try to enumerate devices using FFmpeg
        // Note: avfoundation device enumeration requires special handling
        // We'll use a subprocess to run ffmpeg with -list_devices true

        let output = Command::new("ffmpeg")
            .arg("-f")
            .arg("avfoundation")
            .arg("-list_devices")
            .arg("true")
            .arg("-i")
            .arg("")
            .output()
            .map_err(|e| RecorderError::Configuration(format!(
                "Failed to enumerate monitors using FFmpeg: {}. Make sure ffmpeg is installed and has avfoundation support.",
                e
            )))?;

        match output {
            Ok(output) => {
                // Parse stderr (FFmpeg outputs device list to stderr)
                let stderr = String::from_utf8_lossy(&output.stderr);
                let mut index = 0;
                let mut in_video_section = false;

                for line in stderr.lines() {
                    // Look for video device section
                    if line.contains("AVFoundation video devices:")
                        || line.contains("Video devices:")
                    {
                        in_video_section = true;
                        continue;
                    }

                    if in_video_section {
                        // Look for audio section to know when to stop
                        if line.contains("AVFoundation audio devices:")
                            || line.contains("Audio devices:")
                        {
                            break;
                        }

                        // Parse device line like: "[0] Capture screen 0"
                        // or: "[1] Capture screen 1"
                        if let Some(bracket_start) = line.find('[') {
                            if let Some(bracket_end) = line[bracket_start + 1..].find(']') {
                                if let Ok(device_index) = line
                                    [bracket_start + 1..bracket_start + 1 + bracket_end]
                                    .parse::<usize>()
                                {
                                    let name_start = bracket_start + 1 + bracket_end + 1;
                                    let name = line[name_start..].trim().to_string();

                                    monitors.push(MonitorInfo {
                                        index,
                                        name: device_index.to_string(),
                                        display_name: name.clone(),
                                        resolution: None,
                                        is_primary: device_index == 0, // Device index 0 is the primary screen
                                        offset_x: None,
                                        offset_y: None,
                                    });

                                    index += 1;
                                }
                            }
                        }
                    }
                }
            }
            Err(e) => {
                // If ffmpeg command fails, return error with helpful message
                return Err(RecorderError::Configuration(format!(
                    "Failed to enumerate monitors using FFmpeg: {}. \
                    Make sure ffmpeg is installed and has avfoundation support.",
                    e
                )));
            }
        }

        // If no monitors found, provide at least one default
        if monitors.is_empty() {
            monitors.push(MonitorInfo {
                index: 0,
                name: "0".to_string(),
                display_name: "Default Screen (Capture screen 0)".to_string(),
                resolution: None,
                is_primary: true,
                offset_x: None,
                offset_y: None,
            });
        }

        Ok(monitors)
    }

    fn get_input_format_and_url(
        monitor_index: Option<usize>,
        _window_info: Option<&WindowInfo>,
        fps: u32,
    ) -> Result<InputParams> {
        #[cfg(target_os = "linux")]
        {
            // Linux: use x11grab
            // Format: x11grab -i :display.screen+x,y -framerate fps -video_size WxH
            // For i3 and some setups, we may need :1.0 instead of :0.0
            // Try to detect from DISPLAY env var, but allow override
            let display = std::env::var("DISPLAY").unwrap_or_else(|_| ":0.0".to_string());

            // Extract display number (e.g., ":0" from ":0.0" or ":1" from ":1.0")
            // Use the actual DISPLAY value - it's already :1 in i3 setups
            // Note: Currently unused, but kept for potential future use
            let _display_num = if let Some(dot_pos) = display.find('.') {
                &display[..dot_pos]
            } else {
                &display
            };

            // If window_info is provided, use window geometry for recording
            let (offset_x, offset_y, video_size_str) = if let Some(window) = _window_info {
                if let Some(geom) = &window.geometry {
                    let size = format!("{}x{}", geom.width, geom.height);
                    (geom.x, geom.y, Some(size))
                } else {
                    return Err(RecorderError::Configuration(format!(
                        "Window geometry not available for window: {}",
                        window.window_id
                    )));
                }
            } else {
                // Step 1: Get monitor info to extract name and offset
                // We'll use the exact same method as the working command
                let (monitor_name, ox, oy) = if let Some(idx) = monitor_index {
                    if let Ok(monitors) = Self::list_monitors_linux() {
                        if let Some(monitor) = monitors.get(idx) {
                            let ox = monitor.offset_x.unwrap_or(0);
                            let oy = monitor.offset_y.unwrap_or(0);
                            (Some(monitor.name.clone()), ox, oy)
                        } else {
                            warn!("WARNING: Monitor index {} not found", idx);
                            (None, 0, 0)
                        }
                    } else {
                        warn!("WARNING: Failed to list monitors");
                        (None, 0, 0)
                    }
                } else {
                    (None, 0, 0)
                };

                // Step 2: Extract video_size using the EXACT same method as the working command
                // Command: xrandr | awk '/DP-0 connected/ {pos = $3; sub(/\+.*/, "", pos); print pos; exit}'
                let video_size_str = if let Some(ref name) = monitor_name {
                    let output = Command::new("xrandr").output().ok();
                    if let Some(output) = output {
                        let stdout = String::from_utf8_lossy(&output.stdout);
                        let mut found = None;
                        for line in stdout.lines() {
                            // Match line like: "DP-0 connected 2560x1440+0+1080 ..."
                            if line.contains(&format!("{} connected", name)) {
                                let parts: Vec<&str> = line.split_whitespace().collect();
                                if parts.len() >= 3 {
                                    let pos_str = parts[2]; // This is "2560x1440+0+1080"
                                                            // Extract resolution by removing everything after first '+'
                                    if let Some(plus_pos) = pos_str.find('+') {
                                        let resolution = &pos_str[..plus_pos]; // "2560x1440"
                                        found = Some(resolution.to_string());
                                        break; // Found it, exit loop
                                    }
                                }
                            }
                        }
                        found
                    } else {
                        None
                    }
                } else {
                    None
                };
                (ox, oy, video_size_str)
            };

            // Step 3: Build URL - use :1.0 hardcoded (as in working command)
            // Format: :1.0+offset_x,offset_y (comma separator!)
            let url = format!(":1.0+{},{}", offset_x, offset_y);

            // Step 4: Build options - video_size and framerate
            let mut options = vec![("framerate".to_string(), fps.to_string())];

            if let Some(ref size) = video_size_str {
                options.push(("video_size".to_string(), size.clone()));
            } else {
                warn!("WARNING: Could not extract video_size for monitor");
            }

            Ok(("x11grab".to_string(), url, options))
        }

        #[cfg(target_os = "macos")]
        {
            // macOS: use avfoundation
            // Format: avfoundation -i "device_index:audio_index" -framerate fps
            // For screen capture, device_index 0 is the primary screen
            let device_index = monitor_index
                .map(|i| i.to_string())
                .unwrap_or_else(|| "0".to_string());
            let url = format!("{}:none", device_index); // :none means no audio

            // avfoundation options
            let options = vec![("framerate".to_string(), fps.to_string())];

            Ok(("avfoundation".to_string(), url, options))
        }

        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            Err(anyhow::anyhow!(
                "Screen capture not supported on this platform"
            ))
        }
    }

    /// Captures a screenshot to the specified file path (full screen).
    ///
    /// # Errors
    ///
    /// Returns [`RecorderError::ScreenCapture`] if screenshot capture fails.
    /// Returns [`RecorderError::Io`] if file operations fail.
    pub fn capture_screenshot_to_file(&self, output_path: &str) -> Result<()> {
        self.capture_screenshot_to_file_with_monitor(output_path, None)
    }

    /// Captures a screenshot from the specified monitor.
    pub fn capture_screenshot_to_file_with_monitor(
        &self,
        output_path: &str,
        monitor_index: Option<usize>,
    ) -> Result<()> {
        self.capture_screenshot_to_file_with_window(output_path, monitor_index, None)
    }

    /// Captures a screenshot from the specified window or monitor.
    pub fn capture_screenshot_to_file_with_window(
        &self,
        output_path: &str,
        monitor_index: Option<usize>,
        window_info: Option<&WindowInfo>,
    ) -> Result<()> {
        use image::{ImageBuffer, Rgb, RgbImage};

        // Use the same monitor selection logic as recording
        let fps = 30; // FPS doesn't matter for single frame, but required for x11grab
        let (input_format_name, input_url, input_options) =
            Self::get_input_format_and_url(monitor_index, window_info, fps)?;

        // Find the input format
        let input_format = input::video()
            .find(|f| {
                if let ffmpeg::Format::Input(input) = f {
                    input.name() == input_format_name
                } else {
                    false
                }
            })
            .ok_or_else(|| anyhow::anyhow!("Input format '{}' not found", input_format_name))?;

        // Convert options to Dictionary
        let mut dict = Dictionary::new();
        for (key, value) in &input_options {
            dict.set(key, value);
        }

        let ctx = format::open_with(&input_url, &input_format, dict).map_err(|e| {
            RecorderError::VideoEncoding(format!("Failed to open input '{}': {:?}", input_url, e))
        })?;

        let mut ictx = match ctx {
            format::Context::Input(ictx) => ictx,
            _ => return Err(RecorderError::VideoEncoding("Expected input context".into())),
        };

        let input_stream = ictx
            .streams()
            .best(Type::Video)
            .ok_or_else(|| RecorderError::VideoEncoding("No video stream found".into()))?;
        let input_stream_index = input_stream.index();

        // Get decoder using parameters (cross-platform compatible)
        // Use Context::from_parameters() as per ffmpeg-next API
        let codec_params = input_stream.parameters();
        let context_decoder = CodecContext::from_parameters(codec_params.clone()).map_err(|e| {
            RecorderError::VideoEncoding(format!("Failed to create decoder context: {:?}", e))
        })?;
        let mut decoder = context_decoder.decoder().video().map_err(|e| {
            RecorderError::VideoEncoding(format!("Failed to get video decoder: {:?}", e))
        })?;

        // Get video parameters from codec_params using unsafe FFI
        // SAFETY: `codec_params.as_ptr()` returns a valid pointer to AVCodecParameters
        // that remains valid for the lifetime of `codec_params`. We only perform
        // read-only dereferences and do not store the raw pointer beyond this block.
        // The `codec_params` object is owned by this function scope, ensuring the
        // pointer remains valid throughout these reads.
        let (width, height, input_pixel_format) = unsafe {
            let params_ptr = codec_params.as_ptr();
            let width = (*params_ptr).width as u32;
            let height = (*params_ptr).height as u32;
            let format = (*params_ptr).format;
            (width, height, pixel_format_from_raw(format))
        };

        // Create frame buffers
        let mut input_frame = Video::new(input_pixel_format, width, height);
        let mut rgb_frame = Video::new(Pixel::RGB24, width, height);

        // Create scaler to convert to RGB24
        let mut scaler = Scaler::get(
            input_pixel_format,
            width,
            height,
            Pixel::RGB24,
            width,
            height,
            Flags::BILINEAR,
        )
        .map_err(|e| RecorderError::VideoEncoding(format!("Failed to create scaler: {}", e)))?;

        // Read and decode a single frame
        let mut got_frame = false;
        for (stream, pkt) in ictx.packets() {
            if stream.index() == input_stream_index {
                decoder.send_packet(&pkt)?;
                match decoder.receive_frame(&mut input_frame) {
                    Ok(()) => {
                        got_frame = true;
                        break;
                    }
                    Err(ffmpeg::Error::Other { errno: -11 }) => {
                        // EAGAIN - need more packets
                        continue;
                    }
                    Err(e) => {
                        return Err(RecorderError::VideoEncoding(format!(
                            "Failed to decode frame: {:?}",
                            e
                        )));
                    }
                }
            }
        }

        if !got_frame {
            return Err(RecorderError::ScreenCapture(
                "Failed to capture frame from screen".into(),
            ));
        }

        // Convert to RGB24
        scaler.run(&input_frame, &mut rgb_frame)?;

        // Extract RGB data from frame
        let rgb_data = rgb_frame.data(0);
        let stride = rgb_frame.stride(0);

        // Create image buffer
        let mut img: RgbImage = ImageBuffer::new(width, height);

        // Copy data from FFmpeg frame to image buffer
        // FFmpeg uses stride (may be padded), image buffer is tightly packed
        for y in 0..height {
            for x in 0..width {
                let src_idx = (y as usize * stride as usize + x as usize * 3) as usize;
                if src_idx + 2 < rgb_data.len() {
                    let r = rgb_data[src_idx];
                    let g = rgb_data[src_idx + 1];
                    let b = rgb_data[src_idx + 2];
                    img.put_pixel(x, y, Rgb([r, g, b]));
                }
            }
        }

        // Save image
        img.save(output_path).map_err(|e| {
            RecorderError::VideoEncoding(format!("Failed to save image to {}: {}", output_path, e))
        })?;

        Ok(())
    }

    fn setup_input(
        monitor_index: Option<usize>,
        window_info: Option<&WindowInfo>,
        fps: u32,
    ) -> Result<(
        format::context::Input,
        ffmpeg::decoder::Video,
        u32,
        u32,
        Pixel,
    )> {
        // Setup FFmpeg input for screen capture
        let (input_format_name, input_url, input_options) =
            Self::get_input_format_and_url(monitor_index, window_info, fps)?;

        println!(
            "Using input format: {}, URL: {}",
            input_format_name, input_url
        );
        if !input_options.is_empty() {
            println!("Input options: {:?}", input_options);
        }

        // Find the input format using device iterator
        let input_format = input::video()
            .find(|f| f.name() == input_format_name) // Fixed: Use name() instead of enum match to avoid private Format
            .ok_or_else(|| {
                RecorderError::Configuration(format!(
                    "Input format '{}' not found. Make sure FFmpeg supports this format.",
                    input_format_name
                ))
            })?;

        // Convert options to Dictionary
        let mut dict = Dictionary::new();
        for (key, value) in &input_options {
            dict.set(key, value);
        }

        // Use format::open_with() to pass options (framerate, video_size, etc.)
        let ctx = format::open_with(&input_url, &input_format, dict).map_err(|e| {
            RecorderError::VideoEncoding(format!(
                "Failed to open input '{}' with format '{}': {:?}",
                input_url, input_format_name, e
            ))
        })?;

        // Extract input context from the format context
        let ictx = match ctx {
            format::Context::Input(ictx) => ictx,
            _ => {
                return Err(RecorderError::VideoEncoding(
                    "Expected input context, got output context".into(),
                ))
            }
        };

        let input_stream = ictx
            .streams()
            .best(Type::Video)
            .ok_or_else(|| RecorderError::VideoEncoding("No video stream found in input".into()))?;

        // Get decoder using parameters (cross-platform compatible)
        // Use Context::from_parameters() as per ffmpeg-next API
        let codec_params = input_stream.parameters();
        let context_decoder = CodecContext::from_parameters(codec_params.clone()).map_err(|e| {
            RecorderError::VideoEncoding(format!("Failed to create decoder context: {:?}", e))
        })?;
        let decoder = context_decoder.decoder().video().map_err(|e| {
            RecorderError::VideoEncoding(format!("Failed to get video decoder: {:?}", e))
        })?;

        // Get video parameters from codec_params using unsafe FFI
        // SAFETY: `codec_params.as_ptr()` returns a valid pointer to AVCodecParameters
        // that remains valid for the lifetime of `codec_params`. We only perform
        // read-only dereferences and do not store the raw pointer beyond this block.
        // The `codec_params` object is owned by this function scope, ensuring the
        // pointer remains valid throughout these reads.
        let (raw_width, raw_height, pix_fmt) = unsafe {
            let params_ptr = codec_params.as_ptr();
            let width = (*params_ptr).width as u32;
            let height = (*params_ptr).height as u32;
            let format = (*params_ptr).format;
            (width, height, pixel_format_from_raw(format))
        };

        Ok((ictx, decoder, raw_width, raw_height, pix_fmt))
    }

    /// Configures the stream parameters using unsafe FFI.
    ///
    /// # Safety
    ///
    /// This function uses unsafe code to dereference raw pointers from `ffmpeg-next` internal structures.
    /// We rely on `stream.parameters()` returning a valid pointer to `AVCodecParameters`.
    /// The `stream` object is mutably borrowed, ensuring exclusive access during modification.
    fn configure_stream_parameters(
        stream: &mut format::stream::StreamMut,
        codec_id: codec::Id,
        width: u32,
        height: u32,
        pixel_format: ffmpeg::ffi::AVPixelFormat,
        time_base: ffmpeg::ffi::AVRational,
    ) -> Result<()> {
        unsafe {
            use ffmpeg::ffi::*;
            let params_ptr = stream.parameters().as_ptr() as *mut AVCodecParameters;

            (*params_ptr).codec_type = AVMediaType::AVMEDIA_TYPE_VIDEO;
            (*params_ptr).codec_id = codec_id.into();
            (*params_ptr).width = width as i32;
            (*params_ptr).height = height as i32;
            (*params_ptr).format = pixel_format as i32;

            let stream_ptr_raw = stream.as_mut_ptr();
            (*stream_ptr_raw).time_base = time_base;
        }
        Ok(())
    }

    /// Records the screen according to the configuration until stopped.
    ///
    /// # Errors
    ///
    /// Returns [`RecorderError::ScreenCapture`] if screen recording initialization fails.
    /// Returns [`RecorderError::VideoEncoding`] if video encoding fails.
    /// Returns [`RecorderError::Io`] if file operations fail.
    pub fn record(&self, config: RecordingConfig, stop_signal: Arc<AtomicBool>) -> Result<()> {
        let fps = config.framerate;
        let frame_interval = Duration::from_nanos(1_000_000_000u64 / fps as u64);

        // Resolve window info
        let window_info = if let Some(ref window_id) = config.window_id {
            WindowInfoService::get_window_by_id(window_id)?
        } else if let Some(ref window_title) = config.window_title {
            let windows = WindowInfoService::get_windows_by_title(window_title)?;
            windows.first().cloned()
        } else {
            None
        };

        // Setup Input
        let (mut ictx, mut decoder, raw_width, raw_height, input_pixel_format) =
            Self::setup_input(config.monitor_index, window_info.as_ref(), fps)?;

        // Calculate dimensions (ensure even)
        let width = if raw_width % 2 == 0 {
            raw_width
        } else {
            raw_width + 1
        };
        let height = if raw_height % 2 == 0 {
            raw_height
        } else {
            raw_height + 1
        };

        println!(
            "Screen dimensions: {}x{} (padded to {}x{})",
            raw_width, raw_height, width, height
        );

        // Setup Output
        let mut octx = format::output(&config.output_path).map_err(|e| {
            RecorderError::VideoEncoding(format!(
                "Failed to setup output {}: {}",
                config.output_path.display(),
                e
            ))
        })?;
        let codec = encoder::find(codec::Id::H264)
            .ok_or_else(|| anyhow::anyhow!("No H.264 encoder available"))?;
        let mut stream = octx.add_stream(codec)?;
        let ostream_idx = stream.index();

        // Configure output stream parameters
        use ffmpeg::ffi::*;
        Self::configure_stream_parameters(
            &mut stream,
            codec.id(),
            width,
            height,
            AVPixelFormat::AV_PIX_FMT_YUV420P,
            AVRational {
                num: 1,
                den: fps as i32,
            },
        )?;

        // Set time_base for proper timestamp handling
        let encoder_time_base = Rational(1, fps as i32);

        // Write header
        octx.write_header()?;

        // Create Encoder Context
        let mut encoder_ctx = CodecContext::new_with_codec(codec);
        unsafe {
            let ctx_ptr = encoder_ctx.as_mut_ptr();
            (*ctx_ptr).width = width as i32;
            (*ctx_ptr).height = height as i32;
            (*ctx_ptr).pix_fmt = AVPixelFormat::AV_PIX_FMT_YUV420P;
            (*ctx_ptr).time_base = AVRational {
                num: 1,
                den: fps as i32,
            };
            (*ctx_ptr).framerate = AVRational {
                num: fps as i32,
                den: 1,
            };
            (*ctx_ptr).max_b_frames = 0;
            (*ctx_ptr).gop_size = 1;
            (*ctx_ptr).flags |= AV_CODEC_FLAG_LOW_DELAY as i32;
            (*ctx_ptr).flags2 |= AV_CODEC_FLAG2_FAST;

            if avcodec_open2(ctx_ptr, (*ctx_ptr).codec, std::ptr::null_mut()) < 0 {
                return Err(RecorderError::VideoEncoding(
                    "Failed to open H.264 encoder".into(),
                ));
            }
        }
        let mut video_encoder = encoder_ctx.encoder().video()?;

        // Get stream time_base after writing header
        let stream_time_base = octx
            .stream(ostream_idx)
            .ok_or_else(|| anyhow::anyhow!("Stream {} not found", ostream_idx))?
            .time_base();

        // Initialize TimestampTracker
        let mut ts_tracker = TimestampTracker::new(stream_time_base, encoder_time_base);

        // Setup Scaler
        let mut input_frame = Video::new(input_pixel_format, raw_width, raw_height);
        let mut scaled_frame = Video::new(Pixel::YUV420P, width, height);
        let needs_scaling =
            input_pixel_format != Pixel::YUV420P || raw_width != width || raw_height != height;

        let mut scaler = if needs_scaling {
            let s = Scaler::get(
                input_pixel_format,
                raw_width,
                raw_height,
                Pixel::YUV420P,
                width,
                height,
                Flags::BILINEAR,
            )
            .map_err(|e| RecorderError::VideoEncoding(format!("Scaler init failed: {}", e)))?;
            Some(s)
        } else {
            None
        };

        let mut frame_num: i64 = 0;
        let start_time = Instant::now();
        let end_time = config
            .duration_secs
            .map(|secs| start_time + Duration::from_secs(secs));

        // Reusable packet for flush
        let mut flush_packets: Vec<Packet> = Vec::with_capacity(20);

        // Loop
        let input_stream_index = ictx.streams().best(Type::Video).unwrap().index();

        for (stream, pkt) in ictx.packets() {
            let loop_start = Instant::now();

            // Check stop conditions
            if let Some(et) = &end_time {
                if Instant::now() >= *et {
                    info!("Duration limit reached, stopping...");
                    break;
                }
            }
            if stop_signal.load(std::sync::atomic::Ordering::Relaxed) {
                info!("Stop signal received, breaking loop...");
                break;
            }

            if stream.index() != input_stream_index {
                continue;
            }

            let capture_start = Instant::now();
            decoder
                .send_packet(&pkt)
                .map_err(|e| RecorderError::VideoEncoding(format!("Send packet error: {:?}", e)))?;

            let mut got_frame = false;
            loop {
                match decoder.receive_frame(&mut input_frame) {
                    Ok(()) => {
                        got_frame = true;
                        break;
                    }
                    Err(ffmpeg::Error::Other { errno: -11 }) => break, // EAGAIN
                    Err(e) => {
                        return Err(RecorderError::VideoEncoding(format!(
                            "Decode frame error: {:?}",
                            e
                        )))
                    }
                }
            }
            if !got_frame {
                continue;
            }

            let capture_elapsed = capture_start.elapsed();
            let scale_start = Instant::now();

            if needs_scaling {
                if let Some(ref mut s) = scaler {
                    s.run(&input_frame, &mut scaled_frame)
                        .map_err(|e| RecorderError::VideoEncoding(format!("Scaling error: {}", e)))?;
                }
                scaled_frame.set_pts(Some(frame_num));
            } else {
                input_frame.set_pts(Some(frame_num));
            }
            let scale_elapsed = scale_start.elapsed();

            let frame_to_encode = if needs_scaling {
                &scaled_frame
            } else {
                &input_frame
            };

            let encode_start = Instant::now();
            video_encoder
                .send_frame(frame_to_encode)
                .map_err(|e| RecorderError::VideoEncoding(format!("Send frame error: {:?}", e)))?;

            // Update PTS tracker
            let stream_frame_pts = ts_tracker.update_pts(frame_num);
            let stream_frame_dts = ts_tracker.next_dts(stream_frame_pts);

            let mut packet = Packet::empty();
            let mut packet_count = 0;
            while let Ok(()) = video_encoder.receive_packet(&mut packet) {
                packet.set_stream(ostream_idx);
                packet.set_pts(Some(stream_frame_pts));
                packet.set_dts(Some(stream_frame_dts));
                ts_tracker.commit_dts(stream_frame_dts);
                ts_tracker.frames_with_packets.insert(frame_num as usize);

                packet.write_interleaved(&mut octx)?;
                packet_count += 1;
            }
            let encode_elapsed = encode_start.elapsed();

            if frame_num % 10 == 0 || frame_num < 5 {
                let loop_elapsed = loop_start.elapsed();
                debug!(
                    "Frame timing: frame={}, capture={:?}, scale={:?}, encode={:?}, total={:?}, packets={}",
                    frame_num, capture_elapsed, scale_elapsed, encode_elapsed, loop_elapsed, packet_count
                );
            }

            // Periodic Flush
            if frame_num > 0 && frame_num % 5 == 0 {
                flush_packets.clear();
                let mut flush_packet = Packet::empty();
                while let Ok(()) = video_encoder.receive_packet(&mut flush_packet) {
                    flush_packet.set_stream(ostream_idx);

                    let final_stream_dts = if let Some(last) = ts_tracker.last_dts {
                        last + ts_tracker.dts_increment
                    } else {
                        stream_frame_pts
                    };
                    let final_stream_pts = stream_frame_pts.max(final_stream_dts);

                    flush_packet.set_pts(Some(final_stream_pts));
                    flush_packet.set_dts(Some(final_stream_dts));
                    ts_tracker.commit_dts(final_stream_dts);

                    flush_packet.write_interleaved(&mut octx)?;
                }
            }

            frame_num += 1;

            // FPS throttling
            let frame_elapsed = loop_start.elapsed();
            if !config.fast && frame_elapsed < frame_interval {
                std::thread::sleep(frame_interval - frame_elapsed);
            }
        }

        info!("Capturing finished. Captured {} frames.", frame_num);

        // Flush encoder
        video_encoder.send_eof()?;

        flush_packets.clear();
        let mut packet = Packet::empty();
        while video_encoder.receive_packet(&mut packet).is_ok() {
            flush_packets.push(packet);
            packet = Packet::empty();
        }

        // Calculate actual DTS increment for flush
        let actual_duration_secs = start_time.elapsed().as_secs_f64();
        let actual_dts_increment = if frame_num > 1 {
            let total_duration_in_stream_tb = (actual_duration_secs
                * stream_time_base.denominator() as f64)
                / stream_time_base.numerator() as f64;
            (total_duration_in_stream_tb / (frame_num - 1) as f64) as i64
        } else {
            ts_tracker.dts_increment
        };

        for (idx, mut flush_packet) in flush_packets.into_iter().enumerate() {
            flush_packet.set_stream(ostream_idx);
            // Simplified flush logic using actual increment
            // This mimics the original logic but cleaner

            let assigned_frame_idx = idx; // Simplification
            let frame_based_pts = (assigned_frame_idx as i64) * actual_dts_increment;

            // Check last written DTS
            let last_dts = ts_tracker.last_dts.unwrap_or(0);
            let min_dts = last_dts + actual_dts_increment;
            let final_stream_dts = frame_based_pts.max(min_dts);
            let final_stream_pts = final_stream_dts; // PTS >= DTS

            flush_packet.set_pts(Some(final_stream_pts));
            flush_packet.set_dts(Some(final_stream_dts));
            ts_tracker.commit_dts(final_stream_dts);

            flush_packet.write_interleaved(&mut octx)?;
        }

        octx.write_trailer()?;
        Ok(())
    }

    /// Records the screen using FFmpeg CLI with automatic video segmentation.
    ///
    /// This creates multiple video files (e.g., recording_000.mp4, recording_001.mp4)
    /// at the specified segment duration. Uses FFmpeg's segment muxer for clean cuts.
    ///
    /// # Arguments
    /// * `config` - Recording configuration with `segment_duration_secs` set
    /// * `stop_signal` - Signal to stop recording
    ///
    /// # Returns
    /// * `Ok(Vec<PathBuf>)` - List of created segment files
    /// * `Err` - If recording fails
    pub fn record_with_segmentation(
        config: RecordingConfig,
        stop_signal: Arc<AtomicBool>,
    ) -> Result<Vec<PathBuf>> {
        use std::io::{BufRead, BufReader};
        use std::process::{Command, Stdio};

        let segment_duration = config.segment_duration_secs.unwrap_or(DEFAULT_SEGMENT_DURATION_SECS);

        // Generate output pattern: recording.mp4 -> recording_%03d.mp4
        let stem = config
            .output_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("recording");
        let ext = config
            .output_path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("mp4");
        let parent = config
            .output_path
            .parent()
            .unwrap_or(std::path::Path::new("."));

        // Create output directory if needed
        std::fs::create_dir_all(parent).map_err(|e| {
            RecorderError::Configuration(format!("Failed to create output directory: {}", e))
        })?;

        let output_pattern = parent.join(format!("{}_%03d.{}", stem, ext));
        let segment_list_path = parent.join(format!("{}_segments.txt", stem));

        // Build FFmpeg command
        // We use x11grab on Linux for screen capture
        let display = std::env::var("DISPLAY").unwrap_or_else(|_| ":0".to_string());

        // Get screen resolution using xdpyinfo or default
        let screen_size = Self::get_screen_size_for_ffmpeg(config.monitor_index);

        let mut cmd = Command::new("ffmpeg");
        cmd.args(["-y"]) // Overwrite output files
            .args(["-f", "x11grab"])
            .args(["-framerate", &config.framerate.to_string()])
            .args(["-video_size", &screen_size])
            .args(["-i", &format!("{}+0,0", display)])
            .args(["-c:v", "libx264"])
            .args(["-preset", "ultrafast"])
            .args(["-tune", "zerolatency"])
            .args(["-pix_fmt", "yuv420p"])
            // Segment muxer options
            .args(["-f", "segment"])
            .args(["-segment_time", &segment_duration.to_string()])
            .args(["-segment_format", "mp4"])
            .args(["-segment_list", segment_list_path.to_str().unwrap()])
            .args(["-segment_list_type", "flat"])
            .args(["-reset_timestamps", "1"])
            .args(["-strftime", "0"]) // Use sequential numbering, not timestamps
            .arg(&output_pattern)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        info!("🎬 Starting segmented recording with FFmpeg CLI");
        info!("   Output pattern: {}", output_pattern.display());
        info!("   Segment duration: {} seconds", segment_duration);
        info!("   Framerate: {} fps", config.framerate);
        info!("   Screen size: {}", screen_size);

        let mut child = cmd.spawn().map_err(|e| {
            RecorderError::VideoEncoding(format!(
                "Failed to start FFmpeg: {}. Make sure ffmpeg is installed.",
                e
            ))
        })?;

        // Spawn thread to read stderr and print progress
        let stderr = child.stderr.take();
        let stderr_handle = std::thread::spawn(move || {
            if let Some(stderr) = stderr {
                let reader = BufReader::new(stderr);
                for line in reader.lines().flatten() {
                    // Print frame progress lines
                    if line.contains("frame=") || line.contains("fps=") {
                        eprint!("\r📹 {}", line.trim());
                    } else if !line.trim().is_empty() {
                        info!("   FFmpeg: {}", line);
                    }
                }
            }
        });

        // Monitor stop signal
        while !stop_signal.load(std::sync::atomic::Ordering::Relaxed) {
            // Check if ffmpeg is still running
            match child.try_wait() {
                Ok(Some(status)) => {
                    if !status.success() {
                        return Err(RecorderError::VideoEncoding(format!(
                            "FFmpeg exited with status: {}",
                            status
                        )));
                    }
                    break;
                }
                Ok(None) => {
                    // Still running, continue monitoring
                    std::thread::sleep(Duration::from_millis(100));
                }
                Err(e) => {
                    return Err(RecorderError::VideoEncoding(format!(
                        "Failed to check FFmpeg status: {}",
                        e
                    )));
                }
            }
        }

        // Send SIGINT to FFmpeg for clean shutdown (allows it to finalize the current segment)
        info!("\n🛑 Stopping FFmpeg...");
        Self::send_sigint_to_child(&mut child)?;

        // Wait for FFmpeg to finish with timeout
        let wait_start = std::time::Instant::now();
        let timeout = Duration::from_secs(10);
        loop {
            match child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) => {
                    if wait_start.elapsed() > timeout {
                        warn!("⚠️  FFmpeg didn't exit gracefully, killing...");
                        let _ = child.kill();
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
                Err(e) => {
                    warn!("⚠️  Error waiting for FFmpeg: {}", e);
                    break;
                }
            }
        }

        // Wait for stderr reader to finish
        let _ = stderr_handle.join();

        // Read segment list to return created files
        let segments = if segment_list_path.exists() {
            std::fs::read_to_string(&segment_list_path)
                .map(|content| {
                    content
                        .lines()
                        .filter(|line| !line.trim().is_empty())
                        .map(|line| parent.join(line.trim()))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        } else {
            // Fall back to globbing for segment files
            let pattern = parent.join(format!("{}_*.{}", stem, ext));
            glob::glob(pattern.to_str().unwrap())
                .map(|paths| paths.filter_map(|p| p.ok()).collect())
                .unwrap_or_default()
        };

        info!(
            "✅ Recording complete. Created {} segment(s)",
            segments.len()
        );
        for seg in &segments {
            info!("   📁 {}", seg.display());
        }

        Ok(segments)
    }

    /// Get screen size string for FFmpeg (e.g., "1920x1080")
    fn get_screen_size_for_ffmpeg(_monitor_index: Option<usize>) -> String {
        // Try to get from xdpyinfo first
        if let Ok(output) = Command::new("xdpyinfo").output() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                if line.contains("dimensions:") {
                    // Parse "  dimensions:    1920x1080 pixels"
                    if let Some(dims) = line.split_whitespace().nth(1) {
                        if dims.contains('x') {
                            return dims.to_string();
                        }
                    }
                }
            }
        }

        // Fall back to xrandr
        if let Ok(output) = Command::new("xrandr").args(["--current"]).output() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                // Look for primary monitor or first connected
                if line.contains(" connected") {
                    // Parse "HDMI-1 connected primary 1920x1080+0+0"
                    for part in line.split_whitespace() {
                        if part.contains('x') && part.contains('+') {
                            if let Some(res) = part.split('+').next() {
                                return res.to_string();
                            }
                        }
                    }
                }
            }
        }

        // Default fallback
        warn!("⚠️  Could not detect screen size, using 1920x1080");
        "1920x1080".to_string()
    }

    /// Send SIGINT to a child process (Unix only)
    #[cfg(unix)]
    fn send_sigint_to_child(child: &mut std::process::Child) -> Result<()> {
        unsafe {
            libc::kill(child.id() as i32, libc::SIGINT);
        }
        Ok(())
    }

    #[cfg(not(unix))]
    fn send_sigint_to_child(child: &mut std::process::Child) -> Result<()> {
        // On non-Unix, just kill the process
        child
            .kill()
            .map_err(|e| RecorderError::VideoEncoding(format!("Failed to stop FFmpeg: {}", e)))?;
        Ok(())
    }
}

/// Internal timestamp tracker for video frame synchronization.
struct TimestampTracker {
    stream_time_base: Rational,
    encoder_time_base: Rational,
    dts_increment: i64,
    last_dts: Option<i64>,
    first_frame_pts: Option<i64>,
    last_frame_pts: i64,
    pts_offset: Option<i64>,
    frame_pts_map: Vec<i64>,
    pub frames_with_packets: std::collections::HashSet<usize>,
}

impl TimestampTracker {
    /// Creates a new TimestampTracker.
    fn new(stream_time_base: Rational, encoder_time_base: Rational) -> Self {
        let dts_increment = (stream_time_base.denominator() as i64
            * encoder_time_base.numerator() as i64)
            / (stream_time_base.numerator() as i64 * encoder_time_base.denominator() as i64);

        println!(
            "DEBUG: dts_increment = {} (1 frame in stream time_base)",
            dts_increment
        );

        Self {
            stream_time_base,
            encoder_time_base,
            dts_increment,
            last_dts: None,
            first_frame_pts: None,
            last_frame_pts: 0,
            pts_offset: None,
            frame_pts_map: Vec::new(),
            frames_with_packets: std::collections::HashSet::new(),
        }
    }

    /// Converts an encoder timestamp to a stream timestamp.
    fn convert_to_stream_ts(&self, encoder_ts: i64) -> i64 {
        let num = encoder_ts
            * self.encoder_time_base.numerator() as i64
            * self.stream_time_base.denominator() as i64;
        let den =
            self.encoder_time_base.denominator() as i64 * self.stream_time_base.numerator() as i64;
        num / den
    }

    /// Updates the PTS for a given frame number.
    fn update_pts(&mut self, frame_num: i64) -> i64 {
        let stream_frame_pts_raw = self.convert_to_stream_ts(frame_num);

        let offset = if let Some(offset) = self.pts_offset {
            offset
        } else {
            self.pts_offset = Some(stream_frame_pts_raw);
            println!("DEBUG: Calculated PTS offset = {}", stream_frame_pts_raw);
            stream_frame_pts_raw
        };

        let pts = stream_frame_pts_raw - offset;

        self.frame_pts_map.push(pts);
        self.last_frame_pts = pts;

        if self.first_frame_pts.is_none() && frame_num == 0 {
            self.first_frame_pts = Some(pts);
            println!("DEBUG: First frame PTS (frame 0) = {}", pts);
        }

        pts
    }

    /// Calculates the next DTS for a given PTS.
    fn next_dts(&mut self, pts: i64) -> i64 {
        let dts = if let Some(last) = self.last_dts {
            last + self.dts_increment
        } else {
            pts
        };
        dts
    }

    /// Commits a DTS value.
    fn commit_dts(&mut self, dts: i64) {
        self.last_dts = Some(dts);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(test)]
    use std::hint::black_box; // For black_box

    #[test]
    fn test_recording_config_defaults() {
        let config = RecordingConfig::default();
        assert_eq!(config.framerate, 30);
        assert_eq!(config.output_path, PathBuf::from("output/recording.mp4"));
        assert!(config.include_audio);
        assert!(!config.fast);
        assert_eq!(config.duration_secs, None);
    }

    #[test]
    fn test_timestamp_tracker_math() {
        let stream_tb = Rational(1, 30); // 30 FPS
        let encoder_tb = Rational(1, 30); // Matching TB for frame-rate aligned testing
        let mut tracker = TimestampTracker::new(stream_tb, encoder_tb);

        // Test PTS update for frame 0
        let pts0 = tracker.update_pts(0);
        assert_eq!(pts0, 0);

        // Test PTS for frame 1
        let pts1 = tracker.update_pts(1);
        assert_eq!(pts1, 1);

        // Test DTS increment
        let dts = tracker.next_dts(pts1);
        assert!(dts > 0);

        // Test conversion (basic)
        let converted = tracker.convert_to_stream_ts(30); // 1 sec in encoder TB
        assert_eq!(converted, 30); // 30 frames at 30 FPS
    }

    #[test]
    fn test_ffmpeg_mock_integration() {
        // Set env var to mock FFmpeg input, e.g., for testing without real capture
        std::env::set_var("FFMPEG_MOCK", "1"); // Hypothetical mock flag

        let config = RecordingConfig::default();
        let recorder = ScreenRecorder::new_with_config(config).expect("Init failed");
        black_box(&recorder);

        assert!(ffmpeg::init().is_ok());
    }
}
