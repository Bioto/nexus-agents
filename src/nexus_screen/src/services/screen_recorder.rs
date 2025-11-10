use anyhow::{self, Result};
use xcap::Monitor;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use ffmpeg_next as ffmpeg;
use ffmpeg::{
    format,
    codec,
    encoder,
    format::Pixel,
    software::scaling::{Context as Scaler, flag::Flags},
    frame::Video,
    packet::Packet,
    Rational,
};

#[derive(Clone, Debug)]
pub struct RecordingConfig {
    pub framerate: u32,
    pub duration_secs: Option<u64>,
    pub output_path: PathBuf,
    pub monitor_index: Option<usize>,
    pub include_audio: bool,
}

impl Default for RecordingConfig {
    fn default() -> Self {
        Self {
            framerate: 60,
            duration_secs: None,
            output_path: PathBuf::from("recording.mp4"),
            monitor_index: None,
            include_audio: true,
        }
    }
}

pub struct ScreenRecorder {
    monitor: Monitor,
    config: RecordingConfig,
}

impl ScreenRecorder {
    pub fn new() -> Result<Self> {
        let config = RecordingConfig::default();
        Self::new_with_config(config)
    }

    pub fn new_with_config(config: RecordingConfig) -> Result<Self> {
        let monitors = Monitor::all()
            .map_err(|e| anyhow::anyhow!("Failed to enumerate monitors: {}", e))?;
        let monitor = if let Some(idx) = config.monitor_index {
            monitors.get(idx)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("Invalid monitor index: {}", idx))?
        } else {
            monitors.iter()
                .find(|m| m.is_primary().map_or(false, |p| p))
                .or_else(|| monitors.first())
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("No available monitors"))?
        };
        // Remove caching as id() comparison not possible without Eq
        Ok(Self { monitor, config })
    }

    pub fn capture_screenshot_to_file(&self, output_path: &str) -> Result<()> {
        let img = self.monitor
            .capture_image()
            .map_err(|e| anyhow::anyhow!("Screenshot capture failed: {}", e))?;

        let (width, height) = (img.width() as usize, img.height() as usize);
        let expected_size = width * height * 4;
        let actual_size = img.as_raw().len();
        if actual_size != expected_size {
            return Err(anyhow::anyhow!("Invalid image size: expected {}, got {}", expected_size, actual_size));
        }

        img.save(output_path)
            .map_err(|e| anyhow::anyhow!("Failed to save screenshot to {}: {}", output_path, e))?;

        Ok(())
    }

    pub fn record(&self, config: RecordingConfig, stop_signal: Arc<AtomicBool>) -> Result<()> {
        let fps = config.framerate;
        let frame_interval = Duration::from_nanos(1_000_000_000u64 / fps as u64);
        let end_time = config.duration_secs.map(|secs| Instant::now() + Duration::from_secs(secs));

        // Setup FFmpeg output
        let mut octx = format::output(&config.output_path)?;
        let codec = encoder::find(codec::Id::H264)
            .ok_or_else(|| anyhow::anyhow!("No H.264 encoder available"))?;
        let stream = octx.add_stream(codec)?;
        
        // Get monitor dimensions and pad to even numbers (required for H.264/YUV420P)
        let raw_width = self.monitor.width()?;
        let raw_height = self.monitor.height()?;
        let width = if raw_width % 2 == 0 { raw_width } else { raw_width + 1 };
        let height = if raw_height % 2 == 0 { raw_height } else { raw_height + 1 };
        
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
        let stream_time_base = octx.stream(ostream_idx)
            .ok_or_else(|| anyhow::anyhow!("Stream {} not found", ostream_idx))?
            .time_base();
        println!("Encoder time_base: {:?}, Stream time_base: {:?}", encoder_time_base, stream_time_base);
        println!("DEBUG: encoder_tb = {}/{} = {:.6}", 
            encoder_time_base.numerator(), encoder_time_base.denominator(),
            encoder_time_base.numerator() as f64 / encoder_time_base.denominator() as f64);
        println!("DEBUG: stream_tb = {}/{} = {:.6}", 
            stream_time_base.numerator(), stream_time_base.denominator(),
            stream_time_base.numerator() as f64 / stream_time_base.denominator() as f64);

        // Helper function to convert timestamp from encoder time_base to stream time_base
        // Formula: stream_ts = encoder_ts * (encoder_tb.num * stream_tb.den) / (encoder_tb.den * stream_tb.num)
        let convert_to_stream_ts = |encoder_ts: i64| -> i64 {
            let num = encoder_ts as i64 * encoder_time_base.numerator() as i64 * stream_time_base.denominator() as i64;
            let den = encoder_time_base.denominator() as i64 * stream_time_base.numerator() as i64;
            let result = num / den;
            if encoder_ts < 5 {
                println!("DEBUG convert: encoder_ts={}, num={}, den={}, result={}", encoder_ts, num, den, result);
            }
            result
        };

        let mut input_frame = Video::new(Pixel::RGB24, raw_width, raw_height);
        let mut scaled_frame = Video::new(Pixel::YUV420P, width, height);

        let mut scaler = Scaler::get(
            Pixel::RGB24,
            raw_width,
            raw_height,
            Pixel::YUV420P,
            width,
            height,
            Flags::BILINEAR,
        ).map_err(|e| anyhow::anyhow!("Scaler init failed: {}", e))?;

        let mut frame_num: i64 = 0;
        let mut last_dts: Option<i64> = None; // Track last DTS in stream time_base to ensure monotonicity
        let mut first_frame_pts: Option<i64> = None; // Track the first frame PTS for duration calculation
        let mut last_frame_pts: i64 = 0; // Track the PTS of the last frame in stream time_base for duration calculation
        let mut pts_offset: Option<i64> = None; // Offset to ensure first PTS is 0
        let mut frame_pts_map: Vec<i64> = Vec::new(); // Track PTS for each frame index
        let mut frames_with_packets: std::collections::HashSet<usize> = std::collections::HashSet::new(); // Track which frames have produced packets
        let start_time = Instant::now();
        
        // Calculate DTS increment in stream time_base (1 frame in encoder time_base)
        // This is: 1 * (stream_tb.den * encoder_tb.num) / (stream_tb.num * encoder_tb.den)
        let dts_increment = (stream_time_base.denominator() as i64 * encoder_time_base.numerator() as i64) 
            / (stream_time_base.numerator() as i64 * encoder_time_base.denominator() as i64);
        println!("DEBUG: dts_increment = {} (1 frame in stream time_base)", dts_increment);
        
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

            // Capture frame manually using capture_image
            let capture_start = Instant::now();
            let img = match self.monitor.capture_image() {
                Ok(img) => img,
                Err(e) => {
                    eprintln!("Failed to capture frame {}: {}", frame_num, e);
                    log::warn!("Failed to capture frame {}: {}", frame_num, e);
                    std::thread::sleep(frame_interval);
                    continue;
                }
            };
            let capture_elapsed = capture_start.elapsed();

            // Convert RgbaImage to RGB24 with proper stride handling
            let (frame_w, frame_h) = (img.width() as usize, img.height() as usize);
            let rgba_data = img.as_raw();
            
            // Get stride before borrowing frame_data mutably
            let src_stride = frame_w * 4; // RGBA source stride
            let dst_stride = input_frame.stride(0); // FFmpeg frame stride
            
            // Copy line by line to handle stride correctly
            let frame_data = input_frame.data_mut(0);

            for y in 0..frame_h {
                let src_offset = y * src_stride;
                let dst_offset = y * dst_stride;
                
                // Convert RGBA to RGB24 for this row
                for x in 0..frame_w {
                    let src_idx = src_offset + (x * 4);
                    let dst_idx = dst_offset + (x * 3);
                    
                    if dst_idx + 2 < frame_data.len() && src_idx + 3 < rgba_data.len() {
                        frame_data[dst_idx] = rgba_data[src_idx];         // R
                        frame_data[dst_idx + 1] = rgba_data[src_idx + 1]; // G
                        frame_data[dst_idx + 2] = rgba_data[src_idx + 2]; // B
                    }
                }
            }

            // Scale from RGB24 to YUV420P
            let scale_start = Instant::now();
            scaler.run(&input_frame, &mut scaled_frame)?;
            let scale_elapsed = scale_start.elapsed();

            scaled_frame.set_pts(Some(frame_num));

            // Encode frame
            let encode_start = Instant::now();
            video_encoder.send_frame(&scaled_frame)?;

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
                println!("DEBUG: Calculated PTS offset = {} (first frame raw PTS)", offset);
                offset
            };
            let stream_frame_pts = stream_frame_pts_raw - offset;
            
            // Track this frame's PTS even if no packets are produced yet
            frame_pts_map.push(stream_frame_pts);
            last_frame_pts = stream_frame_pts; // Always update to track the last frame's PTS
            
            // Track first frame PTS (should be 0) - set it on the first frame, not first packet
            if first_frame_pts.is_none() && frame_num == 0 {
                first_frame_pts = Some(stream_frame_pts);
                println!("DEBUG: First frame PTS (frame 0) = {} (in stream time_base, should be 0)", stream_frame_pts);
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
                            let pts_seconds = stream_frame_pts as f64 * stream_time_base.numerator() as f64 / stream_time_base.denominator() as f64;
                            let dts_seconds = stream_frame_dts as f64 * stream_time_base.numerator() as f64 / stream_time_base.denominator() as f64;
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
                println!("Frame {}: capture={:?}, scale={:?}, encode={:?}, total={:?}, packets={}", 
                    frame_num, capture_elapsed, scale_elapsed, encode_elapsed, loop_elapsed, packet_count);
                log::info!("Frame {} encoded in {:?}, packets={}", frame_num, loop_elapsed, packet_count);
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
                            let offset = pts_offset.unwrap_or(0);
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
            let frame_elapsed = loop_start.elapsed();
            if frame_elapsed < frame_interval {
                std::thread::sleep(frame_interval - frame_elapsed);
            } else if frame_num < 5 {
                eprintln!("Warning: Frame {} took {:?}, target was {:?}", 
                    frame_num - 1, frame_elapsed, frame_interval);
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
        
        println!("Actual capture: {} frames in {:.3} seconds = {:.2} fps (target: {} fps)", 
            frame_num, actual_duration_secs, actual_fps, fps);
        
        // Calculate the actual time increment per frame in stream time_base
        // This will space out frames to match the actual recording duration
        let actual_dts_increment = if frame_num > 1 {
            // Calculate increment based on actual duration
            // Total duration in stream time_base = actual_duration_secs / stream_time_base
            let total_duration_in_stream_tb = (actual_duration_secs * stream_time_base.denominator() as f64) / stream_time_base.numerator() as f64;
            // Increment per frame = total duration / (frame_count - 1)
            // We use frame_count - 1 because we have frame_count intervals between frame_count frames
            (total_duration_in_stream_tb / (frame_num - 1) as f64) as i64
        } else {
            dts_increment // Fallback to original increment
        };
        
        println!("DEBUG: Original dts_increment = {}, Actual dts_increment = {} (based on {:.3}s recording)", 
            dts_increment, actual_dts_increment, actual_duration_secs);
        // Get the actual last frame PTS from our tracking
        let actual_last_frame_pts = if last_frame_index >= 0 && (last_frame_index as usize) < frame_pts_map.len() {
            frame_pts_map[last_frame_index as usize]
        } else {
            last_frame_pts
        };
        
        println!("Recording loop ended. Captured {} frames in {:?}", frame_num, actual_duration);
        println!("Expected duration: {:.3} seconds ({} frames / {} fps)", expected_duration_secs, frame_num, fps);
        println!("Last frame index: {}, Last frame PTS (tracked): {}", last_frame_index, actual_last_frame_pts);
        println!("frame_pts_map length: {}, last_frame_pts variable: {}", frame_pts_map.len(), last_frame_pts);
        if let Some(first_pts) = first_frame_pts {
            let duration_in_stream_tb = actual_last_frame_pts - first_pts;
            let duration_seconds = duration_in_stream_tb as f64 * stream_time_base.numerator() as f64 / stream_time_base.denominator() as f64;
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
        println!("DEBUG: Collected {} packets during flush", total_flush_packets);
        
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
            
            println!("DEBUG FLUSH: packet {}, frame_idx={}, pts={}, dts={}, is_last={}", 
                idx, assigned_frame_idx, final_stream_pts, final_stream_dts, is_last);
            
            flush_packet.write_interleaved(&mut octx)?;
            flush_count += 1;
        }
        println!("Flushed {} packets from encoder, final PTS: {} (last_frame_index: {}, total_frames: {})", 
            flush_count, last_frame_pts, last_frame_index, frame_num);
        
        // Final duration calculation
        // Since we recalculated PTS during flush based on actual time, first PTS should be 0
        let final_first_pts = 0i64; // First frame always starts at 0 after recalculation
        let final_duration_in_stream_tb = last_frame_pts - final_first_pts;
        let final_duration_seconds = final_duration_in_stream_tb as f64 * stream_time_base.numerator() as f64 / stream_time_base.denominator() as f64;
        println!("DEBUG FINAL: First PTS = {}, Final PTS = {}, Duration in stream_tb = {}, Duration in seconds = {:.6}", 
            final_first_pts, last_frame_pts, final_duration_in_stream_tb, final_duration_seconds);
        println!("DEBUG FINAL: Expected duration (frame-based) = {:.6} seconds ({} frames / {} fps)", 
            expected_duration_secs, frame_num, fps);
        println!("DEBUG FINAL: Actual recording duration = {:.6} seconds", actual_duration_secs);
        println!("DEBUG FINAL: Video duration should match actual recording duration: {:.6} seconds", actual_duration_secs);

        // Before writing trailer, try to ensure stream duration is correct
        // The stream duration should be calculated from the last packet's PTS
        // But FFmpeg might be using the last frame's PTS instead
        // Let's verify by checking what the stream thinks its duration is
        if let Some(stream) = octx.stream(ostream_idx) {
            // Note: We can't directly set duration in ffmpeg-next, but we can ensure
            // the last packet's PTS is correct, which should make FFmpeg calculate it correctly
            println!("DEBUG: Stream time_base before trailer: {:?}", stream.time_base());
        }
        
        println!("Writing trailer...");
        octx.write_trailer()?;
        println!("Video finalized successfully!");

        Ok(())
    }
}
