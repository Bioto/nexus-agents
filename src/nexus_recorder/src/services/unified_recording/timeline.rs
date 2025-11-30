//! Timeline display for recording sessions.
//!
//! This module provides functions to display a visual timeline of events
//! captured during a recording session.

use crate::error::Result;
use crate::services::storage::TimelineEvent;
use chrono::{DateTime, Local, Utc};

/// Print timeline for a session
pub fn print_timeline(
    session_id: &str,
    events: &[TimelineEvent],
    session_start: DateTime<Utc>,
) -> Result<()> {
    if events.is_empty() {
        println!("📋 No events found for session {}", session_id);
        return Ok(());
    }

    println!("\n╔══════════════════════════════════════════════════════════════════════════════╗");
    println!("║                          📋 Recording Timeline                                 ║");
    println!("╠══════════════════════════════════════════════════════════════════════════════╣");
    println!("║ Session ID: {:<64} ║", session_id);
    println!(
        "║ Started:    {:<64} ║",
        session_start
            .with_timezone(&Local)
            .format("%Y-%m-%d %H:%M:%S%.3f")
    );
    println!("╠══════════════════════════════════════════════════════════════════════════════╣");

    // Store events with their timecodes for proper timeline display
    #[derive(Clone)]
    struct TimedEvent {
        timecode: f64, // Always has a value now
        event_type: String,
        data: String,
    }

    let mut timed_events: Vec<TimedEvent> = Vec::new();

    // Find the earliest event timestamp to use as baseline
    // This ensures all events have positive timecodes relative to the first event
    let earliest_timestamp = events
        .iter()
        .map(|e| e.timestamp)
        .min()
        .unwrap_or(session_start);

    // Use the earlier of session_start or earliest_timestamp as baseline
    // This handles cases where events might be recorded slightly before session_start
    let baseline = if earliest_timestamp < session_start {
        earliest_timestamp
    } else {
        session_start
    };

    for event in events {
        // Use timecode if available, otherwise calculate from timestamp relative to baseline
        let event_timecode = event.timecode.or_else(|| {
            let elapsed = event.timestamp.signed_duration_since(baseline);
            let seconds = elapsed.num_milliseconds() as f64 / 1000.0;
            // Always return a timecode (clamp negative to 0.0 for safety)
            Some(seconds.max(0.0))
        });

        // Ensure we always have a timecode (should never be None after this point)
        let event_timecode = event_timecode.unwrap_or(0.0);

        match event.event_type.as_str() {
            "analysis" => {
                // Extract frame descriptions from metadata
                if let Some(metadata) = event.metadata.as_object() {
                    // Check if this is full video sampling (frames have absolute timestamps)
                    // The mode is stored in metadata.metadata (from job.metadata)
                    let is_full_video = metadata
                        .get("metadata")
                        .and_then(|m| m.get("mode"))
                        .and_then(|v| v.as_str())
                        .map(|v| v == "full_video")
                        .unwrap_or(false);

                    // Get base video timestamp from job metadata if available
                    let base_video_timestamp = metadata
                        .get("job")
                        .and_then(|job| job.get("video_timestamp"))
                        .and_then(|v| v.as_f64());

                    if let Some(frames) = metadata.get("frames") {
                        if let Some(frames_array) = frames.as_array() {
                            for frame in frames_array {
                                if let Some(desc) =
                                    frame.get("description").and_then(|v| v.as_str())
                                {
                                    let frame_offset = frame
                                        .get("offset_secs")
                                        .and_then(|v| v.as_f64())
                                        .unwrap_or(0.0);

                                    // Calculate absolute video timestamp for this frame
                                    // For full video, offset_secs is the absolute video timestamp (0.0, 0.2, 0.4, etc.)
                                    // For click context, offset is relative to click time (+0.0s, +0.2s, etc.)
                                    let frame_timecode = if is_full_video {
                                        // For full video, offset_secs is already the absolute video timestamp
                                        frame_offset
                                    } else if let Some(base) = base_video_timestamp {
                                        // For click context, offset is relative to click time
                                        base + frame_offset
                                    } else {
                                        // Heuristic: if offset is small (< 100s) and first frame is near 0,
                                        // it's likely an absolute timestamp from full video sampling
                                        // Otherwise, if offset is very small (< 5s), assume it's relative to some base
                                        // But we don't have the base, so use offset directly as a best guess
                                        if frame_offset < 100.0 && frame_offset >= 0.0 {
                                            // Likely absolute timestamp from full video
                                            frame_offset
                                        } else {
                                            // Can't determine - this shouldn't happen, but use offset as fallback
                                            frame_offset
                                        }
                                    };

                                    let offset_str = if is_full_video {
                                        format!("{:.2}s", frame_offset)
                                    } else {
                                        format!("+{:.2}s", frame_offset)
                                    };

                                    timed_events.push(TimedEvent {
                                        timecode: frame_timecode,
                                        event_type: "frame".to_string(),
                                        data: format!("{} {}", offset_str, desc),
                                    });
                                }
                            }
                        }
                    }
                    if let Some(summary) = metadata.get("summary").and_then(|v| v.as_str()) {
                        // For summary, use the timecode of the last frame or event timecode
                        let summary_timecode = timed_events
                            .iter()
                            .filter(|e| e.event_type == "frame")
                            .last()
                            .map(|e| e.timecode)
                            .unwrap_or(event_timecode);

                        timed_events.push(TimedEvent {
                            timecode: summary_timecode,
                            event_type: "summary".to_string(),
                            data: format!("Summary: {}", summary),
                        });
                    }
                }
            }
            "keyboard" => {
                if let Some(key) = &event.key {
                    if event.pressed.unwrap_or(false) {
                        timed_events.push(TimedEvent {
                            timecode: event_timecode,
                            event_type: "key".to_string(),
                            data: key.clone(),
                        });
                    }
                }
            }
            "mouse" => {
                if event.event_subtype.as_deref() == Some("click") {
                    let button = event.button.as_deref().unwrap_or("unknown");
                    let coords = if let (Some(x), Some(y)) = (event.x, event.y) {
                        format!("({}, {})", x, y)
                    } else {
                        String::new()
                    };
                    timed_events.push(TimedEvent {
                        timecode: event_timecode,
                        event_type: "click".to_string(),
                        data: format!("{} {}", button, coords),
                    });
                }
            }
            "transcription" => {
                if event.event_subtype.as_deref() == Some("segment") {
                    // Extract text from metadata
                    if let Some(metadata) = event.metadata.as_object() {
                        if let Some(text) = metadata.get("text").and_then(|v| v.as_str()) {
                            // Extract source information (monitor_output or microphone)
                            let source = metadata
                                .get("source")
                                .and_then(|v| v.as_str())
                                .unwrap_or_else(|| {
                                    // Fallback: check monitor_desktop_audio flag
                                    if metadata
                                        .get("monitor_desktop_audio")
                                        .and_then(|v| v.as_bool())
                                        .unwrap_or(false)
                                    {
                                        "monitor_output"
                                    } else {
                                        "microphone"
                                    }
                                });
                            timed_events.push(TimedEvent {
                                timecode: event_timecode,
                                event_type: "transcription".to_string(),
                                data: format!("[{}] {}", source, text),
                            });
                        } else if let Some(key) = event.key.as_ref() {
                            // Fallback: use key field if metadata doesn't have text
                            let source = metadata
                                .get("source")
                                .and_then(|v| v.as_str())
                                .unwrap_or_else(|| {
                                    if metadata
                                        .get("monitor_desktop_audio")
                                        .and_then(|v| v.as_bool())
                                        .unwrap_or(false)
                                    {
                                        "monitor_output"
                                    } else {
                                        "microphone"
                                    }
                                });
                            timed_events.push(TimedEvent {
                                timecode: event_timecode,
                                event_type: "transcription".to_string(),
                                data: format!("[{}] {}", source, key),
                            });
                        }
                    } else if let Some(key) = event.key.as_ref() {
                        // Fallback: use key field (no metadata available, assume microphone)
                        timed_events.push(TimedEvent {
                            timecode: event_timecode,
                            event_type: "transcription".to_string(),
                            data: format!("[microphone] {}", key),
                        });
                    }
                }
            }
            _ => {}
        }
    }

    // Sort by timecode
    timed_events.sort_by(|a, b| {
        a.timecode
            .partial_cmp(&b.timecode)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Group events by timecode for display (with small tolerance for grouping)
    let mut current_timecode: Option<f64> = None;
    let mut frame_descriptions: Vec<String> = Vec::new();
    let mut keys_entered: Vec<String> = Vec::new();
    let mut clicks: Vec<(f64, String)> = Vec::new(); // Store timecode with each click
    let mut transcriptions: Vec<(f64, String)> = Vec::new(); // Store timecode with each transcription

    for timed_event in timed_events {
        // If timecode changed significantly (more than 0.1s difference), print previous group
        let timecode_changed = if let Some(current) = current_timecode {
            (current - timed_event.timecode).abs() > 0.1
        } else {
            true
        };

        if current_timecode.is_some() && timecode_changed {
            print_timeline_entry(
                current_timecode,
                &frame_descriptions,
                &keys_entered,
                &clicks,
                &transcriptions,
            );
            frame_descriptions.clear();
            keys_entered.clear();
            clicks.clear();
            transcriptions.clear();
        }

        current_timecode = Some(timed_event.timecode);

        match timed_event.event_type.as_str() {
            "frame" | "summary" => {
                frame_descriptions.push(timed_event.data);
            }
            "key" => {
                keys_entered.push(timed_event.data);
            }
            "click" => {
                clicks.push((timed_event.timecode, timed_event.data));
            }
            "transcription" => {
                transcriptions.push((timed_event.timecode, timed_event.data));
            }
            _ => {}
        }
    }

    // Print final group
    if !frame_descriptions.is_empty()
        || !keys_entered.is_empty()
        || !clicks.is_empty()
        || !transcriptions.is_empty()
    {
        print_timeline_entry(
            current_timecode,
            &frame_descriptions,
            &keys_entered,
            &clicks,
            &transcriptions,
        );
    }

    println!("╚══════════════════════════════════════════════════════════════════════════════╝\n");

    Ok(())
}

fn print_timeline_entry(
    timecode: Option<f64>,
    frame_descriptions: &[String],
    keys_entered: &[String],
    clicks: &[(f64, String)],
    transcriptions: &[(f64, String)],
) {
    let time_str = if let Some(tc) = timecode {
        format!("{:>8.2}s", tc)
    } else {
        "        ".to_string()
    };

    println!(
        "║ Time: {}                                                                    ║",
        time_str
    );

    if !frame_descriptions.is_empty() {
        println!("║ 🧠 Frame Analysis:                                                          ║");
        for desc in frame_descriptions {
            // Wrap long descriptions
            let wrapped = wrap_text(desc, 75);
            for line in wrapped {
                println!("║    {}", pad_right(&line, 75));
            }
        }
    }

    if !keys_entered.is_empty() {
        let keys_str = keys_entered.join(", ");
        println!("║ ⌨️  Keys: {}", pad_right(&keys_str, 70));
    }

    if !clicks.is_empty() {
        for (click_timecode, click_data) in clicks {
            // Show timecode for each click if different from group timecode
            let click_display = if let Some(group_tc) = timecode {
                if (group_tc - click_timecode).abs() > 0.1 {
                    format!("@ {:.2}s: {}", click_timecode, click_data)
                } else {
                    click_data.clone()
                }
            } else {
                format!("@ {:.2}s: {}", click_timecode, click_data)
            };
            println!("║ 🖱️  Click: {}", pad_right(&click_display, 70));
        }
    }

    if !transcriptions.is_empty() {
        for (trans_timecode, trans_text) in transcriptions {
            // Parse source from text (format: "[source] text")
            let (source, text) = if trans_text.starts_with('[') {
                if let Some(end_bracket) = trans_text.find(']') {
                    let source_str = &trans_text[1..end_bracket];
                    let text_part = trans_text[end_bracket + 1..].trim_start();
                    (source_str, text_part)
                } else {
                    ("unknown", trans_text.as_str())
                }
            } else {
                ("microphone", trans_text.as_str())
            };

            // Format source display
            let source_display = match source {
                "monitor_output" => "📺 Monitor Output",
                "microphone" => "🎙️  Microphone",
                _ => "🎤 Unknown",
            };

            // Show timecode for each transcription if different from group timecode
            let timecode_prefix = if let Some(group_tc) = timecode {
                if (group_tc - trans_timecode).abs() > 0.1 {
                    format!("@ {:.2}s: ", trans_timecode)
                } else {
                    String::new()
                }
            } else {
                format!("@ {:.2}s: ", trans_timecode)
            };

            // Calculate available space for text (box is 78 chars wide, minus borders and prefix)
            // Box format: "║ " (2) + prefix + text + " ║" (2) = 78
            // Available space = 78 - 2 - prefix_len - 2 = 74 - prefix_len
            let prefix = format!("{} Transcription: ", source_display);
            let prefix_len = prefix.chars().count(); // Use char count for proper emoji handling
            let available_width = 74 - prefix_len; // 74 = 78 - 2 (left border) - 2 (right border)

            // Wrap long transcriptions to fit available width
            let full_text = format!("{}{}", timecode_prefix, text);
            let wrapped = wrap_text(&full_text, available_width);
            for (idx, line) in wrapped.iter().enumerate() {
                if idx == 0 {
                    println!("║ {}{}", prefix, pad_right(line, available_width));
                } else {
                    // Continuation lines: "║    " (5 chars) + text + " ║" (2) = 78
                    let continuation_width = 74 - 4; // 4 chars for "    " indent
                    println!("║    {}", pad_right(line, continuation_width));
                }
            }
        }
    }

    println!("╠══════════════════════════════════════════════════════════════════════════════╣");
}

fn wrap_text(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut current_line = String::new();

    for word in words {
        if current_line.is_empty() {
            current_line = word.to_string();
        } else if current_line.len() + word.len() + 1 <= width {
            current_line.push(' ');
            current_line.push_str(word);
        } else {
            lines.push(current_line);
            current_line = word.to_string();
        }
    }
    if !current_line.is_empty() {
        lines.push(current_line);
    }
    lines
}

fn pad_right(s: &str, width: usize) -> String {
    if s.len() >= width {
        s.chars().take(width).collect()
    } else {
        format!("{:<width$}", s, width = width)
    }
}

