use crate::error::{Result, ScreenError};
use xcap::Monitor;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

// Video encoding using ffmpeg-next
use ffmpeg_next as ffmpeg;

/// Screen recording configuration
#[derive(Debug, Clone)]
pub struct RecordingConfig {
    /// Frame rate in frames per second
    pub fps: u32,
    /// Duration to record (None = record until stopped)
    pub duration: Option<Duration>,
    /// Output file path
    pub output_path: std::path::PathBuf,
    /// Include audio in recording
    pub include_audio: bool,
    /// Monitor index to record (None = primary monitor)
    pub monitor_index: Option<usize>,
}

impl Default for RecordingConfig {
    fn default() -> Self {
        Self {
            fps: 60,
            duration: None,
            output_path: std::path::PathBuf::from("recording.mp4"),
            include_audio: true,
            monitor_index: None,
        }
    }
}

/// Screen recorder service
pub struct ScreenRecorder;

impl ScreenRecorder {
    /// Create a new screen recorder instance
    pub fn new() -> Result<Self> {
        // xcap doesn't require explicit permission checks
        // It will handle platform-specific requirements automatically
        Ok(Self)
    }

    /// Record screen to a file
    pub fn record_to_file(&self, config: RecordingConfig) -> Result<()> {
        // Get all monitors
        let monitors = Monitor::all()
            .map_err(|e| ScreenError::ScreenCapture(format!("Failed to get monitors: {}", e)))?;
        
        if monitors.is_empty() {
            return Err(ScreenError::ScreenCapture("No monitors found".to_string()).into());
        }
        
        // Select monitor based on config
        let monitor = if let Some(idx) = config.monitor_index {
            monitors.get(idx)
                .ok_or_else(|| ScreenError::ScreenCapture(
                    format!("Monitor index {} not found. Available monitors: 0-{}", 
                        idx, monitors.len() - 1)))?
        } else {
            // Use first monitor (typically the primary)
            &monitors[0]
        };

        let width_raw = monitor.width().map_err(|e| ScreenError::ScreenCapture(format!("Failed to get monitor width: {}", e)))?;
        let height_raw = monitor.height().map_err(|e| ScreenError::ScreenCapture(format!("Failed to get monitor height: {}", e)))?;
        
        // H.264 requires dimensions to be divisible by 2
        // Round down to nearest even number
        let width = (width_raw / 2) * 2;
        let height = (height_raw / 2) * 2;
        
        if width != width_raw || height != height_raw {
            log::info!(
                "Adjusted dimensions from {}x{} to {}x{} for H.264 encoding",
                width_raw, height_raw, width, height
            );
        }
        
        let monitor_info = if let Some(idx) = config.monitor_index {
            format!("Monitor {} ({}x{})", idx, width, height)
        } else {
            format!("Primary monitor ({}x{})", width, height)
        };
        
        log::info!(
            "Starting screen recording: {} fps, output: {}, {}",
            config.fps,
            config.output_path.display(),
            monitor_info
        );

        // Shared state for stopping recording
        let recording = Arc::new(AtomicBool::new(true));
        let recording_clone = Arc::clone(&recording);

        // Set up Ctrl+C handler if duration is not specified
        // Note: If a handler is already registered (e.g., by tokio), we'll skip setting one
        // and rely on the default behavior which will terminate the process
        if config.duration.is_none() {
            let _ = ctrlc::set_handler(move || {
                log::info!("Ctrl+C received, stopping recording...");
                recording_clone.store(false, Ordering::Relaxed);
            });
            // If handler registration fails (already set), that's okay - Ctrl+C will still work
        }

        // Initialize ffmpeg
        ffmpeg::init().map_err(|e| ScreenError::VideoEncoding(format!("Failed to initialize ffmpeg: {}", e)))?;

        // Set up video encoder
        let output_path_str = config.output_path.to_string_lossy().to_string();
        let mut output_context = ffmpeg::format::output(&output_path_str)
            .map_err(|e| ScreenError::VideoEncoding(format!("Failed to create output context: {}", e)))?;

        // Add video stream
        let codec = ffmpeg::encoder::find(ffmpeg::codec::Id::H264)
            .ok_or_else(|| ScreenError::VideoEncoding("H.264 codec not found".to_string()))?;
        
        let mut video_stream = output_context.add_stream(codec)
            .map_err(|e| ScreenError::VideoEncoding(format!("Failed to add video stream: {}", e)))?;

        let mut video_encoder_context = video_stream.codec().encoder().video()
            .map_err(|e| ScreenError::VideoEncoding(format!("Failed to get video encoder: {}", e)))?;

        video_encoder_context.set_width(width);
        video_encoder_context.set_height(height);
        // Set SAR to 1:1 for square pixels
        video_encoder_context.set_aspect_ratio(ffmpeg::util::rational::Rational::new(1, 1));
        // Don't set frame_rate - this allows the encoder to respect our explicit PTS values
        // Instead, we'll use the time_base and frame PTS to control timing
        video_encoder_context.set_time_base(ffmpeg::util::rational::Rational::new(1, 1000)); // Use milliseconds
        video_encoder_context.set_format(ffmpeg::format::Pixel::YUV420P);
        video_encoder_context.set_flags(ffmpeg::codec::flag::Flags::GLOBAL_HEADER);
        
        // Set encoding parameters optimized for screen recording
        video_encoder_context.set_bit_rate(10_000_000); // 10 Mbps for better quality
        video_encoder_context.set_max_b_frames(0); // Disable B-frames to reduce prediction artifacts
        
        // Open encoder with options for screen content
        let mut encoder_opts = ffmpeg::Dictionary::new();
        encoder_opts.set("preset", "ultrafast"); // Fast encoding, less compression
        encoder_opts.set("tune", "zerolatency"); // Optimize for low latency
        encoder_opts.set("profile", "main"); // Use main profile for compatibility
        // x264-specific options to respect input timing
        encoder_opts.set("x264opts", "keyint=250:min-keyint=25:scenecut=40"); // GOP settings for VFR

        let mut video_encoder_context = video_encoder_context.open_as_with(codec, encoder_opts)
            .map_err(|e| ScreenError::VideoEncoding(format!("Failed to open video encoder: {}", e)))?;

        // Get encoder time base and set stream time base
        let encoder_time_base = video_encoder_context.time_base();
        log::info!("Encoder time base: {}/{}", encoder_time_base.0, encoder_time_base.1);
        
        // Set stream time base to match encoder
        video_stream.set_time_base(encoder_time_base);
        
        // Set stream parameters before writing header
        video_stream.set_parameters(&video_encoder_context);
        
        
        // Get stream time base for packet rescaling
        let mut stream_time_base = video_stream.time_base();
        log::info!("Stream time base (pre-header): {}/{}", stream_time_base.0, stream_time_base.1);
        
        // Drop video_stream to release borrow on output_context
        drop(video_stream);
        
        // Write header
        output_context.write_header()
            .map_err(|e| ScreenError::VideoEncoding(format!("Failed to write header: {}", e)))?;

        // Query the actual stream time base after header write (muxer may adjust it)
        if let Some(stream) = output_context.stream(0) {
            let actual_time_base = stream.time_base();
            if actual_time_base != stream_time_base {
                log::info!(
                    "Muxer adjusted stream time base to {}/{}",
                    actual_time_base.0,
                    actual_time_base.1
                );
                stream_time_base = actual_time_base;
            }
        }

        // Create scaler to convert RGBA to YUV420P with high quality
        let mut scaler = ffmpeg::software::scaling::Context::get(
            ffmpeg::format::Pixel::RGBA,
            width,
            height,
            ffmpeg::format::Pixel::YUV420P,
            width,
            height,
            ffmpeg::software::scaling::flag::Flags::LANCZOS | ffmpeg::software::scaling::flag::Flags::ACCURATE_RND,
        ).map_err(|e| ScreenError::VideoEncoding(format!("Failed to create scaler: {}", e)))?;

        let start_time = Instant::now();
        let mut frame_count = 0u64;
        let frame_interval = Duration::from_secs_f64(1.0 / config.fps as f64);
        // Track the most recent captured frame PTS (in encoder timebase ticks)
        let mut last_captured_frame_pts_ticks: Option<i64> = None;
        // Track pending frame timestamps and durations for packet timestamp assignment (in encoder timebase ticks)
        let mut pending_frame_timings: std::collections::VecDeque<(i64, i64)> =
            std::collections::VecDeque::new();
        let mut last_packet_pts_stream_ticks: Option<i64> = None;
        let mut last_packet_duration_stream_ticks: i64 = 0;

        let encoder_tb_num = encoder_time_base.0 as i128;
        let encoder_tb_den = encoder_time_base.1 as i128;
        let stream_tb_num = stream_time_base.0 as i128;
        let stream_tb_den = stream_time_base.1 as i128;

        let duration_to_ticks = |duration: Duration| -> i64 {
            if encoder_tb_num == 0 {
                return 0;
            }

            let nanos = duration.as_nanos() as i128;
            ((nanos * encoder_tb_den) / (encoder_tb_num * 1_000_000_000)).max(0) as i64
        };

        let ticks_to_millis = |ticks: i64| -> f64 {
            if encoder_tb_den == 0 {
                return 0.0;
            }
            (ticks as f64 * encoder_tb_num as f64 / encoder_tb_den as f64) * 1000.0
        };

        let target_frame_duration_ticks = duration_to_ticks(frame_interval).max(1);
        let mut last_frame_duration_ticks = target_frame_duration_ticks;

        let stream_ticks_to_seconds = |ticks: i64| -> f64 {
            if stream_tb_den == 0 {
                return 0.0;
            }
            ticks as f64 * stream_tb_num as f64 / stream_tb_den as f64
        };

        let stream_ticks_to_millis = |ticks: i64| -> f64 { stream_ticks_to_seconds(ticks) * 1000.0 };

        // Recording loop
        loop {
            // Check if we should stop
            if !recording.load(Ordering::Relaxed) {
                log::info!("Recording stopped by user");
                break;
            }

            // Check duration limit
            if let Some(duration) = config.duration {
                if start_time.elapsed() >= duration {
                    log::info!("Recording duration limit reached");
                    break;
                }
            }

            // Capture a frame
            let frame_start = Instant::now();
            match monitor.capture_image() {
                Ok(image) => {
                    frame_count += 1;
                    
                    // Convert image to RGBA bytes - xcap returns ImageBuffer<Rgba<u8>, Vec<u8>>
                    // Access the underlying buffer
                    let rgba_data: &[u8] = image.as_raw();
                    
                    // Create input frame from RGBA data
                    // Allocate frame with proper format and dimensions
                    let mut input_frame = ffmpeg::frame::Video::empty();
                    input_frame.set_format(ffmpeg::format::Pixel::RGBA);
                    input_frame.set_width(width);
                    input_frame.set_height(height);
                    
                    // Allocate the frame buffer
                    unsafe {
                        input_frame.alloc(ffmpeg::format::Pixel::RGBA, width, height);
                    }
                    
                    // Copy RGBA data into the frame (mutable borrow ends after this block)
                    {
                        let linesize = input_frame.stride(0);
                        unsafe {
                            let data = input_frame.data_mut(0);
                            for y in 0..height {
                                let src_offset = (y as usize) * (width as usize) * 4;
                                let dst_offset = (y as usize) * linesize;
                                let copy_len = (width as usize) * 4;
                                if src_offset + copy_len <= rgba_data.len() && dst_offset + copy_len <= data.len() {
                                    std::ptr::copy_nonoverlapping(
                                        rgba_data.as_ptr().add(src_offset),
                                        data.as_mut_ptr().add(dst_offset),
                                        copy_len,
                                    );
                                }
                            }
                        } // mutable borrow ends here
                    }

                    // Scale to YUV420P (input_frame is now only borrowed immutably)
                    let mut output_frame = ffmpeg::frame::Video::empty();
                    scaler.run(&input_frame, &mut output_frame)
                        .map_err(|e| ScreenError::VideoEncoding(format!("Failed to scale frame: {}", e)))?;

                    // Set frame timestamp based on actual elapsed time
                    let elapsed_duration = start_time.elapsed();
                    let elapsed_ticks = duration_to_ticks(elapsed_duration);
                    output_frame.set_pts(Some(elapsed_ticks));
                    
                    // Calculate frame duration as the time since previous frame capture
                    // This is crucial for correct video duration in the MP4 container
                    let frame_duration_ticks = if let Some(prev_pts_ticks) = last_captured_frame_pts_ticks {
                        (elapsed_ticks - prev_pts_ticks).max(1)
                    } else {
                        // First frame: use target frame interval as estimate
                        target_frame_duration_ticks
                    };

                    log::info!(
                        "Frame {}: PTS={:.3}ms ({} ticks), duration={:.3}ms ({} ticks), timebase={}/{}",
                        frame_count,
                        ticks_to_millis(elapsed_ticks),
                        elapsed_ticks,
                        ticks_to_millis(frame_duration_ticks),
                        frame_duration_ticks,
                        encoder_time_base.0,
                        encoder_time_base.1
                    );

                    // Queue the frame timing information to apply to output packets
                    // Encoder may buffer frames, so we need to track timings separately
                    pending_frame_timings.push_back((elapsed_ticks, frame_duration_ticks));

                    // Remember the PTS of the most recently captured frame
                    last_captured_frame_pts_ticks = Some(elapsed_ticks);
                    last_frame_duration_ticks = frame_duration_ticks;

                    // Encode frame
                    video_encoder_context.send_frame(&output_frame)
                        .map_err(|e| ScreenError::VideoEncoding(format!("Failed to send frame: {}", e)))?;

                    // Receive and write encoded packets
                    // Note: H.264 encoder may buffer frames, so we might not get packets immediately
                    let mut encoded = ffmpeg::packet::Packet::empty();
                    let mut packets_written = 0;
                    while video_encoder_context.receive_packet(&mut encoded).is_ok() {
                        encoded.set_stream(0);
                        let pts_before = encoded.pts();
                        let dts_before = encoded.dts();
                        let duration_before = encoded.duration();
                        
                        // Apply the calculated frame timing information to this packet
                        // This forces the encoder output to match our capture timestamps
                        if let Some((frame_pts_ticks, frame_duration_ticks)) =
                            pending_frame_timings.pop_front()
                        {
                            encoded.set_pts(Some(frame_pts_ticks));
                            encoded.set_dts(Some(frame_pts_ticks));
                            encoded.set_duration(frame_duration_ticks);
                            log::debug!(
                                "Applied frame timing to packet -> PTS: {:.3}ms ({} ticks), duration: {:.3}ms ({} ticks)",
                                ticks_to_millis(frame_pts_ticks),
                                frame_pts_ticks,
                                ticks_to_millis(frame_duration_ticks),
                                frame_duration_ticks
                            );
                        }
                        
                        // Rescale timestamps from encoder timebase to stream timebase
                        encoded.rescale_ts(encoder_time_base, stream_time_base);
                        
                        let pts_after = encoded.pts();
                        let dts_after = encoded.dts();
                        let duration_after = encoded.duration();
                        let pts_after_ms = pts_after.map(stream_ticks_to_millis);
                        let dts_after_ms = dts_after.map(stream_ticks_to_millis);
                        let duration_after_ms = stream_ticks_to_millis(duration_after);
                        if let Some(pts_value) = pts_after {
                            last_packet_pts_stream_ticks = Some(pts_value);
                            last_packet_duration_stream_ticks = duration_after;
                        }
 
                        log::info!(
                            "Packet for frame {}: PTS {} -> {} ({:.3}ms) | DTS {} -> {} ({:.3}ms) | Duration {} -> {} ({:.3}ms)",
                            frame_count,
                            pts_before.unwrap_or(-1),
                            pts_after.unwrap_or(-1),
                            pts_after_ms.unwrap_or(0.0),
                            dts_before.unwrap_or(-1),
                            dts_after.unwrap_or(-1),
                            dts_after_ms.unwrap_or(0.0),
                            duration_before,
                            duration_after,
                            duration_after_ms
                        );
                        
                        encoded.write_interleaved(&mut output_context)
                            .map_err(|e| ScreenError::VideoEncoding(format!("Failed to write packet: {}", e)))?;
                        packets_written += 1;
                    }
                    if packets_written > 0 {
                        log::info!("Wrote {} packets for frame {}", packets_written, frame_count);
                    }

                    log::debug!("Captured and encoded frame {} ({}x{})", frame_count, image.width(), image.height());
                }
                Err(e) => {
                    log::warn!("Error capturing frame: {}", e);
                    // Continue recording despite frame errors
                    std::thread::sleep(frame_interval);
                    continue;
                }
            }

            // Sleep to maintain frame rate
            let elapsed = frame_start.elapsed();
            if elapsed < frame_interval {
                std::thread::sleep(frame_interval - elapsed);
            }
        }

        log::info!(
            "Recording loop ended. Last frame timestamp: {:.3}ms ({} ticks)",
            last_captured_frame_pts_ticks
                .map(|ticks| ticks_to_millis(ticks))
                .unwrap_or(0.0),
            last_captured_frame_pts_ticks.unwrap_or(0)
        );
        
        // Flush encoder - send EOF to signal end of input
        video_encoder_context.send_eof()
            .map_err(|e| ScreenError::VideoEncoding(format!("Failed to send EOF: {}", e)))?;

        // Flush remaining packets from encoder
        let mut flush_packets_written = 0;
        loop {
            let mut encoded = ffmpeg::packet::Packet::empty();
            match video_encoder_context.receive_packet(&mut encoded) {
                Ok(()) => {
                    encoded.set_stream(0);
                    // Track the last packet's timestamp for logging
                    let pts_before = encoded.pts();
                    let dts_before = encoded.dts();
                    let duration_before = encoded.duration();
                    
                    // Apply remaining frame timing information to flushed packets
                    if let Some((frame_pts_ticks, frame_duration_ticks)) = pending_frame_timings.pop_front() {
                        encoded.set_pts(Some(frame_pts_ticks));
                        encoded.set_dts(Some(frame_pts_ticks));
                        encoded.set_duration(frame_duration_ticks);
                        log::debug!(
                            "Set flushed packet timing -> PTS: {:.3}ms ({} ticks), duration: {:.3}ms ({} ticks)",
                            ticks_to_millis(frame_pts_ticks),
                            frame_pts_ticks,
                            ticks_to_millis(frame_duration_ticks),
                            frame_duration_ticks
                        );
                    }
                    
                    // Rescale timestamps from encoder timebase to stream timebase
                    encoded.rescale_ts(encoder_time_base, stream_time_base);
                    
                    let pts_after = encoded.pts();
                    let dts_after = encoded.dts();
                    let duration_after = encoded.duration();
                    if let Some(pts_value) = pts_after {
                        last_packet_pts_stream_ticks = Some(pts_value);
                        last_packet_duration_stream_ticks = duration_after;
                    }
                    
                    let pts_after_ms = pts_after.map(stream_ticks_to_millis);
                    let dts_after_ms = dts_after.map(stream_ticks_to_millis);
                    let duration_after_ms = stream_ticks_to_millis(duration_after);

                    log::info!(
                        "Flush packet {}: PTS {} -> {} ({:.3}ms) | DTS {} -> {} ({:.3}ms) | Duration {} -> {} ({:.3}ms)",
                        flush_packets_written,
                        pts_before.unwrap_or(-1),
                        pts_after.unwrap_or(-1),
                        pts_after_ms.unwrap_or(0.0),
                        dts_before.unwrap_or(-1),
                        dts_after.unwrap_or(-1),
                        dts_after_ms.unwrap_or(0.0),
                        duration_before,
                        duration_after,
                        duration_after_ms
                    );
                    
                    encoded.write_interleaved(&mut output_context)
                        .map_err(|e| ScreenError::VideoEncoding(format!("Failed to write flush packet: {}", e)))?;
                    flush_packets_written += 1;
                }
                Err(_) => {
                    // No more packets to flush
                    break;
                }
            }
        }
        
        // Calculate expected video duration: last packet PTS + last packet duration
        let expected_video_duration_stream_ticks =
            last_packet_pts_stream_ticks.unwrap_or(0) + last_packet_duration_stream_ticks;
        let expected_video_duration_stream_seconds =
            stream_ticks_to_seconds(expected_video_duration_stream_ticks);
        let last_packet_pts_stream_ms =
            stream_ticks_to_millis(last_packet_pts_stream_ticks.unwrap_or(0));
        let last_packet_duration_stream_ms = stream_ticks_to_millis(last_packet_duration_stream_ticks);

        log::info!(
            "Flushed {} packets. Last packet: PTS={} ({:.3}ms), duration={} ({:.3}ms). Expected video duration: {:.3}s",
            flush_packets_written,
            last_packet_pts_stream_ticks.unwrap_or(0),
            last_packet_pts_stream_ms,
            last_packet_duration_stream_ticks,
            last_packet_duration_stream_ms,
            expected_video_duration_stream_seconds
        );
        if !pending_frame_timings.is_empty() {
            log::warn!(
                "{} frame timing entries remained unapplied after flush; this indicates encoder buffering more packets than expected.",
                pending_frame_timings.len()
            );
        }

        // Write trailer
        output_context.write_trailer()
            .map_err(|e| ScreenError::VideoEncoding(format!("Failed to write trailer: {}", e)))?;

        let elapsed = start_time.elapsed();
        let actual_duration_secs = elapsed.as_secs_f64();
        let actual_fps = if actual_duration_secs > 0.0 {
            frame_count as f64 / actual_duration_secs
        } else {
            0.0
        };
        
        log::info!(
            "Recording completed: {} frames in {:.2}s ({:.2} fps average)",
            frame_count,
            actual_duration_secs,
            actual_fps
        );

        let capture_duration_ticks = if let Some(last_pts) = last_captured_frame_pts_ticks {
            last_pts + last_frame_duration_ticks
        } else {
            0
        };
        let capture_duration_seconds = ticks_to_millis(capture_duration_ticks) / 1000.0;

        log::info!(
            "Video duration estimates -> capture timeline: {:.3}s, muxed timeline: {:.3}s",
            capture_duration_seconds,
            expected_video_duration_stream_seconds
        );

        Ok(())
    }
}
