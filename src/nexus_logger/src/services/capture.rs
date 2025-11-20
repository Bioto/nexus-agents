use crate::error::{LoggerError, Result};
use crate::services::click_context::{ClickContextEvent, ClickContextHandle, ClickContextService};
use crate::services::database::Database;
use chrono::{Local, Utc};
use device_query::{DeviceQuery, DeviceState, Keycode};
use serde::Serialize;
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
pub enum InputEvent {
    #[serde(rename = "keyboard")]
    Keyboard {
        key: String,
        pressed: bool,
        timestamp: String,
    },
    #[serde(rename = "mouse")]
    Mouse {
        event_type: String,
        button: Option<String>,
        x: Option<i32>,
        y: Option<i32>,
        timestamp: String,
    },
}

impl InputEvent {
    pub fn to_text(&self) -> String {
        match self {
            InputEvent::Keyboard { key, pressed, .. } => {
                let action = if *pressed { "PRESS" } else { "RELEASE" };
                format!("[KEYBOARD] {}: {}", action, key)
            }
            InputEvent::Mouse {
                event_type,
                button,
                x,
                y,
                timestamp: _,
            } => match event_type.as_str() {
                "click" => {
                    format!(
                        "[MOUSE] CLICK: {} at ({}, {})",
                        button.as_ref().unwrap_or(&"unknown".to_string()),
                        x.unwrap_or(0),
                        y.unwrap_or(0)
                    )
                }
                "move" => {
                    format!("[MOUSE] MOVE: ({}, {})", x.unwrap_or(0), y.unwrap_or(0))
                }
                _ => format!("[MOUSE] {}: {:?}", event_type, button),
            },
        }
    }
}

pub async fn run_capture_service(
    capture_keyboard: bool,
    capture_mouse: bool,
    capture_mouse_moves: bool,
    format: String,
    output_file: Option<PathBuf>,
    metrics_interval: u64,
    running: Arc<AtomicBool>,
) -> Result<()> {
    // Initialize database
    let db = Database::new().await?;
    let session_id = Uuid::new_v4().to_string();
    db.create_session(&session_id).await?;

    let click_context = ClickContextService::maybe_start();

    let device_state = DeviceState::new();
    let mut last_keys: Vec<Keycode> = vec![];
    let mut last_mouse_buttons: Vec<bool> = vec![];

    // Track last mouse position for move detection
    let mut last_mouse_pos: Option<(i32, i32)> = None;

    // Metrics tracking
    let mut metrics = MetricsTracker {
        keyboard_events: 0,
        keyboard_presses: 0,
        keyboard_releases: 0,
        mouse_events: 0,
        mouse_clicks: 0,
        mouse_releases: 0,
        mouse_moves: 0,
        key_frequency: HashMap::new(),
        mouse_button_frequency: HashMap::new(),
        start_time: Instant::now(),
        last_metrics_display: Instant::now(),
    };

    // Open output file if specified
    let mut file_handle: Option<std::fs::File> = if let Some(ref path) = output_file {
        Some(
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .map_err(|e| {
                    LoggerError::Io(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        format!("Failed to open output file: {}", e),
                    ))
                })?,
        )
    } else {
        None
    };

    // Create a channel for async database operations
    let (db_tx, mut db_rx) = tokio::sync::mpsc::unbounded_channel::<(InputEvent, String)>();
    let db_clone_for_handle = db.clone();
    let db_clone_for_final = db.clone();
    let _session_id_clone = session_id.clone();

    // Spawn async task to handle database writes
    let db_handle = tokio::spawn(async move {
        while let Some((event, session_id)) = db_rx.recv().await {
            match &event {
                InputEvent::Keyboard {
                    key,
                    pressed,
                    timestamp,
                } => {
                    let _ = db_clone_for_handle
                        .insert_event(
                            &session_id,
                            "keyboard",
                            Some(if *pressed { "press" } else { "release" }),
                            Some(key),
                            None,
                            None,
                            None,
                            Some(*pressed),
                            timestamp,
                            None, // timecode
                            None, // metadata
                            None, // screenshot_id
                        )
                        .await;

                    // Update key frequency
                    if *pressed {
                        let _ = db_clone_for_handle
                            .update_key_frequency(&session_id, key)
                            .await;
                    }
                }
                InputEvent::Mouse {
                    event_type,
                    button,
                    x,
                    y,
                    timestamp,
                } => {
                    let _ = db_clone_for_handle
                        .insert_event(
                            &session_id,
                            "mouse",
                            Some(event_type),
                            None,
                            button.as_deref(),
                            *x,
                            *y,
                            None,
                            timestamp,
                            None, // timecode
                            None, // metadata
                            None, // screenshot_id
                        )
                        .await;

                    // Update button frequency for clicks
                    if event_type == "click" {
                        if let Some(ref btn) = button {
                            let _ = db_clone_for_handle
                                .update_mouse_button_frequency(&session_id, btn)
                                .await;
                        }
                    }
                }
            }
        }
    });

    let mut write_output = |event: &InputEvent| -> Result<()> {
        // Send event to async database handler
        let _ = db_tx.send((event.clone(), session_id.clone()));
        let output = match format.as_str() {
            "json" => serde_json::to_string(event)
                .map_err(|e| LoggerError::Other(format!("Failed to serialize event: {}", e)))?,
            "text" => event.to_text(),
            "both" => {
                format!(
                    "{} | {}",
                    event.to_text(),
                    serde_json::to_string(event).map_err(|e| {
                        LoggerError::Other(format!("Failed to serialize event: {}", e))
                    })?
                )
            }
            _ => return Err(LoggerError::Configuration("Invalid format".to_string())),
        };

        if let Some(ref mut file) = file_handle {
            writeln!(file, "{}", output).map_err(|e| {
                LoggerError::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Failed to write to file: {}", e),
                ))
            })?;
        } else {
            println!("{}", output);
        }
        Ok(())
    };

    // Main capture loop
    while running.load(Ordering::SeqCst) {
        let timestamp_utc = Utc::now();
        let timestamp = timestamp_utc.with_timezone(&Local).to_rfc3339();

        // Capture keyboard events
        if capture_keyboard {
            let keys = device_state.get_keys();
            let keys_set: std::collections::HashSet<Keycode> = keys.iter().cloned().collect();
            let last_keys_set: std::collections::HashSet<Keycode> =
                last_keys.iter().cloned().collect();

            // Detect key presses (new keys not in last_keys)
            for key in &keys {
                if !last_keys_set.contains(key) {
                    let key_str = format!("{:?}", key);
                    let event = InputEvent::Keyboard {
                        key: key_str.clone(),
                        pressed: true,
                        timestamp: timestamp.clone(),
                    };
                    write_output(&event)?;

                    // Update metrics
                    metrics.keyboard_events += 1;
                    metrics.keyboard_presses += 1;
                    *metrics.key_frequency.entry(key_str).or_insert(0) += 1;
                }
            }

            // Detect key releases (keys in last_keys but not in current keys)
            for key in &last_keys {
                if !keys_set.contains(key) {
                    let event = InputEvent::Keyboard {
                        key: format!("{:?}", key),
                        pressed: false,
                        timestamp: timestamp.clone(),
                    };
                    write_output(&event)?;

                    // Update metrics
                    metrics.keyboard_events += 1;
                    metrics.keyboard_releases += 1;
                }
            }

            last_keys = keys;
        }

        // Capture mouse events
        if capture_mouse {
            let mouse = device_state.get_mouse();
            let current_pos = (mouse.coords.0, mouse.coords.1);
            let pos_changed = last_mouse_pos
                .map(|last| last != current_pos)
                .unwrap_or(true);
            last_mouse_pos = Some(current_pos);

            // Button names for indices: 0=Left, 1=Right, 2=Middle, etc.
            let button_names = ["Left", "Right", "Middle", "X1", "X2"];

            // Detect button presses and releases
            for (idx, &pressed) in mouse.button_pressed.iter().enumerate().skip(1) {
                let was_pressed = last_mouse_buttons.get(idx).copied().unwrap_or(false);
                let button_name = button_names
                    .get(idx - 1)
                    .copied()
                    .map(String::from)
                    .unwrap_or_else(|| format!("Button{}", idx));

                if pressed && !was_pressed {
                    // Button press
                    let event = InputEvent::Mouse {
                        event_type: "click".to_string(),
                        button: Some(button_name.clone()),
                        x: Some(mouse.coords.0),
                        y: Some(mouse.coords.1),
                        timestamp: timestamp.clone(),
                    };
                    write_output(&event)?;
                    trigger_click_context(
                        &click_context,
                        &session_id,
                        timestamp_utc,
                        Some(button_name.clone()),
                        Some(mouse.coords.0),
                        Some(mouse.coords.1),
                    );

                    // Update metrics
                    metrics.mouse_events += 1;
                    metrics.mouse_clicks += 1;
                    *metrics
                        .mouse_button_frequency
                        .entry(button_name.clone())
                        .or_insert(0) += 1;
                } else if !pressed && was_pressed {
                    // Button release
                    let event = InputEvent::Mouse {
                        event_type: "release".to_string(),
                        button: Some(button_name.clone()),
                        x: Some(mouse.coords.0),
                        y: Some(mouse.coords.1),
                        timestamp: timestamp.clone(),
                    };
                    write_output(&event)?;

                    // Update metrics
                    metrics.mouse_events += 1;
                    metrics.mouse_releases += 1;
                }
            }

            // Capture mouse moves (if enabled)
            if capture_mouse_moves && pos_changed {
                let event = InputEvent::Mouse {
                    event_type: "move".to_string(),
                    button: None,
                    x: Some(mouse.coords.0),
                    y: Some(mouse.coords.1),
                    timestamp: timestamp.clone(),
                };
                write_output(&event)?;

                // Update metrics
                metrics.mouse_events += 1;
                metrics.mouse_moves += 1;
            }

            // Update last mouse buttons
            last_mouse_buttons = mouse.button_pressed.clone();
        }

        // Update database metrics periodically (async)
        let db_clone_for_metrics = db_clone_for_final.clone();
        let session_id_for_metrics = session_id.clone();
        let (ke, kp, kr, me, mc, mr, mm) = (
            metrics.keyboard_events,
            metrics.keyboard_presses,
            metrics.keyboard_releases,
            metrics.mouse_events,
            metrics.mouse_clicks,
            metrics.mouse_releases,
            metrics.mouse_moves,
        );
        tokio::spawn(async move {
            let _ = db_clone_for_metrics
                .update_session_metrics(&session_id_for_metrics, ke, kp, kr, me, mc, mr, mm)
                .await;
        });

        // Display metrics summary periodically
        if metrics_interval > 0
            && metrics.last_metrics_display.elapsed().as_secs() >= metrics_interval
        {
            // Note: display_metrics_summary is now async but we're in a blocking context
            // For now, just display local metrics
            let duration = metrics.start_time.elapsed();
            let total_events = metrics.keyboard_events + metrics.mouse_events;
            let events_per_second = if duration.as_secs() > 0 {
                total_events as f64 / duration.as_secs() as f64
            } else {
                0.0
            };
            println!(
                "\n📊 Metrics: {} events, {:.2} events/sec",
                total_events, events_per_second
            );
            metrics.last_metrics_display = Instant::now();
        }

        // Small delay to avoid excessive CPU usage
        std::thread::sleep(Duration::from_millis(10));
    }

    // Close the database channel
    drop(db_tx);

    // Wait for database operations to complete
    let _ = db_handle.await;

    // Final metrics update and display
    db_clone_for_final
        .update_session_metrics(
            &session_id,
            metrics.keyboard_events,
            metrics.keyboard_presses,
            metrics.keyboard_releases,
            metrics.mouse_events,
            metrics.mouse_clicks,
            metrics.mouse_releases,
            metrics.mouse_moves,
        )
        .await?;
    db_clone_for_final.end_session(&session_id).await?;

    // Display final summary
    println!("\n📊 Final Session Summary:");
    display_metrics_summary(&metrics, &db_clone_for_final, &session_id).await?;

    Ok(())
}

fn trigger_click_context(
    handle: &Option<ClickContextHandle>,
    session_id: &str,
    timestamp: chrono::DateTime<Utc>,
    button: Option<String>,
    x: Option<i32>,
    y: Option<i32>,
) {
    if let Some(ctx) = handle {
        ctx.trigger(ClickContextEvent::new(
            Some(session_id.to_string()),
            timestamp,
            button,
            x,
            y,
        ));
    }
}

struct MetricsTracker {
    keyboard_events: u64,
    keyboard_presses: u64,
    keyboard_releases: u64,
    mouse_events: u64,
    mouse_clicks: u64,
    mouse_releases: u64,
    mouse_moves: u64,
    key_frequency: HashMap<String, u64>,
    mouse_button_frequency: HashMap<String, u64>,
    start_time: Instant,
    last_metrics_display: Instant,
}

async fn display_metrics_summary(
    metrics: &MetricsTracker,
    _db: &Database,
    _session_id: &str,
) -> Result<()> {
    let duration = metrics.start_time.elapsed();
    let total_events = metrics.keyboard_events + metrics.mouse_events;
    let events_per_second = if duration.as_secs() > 0 {
        total_events as f64 / duration.as_secs() as f64
    } else {
        0.0
    };

    println!("\n╔════════════════════════════════════════════════════════╗");
    println!("║              📊 Metrics Summary                        ║");
    println!("╠════════════════════════════════════════════════════════╣");
    println!("║ Duration: {:>42} ║", format_duration(duration));
    println!("║ Total Events: {:>38} ║", total_events);
    println!("║ Events/sec: {:>40.2} ║", events_per_second);
    println!("╠════════════════════════════════════════════════════════╣");
    println!("║ Keyboard Events: {:>35} ║", metrics.keyboard_events);
    println!("║   Presses: {:>42} ║", metrics.keyboard_presses);
    println!("║   Releases: {:>40} ║", metrics.keyboard_releases);
    println!("╠════════════════════════════════════════════════════════╣");
    println!("║ Mouse Events: {:>38} ║", metrics.mouse_events);
    println!("║   Clicks: {:>43} ║", metrics.mouse_clicks);
    println!("║   Releases: {:>40} ║", metrics.mouse_releases);
    println!("║   Moves: {:>44} ║", metrics.mouse_moves);

    // Top keys
    if !metrics.key_frequency.is_empty() {
        println!("╠════════════════════════════════════════════════════════╣");
        println!("║ Top 5 Keys:                                            ║");
        let mut sorted_keys: Vec<_> = metrics.key_frequency.iter().collect();
        sorted_keys.sort_by(|a, b| b.1.cmp(a.1));
        for (i, (key, count)) in sorted_keys.iter().take(5).enumerate() {
            println!("║   {}. {:30} {:>10} ║", i + 1, key, count);
        }
    }

    // Top mouse buttons
    if !metrics.mouse_button_frequency.is_empty() {
        println!("╠════════════════════════════════════════════════════════╣");
        println!("║ Mouse Button Clicks:                                   ║");
        let mut sorted_buttons: Vec<_> = metrics.mouse_button_frequency.iter().collect();
        sorted_buttons.sort_by(|a, b| b.1.cmp(a.1));
        for (button, count) in sorted_buttons.iter() {
            println!("║   {:30} {:>10} ║", button, count);
        }
    }

    println!("╚════════════════════════════════════════════════════════╝");

    Ok(())
}

fn format_duration(duration: std::time::Duration) -> String {
    let secs = duration.as_secs();
    let hours = secs / 3600;
    let minutes = (secs % 3600) / 60;
    let seconds = secs % 60;

    if hours > 0 {
        format!("{}h {}m {}s", hours, minutes, seconds)
    } else if minutes > 0 {
        format!("{}m {}s", minutes, seconds)
    } else {
        format!("{}s", seconds)
    }
}
