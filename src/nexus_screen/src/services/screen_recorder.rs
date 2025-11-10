use anyhow::{self, Result};
use ffmpeg::{
    codec,
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

use super::window_info::{WindowInfo, WindowInfoService};

#[derive(Clone, Debug)]
pub struct RecordingConfig {
    pub framerate: u32,
    pub duration_secs: Option<u64>,
    pub output_path: PathBuf,
    pub monitor_index: Option<usize>,
    pub window_id: Option<String>, // Record a specific window by ID
    pub window_title: Option<String>, // Record a specific window by title pattern
    pub include_audio: bool,
    pub fast: bool, // Capture as fast as possible, ignore target FPS
}

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
    fn default() -> Self {
        Self {
            framerate: 30,
            duration_secs: None,
            output_path: PathBuf::from("recording.mp4"),
            monitor_index: None,
            window_id: None,
            window_title: None,
            include_audio: true,
            fast: false,
        }
    }
}

pub struct ScreenRecorder {
    _width: u32,
    _height: u32,
    _config: RecordingConfig,
}

impl ScreenRecorder {
    pub fn new() -> Result<Self> {
        let config = RecordingConfig::default();
        Self::new_with_config(config)
    }

    pub fn new_with_config(config: RecordingConfig) -> Result<Self> {
        // For now, we'll get dimensions when we start recording
        // Default to common resolution - will be updated from FFmpeg input
        Ok(Self {
            _width: 1920,
            _height: 1080,
            _config: config,
        })
    }

    /// List all available monitors/displays
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

    /// Get windows on a specific monitor
    /// Uses window geometry to determine which windows are on the monitor
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

    /// Get the active window information
    pub fn get_active_window() -> Result<Option<WindowInfo>> {
        WindowInfoService::get_active_window()
    }

    #[cfg(target_os = "linux")]
    fn list_monitors_linux() -> Result<Vec<MonitorInfo>> {
        // Use xrandr to enumerate monitors
        let output = Command::new("xrandr")
            .arg("--listmonitors")
            .output()
            .map_err(|e| anyhow::anyhow!("Failed to run xrandr: {}. Is xrandr installed?", e))?;

        if !output.status.success() {
            return Err(anyhow::anyhow!(
                "xrandr command failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
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
                .map_err(|e| anyhow::anyhow!("Failed to run xrandr --query: {}", e))?;

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
                                    let ox = offsets.get(0).and_then(|x| x.parse::<i32>().ok());
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
            .output();

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
                                        is_primary: device_index == 1, // Typically index 1 is primary
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
                return Err(anyhow::anyhow!(
                    "Failed to enumerate monitors using FFmpeg: {}. \
                    Make sure ffmpeg is installed and has avfoundation support.",
                    e
                ));
            }
        }

        // If no monitors found, provide at least one default
        if monitors.is_empty() {
            monitors.push(MonitorInfo {
                index: 0,
                name: "1".to_string(),
                display_name: "Default Screen (1)".to_string(),
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
        window_info: Option<&WindowInfo>,
        fps: u32,
    ) -> Result<(String, String, Vec<(String, String)>)> {
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
            let (offset_x, offset_y, video_size_str) = if let Some(window) = window_info {
                if let Some(geom) = &window.geometry {
                    let size = format!("{}x{}", geom.width, geom.height);
                    (geom.x, geom.y, Some(size))
                } else {
                    return Err(anyhow::anyhow!(
                        "Window geometry not available for window: {}",
                        window.window_id
                    ));
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
                            eprintln!("WARNING: Monitor index {} not found", idx);
                            (None, 0, 0)
                        }
                    } else {
                        eprintln!("WARNING: Failed to list monitors");
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
                eprintln!("WARNING: Could not extract video_size for monitor");
            }

            Ok(("x11grab".to_string(), url, options))
        }

        #[cfg(target_os = "macos")]
        {
            // macOS: use avfoundation
            // Format: avfoundation -i "device_index:audio_index" -framerate fps
            // For screen capture, device_index is typically 1 (screen), audio_index can be :none or a number
            let device_index = monitor_index
                .map(|i| i.to_string())
                .unwrap_or_else(|| "1".to_string());
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

    pub fn capture_screenshot_to_file(&self, output_path: &str) -> Result<()> {
        self.capture_screenshot_to_file_with_monitor(output_path, None)
    }

    pub fn capture_screenshot_to_file_with_monitor(
        &self,
        output_path: &str,
        monitor_index: Option<usize>,
    ) -> Result<()> {
        self.capture_screenshot_to_file_with_window(output_path, monitor_index, None)
    }

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

        // Open input
        let ctx = format::open_with(&input_url, &input_format, dict)
            .map_err(|e| anyhow::anyhow!("Failed to open input '{}': {:?}", input_url, e))?;

        // Extract input context
        let mut ictx = match ctx {
            format::Context::Input(ictx) => ictx,
            _ => return Err(anyhow::anyhow!("Expected input context")),
        };

        let input_stream = ictx
            .streams()
            .best(Type::Video)
            .ok_or_else(|| anyhow::anyhow!("No video stream found"))?;
        let input_stream_index = input_stream.index();

        // Get decoder
        let codec_ctx = input_stream.codec();
        let decoder_result = codec_ctx.decoder();
        let mut decoder = decoder_result.video()?;

        let width = decoder.width();
        let height = decoder.height();
        let input_pixel_format = decoder.format();

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
        .map_err(|e| anyhow::anyhow!("Failed to create scaler: {}", e))?;

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
                        return Err(anyhow::anyhow!("Failed to decode frame: {:?}", e));
                    }
                }
            }
        }

        if !got_frame {
            return Err(anyhow::anyhow!("Failed to capture frame from screen"));
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
        img.save(output_path)
            .map_err(|e| anyhow::anyhow!("Failed to save image to {}: {}", output_path, e))?;

        Ok(())
    }

    pub fn record(&self, config: RecordingConfig, stop_signal: Arc<AtomicBool>) -> Result<()> {
        let fps = config.framerate;
        let frame_interval = Duration::from_nanos(1_000_000_000u64 / fps as u64);
        let end_time = config
            .duration_secs
            .map(|secs| Instant::now() + Duration::from_secs(secs));

        // Setup FFmpeg output
        let mut octx = format::output(&config.output_path)?;
        let codec = encoder::find(codec::Id::H264)
            .ok_or_else(|| anyhow::anyhow!("No H.264 encoder available"))?;
        let stream = octx.add_stream(codec)?;

        // Resolve window info if window_id or window_title is specified
        let window_info = if let Some(ref window_id) = config.window_id {
            WindowInfoService::get_window_by_id(window_id)?
        } else if let Some(ref window_title) = config.window_title {
            let windows = WindowInfoService::get_windows_by_title(window_title)?;
            windows.first().cloned()
        } else {
            None
        };

        // Setup FFmpeg input for screen capture
        let (input_format_name, input_url, input_options) =
            Self::get_input_format_and_url(config.monitor_index, window_info.as_ref(), fps)?;
        println!(
            "Using input format: {}, URL: {}",
            input_format_name, input_url
        );
        if !input_options.is_empty() {
            println!("Input options: {:?}", input_options);
        }

        // Find the input format using device iterator (working approach from Test 15)
        let input_format = input::video()
            .find(|f| {
                if let ffmpeg::Format::Input(input) = f {
                    input.name() == input_format_name
                } else {
                    false
                }
            })
            .ok_or_else(|| anyhow::anyhow!("Input format '{}' not found. Make sure FFmpeg was compiled with support for this format.", input_format_name))?;

        // Convert options to Dictionary
        let mut dict = Dictionary::new();
        for (key, value) in &input_options {
            dict.set(key, value);
        }

        // Use format::open_with() to pass options (framerate, video_size, etc.)
        let ctx = format::open_with(&input_url, &input_format, dict).map_err(|e| {
            anyhow::anyhow!(
                "Failed to open input '{}' with format '{}': {:?}",
                input_url,
                input_format_name,
                e
            )
        })?;

        // Extract input context from the format context
        let mut ictx = match ctx {
            format::Context::Input(ictx) => ictx,
            _ => {
                return Err(anyhow::anyhow!(
                    "Expected input context, got output context"
                ))
            }
        };

        let input_stream = ictx
            .streams()
            .best(Type::Video)
            .ok_or_else(|| anyhow::anyhow!("No video stream found in input"))?;
        let input_stream_index = input_stream.index();

        // Get decoder
        let codec_ctx = input_stream.codec();
        let decoder_result = codec_ctx.decoder();
        let mut decoder = decoder_result.video()?;

        // Get actual dimensions from input stream
        let raw_width = decoder.width();
        let raw_height = decoder.height();
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

        let ostream_idx = stream.index();
        let mut encoder_ctx = stream.codec().encoder().video()?;
        encoder_ctx.set_width(width);
        encoder_ctx.set_height(height);
        encoder_ctx.set_format(Pixel::YUV420P);

        // Set time_base for proper timestamp handling
        // Use a standard time_base that works well with MP4 container
        // MP4 typically uses 1/90000 or 1/1000, but we'll use 1/fps for encoder
        // and let FFmpeg handle conversion to stream time_base
        let encoder_time_base = Rational(1, fps as i32);
        encoder_ctx.set_time_base(encoder_time_base);
        encoder_ctx.set_frame_rate(Some(Rational(fps as i32, 1)));

        // Set encoder options for immediate packet output
        encoder_ctx.set_max_b_frames(0); // No B-frames for lower latency
        encoder_ctx.set_gop(1); // Force every frame to be a keyframe for immediate output

        // Set x264-specific options via parameters
        unsafe {
            use ffmpeg::ffi::*;
            let ctx_ptr = encoder_ctx.as_mut_ptr();
            (*ctx_ptr).flags |= AV_CODEC_FLAG_LOW_DELAY as i32;
            (*ctx_ptr).flags2 |= AV_CODEC_FLAG2_FAST as i32;
        }

        let mut video_encoder = encoder_ctx.open_as(codec)?;

        // Write header to finalize stream setup
        // Note: We need to drop the stream reference before this mutable borrow
        octx.write_header()?;

        // Get stream time_base after writing header (access stream through octx)
        let stream_time_base = octx
            .stream(ostream_idx)
            .ok_or_else(|| anyhow::anyhow!("Stream {} not found", ostream_idx))?
            .time_base();
        println!(
            "Encoder time_base: {:?}, Stream time_base: {:?}",
            encoder_time_base, stream_time_base
        );
        println!(
            "DEBUG: encoder_tb = {}/{} = {:.6}",
            encoder_time_base.numerator(),
            encoder_time_base.denominator(),
            encoder_time_base.numerator() as f64 / encoder_time_base.denominator() as f64
        );
        println!(
            "DEBUG: stream_tb = {}/{} = {:.6}",
            stream_time_base.numerator(),
            stream_time_base.denominator(),
            stream_time_base.numerator() as f64 / stream_time_base.denominator() as f64
        );

        // Helper function to convert timestamp from encoder time_base to stream time_base
        // Formula: stream_ts = encoder_ts * (encoder_tb.num * stream_tb.den) / (encoder_tb.den * stream_tb.num)
        let convert_to_stream_ts = |encoder_ts: i64| -> i64 {
            let num = encoder_ts as i64
                * encoder_time_base.numerator() as i64
                * stream_time_base.denominator() as i64;
            let den = encoder_time_base.denominator() as i64 * stream_time_base.numerator() as i64;
            let result = num / den;
            if encoder_ts < 5 {
                println!(
                    "DEBUG convert: encoder_ts={}, num={}, den={}, result={}",
                    encoder_ts, num, den, result
                );
            }
            result
        };

        // Input frames from FFmpeg will be in the decoder's format
        // We may need to scale if dimensions don't match or format differs
        let input_pixel_format = decoder.format();
        let mut input_frame = Video::new(input_pixel_format, raw_width, raw_height);
        let mut scaled_frame = Video::new(Pixel::YUV420P, width, height);

        // Only create scaler if we need to convert format or resize
        let needs_scaling =
            input_pixel_format != Pixel::YUV420P || raw_width != width || raw_height != height;
        let mut scaler = if needs_scaling {
            Some(
                Scaler::get(
                    input_pixel_format,
                    raw_width,
                    raw_height,
                    Pixel::YUV420P,
                    width,
                    height,
                    Flags::BILINEAR,
                )
                .map_err(|e| anyhow::anyhow!("Scaler init failed: {}", e))?,
            )
        } else {
            None
        };

        let mut frame_num: i64 = 0;
        let mut last_dts: Option<i64> = None; // Track last DTS in stream time_base to ensure monotonicity
        let mut first_frame_pts: Option<i64> = None; // Track the first frame PTS for duration calculation
        let mut last_frame_pts: i64 = 0; // Track the PTS of the last frame in stream time_base for duration calculation
        let mut pts_offset: Option<i64> = None; // Offset to ensure first PTS is 0
        let mut frame_pts_map: Vec<i64> = Vec::new(); // Track PTS for each frame index
        let mut frames_with_packets: std::collections::HashSet<usize> =
            std::collections::HashSet::new(); // Track which frames have produced packets
        let start_time = Instant::now();

        // Calculate DTS increment in stream time_base (1 frame in encoder time_base)
        // This is: 1 * (stream_tb.den * encoder_tb.num) / (stream_tb.num * encoder_tb.den)
        let dts_increment = (stream_time_base.denominator() as i64
            * encoder_time_base.numerator() as i64)
            / (stream_time_base.numerator() as i64 * encoder_time_base.denominator() as i64);
        println!(
            "DEBUG: dts_increment = {} (1 frame in stream time_base)",
            dts_increment
        );

        // Calculate expected duration for debugging
        let calculate_expected_duration = |total_frames: i64| -> f64 {
            let duration_in_encoder_tb = total_frames as f64 / fps as f64;
            let duration_in_seconds = duration_in_encoder_tb;
            duration_in_seconds
        };

        loop {
            let loop_start = Instant::now();

            if let Some(et) = &end_time {
                if Instant::now() >= *et {
                    println!("Duration limit reached, stopping...");
                    break;
                }
            }
            if stop_signal.load(std::sync::atomic::Ordering::Relaxed) {
                println!("Stop signal received, breaking loop...");
                break;
            }

            // Read frame from FFmpeg input
            let capture_start = Instant::now();

            // Use packets() iterator to read from input
            // For live capture, we need to read one packet at a time
            let mut got_frame = false;
            for (stream, pkt) in ictx.packets() {
                if stream.index() == input_stream_index {
                    // Decode the packet into a frame
                    decoder.send_packet(&pkt)?;
                    match decoder.receive_frame(&mut input_frame) {
                        Ok(()) => {
                            // Got a frame!
                            got_frame = true;
                            break;
                        }
                        Err(ffmpeg::Error::Other { errno: -11 }) => {
                            // EAGAIN - need more packets, continue reading
                            continue;
                        }
                        Err(e) => {
                            eprintln!("Failed to decode frame {}: {:?}", frame_num, e);
                            if !config.fast {
                                std::thread::sleep(frame_interval);
                            }
                            continue;
                        }
                    }
                }
            }

            if !got_frame {
                // No frame available yet, skip this iteration
                if !config.fast {
                    std::thread::sleep(frame_interval);
                }
                continue;
            }

            let capture_elapsed = capture_start.elapsed();

            // Scale/convert frame if needed
            let scale_start = Instant::now();
            if needs_scaling {
                if let Some(ref mut s) = scaler {
                    s.run(&input_frame, &mut scaled_frame)?;
                }
                scaled_frame.set_pts(Some(frame_num));
            } else {
                input_frame.set_pts(Some(frame_num));
            }
            let scale_elapsed = scale_start.elapsed();

            // Get reference to the frame we'll encode
            let frame_to_encode = if needs_scaling {
                &scaled_frame
            } else {
                &input_frame
            };

            // Encode frame
            let encode_start = Instant::now();
            video_encoder.send_frame(frame_to_encode)?;

            // Calculate PTS and DTS in stream time_base
            // PTS represents when the frame should be displayed
            let stream_frame_pts_raw = convert_to_stream_ts(frame_num);
            // Ensure first frame PTS is 0 for proper duration calculation
            // Calculate offset on first frame, then use it for all frames
            let offset = if let Some(offset) = pts_offset {
                offset
            } else {
                let offset = stream_frame_pts_raw; // First frame's raw PTS becomes the offset
                pts_offset = Some(offset);
                println!(
                    "DEBUG: Calculated PTS offset = {} (first frame raw PTS)",
                    offset
                );
                offset
            };
            let stream_frame_pts = stream_frame_pts_raw - offset;

            // Track this frame's PTS even if no packets are produced yet
            frame_pts_map.push(stream_frame_pts);
            last_frame_pts = stream_frame_pts; // Always update to track the last frame's PTS

            // Track first frame PTS (should be 0) - set it on the first frame, not first packet
            if first_frame_pts.is_none() && frame_num == 0 {
                first_frame_pts = Some(stream_frame_pts);
                println!(
                    "DEBUG: First frame PTS (frame 0) = {} (in stream time_base, should be 0)",
                    stream_frame_pts
                );
                if stream_frame_pts != 0 {
                    eprintln!("WARNING: First frame PTS is not 0! This may cause duration issues.");
                }
            }

            // DTS represents when the frame should be decoded (must be monotonic)
            // For the first frame, use the PTS; for subsequent frames, increment from last DTS
            // Note: last_dts is already in offset-adjusted stream time_base
            let stream_frame_dts = if let Some(last) = last_dts {
                last + dts_increment
            } else {
                stream_frame_pts // First frame: DTS = PTS (both should be 0)
            };

            let mut packet = Packet::empty();
            let mut packet_count = 0;
            loop {
                match video_encoder.receive_packet(&mut packet) {
                    Ok(()) => {
                        packet.set_stream(ostream_idx);
                        // All packets from the same frame should have the same PTS and DTS
                        // Convert from encoder time_base to stream time_base
                        // first_frame_pts is already set above on frame 0

                        packet.set_pts(Some(stream_frame_pts));
                        packet.set_dts(Some(stream_frame_dts));
                        last_dts = Some(stream_frame_dts);
                        frames_with_packets.insert(frame_num as usize); // Mark this frame as having produced packets
                                                                        // last_frame_pts already updated above

                        // Debug: log timestamps for first few frames to diagnose issues
                        if frame_num < 5 || packet_count == 0 {
                            let pts_seconds = stream_frame_pts as f64
                                * stream_time_base.numerator() as f64
                                / stream_time_base.denominator() as f64;
                            let dts_seconds = stream_frame_dts as f64
                                * stream_time_base.numerator() as f64
                                / stream_time_base.denominator() as f64;
                            println!("Frame {} packet {}: encoder_pts={}, stream_pts={} ({:.3}s), stream_dts={} ({:.3}s)", 
                                frame_num, packet_count, frame_num, stream_frame_pts, pts_seconds, stream_frame_dts, dts_seconds);
                        }
                        packet.write_interleaved(&mut octx)?;
                        packet_count += 1;
                        // Continue to get more packets if available (all from same frame)
                    }
                    Err(ffmpeg::Error::Other { errno: -11 }) | Err(ffmpeg::Error::Eof) => {
                        // EAGAIN or EOF - no more packets available right now
                        break;
                    }
                    Err(e) => {
                        if frame_num < 3 {
                            eprintln!("receive_packet error on frame {}: {:?}", frame_num, e);
                        }
                        break;
                    }
                }
            }
            let encode_elapsed = encode_start.elapsed();

            if frame_num % 10 == 0 || frame_num < 5 {
                let loop_elapsed = loop_start.elapsed();
                println!(
                    "Frame {}: capture={:?}, scale={:?}, encode={:?}, total={:?}, packets={}",
                    frame_num,
                    capture_elapsed,
                    scale_elapsed,
                    encode_elapsed,
                    loop_elapsed,
                    packet_count
                );
                log::info!(
                    "Frame {} encoded in {:?}, packets={}",
                    frame_num,
                    loop_elapsed,
                    packet_count
                );
            }

            if packet_count == 0 && frame_num < 5 {
                eprintln!("WARNING: Frame {} produced no packets!", frame_num);
            }

            // Periodically flush encoder to force packet output
            if frame_num > 0 && frame_num % 5 == 0 {
                // Try to flush any buffered packets
                let mut flush_packet = Packet::empty();
                let mut flushed = 0;
                loop {
                    match video_encoder.receive_packet(&mut flush_packet) {
                        Ok(()) => {
                            flush_packet.set_stream(ostream_idx);
                            // Ensure flush packets have monotonic timestamps in stream time_base
                            let final_stream_dts = if let Some(last) = last_dts {
                                last + dts_increment // Increment by one frame period
                            } else {
                                stream_frame_pts // Shouldn't happen, but use current frame PTS
                            };
                            // PTS must be >= DTS, use current frame PTS
                            let final_stream_pts = stream_frame_pts.max(final_stream_dts);
                            flush_packet.set_pts(Some(final_stream_pts));
                            flush_packet.set_dts(Some(final_stream_dts));
                            last_dts = Some(final_stream_dts);
                            flush_packet.write_interleaved(&mut octx)?;
                            flushed += 1;
                        }
                        Err(_) => break,
                    }
                }
                if flushed > 0 && frame_num < 10 {
                    println!("Flushed {} packets after frame {}", flushed, frame_num);
                }
            }

            frame_num += 1;

            // Maintain target FPS - sleep if we have time left in this frame period
            // Skip sleep if we're already behind (can't catch up anyway) or if fast mode is enabled
            let frame_elapsed = loop_start.elapsed();
            if !config.fast && frame_elapsed < frame_interval {
                std::thread::sleep(frame_interval - frame_elapsed);
            } else if !config.fast && frame_num <= 10 {
                // Only warn for first few frames to avoid spam (and only in non-fast mode)
                eprintln!(
                    "Warning: Frame {} took {:?}, target was {:?} (behind by {:?})",
                    frame_num - 1,
                    frame_elapsed,
                    frame_interval,
                    frame_elapsed - frame_interval
                );
            }
        }

        // frame_num is now the total count (was incremented after last frame)
        // The last encoded frame had PTS = frame_num - 1
        let last_frame_index = frame_num - 1;
        let actual_duration = start_time.elapsed();
        let actual_duration_secs = actual_duration.as_secs_f64();
        let expected_duration_secs = calculate_expected_duration(frame_num);

        // Calculate actual frame rate based on real capture time
        // This ensures the video duration matches the actual recording time
        let actual_fps = if frame_num > 0 && actual_duration_secs > 0.0 {
            frame_num as f64 / actual_duration_secs
        } else {
            fps as f64
        };

        println!(
            "Actual capture: {} frames in {:.3} seconds = {:.2} fps (target: {} fps)",
            frame_num, actual_duration_secs, actual_fps, fps
        );

        // Calculate the actual time increment per frame in stream time_base
        // This will space out frames to match the actual recording duration
        let actual_dts_increment = if frame_num > 1 {
            // Calculate increment based on actual duration
            // Total duration in stream time_base = actual_duration_secs / stream_time_base
            let total_duration_in_stream_tb = (actual_duration_secs
                * stream_time_base.denominator() as f64)
                / stream_time_base.numerator() as f64;
            // Increment per frame = total duration / (frame_count - 1)
            // We use frame_count - 1 because we have frame_count intervals between frame_count frames
            (total_duration_in_stream_tb / (frame_num - 1) as f64) as i64
        } else {
            dts_increment // Fallback to original increment
        };

        println!("DEBUG: Original dts_increment = {}, Actual dts_increment = {} (based on {:.3}s recording)", 
            dts_increment, actual_dts_increment, actual_duration_secs);
        // Get the actual last frame PTS from our tracking
        let actual_last_frame_pts =
            if last_frame_index >= 0 && (last_frame_index as usize) < frame_pts_map.len() {
                frame_pts_map[last_frame_index as usize]
            } else {
                last_frame_pts
            };

        println!(
            "Recording loop ended. Captured {} frames in {:?}",
            frame_num, actual_duration
        );
        println!(
            "Expected duration: {:.3} seconds ({} frames / {} fps)",
            expected_duration_secs, frame_num, fps
        );
        println!(
            "Last frame index: {}, Last frame PTS (tracked): {}",
            last_frame_index, actual_last_frame_pts
        );
        println!(
            "frame_pts_map length: {}, last_frame_pts variable: {}",
            frame_pts_map.len(),
            last_frame_pts
        );
        if let Some(first_pts) = first_frame_pts {
            let duration_in_stream_tb = actual_last_frame_pts - first_pts;
            let duration_seconds = duration_in_stream_tb as f64
                * stream_time_base.numerator() as f64
                / stream_time_base.denominator() as f64;
            println!("DEBUG: First PTS = {}, Last PTS (tracked) = {}, Duration in stream_tb = {}, Duration in seconds = {:.6}", 
                first_pts, actual_last_frame_pts, duration_in_stream_tb, duration_seconds);
        }

        // Flush encoder - send EOF
        println!("Flushing encoder...");
        video_encoder.send_eof()?;

        let mut packet = Packet::empty();
        let mut flush_count = 0;

        // During flush, collect all packets first, then assign PTS correctly
        // The last packet should have PTS = end of video (last frame PTS + one frame duration)
        let mut flush_packets: Vec<Packet> = Vec::new();
        while video_encoder.receive_packet(&mut packet).is_ok() {
            flush_packets.push(packet);
            packet = Packet::empty();
        }

        let total_flush_packets = flush_packets.len();
        println!(
            "DEBUG: Collected {} packets during flush",
            total_flush_packets
        );

        // Now assign PTS to each packet
        // The packets are from frames that were buffered (frames 0 to total_flush_packets-1)
        // We need to ensure DTS is monotonic and PTS matches the frame
        // DTS should be based on frame order, not packet write order
        for (idx, mut flush_packet) in flush_packets.into_iter().enumerate() {
            flush_packet.set_stream(ostream_idx);

            // Map packet index to frame index (packets are from buffered frames)
            let assigned_frame_idx = idx; // Packet idx corresponds to frame idx for buffered frames

            // Recalculate PTS based on actual recording time, not target frame rate
            // This ensures the video duration matches the actual recording time
            let flush_frame_pts = if assigned_frame_idx < frame_pts_map.len() {
                // Calculate PTS based on actual time spacing
                // PTS = frame_index * actual_dts_increment (starting from 0)
                (assigned_frame_idx as i64) * actual_dts_increment
            } else {
                // Fallback: calculate from frame index using actual increment
                (assigned_frame_idx as i64) * actual_dts_increment
            };

            // Calculate DTS - must be monotonic
            // Use actual_dts_increment to space frames according to real recording time
            // If we've already written packets from later frames, we need to ensure
            // flush packet DTS is >= the last written DTS
            let frame_based_dts = (assigned_frame_idx as i64) * actual_dts_increment;
            let final_stream_dts = if let Some(last_written_dts) = last_dts {
                // Check if this frame has already produced a packet
                if frames_with_packets.contains(&assigned_frame_idx) {
                    // This frame already produced a packet, use its DTS
                    // This shouldn't happen during flush, but handle it
                    last_written_dts + actual_dts_increment
                } else {
                    // This frame hasn't produced a packet yet
                    // Use frame-based DTS, but ensure it's >= last written DTS
                    // If frame-based DTS is less, it means this frame comes before frames we've already written
                    // In that case, we need to use a DTS that's >= last_written_dts
                    // But we also need to ensure PTS >= DTS
                    let min_dts = last_written_dts + actual_dts_increment;
                    frame_based_dts.max(min_dts)
                }
            } else {
                frame_based_dts // No packets written yet, use frame-based DTS
            };

            // Ensure PTS >= DTS (required by FFmpeg)
            // If DTS was adjusted to be > PTS (due to monotonicity), adjust PTS to match
            let adjusted_pts = flush_frame_pts.max(final_stream_dts);

            // For the last packet in the flush, we need to check if this is also the last frame of the video
            // If so, set PTS to represent the END of the video
            let is_last_flush_packet = idx == (total_flush_packets - 1);
            let is_actual_last_frame = assigned_frame_idx == (last_frame_index as usize);
            let is_last = is_last_flush_packet && is_actual_last_frame;

            let (final_stream_pts, final_stream_dts) = if is_last {
                // This is both the last packet in flush AND the last frame of the video
                // PTS should represent end of video (last frame PTS + one frame duration)
                // Use actual_dts_increment to match real recording time
                let last_frame_end_pts = adjusted_pts + actual_dts_increment;
                // For the last packet, set DTS to match PTS (or be very close)
                // This ensures FFmpeg uses the correct value for duration calculation
                // FFmpeg calculates stream duration from DTS, not PTS!
                let end_dts = last_frame_end_pts.max(final_stream_dts); // Ensure DTS <= PTS
                (last_frame_end_pts, end_dts)
            } else {
                // Regular packet: use adjusted PTS and calculated DTS
                // PTS must be >= DTS (already ensured above)
                (adjusted_pts, final_stream_dts)
            };

            flush_packet.set_pts(Some(final_stream_pts));
            flush_packet.set_dts(Some(final_stream_dts));
            last_dts = Some(final_stream_dts);
            last_frame_pts = final_stream_pts;

            println!(
                "DEBUG FLUSH: packet {}, frame_idx={}, pts={}, dts={}, is_last={}",
                idx, assigned_frame_idx, final_stream_pts, final_stream_dts, is_last
            );

            flush_packet.write_interleaved(&mut octx)?;
            flush_count += 1;
        }
        println!("Flushed {} packets from encoder, final PTS: {} (last_frame_index: {}, total_frames: {})", 
            flush_count, last_frame_pts, last_frame_index, frame_num);

        // Final duration calculation
        // Since we recalculated PTS during flush based on actual time, first PTS should be 0
        let final_first_pts = 0i64; // First frame always starts at 0 after recalculation
        let final_duration_in_stream_tb = last_frame_pts - final_first_pts;
        let final_duration_seconds = final_duration_in_stream_tb as f64
            * stream_time_base.numerator() as f64
            / stream_time_base.denominator() as f64;
        println!("DEBUG FINAL: First PTS = {}, Final PTS = {}, Duration in stream_tb = {}, Duration in seconds = {:.6}", 
            final_first_pts, last_frame_pts, final_duration_in_stream_tb, final_duration_seconds);
        println!(
            "DEBUG FINAL: Expected duration (frame-based) = {:.6} seconds ({} frames / {} fps)",
            expected_duration_secs, frame_num, fps
        );
        println!(
            "DEBUG FINAL: Actual recording duration = {:.6} seconds",
            actual_duration_secs
        );
        println!(
            "DEBUG FINAL: Video duration should match actual recording duration: {:.6} seconds",
            actual_duration_secs
        );

        // Before writing trailer, try to ensure stream duration is correct
        // The stream duration should be calculated from the last packet's PTS
        // But FFmpeg might be using the last frame's PTS instead
        // Let's verify by checking what the stream thinks its duration is
        if let Some(stream) = octx.stream(ostream_idx) {
            // Note: We can't directly set duration in ffmpeg-next, but we can ensure
            // the last packet's PTS is correct, which should make FFmpeg calculate it correctly
            println!(
                "DEBUG: Stream time_base before trailer: {:?}",
                stream.time_base()
            );
        }

        println!("Writing trailer...");
        octx.write_trailer()?;
        println!("Video finalized successfully!");

        Ok(())
    }
}
